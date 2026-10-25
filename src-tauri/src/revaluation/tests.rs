use super::*;
use crate::db::test_pool;
use serde_json::json;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn balance_on(pool: &SqlitePool, account: i64, date: &str) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1 AND entry_date <= ?2")
        .bind(account)
        .bind(date)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn rates(pairs: &[(&str, i64)]) -> BTreeMap<String, i64> {
    pairs.iter().map(|(c, r)| (c.to_string(), *r)).collect()
}

#[tokio::test]
async fn revalues_open_receivables_at_period_end_and_reverses_the_next_day() {
    let pool = test_pool().await;
    let (services, checking) = (account(&pool, "4100").await, account(&pool, "1010").await);
    let ar: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'ACCOUNTS_RECEIVABLE' AND is_system = 1").fetch_one(&pool).await.unwrap();
    let unrealized: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = 'Unrealized Exchange Gain or Loss'").fetch_one(&pool).await.unwrap();
    let realized: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = 'Exchange Gain or Loss'").fetch_one(&pool).await.unwrap();
    let invoice = |code: &str, rate: i64, cents: i64| {
        let pool = pool.clone();
        let input: crate::invoices::InvoiceInput = serde_json::from_value(json!({
            "customerName": format!("{code} customer"), "issueDate": "2026-03-10", "dueDate": "2026-04-10",
            "currency": { "code": code, "exchangeRateE6": rate },
            "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }]
        }))
        .unwrap();
        async move { crate::invoices::create_and_post_invoice(&pool, input).await.unwrap() }
    };
    let eur = invoice("EUR", 1_080_000, 100_000).await; // EUR 1,000 booked at 1.08 = $1,080
    invoice("GBP", 1_270_000, 50_000).await; // GBP 500 booked at 1.27 = $635
    // EUR 400 paid on 3/20 at 1.10, relieving $432 of A/R.
    crate::invoices::record_payment(
        &pool,
        serde_json::from_value(json!({ "invoiceId": eur.id, "paymentDate": "2026-03-20", "amountCents": 40_000, "paymentMethod": "ACH", "depositAccountId": checking, "exchangeRateE6": 1_100_000 })).unwrap(),
    )
    .await
    .unwrap();

    let at = |r: &[(&str, i64)]| rates(r);
    let quarter_end: NaiveDate = "2026-03-31".parse().unwrap();
    let p = preview(&pool, quarter_end, &at(&[("EUR", 1_120_000)])).await.unwrap();
    assert_eq!(p.missing_rates, ["GBP"]);
    assert!(post(&pool, quarter_end, &at(&[("EUR", 1_120_000)])).await.unwrap_err().starts_with("FX_REVAL_RATES"));

    // EUR 600 open: carried at $648, worth $672 at 1.12 (+$24). GBP 500: $635 → $625 at 1.25 (−$10).
    let p = preview(&pool, quarter_end, &at(&[("EUR", 1_120_000), ("GBP", 1_250_000)])).await.unwrap();
    let summary: Vec<(String, i64, i64, Option<i64>)> = p.lines.iter().map(|l| (l.currency_code.clone(), l.foreign_open_cents, l.carrying_cents, l.change_cents)).collect();
    assert_eq!(summary, [("EUR".into(), 60_000, 64_800, Some(2_400)), ("GBP".into(), 50_000, 63_500, Some(-1_000))]);
    assert_eq!(p.net_change_cents, 1_400);

    let ar_before = balance_on(&pool, ar, "2026-03-31").await;
    let posted = post(&pool, quarter_end, &at(&[("EUR", 1_120_000), ("GBP", 1_250_000)])).await.unwrap();
    assert_eq!(posted.net_change_cents, 1_400);
    assert_eq!(balance_on(&pool, ar, "2026-03-31").await - ar_before, 1_400, "the quarter-end balance sheet shows A/R at quarter-end rates");
    assert_eq!(balance_on(&pool, unrealized, "2026-03-31").await, -1_400, "a $14 unrealized gain in the quarter");
    assert_eq!(balance_on(&pool, unrealized, "2026-04-01").await, 0, "reversed the next day");
    assert_eq!(balance_on(&pool, ar, "2026-04-01").await, ar_before, "A/R back at booked rates");
    assert!(post(&pool, quarter_end, &at(&[("EUR", 1_120_000), ("GBP", 1_250_000)])).await.unwrap_err().starts_with("FX_REVAL_EXISTS"));

    // The customer pays the EUR 600 in April at 1.12: the realized gain is still exactly $24.
    crate::invoices::record_payment(
        &pool,
        serde_json::from_value(json!({ "invoiceId": eur.id, "paymentDate": "2026-04-15", "amountCents": 60_000, "paymentMethod": "ACH", "depositAccountId": checking, "exchangeRateE6": 1_120_000 })).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(balance_on(&pool, realized, "2026-04-30").await, -800 - 2_400, "−$8 at 1.10 earlier, then −$24 now (credits are gains)");

    // Undo: both entries are voided and the quarter can be revalued again.
    void(&pool, posted.id).await.unwrap();
    assert_eq!(balance_on(&pool, unrealized, "2026-03-31").await, 0);
    assert!(list(&pool).await.unwrap()[0].is_voided);
    let again = post(&pool, quarter_end, &at(&[("EUR", 1_120_000), ("GBP", 1_250_000)])).await.unwrap();
    assert_eq!(again.net_change_cents, 1_400);

    // Revaluing a date where nothing is open (before any invoice) has nothing to post.
    assert!(post(&pool, "2026-03-01".parse().unwrap(), &BTreeMap::new()).await.unwrap_err().starts_with("FX_REVAL_NOTHING"));
}
