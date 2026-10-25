use super::*;
use crate::db::test_pool;

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn aging_is_as_of_a_date_and_summaries_and_ledger_add_up() {
    let pool = test_pool().await;
    let (checking, services, supplies) = (account(&pool, "1010").await, account(&pool, "4100").await, account(&pool, "6250").await);
    let invoice = |customer: &str, issue: &str, due: &str, cents: i64| {
        let pool = pool.clone();
        let input: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
            "customerName": customer, "issueDate": issue, "dueDate": due,
            "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }]
        }))
        .unwrap();
        async move { crate::invoices::create_and_post_invoice(&pool, input).await.unwrap().id }
    };
    let old = invoice("Acme", "2026-01-01", "2026-01-31", 10_000).await;
    invoice("Acme", "2026-03-01", "2026-03-31", 20_000).await;
    invoice("Globex", "2026-03-15", "2026-04-14", 5_000).await;
    crate::invoices::record_payment(
        &pool,
        crate::invoices::PaymentInput { invoice_id: old, deposit_account_id: checking, payment_date: d(4, 10), amount_cents: 10_000, payment_method: "CHECK".into(), reference: None, exchange_rate_e6: None },
    )
    .await
    .unwrap();

    // On April 1 the January invoice was still open (paid April 10) and 60 days past due.
    let ar = aging(&pool, false, d(4, 1)).await.unwrap();
    let acme = ar.rows.iter().find(|r| r.party_name == "Acme").unwrap();
    assert_eq!(acme.buckets_cents, [0, 20_000, 10_000, 0, 0], "March invoice 1 day late; January invoice 60 days late");
    assert_eq!(ar.total_cents, 35_000);
    let later = aging(&pool, false, d(4, 30)).await.unwrap();
    assert_eq!(later.total_cents, 25_000);
    assert_eq!(later.items.len(), 2);

    let bill: crate::expenses::BillInput = serde_json::from_value(serde_json::json!({
        "vendorName": "Staples", "billNumber": "B-9", "billDate": "2026-03-01", "dueDate": "2026-03-11", "memo": null, "totalCents": 7_000,
        "lines": [{ "accountId": supplies, "amountCents": 7_000, "memo": null }]
    }))
    .unwrap();
    crate::expenses::post_vendor_bill(&pool, bill).await.unwrap();
    let ap = aging(&pool, true, d(5, 15)).await.unwrap();
    assert_eq!(ap.rows[0].buckets_cents, [0, 0, 0, 7_000, 0], "65 days past due");

    let sales = by_party(&pool, false, d(1, 1), d(12, 31)).await.unwrap();
    let names: Vec<(&str, i64)> = sales.rows.iter().map(|r| (r.label.as_str(), r.amount_cents)).collect();
    assert_eq!(names, [("Acme", 30_000), ("Globex", 5_000)]);
    let spend = by_party(&pool, true, d(1, 1), d(12, 31)).await.unwrap();
    assert_eq!((spend.rows[0].label.as_str(), spend.total_cents), ("Staples", 7_000));
    let products = sales_by_product(&pool, d(1, 1), d(12, 31)).await.unwrap();
    assert_eq!((products.rows[0].label.as_str(), products.rows[0].quantity_e4, products.total_cents), ("No product/service", Some(30_000), 35_000));

    let gl = general_ledger(&pool, d(3, 1), d(4, 30)).await.unwrap();
    let checking_gl = gl.accounts.iter().find(|a| a.account_id == checking).unwrap();
    assert_eq!((checking_gl.opening_balance_cents, checking_gl.ending_balance_cents), (0, 10_000));
    let services_gl = gl.accounts.iter().find(|a| a.account_id == services).unwrap();
    assert_eq!((services_gl.opening_balance_cents, services_gl.ending_balance_cents), (10_000, 35_000), "income opens at its year-to-date amount");
    let debits: i64 = gl.accounts.iter().map(|a| a.total_debit_cents).sum();
    let credits: i64 = gl.accounts.iter().map(|a| a.total_credit_cents).sum();
    assert_eq!(debits, credits);

    let tb = crate::statements::trial_balance_as_of(&pool, d(4, 30)).await.unwrap();
    assert_eq!(tb.total_debit_cents, tb.total_credit_cents);
    let cash = crate::statements::cash_flow(&pool, d(1, 1), d(12, 31)).await.unwrap();
    assert_eq!((cash.net_change_cents, cash.cash_end_cents), (10_000, 10_000));
    let net = cash.lines.iter().find(|l| l.label == "Net Income").unwrap().amount_cents;
    assert_eq!(net, Some(28_000), "35,000 invoiced less 7,000 of supplies");
}
