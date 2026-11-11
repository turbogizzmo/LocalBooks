use super::*;
use crate::db::test_pool;
use crate::money::Cents;

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn only_records_nothing_uses_are_deleted() {
    let pool = test_pool().await;
    let income: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(&pool).await.unwrap();
    let unused = crate::sales::create_customer(&pool, "Typo Co", None).await.unwrap().id;
    let used = crate::sales::create_customer(&pool, "Acme", None).await.unwrap().id;
    let invoice: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
        "customerName": "Acme", "issueDate": "2026-06-01", "dueDate": "2026-07-01",
        "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": 5_000, "incomeAccountId": income }]
    }))
    .unwrap();
    crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap();

    assert_eq!(delete_record(&pool, "customer", unused).await.unwrap(), "Typo Co");
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM customers WHERE name = 'Typo Co'").await, 0);
    let err = delete_record(&pool, "customer", used).await.unwrap_err();
    assert!(err.starts_with("LIST_IN_USE") && err.contains("make it inactive"), "{err}");

    // A customer with a job can't go either.
    let parent = crate::sales::create_customer(&pool, "Builder", None).await.unwrap().id;
    sqlx::query("INSERT INTO customers (name, parent_id) VALUES ('Builder:Kitchen', ?1)").bind(parent).execute(&pool).await.unwrap();
    assert!(delete_record(&pool, "customer", parent).await.unwrap_err().starts_with("LIST_IN_USE"));

    // Vendors and items the same way.
    let vendor: i64 = sqlx::query_scalar("INSERT INTO vendors (name) VALUES ('Spare Vendor') RETURNING id").fetch_one(&pool).await.unwrap();
    delete_record(&pool, "vendor", vendor).await.unwrap();
    let item = crate::sales::create_product(&pool, crate::sales::ProductInput { name: "Old item".into(), description: None, rate_cents: Cents(100), income_account_id: income, is_taxable: true })
        .await
        .unwrap()
        .id;
    delete_record(&pool, "item", item).await.unwrap();
    assert!(delete_record(&pool, "item", item).await.unwrap_err().starts_with("LIST_NOT_FOUND"));
    assert!(delete_record(&pool, "account", 1).await.unwrap_err().starts_with("LIST_BAD_KIND"));

    // Each delete is in the audit trail.
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM ledger_audit_log WHERE action_type = 'LIST_CHANGE' AND payload_snapshot LIKE '%\"deleted\"%'").await, 3);
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}
