use super::*;
use crate::db::test_pool;
use crate::invoices::{create_and_post_invoice, record_payment, PaymentInput};

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).expect("valid date")
}

async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.expect("seeded account")
}

async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1").bind(account).fetch_one(pool).await.expect("balance")
}

async fn invoice(pool: &SqlitePool, customer: &str, issue: NaiveDate, due: NaiveDate, cents: i64) -> InvoiceSummary {
    let services = account_id(pool, "4100").await;
    create_and_post_invoice(
        pool,
        InvoiceInput {
            customer_name: customer.into(),
            issue_date: issue,
            due_date: due,
            lines: vec![InvoiceLineInput {
                description: "Work".into(),
                quantity_e4: QUANTITY_SCALE,
                rate_cents: cents,
                income_account_id: services,
                product_service_id: None,
                class_id: None,
                is_taxable: false,
            }],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        },
    )
    .await
    .expect("invoice posts")
}

/// 18% a year, credited to Interest Income (7000).
async fn turn_on(pool: &SqlitePool) -> i64 {
    let interest = account_id(pool, "7000").await;
    crate::preferences::set(pool, "finance_charge_annual_rate", "18").await.unwrap();
    crate::preferences::set(pool, "finance_charge_account_id", &interest.to_string()).await.unwrap();
    interest
}

fn overdue(balance_cents: i64, days: i64) -> OverdueInvoice {
    OverdueInvoice { invoice_id: 1, invoice_number: 1001, due_date: d(1, 1), balance_cents, from_date: d(1, 1), days, is_finance_charge: false }
}

#[test]
fn rates_and_minimums_parse_with_string_arithmetic() {
    assert_eq!(parse_rate("18").unwrap(), 18_000);
    assert_eq!(parse_rate("18.5%").unwrap(), 18_500);
    assert_eq!(parse_rate("1.375").unwrap(), 1_375);
    assert_eq!(parse_rate(".5").unwrap(), 500);
    assert_eq!(parse_rate("100").unwrap(), 100_000);
    for bad in ["", "abc", "100.001", "1.2345", "-1", "1e2", "."] {
        assert!(parse_rate(bad).is_err(), "{bad:?} should be rejected");
    }
    assert_eq!(format_rate(18_000), "18");
    assert_eq!(format_rate(18_500), "18.5");
    assert_eq!(format_rate(1_375), "1.375");
    assert_eq!(format_rate(0), "0");
    assert_eq!(parse_minimum("$2.50").unwrap(), 250);
    assert!(parse_minimum("-1").is_err());
    assert!(parse_minimum("0.001").is_err());
    assert!(parse_minimum("10000.01").is_err());
}

#[test]
fn interest_is_summed_then_rounded_half_up_once_per_customer() {
    // $1,000.00 for 30 days at 18%: 100,000 × 30 × 18,000 / 36,500,000 = 1,479.45 → $14.79.
    assert_eq!(interest_cents(&[overdue(100_000, 30)], 18_000).unwrap(), 1_479);
    // Exactly half a cent rounds up: 1,000 × 1 × 18,250 = 18,250,000 = 36,500,000 / 2.
    assert_eq!(interest_cents(&[overdue(1_000, 1)], 18_250).unwrap(), 1);
    assert_eq!(interest_cents(&[overdue(999, 1)], 18_250).unwrap(), 0);
    // Two invoices each worth 0.4 cent (14,600,000) add up to 0.8 cent: one cent, not zero.
    let parts = [overdue(14_600, 1), overdue(14_600, 1)];
    assert_eq!(interest_cents(&parts, 1_000).unwrap(), 1);
    assert_eq!(interest_cents(&parts[..1], 1_000).unwrap(), 0);

    let settings = Settings { annual_rate_milli_percent: 18_000, minimum_cents: 2_500, grace_days: 0, income_account_id: None, charge_finance_charges: false };
    assert_eq!(charge_cents(&[overdue(100_000, 30)], &settings).unwrap(), (1_479, 2_500));
    assert_eq!(charge_cents(&[overdue(1_000_000, 30)], &settings).unwrap(), (14_795, 14_795));
}

#[test]
fn the_rounding_rule_never_overflows() {
    let huge = overdue(MAX_LINE_CENTS, 365 * 50);
    assert!(interest_cents(&[huge], MAX_RATE_MILLI_PERCENT).is_err());
    assert_eq!(interest_cents(&[overdue(MAX_LINE_CENTS, 365)], 1_000).unwrap(), MAX_LINE_CENTS / 100);
}

