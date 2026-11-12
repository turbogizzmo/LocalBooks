use super::*;
use crate::customer_groups::{CustomerGroupInput, PaymentReminder, Rule, StatementReminder};
use crate::db::test_pool;

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

async fn customer(pool: &SqlitePool, name: &str, address: &str, delivery: &str, terms_days: Option<i64>) -> i64 {
    sqlx::query_scalar("INSERT INTO customers (name, billing_address, preferred_delivery, terms_days) VALUES (?1, ?2, ?3, ?4) RETURNING id")
        .bind(name)
        .bind(address)
        .bind(delivery)
        .bind(terms_days)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn a_batch_invoices_every_customer_in_a_group_and_queues_each_as_they_prefer() {
    let pool = test_pool().await;
    let services: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(&pool).await.unwrap();
    let ar: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '1200'").fetch_one(&pool).await.unwrap();
    let a = customer(&pool, "Alder Cafe", "Salem, OR 97301", "EMAIL", Some(15)).await;
    let _b = customer(&pool, "Birch Books", "Seattle, WA 98101", "PRINT", None).await;
    let c = customer(&pool, "Cedar Clinic", "Portland, OR 97201", "PRINT", None).await;
    let d_id = customer(&pool, "Dogwood Deli", "Eugene, OR 97401", "NONE", None).await;
    sqlx::query("UPDATE customers SET is_active = 0 WHERE id = ?1").bind(d_id).execute(&pool).await.unwrap();
    let oregon = crate::customer_groups::save(
        &pool,
        None,
        &CustomerGroupInput {
            name: "Oregon".into(),
            rules: vec![Rule::State { code: "OR".into() }],
            member_ids: vec![],
            statement: StatementReminder::default(),
            payments: PaymentReminder::default(),
        },
    )
    .await
    .unwrap();

    let input: BatchInput = serde_json::from_value(serde_json::json!({
        "groupId": oregon.id, "issueDate": d(6, 1),
        "lines": [{ "description": "June maintenance", "quantityE4": 10_000, "rateCents": 12_000, "incomeAccountId": services },
                  { "description": "Filters", "quantityE4": 20_000, "rateCents": 1_500, "incomeAccountId": services }],
    }))
    .unwrap();
    let result = create(&pool, input).await.unwrap();
    assert_eq!(result.invoices.iter().map(|i| i.customer_name.as_str()).collect::<Vec<_>>(), ["Alder Cafe", "Cedar Clinic"]);
    assert_eq!(result.skipped, vec![("Dogwood Deli".to_string(), "inactive".to_string())]);
    assert!(result.invoices.iter().all(|i| i.total_cents == 15_000));
    // Each on its own terms: Alder's 15 days, Cedar the batch's 30.
    assert_eq!(result.invoices.iter().map(|i| i.due_date.as_str()).collect::<Vec<_>>(), ["2026-06-16", "2026-07-01"]);
    assert_eq!((result.emailed_later, result.printed_later), (1, 1));
    let queued: Vec<i64> = sqlx::query_scalar("SELECT document_id FROM form_email_queue WHERE form_type = 'INVOICE'").fetch_all(&pool).await.unwrap();
    assert_eq!(queued, vec![result.invoices[0].id]);
    assert!(result.invoices[1].to_be_printed);
    let ar_total: i64 =
        sqlx::query_scalar("SELECT SUM(debit_cents - credit_cents) FROM v_posted_lines WHERE account_id = ?1").bind(ar).fetch_one(&pool).await.unwrap();
    assert_eq!(ar_total, 30_000);
    let _ = (a, c);
}

#[tokio::test]
async fn a_batch_posts_all_or_nothing_and_is_checked_first() {
    let pool = test_pool().await;
    let services: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(&pool).await.unwrap();
    let a = customer(&pool, "Alder Cafe", "Salem, OR 97301", "NONE", None).await;
    let b = customer(&pool, "Birch Books", "Seattle, WA 98101", "NONE", None).await;
    let line = |rate: i64| serde_json::json!([{ "description": "Work", "quantityE4": 10_000, "rateCents": rate, "incomeAccountId": services }]);
    let batch = |ids: Vec<i64>, lines: serde_json::Value| -> BatchInput {
        serde_json::from_value(serde_json::json!({ "customerIds": ids, "issueDate": d(6, 1), "lines": lines })).unwrap()
    };
    assert!(create(&pool, batch(vec![a, b], serde_json::json!([]))).await.unwrap_err().starts_with("BATCH_NO_LINES"));
    assert!(create(&pool, batch(vec![], line(100))).await.unwrap_err().starts_with("BATCH_NO_CUSTOMERS"));
    // A line every invoice refuses: nothing is left behind.
    assert!(create(&pool, batch(vec![a, b], line(0))).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices").fetch_one(&pool).await.unwrap();
    assert_eq!(count, 0);
    // Repeats are invoiced once.
    assert_eq!(create(&pool, batch(vec![a, b, a], line(100))).await.unwrap().invoices.len(), 2);
}
