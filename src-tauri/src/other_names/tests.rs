use super::*;
use crate::db::test_pool;

fn named(name: &str) -> OtherNameInput {
    OtherNameInput { name: name.into(), ..Default::default() }
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

fn check(payee: &str, bank: i64, expense: i64, cents: i64) -> crate::expenses::ExpenseInput {
    serde_json::from_value(serde_json::json!({ "kind": "CHECK", "payeeName": payee, "paymentAccountId": bank, "expenseDate": "2026-05-02",
        "checkNumber": null, "paymentMethod": null, "memo": null, "lines": [{ "accountId": expense, "amountCents": cents, "memo": null }] }))
    .unwrap()
}

#[tokio::test]
async fn other_names_are_kept_paid_by_check_and_named_on_their_entries() {
    let pool = test_pool().await;
    let owner = create(&pool, &OtherNameInput { name: " Sam Owner ".into(), phone: Some("555-0100".into()), notes: Some("Owner".into()), ..Default::default() }).await.unwrap();
    assert_eq!((owner.name.as_str(), owner.phone.as_deref(), owner.payment_count), ("Sam Owner", Some("555-0100"), 0));

    // A name is used by one record only, among customers, vendors and other names.
    crate::sales::create_customer(&pool, "Acme", None).await.unwrap();
    assert!(create(&pool, &named("acme")).await.unwrap_err().starts_with("OTHER_DUPLICATE"));
    assert!(create(&pool, &named("sam owner")).await.unwrap_err().starts_with("OTHER_DUPLICATE"));
    assert!(create(&pool, &named("A:B")).await.unwrap_err().starts_with("OTHER_BAD_NAME"));
    assert!(create(&pool, &OtherNameInput { email: Some("nope".into()), ..named("Pat") }).await.unwrap_err().starts_with("OTHER_BAD_EMAIL"));

    // A check typed to the other name is paid to them: no vendor is made.
    let (bank, draws) = (account(&pool, "1010").await, account(&pool, "6300").await);
    let posted = crate::expenses::post_expense(&pool, check("sam owner", bank, draws, 50_000)).await.unwrap();
    let vendors: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vendors").fetch_one(&pool).await.unwrap();
    assert_eq!(vendors, 0);
    let (other, vendor): (Option<i64>, Option<i64>) = sqlx::query_as("SELECT other_name_id, vendor_id FROM expenses WHERE id = ?1").bind(posted.id).fetch_one(&pool).await.unwrap();
    assert_eq!((other, vendor), (Some(owner.id), None));
    let party: Option<String> = sqlx::query_scalar("SELECT party_name FROM v_entry_party WHERE journal_entry_id = ?1").bind(posted.journal_entry_id).fetch_one(&pool).await.unwrap();
    assert_eq!(party.as_deref(), Some("Sam Owner"));
    let listed = crate::expenses::list_expenses(&pool).await.unwrap();
    assert_eq!(listed[0].payee_name.as_deref(), Some("Sam Owner"));
    let after = get(&pool, owner.id).await.unwrap();
    assert_eq!((after.payment_count, after.paid_cents), (1, 50_000));

    // A name that isn't one still makes a vendor, as before.
    crate::expenses::post_expense(&pool, check("City Water", bank, draws, 1_000)).await.unwrap();
    let vendors: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vendors WHERE name = 'City Water'").fetch_one(&pool).await.unwrap();
    assert_eq!(vendors, 1);

    // Editing and inactivating are list changes in the audit trail.
    let renamed = update(&pool, owner.id, &OtherNameInput { name: "Sam Owner Jr".into(), notes: Some("05/03/2026: draw".into()), ..Default::default() }).await.unwrap();
    assert_eq!((renamed.name.as_str(), renamed.notes.as_deref(), renamed.phone), ("Sam Owner Jr", Some("05/03/2026: draw"), None));
    assert!(!set_active(&pool, owner.id, false).await.unwrap().is_active);
    let audited: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ledger_audit_log WHERE action_type = 'LIST_CHANGE' AND json_extract(payload_snapshot, '$.list') = 'OTHER_NAME'").fetch_one(&pool).await.unwrap();
    assert_eq!(audited, 3, "added, edited, made inactive");
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}