#[tokio::test]
async fn preferences_validate_and_normalize() {
    let pool = test_pool().await;
    let s = settings(&pool).await.unwrap();
    assert_eq!(s, Settings { annual_rate_milli_percent: 0, minimum_cents: 0, grace_days: 0, income_account_id: None, charge_finance_charges: false });
    use crate::preferences::set;
    assert_eq!(set(&pool, "finance_charge_annual_rate", "18.50%").await.unwrap(), "18.5");
    assert_eq!(set(&pool, "finance_charge_minimum", "$1,000").await.unwrap(), "1000.00");
    assert_eq!(set(&pool, "finance_charge_grace_days", "10").await.unwrap(), "10");
    assert_eq!(set(&pool, "finance_charge_on_finance_charges", "true").await.unwrap(), "true");
    let checking = account_id(&pool, "1010").await;
    assert!(set(&pool, "finance_charge_account_id", &checking.to_string()).await.unwrap_err().starts_with("PREF_BAD_FC_ACCOUNT"));
    assert!(set(&pool, "finance_charge_account_id", "999999").await.unwrap_err().starts_with("PREF_BAD_FC_ACCOUNT"));
    assert!(set(&pool, "finance_charge_grace_days", "366").await.is_err());
    let interest = account_id(&pool, "7000").await;
    set(&pool, "finance_charge_account_id", &interest.to_string()).await.unwrap();
    let s = settings(&pool).await.unwrap();
    assert_eq!(s, Settings { annual_rate_milli_percent: 18_500, minimum_cents: 100_000, grace_days: 10, income_account_id: Some(interest), charge_finance_charges: true });
}

#[tokio::test]
async fn assessing_posts_an_invoice_to_ar_and_income_and_shows_in_aging_and_statements() {
    let pool = test_pool().await;
    let interest = turn_on(&pool).await;
    let ar = account_id(&pool, "1200").await;
    // Due Sept 1; $1,000 open on Oct 1 is 30 days overdue: $14.79.
    let late = invoice(&pool, "Acme", d(8, 1), d(9, 1), 100_000).await;
    // Not yet due on Oct 1.
    invoice(&pool, "Acme", d(9, 15), d(10, 15), 50_000).await;
    invoice(&pool, "Bolt", d(9, 20), d(10, 20), 20_000).await;

    let preview = preview(&pool, d(10, 1)).await.unwrap();
    assert_eq!(preview.customers.len(), 1, "only Acme is overdue");
    let acme = &preview.customers[0];
    assert_eq!((acme.customer_name.as_str(), acme.overdue_balance_cents, acme.charge_cents), ("Acme", 100_000, 1_479));
    assert_eq!(acme.invoices, vec![OverdueInvoice {
        invoice_id: late.id,
        invoice_number: late.invoice_number,
        due_date: d(9, 1),
        balance_cents: 100_000,
        from_date: d(9, 1),
        days: 30,
        is_finance_charge: false,
    }]);

    let (ar_before, interest_before) = (balance(&pool, ar).await, balance(&pool, interest).await);
    // The charge can be changed before assessing; the calculated one is kept for the record.
    let made = assess(&pool, AssessInput { as_of: d(10, 1), charges: vec![AssessCharge { customer_name: "Acme".into(), charge_cents: 1_500 }] }).await.unwrap();
    assert_eq!(made.len(), 1);
    let fc = &made[0];
    assert_eq!((fc.customer_name.as_str(), fc.total_cents, fc.issue_date.as_str(), fc.status.as_str()), ("Acme", 1_500, "2026-10-01", "UNPAID"));
    assert!(fc.is_finance_charge);
    assert_eq!(balance(&pool, ar).await - ar_before, 1_500);
    assert_eq!(balance(&pool, interest).await - interest_before, 1_500, "credit-normal income grows");
    let computed: i64 = sqlx::query_scalar("SELECT computed_cents FROM finance_charge_invoices WHERE invoice_id = ?1").bind(fc.id).fetch_one(&pool).await.unwrap();
    assert_eq!(computed, 1_479);

    // In A/R aging (current on its own due date) and on Acme's statement.
    let aging = crate::invoices::ar_aging(&pool, d(10, 1)).await.unwrap();
    assert_eq!(aging.total_cents, 100_000 + 50_000 + 20_000 + 1_500);
    let customer_id = fc.customer_id.unwrap();
    let doc = crate::forms::statement_doc(&pool, customer_id, crate::forms::Period { start_date: d(9, 1), end_date: d(10, 31) }, "$").await.unwrap();
    let rows = doc.statement.unwrap().rows;
    assert!(rows.iter().any(|r| r[1] == format!("Finance Charge #{}", fc.invoice_number) && r[3] == "$15.00"), "{rows:?}");
}

