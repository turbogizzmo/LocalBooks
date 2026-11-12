use super::*;
use crate::db::test_pool;
use crate::money::Cents;

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

fn charge(customer: &str, date: NaiveDate, description: &str, rate_cents: i64, income: i64) -> StatementChargeInput {
    serde_json::from_value(
        serde_json::json!({ "customerName": customer, "chargeDate": date, "description": description, "rateCents": rate_cents, "incomeAccountId": income }),
    )
    .unwrap()
}

#[tokio::test]
async fn a_statement_charge_posts_to_ar_shows_on_the_statement_ages_and_takes_a_payment() {
    let pool = test_pool().await;
    let (checking, ar, services) = (account(&pool, "1010").await, account(&pool, "1200").await, account(&pool, "4100").await);

    let first = create(&pool, charge("Maple Dental", d(3, 4), "Monthly retainer", 25_000, services)).await.unwrap();
    let mut with_qty = charge("Maple Dental", d(3, 18), "Extra visits", 4_000, services);
    with_qty.quantity_e4 = Some(30_000);
    with_qty.due_date = Some(d(4, 15));
    let second = create(&pool, with_qty).await.unwrap();
    assert!(first.is_statement_charge && second.is_statement_charge);
    assert_eq!((first.total_cents, second.total_cents), (Cents(25_000), Cents(12_000)));
    assert_eq!(second.due_date, "2026-04-15");

    // Posted to A/R for the customer.
    let ar_balance: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1")
        .bind(ar)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(ar_balance, 37_000);

    // The customer's register lists both, oldest first.
    let customer = first.customer_id.unwrap();
    let register = list(&pool, customer).await.unwrap();
    assert_eq!(
        register.iter().map(|c| (c.invoice.id, c.description.as_str())).collect::<Vec<_>>(),
        [(first.id, "Monthly retainer"), (second.id, "Extra visits")]
    );

    // On the next statement, as statement charges with their descriptions and no invoice number.
    let statement = crate::workflows::customer_statement(&pool, customer, d(3, 1), d(3, 31)).await.unwrap();
    let rows: Vec<(&str, Option<&str>, Option<&str>, i64)> =
        statement.rows.iter().map(|r| (r.source_type.as_str(), r.number.as_deref(), r.memo.as_deref(), r.amount_cents.get())).collect();
    assert_eq!(rows, [("STATEMENT_CHARGE", None, Some("Monthly retainer"), 25_000), ("STATEMENT_CHARGE", None, Some("Extra visits"), 12_000)]);
    assert_eq!(statement.closing_balance_cents, Cents(37_000));
    let doc = crate::forms::statement_doc(&pool, customer, crate::forms::Period { start_date: d(3, 1), end_date: d(3, 31) }, "$").await.unwrap();
    assert!(doc.statement.unwrap().rows.iter().any(|r| r[1] == "Statement Charge" && r.iter().any(|c| c == "Monthly retainer")));

    // In the aging, by due date.
    let aging = crate::invoices::ar_aging(&pool, d(4, 20)).await.unwrap();
    assert_eq!(aging.total_cents, Cents(37_000));

    // A payment applies to it like an invoice.
    crate::invoices::record_payment(
        &pool,
        crate::invoices::PaymentInput {
            discount_cents: Cents::ZERO,
            payment_method_id: None,
            invoice_id: first.id,
            deposit_account_id: checking,
            payment_date: d(3, 20),
            amount_cents: Cents(25_000),
            payment_method: "CHECK".into(),
            reference: Some("884".into()),
            exchange_rate_e6: None,
        },
    )
    .await
    .unwrap();
    let paid = crate::invoices::get_summary(&pool, first.id).await.unwrap();
    assert_eq!(paid.balance_cents, Cents(0));
    assert_eq!(crate::invoices::ar_aging(&pool, d(4, 20)).await.unwrap().total_cents, Cents(12_000));
}

#[tokio::test]
async fn charges_are_checked_before_anything_posts() {
    let pool = test_pool().await;
    let services = account(&pool, "4100").await;
    assert!(create(&pool, charge("Maple Dental", d(3, 4), "  ", 25_000, services)).await.unwrap_err().starts_with("SC_DESCRIPTION"));
    let mut early = charge("Maple Dental", d(3, 4), "Retainer", 25_000, services);
    early.due_date = Some(d(3, 1));
    assert!(create(&pool, early).await.unwrap_err().starts_with("SC_DUE_BEFORE"));
    assert!(create(&pool, charge("Maple Dental", d(3, 4), "Retainer", 0, services)).await.is_err());
    let marked: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM statement_charge_invoices").fetch_one(&pool).await.unwrap();
    let invoices: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices").fetch_one(&pool).await.unwrap();
    assert_eq!((marked, invoices), (0, 0), "a refused charge leaves nothing behind");
}
