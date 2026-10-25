use super::*;
use crate::db::test_pool;
use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};

fn input(name: &str, parent_id: Option<i64>) -> ClassInput {
    ClassInput { name: name.into(), parent_id }
}

#[tokio::test]
async fn classes_nest_and_refuse_duplicates_and_cycles() {
    let pool = test_pool().await;
    let east = create_class(&pool, input("East", None)).await.unwrap();
    let retail = create_class(&pool, input(" Retail ", Some(east.id))).await.unwrap();
    assert_eq!((retail.full_name.as_str(), retail.depth), ("East:Retail", 1));

    assert!(create_class(&pool, input("east", None)).await.unwrap_err().starts_with("CLASS_DUPLICATE"));
    assert!(create_class(&pool, input("A:B", None)).await.unwrap_err().starts_with("CLASS_NAME_COLON"));
    // The same name is fine under a different parent.
    create_class(&pool, input("Retail", None)).await.unwrap();

    let err = update_class(&pool, east.id, input("East", Some(retail.id))).await.unwrap_err();
    assert!(err.starts_with("CLASS_CYCLE"), "{err}");

    // Listed as a tree: each class directly followed by its subclasses.
    create_class(&pool, input("East Coast", None)).await.unwrap();
    let order: Vec<String> = list_classes(&pool).await.unwrap().into_iter().map(|c| c.full_name).collect();
    assert_eq!(order, ["East", "East:Retail", "East Coast", "Retail"]);

    let moved = update_class(&pool, retail.id, input("Stores", None)).await.unwrap();
    assert_eq!((moved.full_name.as_str(), moved.parent_id), ("Stores", None));
}

#[tokio::test]
async fn inactive_cascades_and_used_classes_cannot_be_deleted() {
    let pool = test_pool().await;
    let east = create_class(&pool, input("East", None)).await.unwrap();
    let retail = create_class(&pool, input("Retail", Some(east.id))).await.unwrap();

    set_class_active(&pool, east.id, false).await.unwrap();
    assert!(!get_class(&pool, retail.id).await.unwrap().is_active);
    let err = set_class_active(&pool, retail.id, true).await.unwrap_err();
    assert!(err.starts_with("CLASS_PARENT_INACTIVE"), "{err}");
    set_class_active(&pool, east.id, true).await.unwrap();
    set_class_active(&pool, retail.id, true).await.unwrap();

    assert!(delete_class(&pool, east.id).await.unwrap_err().starts_with("CLASS_HAS_SUBCLASSES"));
    let account = |n: &'static str| sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n);
    let (checking, rent) = (account("1010").fetch_one(&pool).await.unwrap(), account("6300").fetch_one(&pool).await.unwrap());
    let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let lines = vec![JournalLine::debit(rent, 500).with_class(Some(retail.id)), JournalLine::credit(checking, 500)];
    post_journal_entry(&pool, JournalEntry { id: None, date, description: String::new(), lines, currency: None }).await.unwrap();
    assert!(delete_class(&pool, retail.id).await.unwrap_err().starts_with("CLASS_IN_USE"));

    // Inactive classes cannot be chosen for new lines.
    set_class_active(&pool, retail.id, false).await.unwrap();
    let lines = vec![JournalLine::debit(rent, 500).with_class(Some(retail.id)), JournalLine::credit(checking, 500)];
    let err = post_journal_entry(&pool, JournalEntry { id: None, date, description: String::new(), lines, currency: None }).await.unwrap_err();
    assert!(err.starts_with("JE_INACTIVE_CLASS: line 1"), "{err}");

    let unused = create_class(&pool, input("Spare", None)).await.unwrap();
    delete_class(&pool, unused.id).await.unwrap();
    assert!(list_classes(&pool).await.unwrap().iter().all(|c| c.id != unused.id));
}

#[tokio::test]
async fn reclassifying_moves_lines_between_classes_and_voids_in_order() {
    let pool = test_pool().await;
    let account = |n: &'static str| sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n);
    let (checking, rent) = (account("1010").fetch_one(&pool).await.unwrap(), account("6300").fetch_one(&pool).await.unwrap());
    let east = create_class(&pool, input("East", None)).await.unwrap().id;
    let west = create_class(&pool, input("West", None)).await.unwrap().id;
    let date = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let lines = vec![JournalLine::debit(rent, 800).with_class(Some(east)), JournalLine::credit(checking, 800)];
    let original = post_journal_entry(&pool, JournalEntry { id: None, date, description: "Rent".into(), lines, currency: None }).await.unwrap();
    let class_net = |class: i64| {
        sqlx::query_scalar::<_, i64>("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1 AND class_id = ?2").bind(rent).bind(class)
    };
    let query = ReclassQuery { start_date: date, end_date: date, account_id: None, class_id: None, unclassified: false };
    let candidates = reclass_candidates(&pool, &query).await.unwrap();
    assert_eq!(candidates.len(), 1, "only the expense line; the bank line is not income or expense");
    let line = candidates[0].line_id;

    // East → West, then West → no class: each move is its own entry and nothing doubles up.
    set_class_active(&pool, east, false).await.unwrap(); // moving out of an inactive class is allowed
    assert_eq!(reclassify(&pool, &[line], Some(west)).await.unwrap(), 1);
    assert_eq!((class_net(east).fetch_one(&pool).await.unwrap(), class_net(west).fetch_one(&pool).await.unwrap()), (0, 800));
    assert_eq!(reclassify(&pool, &[line], Some(west)).await.unwrap(), 0, "already in West");
    assert_eq!(reclass_candidates(&pool, &query).await.unwrap()[0].class_id, Some(west));
    assert_eq!(reclassify(&pool, &[line], None).await.unwrap(), 1);
    assert_eq!(class_net(west).fetch_one(&pool).await.unwrap(), 0);
    assert_eq!(reclass_candidates(&pool, &query).await.unwrap()[0].class_id, None);

    // The original cannot be voided while moved; reclassifying entries are undone newest first.
    let err = crate::ledger::void_transaction(&pool, original).await.unwrap_err();
    assert!(err.starts_with("VOID_RECLASSIFIED"), "{err}");
    let reclass_entries: Vec<i64> =
        sqlx::query_scalar("SELECT DISTINCT journal_entry_id FROM class_reclassifications ORDER BY id").fetch_all(&pool).await.unwrap();
    let err = crate::ledger::void_transaction(&pool, reclass_entries[0]).await.unwrap_err();
    assert!(err.starts_with("VOID_RECLASS_ORDER"), "{err}");
    crate::ledger::void_transaction(&pool, reclass_entries[1]).await.unwrap();
    assert_eq!(reclass_candidates(&pool, &query).await.unwrap()[0].class_id, Some(west));
    crate::ledger::void_transaction(&pool, reclass_entries[0]).await.unwrap();
    assert_eq!(reclass_candidates(&pool, &query).await.unwrap()[0].class_id, Some(east));
    crate::ledger::void_transaction(&pool, original).await.unwrap();
    assert_eq!(class_net(east).fetch_one(&pool).await.unwrap(), 0);
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}