#[tokio::test]
async fn charges_never_cover_the_same_days_twice_and_voiding_reopens_them() {
    let pool = test_pool().await;
    turn_on(&pool).await;
    invoice(&pool, "Acme", d(8, 1), d(9, 1), 100_000).await;
    let charge = |as_of| AssessInput { as_of, charges: vec![AssessCharge { customer_name: "Acme".into(), charge_cents: 1 }] };

    let first = assess(&pool, AssessInput { as_of: d(10, 1), charges: vec![AssessCharge { customer_name: "Acme".into(), charge_cents: 1_479 }] }).await.unwrap();
    assert!(preview(&pool, d(10, 1)).await.unwrap().customers.is_empty(), "nothing left to charge on the same day");
    let err = assess(&pool, charge(d(10, 1))).await.unwrap_err();
    assert!(err.starts_with("FC_NOT_OVERDUE"), "{err}");

    // A month later only October is charged, and the finance charge itself is left alone.
    let later = preview(&pool, d(10, 31)).await.unwrap();
    let acme = &later.customers[0];
    assert_eq!(acme.invoices.len(), 1);
    assert_eq!((acme.invoices[0].from_date, acme.invoices[0].days), (d(10, 1), 30));
    assert_eq!(acme.last_finance_charge_date, Some(d(10, 1)));

    // Unless finance charges earn finance charges.
    crate::preferences::set(&pool, "finance_charge_on_finance_charges", "true").await.unwrap();
    let later = preview(&pool, d(10, 31)).await.unwrap();
    assert_eq!(later.customers[0].invoices.len(), 2);
    assert!(later.customers[0].invoices[1].is_finance_charge);

    // Voiding the charge makes September chargeable again.
    crate::ledger::void_transaction(&pool, first[0].journal_entry_id.unwrap()).await.unwrap();
    let reopened = preview(&pool, d(10, 31)).await.unwrap();
    assert_eq!(reopened.customers[0].invoices.len(), 1);
    assert_eq!((reopened.customers[0].invoices[0].from_date, reopened.customers[0].invoices[0].days), (d(9, 1), 60));
    assert_eq!(reopened.customers[0].last_finance_charge_date, None);
}

#[tokio::test]
async fn grace_days_minimum_and_balances_as_of_the_assessment_date() {
    let pool = test_pool().await;
    turn_on(&pool).await;
    let checking = account_id(&pool, "1010").await;
    let inv = invoice(&pool, "Acme", d(8, 1), d(9, 1), 100_000).await;
    let pay = |day: NaiveDate, cents| PaymentInput {
        invoice_id: inv.id,
        deposit_account_id: checking,
        payment_date: day,
        amount_cents: cents,
        payment_method: "CHECK".into(),
        reference: None,
        exchange_rate_e6: None,
    };
    // Paid down to $400 on Sept 10; another $400 arrives Oct 20, after the assessment date.
    record_payment(&pool, pay(d(9, 10), 60_000)).await.unwrap();
    record_payment(&pool, pay(d(10, 20), 40_000)).await.unwrap();
    let p = preview(&pool, d(10, 1)).await.unwrap();
    assert_eq!(p.customers[0].overdue_balance_cents, 40_000, "the balance on Oct 1, not today's");
    // 40,000 × 30 × 18,000 / 36,500,000 = 591.78 → $5.92.
    assert_eq!(p.customers[0].charge_cents, 592);

    crate::preferences::set(&pool, "finance_charge_minimum", "10").await.unwrap();
    let p = preview(&pool, d(10, 1)).await.unwrap();
    assert_eq!((p.customers[0].interest_cents, p.customers[0].charge_cents), (592, 1_000));

    // 30 days overdue is within 30 grace days; 31 is not, and interest still runs from the due date.
    crate::preferences::set(&pool, "finance_charge_grace_days", "30").await.unwrap();
    assert!(preview(&pool, d(10, 1)).await.unwrap().customers.is_empty());
    assert_eq!(preview(&pool, d(10, 2)).await.unwrap().customers[0].invoices[0].days, 31);

    // Fully paid by Oct 20: nothing to charge after that.
    assert!(preview(&pool, d(10, 21)).await.unwrap().customers.is_empty());
}

#[tokio::test]
async fn assessing_needs_an_account_a_positive_charge_and_is_all_or_nothing() {
    let pool = test_pool().await;
    invoice(&pool, "Acme", d(8, 1), d(9, 1), 100_000).await;
    invoice(&pool, "Bolt", d(8, 1), d(9, 1), 100_000).await;
    let both = |acme, bolt| AssessInput {
        as_of: d(10, 1),
        charges: vec![AssessCharge { customer_name: "Acme".into(), charge_cents: acme }, AssessCharge { customer_name: "Bolt".into(), charge_cents: bolt }],
    };
    assert!(assess(&pool, both(100, 100)).await.unwrap_err().starts_with("FC_NO_ACCOUNT"));
    turn_on(&pool).await;
    assert!(assess(&pool, AssessInput { as_of: d(10, 1), charges: vec![] }).await.unwrap_err().starts_with("FC_NOTHING_SELECTED"));
    assert!(assess(&pool, both(100, 0)).await.unwrap_err().starts_with("FC_BAD_AMOUNT"));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM invoices").fetch_one(&pool).await.unwrap();
    assert_eq!(count, 2, "Acme's charge was rolled back with Bolt's");
    let dup = AssessInput { as_of: d(10, 1), charges: vec![AssessCharge { customer_name: "Acme".into(), charge_cents: 1 }; 2] };
    assert!(assess(&pool, dup).await.unwrap_err().starts_with("FC_DUPLICATE"));
    assert_eq!(assess(&pool, both(100, 200)).await.unwrap().len(), 2);

    // The record of an assessment cannot be rewritten.
    let err = sqlx::query("UPDATE finance_charge_basis SET to_date = '2026-12-31'").execute(&pool).await.unwrap_err();
    assert!(err.to_string().contains("FC_IMMUTABLE"), "{err}");
}
