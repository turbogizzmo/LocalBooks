use super::*;
use crate::db::test_pool;
use crate::money::Cents;

fn day() -> NaiveDate {
    "2026-11-09".parse().unwrap()
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn id_of(pool: &SqlitePool, table: &str, name: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT id FROM {table} WHERE name = ?1"))).bind(name).fetch_one(pool).await.unwrap()
}

async fn invoice(pool: &SqlitePool, customer: &str, cents: i64, item: Option<i64>) -> i64 {
    let services = account(pool, "4100").await;
    let input: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({ "customerName": customer, "issueDate": "2026-10-01", "dueDate": "2026-10-31",
        "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services, "productServiceId": item, "isTaxable": false }] }))
    .unwrap();
    crate::invoices::create_and_post_invoice(pool, input).await.unwrap().id
}

/// Every posted journal row, to show a merge leaves them as they were.
async fn journal(pool: &SqlitePool) -> Vec<(i64, i64, i64, i64, i64)> {
    sqlx::query_as("SELECT id, journal_entry_id, account_id, debit_cents, credit_cents FROM journal_lines ORDER BY id").fetch_all(pool).await.unwrap()
}

async fn ar_total(pool: &SqlitePool) -> i64 {
    crate::invoices::ar_aging(pool, day()).await.unwrap().total_cents.get()
}

#[tokio::test]
async fn merging_customers_moves_their_documents_and_leaves_the_journal_alone() {
    let pool = test_pool().await;
    let first = invoice(&pool, "Acme Co", 40_000, None).await;
    let second = invoice(&pool, "ACME Company", 25_000, None).await;
    let (from, to) = (id_of(&pool, "customers", "ACME Company").await, id_of(&pool, "customers", "Acme Co").await);
    let before = journal(&pool).await;
    let aging = ar_total(&pool).await;

    assert_eq!(merge_target(&pool, MergeKind::Customer, from, " acme co ").await.unwrap().map(|t| t.0), Some(to), "renaming to the other's name offers it");
    // Customer groups: one with both, one with only the merged customer.
    let both: i64 = sqlx::query_scalar("INSERT INTO customer_groups (name) VALUES ('Both') RETURNING id").fetch_one(&pool).await.unwrap();
    let only: i64 = sqlx::query_scalar("INSERT INTO customer_groups (name) VALUES ('Only') RETURNING id").fetch_one(&pool).await.unwrap();
    for (group, customer) in [(both, from), (both, to), (only, from)] {
        sqlx::query("INSERT INTO customer_group_members (group_id, customer_id) VALUES (?1, ?2)").bind(group).bind(customer).execute(&pool).await.unwrap();
    }
    let result = merge(&pool, MergeKind::Customer, from, to, day()).await.unwrap();
    let members: Vec<(i64, i64)> = sqlx::query_as("SELECT group_id, customer_id FROM customer_group_members ORDER BY group_id").fetch_all(&pool).await.unwrap();
    assert_eq!(members, vec![(both, to), (only, to)], "the kept customer is in every group the merged one was in");
    assert_eq!((result.from_name.as_str(), result.to_name.as_str()), ("ACME Company", "Acme Co"));
    assert_eq!(journal(&pool).await, before, "posted journal rows never change");
    assert_eq!(ar_total(&pool).await, aging, "A/R ties out");
    let moved: Vec<(i64, String)> = sqlx::query_as("SELECT customer_id, customer_name FROM invoices WHERE id IN (?1, ?2) ORDER BY id")
        .bind(first)
        .bind(second)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(moved, vec![(to, "Acme Co".to_string()), (to, "Acme Co".to_string())]);
    let statement = crate::workflows::customer_statement(&pool, to, "2026-01-01".parse().unwrap(), day()).await.unwrap();
    assert_eq!(statement.closing_balance_cents, 65_000, "the kept customer's statement has both");
    let (active, merged_into): (bool, Option<i64>) =
        sqlx::query_as("SELECT is_active, merged_into_id FROM customers WHERE id = ?1").bind(from).fetch_one(&pool).await.unwrap();
    assert_eq!((active, merged_into), (false, Some(to)));
    let audited: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM ledger_audit_log WHERE action_type = 'LIST_CHANGE' AND json_extract(payload_snapshot, '$.merge.fromId') = ?1")
            .bind(from)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(audited, 1);
    assert!(merge(&pool, MergeKind::Customer, from, to, day()).await.unwrap_err().starts_with("MERGE_ALREADY"));
}

#[tokio::test]
async fn the_document_exception_is_only_for_a_merge_being_applied_and_only_the_name() {
    let pool = test_pool().await;
    let inv = invoice(&pool, "Acme Co", 40_000, None).await;
    let other = invoice(&pool, "Globex", 1_000, None).await;
    let (acme, globex) = (id_of(&pool, "customers", "Acme Co").await, id_of(&pool, "customers", "Globex").await);
    let refused = |sql: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query(sql).execute(&pool).await.is_err() }
    };
    assert!(refused("UPDATE invoices SET customer_name = 'Someone' WHERE status <> 'DRAFT'").await, "no merge: a posted invoice is fixed");
    // A merge Acme -> Globex being applied lets exactly that move through, and nothing else.
    sqlx::query("INSERT INTO name_merges (kind, from_id, to_id, from_name, to_name) VALUES ('CUSTOMER', ?1, ?2, 'Acme Co', 'Globex')")
        .bind(acme)
        .bind(globex)
        .execute(&pool)
        .await
        .unwrap();
    assert!(sqlx::query("UPDATE invoices SET total_cents = 1 WHERE id = ?1").bind(inv).execute(&pool).await.is_err(), "other columns stay fixed");
    assert!(sqlx::query("UPDATE invoices SET customer_name = 'Globex', due_date = '2027-01-01' WHERE id = ?1").bind(inv).execute(&pool).await.is_err());
    assert!(sqlx::query("UPDATE invoices SET customer_name = 'Acme Co' WHERE id = ?1").bind(other).execute(&pool).await.is_err(), "only from the merged name");
    sqlx::query("UPDATE invoices SET customer_id = ?2, customer_name = 'Globex' WHERE id = ?1").bind(inv).bind(globex).execute(&pool).await.unwrap();
    assert!(
        sqlx::query("INSERT INTO name_merges (kind, from_id, to_id, from_name, to_name) VALUES ('VENDOR', 1, 2, 'a', 'b')").execute(&pool).await.is_err(),
        "one at a time"
    );
    sqlx::query("UPDATE name_merges SET applying = 0").execute(&pool).await.unwrap();
    assert!(sqlx::query("UPDATE invoices SET customer_name = 'Acme Co' WHERE id = ?1").bind(inv).execute(&pool).await.is_err(), "fixed again once applied");
    assert!(sqlx::query("UPDATE name_merges SET applying = 1").execute(&pool).await.is_err(), "a merge can't be reopened");
    assert!(sqlx::query("DELETE FROM name_merges").execute(&pool).await.is_err());
}

#[tokio::test]
async fn merge_exceptions_cover_every_trigger_on_the_documents_a_merge_moves() {
    let pool = test_pool().await;
    let moved: Vec<(&str, &str)> = CUSTOMER_COLUMNS
        .iter()
        .chain(VENDOR_COLUMNS)
        .chain(ITEM_COLUMNS)
        .copied()
        .chain([("invoices", "customer_id"), ("invoices", "customer_name")])
        .collect();
    let tables: Vec<&str> = moved.iter().map(|(t, _)| *t).collect();
    // A trigger limited to other columns (`BEFORE UPDATE OF a, b ON t`) never sees a repoint.
    let watches_a_moved_column = |table: &str, sql: &str| {
        let upper = sql.to_uppercase();
        let Some(start) = upper.find("BEFORE UPDATE OF ") else { return true };
        let rest = &sql[start + "BEFORE UPDATE OF ".len()..];
        let columns = &rest[..rest.to_uppercase().find(" ON ").unwrap_or(rest.len())];
        columns.split(',').map(str::trim).any(|c| moved.iter().any(|(t, m)| *t == table && c.eq_ignore_ascii_case(m)))
    };
    let triggers: Vec<(String, String, String)> =
        sqlx::query_as("SELECT name, tbl_name, sql FROM sqlite_master WHERE type = 'trigger'").fetch_all(&pool).await.unwrap();
    let blocking: Vec<&str> = triggers
        .iter()
        .filter(|(_, table, sql)| tables.contains(&table.as_str()) && sql.to_uppercase().contains("BEFORE UPDATE") && sql.contains("RAISE"))
        .filter(|(_, _, sql)| !sql.contains("name_merges"))
        .filter(|(_, table, sql)| watches_a_moved_column(table, sql))
        // Fires only on voiding, which a merge never does.
        .filter(|(name, _, _)| !matches!(name.as_str(), "trg_bounced_receipt_void_receipt" | "trg_uf_matched_payment_void" | "trg_uf_transferred_receipt_void"))
        .map(|(name, _, _)| name.as_str())
        .collect();
    assert!(blocking.is_empty(), "these triggers would refuse a merge's repoint; add them to 0099's exceptions (or a later migration's): {blocking:?}");
}

#[tokio::test]
async fn merging_vendors_and_items_moves_bills_checks_and_lines() {
    let pool = test_pool().await;
    let (rent, checking) = (account(&pool, "6300").await, account(&pool, "1010").await);
    let bill: crate::expenses::BillInput =
        serde_json::from_value(serde_json::json!({ "vendorName": "Landlord LLC", "billNumber": "1", "billDate": "2026-10-01", "dueDate": "2026-10-31",
        "totalCents": 5_000, "lines": [{ "accountId": rent, "amountCents": 5_000 }] }))
        .unwrap();
    crate::expenses::post_vendor_bill(&pool, bill).await.unwrap();
    let check: crate::expenses::ExpenseInput =
        serde_json::from_value(serde_json::json!({ "kind": "CHECK", "payeeName": "Landlord", "paymentAccountId": checking, "expenseDate": "2026-10-02",
        "lines": [{ "accountId": rent, "amountCents": 1_000 }] }))
        .unwrap();
    crate::expenses::post_expense(&pool, check).await.unwrap();
    let (from, to) = (id_of(&pool, "vendors", "Landlord").await, id_of(&pool, "vendors", "Landlord LLC").await);
    // A file kept with the merged vendor (the attachments library), and an item it supplies.
    let file: i64 = sqlx::query_scalar("INSERT INTO record_attachments (record_kind, record_id, file_name, media_type, size_bytes, content) VALUES ('VENDOR', ?1, 'w9.pdf', 'application/pdf', 3, x'255044') RETURNING id")
        .bind(from)
        .fetch_one(&pool)
        .await
        .unwrap();
    let supplied: i64 = sqlx::query_scalar(
        "INSERT INTO products_services (name, rate_cents, income_account_id, preferred_vendor_id) VALUES ('Lease', 100, ?1, ?2) RETURNING id",
    )
    .bind(account(&pool, "4100").await)
    .bind(from)
    .fetch_one(&pool)
    .await
    .unwrap();
    let before = journal(&pool).await;
    merge(&pool, MergeKind::Vendor, from, to, day()).await.unwrap();
    let owner: i64 = sqlx::query_scalar("SELECT record_id FROM record_attachments WHERE id = ?1").bind(file).fetch_one(&pool).await.unwrap();
    assert_eq!(owner, to, "the merged vendor's files move to the one kept");
    let preferred: Option<i64> =
        sqlx::query_scalar("SELECT preferred_vendor_id FROM products_services WHERE id = ?1").bind(supplied).fetch_one(&pool).await.unwrap();
    assert_eq!(preferred, Some(to), "items it supplies name the kept vendor");
    assert_eq!(journal(&pool).await, before);
    let theirs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM expenses WHERE vendor_id = ?1").bind(to).fetch_one(&pool).await.unwrap();
    assert_eq!(theirs, 1);

    let income = account(&pool, "4100").await;
    let item = |name: &str| crate::sales::ProductInput {
        name: name.into(),
        description: None,
        rate_cents: Cents(1_000),
        income_account_id: income,
        is_taxable: false,
    };
    let a = crate::sales::create_product(&pool, item("Consulting")).await.unwrap().id;
    let b = crate::sales::create_product(&pool, item("Consulting hours")).await.unwrap().id;
    let inv = invoice(&pool, "Acme", 2_000, Some(b)).await;
    merge(&pool, MergeKind::Item, b, a, day()).await.unwrap();
    let line: i64 = sqlx::query_scalar("SELECT product_service_id FROM invoice_items WHERE invoice_id = ?1").bind(inv).fetch_one(&pool).await.unwrap();
    assert_eq!(line, a);
}

#[tokio::test]
async fn merging_accounts_moves_the_balance_with_one_entry_and_points_lists_at_the_kept_one() {
    let pool = test_pool().await;
    let (checking, rent) = (account(&pool, "1010").await, account(&pool, "6300").await);
    let office: i64 =
        sqlx::query_scalar("INSERT INTO chart_of_accounts (name, account_type) VALUES ('Office Rent', 'EXPENSE') RETURNING id").fetch_one(&pool).await.unwrap();
    for (to, cents) in [(rent, 50_000), (office, 20_000)] {
        let check: crate::expenses::ExpenseInput =
            serde_json::from_value(serde_json::json!({ "kind": "CHECK", "payeeName": "Landlord", "paymentAccountId": checking, "expenseDate": "2026-10-02",
            "lines": [{ "accountId": to, "amountCents": cents }] }))
            .unwrap();
        crate::expenses::post_expense(&pool, check).await.unwrap();
    }
    sqlx::query("INSERT INTO bank_rules (pattern, direction, account_id, origin) VALUES ('RENT', 'OUTFLOW', ?1, 'LEARNED')")
        .bind(office)
        .execute(&pool)
        .await
        .unwrap();
    let before = journal(&pool).await;
    let result = merge(&pool, MergeKind::Account, office, rent, day()).await.unwrap();
    assert_eq!(result.balance_cents, 20_000);
    let after = journal(&pool).await;
    assert_eq!(&after[..before.len()], &before[..], "the earlier rows are untouched; one entry is added");
    let balance = |id: i64| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1").bind(id).fetch_one(&pool).await.unwrap() }
    };
    assert_eq!((balance(office).await, balance(rent).await), (0, 70_000));
    let rule: i64 = sqlx::query_scalar("SELECT account_id FROM bank_rules WHERE pattern = 'RENT'").fetch_one(&pool).await.unwrap();
    assert_eq!(rule, rent);

    assert!(merge(&pool, MergeKind::Account, checking, account(&pool, "6100").await, day()).await.unwrap_err().starts_with("MERGE_ACCOUNT_TYPES"));
    let savings: i64 = sqlx::query_scalar("INSERT INTO chart_of_accounts (name, account_type) VALUES ('Reserve Savings', 'BANK') RETURNING id")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(merge(&pool, MergeKind::Account, savings, checking, day()).await.unwrap_err().starts_with("MERGE_BANK_ACCOUNT"));
}
