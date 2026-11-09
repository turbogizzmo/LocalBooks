use super::*;
use crate::customer_groups::{CustomerGroupInput, PaymentReminder, StatementReminder};
use crate::db::test_pool;

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

#[tokio::test]
async fn overdue_invoices_come_up_for_review_and_sending_queues_them() {
    let pool = test_pool().await;
    let services: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(&pool).await.unwrap();
    let invoice = |customer: &str, due: NaiveDate, cents: i64| {
        let pool = pool.clone();
        let customer = customer.to_string();
        async move {
            let input: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({ "customerName": customer, "issueDate": due - chrono::Duration::days(30), "dueDate": due,
                "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }] }))
            .unwrap();
            crate::invoices::create_and_post_invoice(&pool, input).await.unwrap()
        }
    };
    let late = invoice("Alder Cafe", d(5, 1), 20_000).await;
    let barely = invoice("Alder Cafe", d(5, 28), 5_000).await;
    let other = invoice("Birch Books", d(4, 1), 9_000).await;
    let alder: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Alder Cafe'").fetch_one(&pool).await.unwrap();
    let group = crate::customer_groups::save(
        &pool,
        None,
        &CustomerGroupInput { name: "Regulars".into(), rules: vec![], member_ids: vec![alder], statement: StatementReminder::default(), payments: PaymentReminder { every_days: Some(14), min_days_overdue: 7, last_sent: None } },
    )
    .await
    .unwrap();

    // Only the group's invoices at least 7 days late.
    let found = overdue(&pool, &group, d(6, 1)).await.unwrap();
    assert_eq!(found.iter().map(|o| (o.invoice_id, o.days_overdue)).collect::<Vec<_>>(), [(late.id, 31)]);
    let due_now = due(&pool, d(6, 1)).await.unwrap();
    assert_eq!(due_now.iter().map(|(g, n, total)| (g.name.as_str(), *n, *total)).collect::<Vec<_>>(), [("Regulars", 1, 20_000)]);

    assert!(send(&pool, group.id, &[other.id], d(6, 1)).await.unwrap_err().starts_with("REMINDER_NOT_OVERDUE"));
    assert!(send(&pool, group.id, &[barely.id], d(6, 1)).await.is_err(), "not late enough yet");
    assert_eq!(send(&pool, group.id, &[late.id], d(6, 1)).await.unwrap(), 1);
    assert!(overdue(&pool, &group, d(6, 1)).await.unwrap()[0].queued);
    // Sent: nothing until the next round, 14 days on.
    assert!(due(&pool, d(6, 10)).await.unwrap().is_empty());
    assert_eq!(due(&pool, d(6, 15)).await.unwrap().len(), 1);
}
