use super::*;
use crate::db::test_pool;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(credit_cents - debit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(account).fetch_one(pool).await.unwrap()
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 7, day).unwrap()
}

#[test]
fn tax_rounds_half_up() {
    assert_eq!(tax_on(10_000, 82_500), Some(825));
    assert_eq!(tax_on(1_000, 82_500), Some(83), "82.5 cents rounds up");
    assert_eq!(tax_on(0, 82_500), Some(0));
}

#[tokio::test]
async fn sales_tax_is_charged_reported_and_paid() {
    let pool = test_pool().await;
    let (checking, ar, payable, services) = (account(&pool, "1010").await, account(&pool, "1200").await, account(&pool, "2200").await, account(&pool, "4100").await);
    let rate = create_tax_rate(&pool, TaxRateInput { name: "State 8.25%".into(), rate_ppm: 82_500, agency_name: Some("Dept. of Revenue".into()), liability_account_id: None }).await.unwrap();
    assert_eq!((rate.liability_account_id, rate.agency_name.as_deref()), (payable, Some("Dept. of Revenue")));

    let invoice: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
        "customerName": "Acme", "issueDate": "2026-07-01", "dueDate": "2026-07-31", "taxRateId": rate.id,
        "lines": [
            { "description": "Widgets", "quantityE4": 10_000, "rateCents": 10_000, "incomeAccountId": services, "isTaxable": true },
            { "description": "Labor", "quantityE4": 10_000, "rateCents": 5_000, "incomeAccountId": services, "isTaxable": false }
        ]
    }))
    .unwrap();
    let inv = crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap();
    assert_eq!(inv.total_cents, 15_825);
    assert_eq!((balance(&pool, ar).await, balance(&pool, payable).await), (-15_825, 825));

    let receipt: crate::sales::SalesReceiptInput = serde_json::from_value(serde_json::json!({
        "customerName": null, "receiptDate": "2026-07-02", "depositAccountId": checking, "paymentMethod": "CASH", "reference": null, "memo": null, "taxRateId": rate.id,
        "lines": [{ "productServiceId": null, "description": "Widget", "quantityE4": 10_000, "rateCents": 2_000, "incomeAccountId": services, "isTaxable": true }]
    }))
    .unwrap();
    assert_eq!(crate::sales::post_sales_receipt(&pool, receipt).await.unwrap().total_cents, 2_165);

    let memo = crate::credits::create_credit_memo(
        &pool,
        crate::credits::CreditMemoInput { credit_number: None, form_lines: Vec::new(),
            customer_name: "Acme".into(),
            credit_date: d(3),
            memo: None,
            refund_account_id: None,
            tax_rate_id: Some(rate.id),
            lines: vec![crate::sales::SalesLineInput { product_service_id: None, description: "Return".into(), quantity_e4: 10_000, rate_cents: Cents(1_000), income_account_id: services, class_id: None, is_taxable: true, service_date: None }],
            header: Default::default(),
        },
    )
    .await
    .unwrap();
    assert_eq!(memo.total_cents, 1_083);
    assert_eq!(balance(&pool, payable).await, 825 + 165 - 83);

    // An estimate keeps its tax when it becomes an invoice.
    let estimate: crate::estimates::EstimateInput = serde_json::from_value(serde_json::json!({
        "customerName": "Acme", "estimateDate": "2026-07-04", "expirationDate": null, "memo": null, "taxRateId": rate.id,
        "lines": [{ "productServiceId": null, "description": "Install", "quantityE4": 10_000, "rateCents": 4_000, "incomeAccountId": services, "isTaxable": true }]
    }))
    .unwrap();
    let est = crate::estimates::create_estimate(&pool, estimate).await.unwrap();
    assert_eq!((est.total_cents, est.tax_cents), (4_330, 330));
    let converted = crate::estimates::convert_to_invoice(&pool, est.id, d(5), d(30)).await.unwrap();
    let invoice_total: i64 = sqlx::query_scalar("SELECT total_cents FROM invoices WHERE id = ?1").bind(converted.converted_to_invoice_id).fetch_one(&pool).await.unwrap();
    assert_eq!(invoice_total, 4_330);

    let report = tax_liability(&pool, d(1), d(31)).await.unwrap();
    let row = &report.rows[0];
    assert_eq!((row.taxable_sales_cents, row.non_taxable_sales_cents, row.tax_collected_cents), (10_000 + 2_000 - 1_000 + 4_000, 5_000, 825 + 165 - 83 + 330));
    assert_eq!(report.owed_cents, 1_237);

    pay_sales_tax(&pool, SalesTaxPaymentInput { tax_rate_id: Some(rate.id), liability_account_id: None, account_id: checking, payment_date: d(31), amount_cents: Cents::from(1_237), reference: Some("Q3".into()) }).await.unwrap();
    let after = tax_liability(&pool, d(1), d(31)).await.unwrap();
    assert_eq!((after.paid_cents, after.owed_cents), (1_237, 0));
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}

/// Preferences › Sales Tax › "Owe sales tax as of": invoice date (accrual) or payment date (cash).
#[tokio::test]
async fn sales_tax_is_owed_when_invoiced_or_when_paid() {
    let pool = test_pool().await;
    let (checking, services) = (account(&pool, "1010").await, account(&pool, "4100").await);
    let rate = create_tax_rate(&pool, TaxRateInput { name: "State 10%".into(), rate_ppm: 100_000, agency_name: None, liability_account_id: None }).await.unwrap();
    // July 1: $1,000 taxable + $200 labor, $100 tax → $1,300.
    let invoice: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
        "customerName": "Acme", "issueDate": "2026-07-01", "dueDate": "2026-07-31", "taxRateId": rate.id,
        "lines": [
            { "description": "Widgets", "quantityE4": 10_000, "rateCents": 100_000, "incomeAccountId": services, "isTaxable": true },
            { "description": "Labor", "quantityE4": 10_000, "rateCents": 20_000, "incomeAccountId": services, "isTaxable": false }
        ]
    }))
    .unwrap();
    let inv = crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap();
    assert_eq!(inv.total_cents, 130_000);
    // July 2: a cash sale of $50 taxable, $5 tax. Cash on either basis.
    let receipt: crate::sales::SalesReceiptInput = serde_json::from_value(serde_json::json!({
        "customerName": null, "receiptDate": "2026-07-02", "depositAccountId": checking, "paymentMethod": "CASH", "reference": null, "memo": null, "taxRateId": rate.id,
        "lines": [{ "productServiceId": null, "description": "Widget", "quantityE4": 10_000, "rateCents": 5_000, "incomeAccountId": services, "isTaxable": true }]
    }))
    .unwrap();
    crate::sales::post_sales_receipt(&pool, receipt).await.unwrap();
    // August 10: Acme pays $650, half the invoice.
    let pay = |date: NaiveDate, cents: i64| crate::invoices::PaymentInput { discount_cents: Cents::ZERO, invoice_id: inv.id, deposit_account_id: checking, payment_date: date, amount_cents: Cents(cents), payment_method: "CHECK".into(), reference: None, exchange_rate_e6: None, payment_method_id: None };
    crate::invoices::record_payment(&pool, pay(NaiveDate::from_ymd_opt(2026, 8, 10).unwrap(), 65_000)).await.unwrap();

    let (july, august) = ((d(1), d(31)), (NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(), NaiveDate::from_ymd_opt(2026, 8, 31).unwrap()));
    let row = |r: &TaxLiabilityReport| r.rows.iter().map(|r| (r.taxable_sales_cents, r.non_taxable_sales_cents, r.tax_collected_cents)).next().unwrap_or_default();

    // Accrual, the default: all of the invoice in July.
    let accrual = tax_liability(&pool, july.0, july.1).await.unwrap();
    assert_eq!(accrual.basis, Basis::Accrual);
    assert_eq!(row(&accrual), (100_000 + 5_000, 20_000, 10_000 + 500));
    assert_eq!(accrual.owed_cents, 10_500);
    assert_eq!(row(&tax_liability(&pool, august.0, august.1).await.unwrap()), (0, 0, 0), "nothing new in August");

    // Cash: in July only the sales receipt; in August half the invoice, when it was paid.
    crate::preferences::set(&pool, "sales_tax_basis", "cash").await.unwrap();
    let cash_july = tax_liability(&pool, july.0, july.1).await.unwrap();
    assert_eq!(cash_july.basis, Basis::Cash);
    assert_eq!(row(&cash_july), (5_000, 0, 500));
    assert_eq!(cash_july.owed_cents, 500, "the unpaid invoice's tax isn't owed yet");
    let cash_august = tax_liability(&pool, august.0, august.1).await.unwrap();
    assert_eq!(row(&cash_august), (50_000, 10_000, 5_000));
    assert_eq!(cash_august.owed_cents, 5_500);
    assert_eq!(tax_liability_on(&pool, august.0, august.1, Basis::Accrual).await.unwrap().owed_cents, 10_500, "the ledger still carries all of it");

    // The rest is paid in September, and the two bases agree over the whole time.
    crate::invoices::record_payment(&pool, pay(NaiveDate::from_ymd_opt(2026, 9, 3).unwrap(), 65_000)).await.unwrap();
    let year = (NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
    let cash_year = tax_liability(&pool, year.0, year.1).await.unwrap();
    let accrual_year = tax_liability_on(&pool, year.0, year.1, Basis::Accrual).await.unwrap();
    assert_eq!(row(&cash_year), row(&accrual_year));
    assert_eq!((cash_year.owed_cents, accrual_year.owed_cents), (10_500, 10_500));
    assert!(crate::preferences::set(&pool, "sales_tax_basis", "when paid").await.unwrap_err().starts_with("PREF_BAD_TAX_BASIS"));
}

/// Preferences › Sales Tax › filing frequency sets the report's period and the return reminder.
#[tokio::test]
async fn filing_frequency_sets_the_period_and_the_reminder() {
    let day = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
    let range = |r: crate::preferences::DateRange| (r.start, r.end);
    let monthly = filing_periods(day(2026, 11, 6), Frequency::Monthly, Basis::Accrual);
    assert_eq!((range(monthly.current), range(monthly.last), monthly.last_due), ((day(2026, 11, 1), day(2026, 11, 30)), (day(2026, 10, 1), day(2026, 10, 31)), day(2026, 11, 20)));
    let quarterly = filing_periods(day(2026, 11, 6), Frequency::Quarterly, Basis::Accrual);
    assert_eq!((range(quarterly.current), range(quarterly.last), quarterly.last_due), ((day(2026, 10, 1), day(2026, 12, 31)), (day(2026, 7, 1), day(2026, 9, 30)), day(2026, 10, 20)));
    let yearly = filing_periods(day(2026, 2, 1), Frequency::Yearly, Basis::Cash);
    assert_eq!((range(yearly.current), range(yearly.last), yearly.last_due), ((day(2026, 1, 1), day(2026, 12, 31)), (day(2025, 1, 1), day(2025, 12, 31)), day(2026, 1, 20)));

    let pool = test_pool().await;
    assert_eq!(company_filing_periods(&pool, day(2026, 11, 6)).await.unwrap().frequency, Frequency::Quarterly, "quarterly by default");
    crate::preferences::set(&pool, "sales_tax_frequency", "monthly").await.unwrap();
    crate::preferences::set(&pool, "sales_tax_enabled", "true").await.unwrap();
    let reminders = crate::reminders::tax_dates(&pool, day(2026, 11, 6), 30).await.unwrap();
    let sales: Vec<_> = reminders.iter().filter(|r| r.label.starts_with("Sales tax return")).map(|r| (r.label.as_str(), r.date.as_deref())).collect();
    assert_eq!(sales, [("Sales tax return for 10/01/2026 – 10/31/2026", Some("2026-11-20"))]);
    crate::preferences::set(&pool, "sales_tax_enabled", "false").await.unwrap();
    assert!(crate::reminders::tax_dates(&pool, day(2026, 11, 6), 30).await.unwrap().iter().all(|r| !r.label.starts_with("Sales tax")));
}

/// Cash basis and credit memos: a memo counts on its own date (as on accrual), and a memo applied to
/// an invoice settles that share of the invoice on the date it is applied, so the two cancel and
/// both bases agree once everything is settled.
#[tokio::test]
async fn cash_basis_counts_a_credit_applied_to_an_invoice_as_settling_it() {
    let pool = test_pool().await;
    let services = account(&pool, "4100").await;
    crate::preferences::set(&pool, "sales_tax_basis", "cash").await.unwrap();
    let rate = create_tax_rate(&pool, TaxRateInput { name: "State 10%".into(), rate_ppm: 100_000, agency_name: None, liability_account_id: None }).await.unwrap();
    let invoice: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
        "customerName": "Acme", "issueDate": "2026-07-01", "dueDate": "2026-07-31", "taxRateId": rate.id,
        "lines": [{ "description": "Widgets", "quantityE4": 10_000, "rateCents": 10_000, "incomeAccountId": services, "isTaxable": true }]
    }))
    .unwrap();
    let inv = crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap();
    // A full credit for the invoice, dated July 5 and applied August 3.
    let memo = crate::credits::create_credit_memo(
        &pool,
        crate::credits::CreditMemoInput { credit_number: None, form_lines: Vec::new(),
            customer_name: "Acme".into(),
            credit_date: d(5),
            memo: None,
            refund_account_id: None,
            tax_rate_id: Some(rate.id),
            lines: vec![crate::sales::SalesLineInput { product_service_id: None, description: "Return".into(), quantity_e4: 10_000, rate_cents: Cents(10_000), income_account_id: services, class_id: None, is_taxable: true, service_date: None }],
            header: Default::default(),
        },
    )
    .await
    .unwrap();
    assert_eq!(memo.total_cents, inv.total_cents);
    let aug = |day| NaiveDate::from_ymd_opt(2026, 8, day).unwrap();
    crate::credits::apply_customer_credit(&pool, crate::credits::ApplyCreditInput { credit_id: memo.id, document_id: inv.id, amount_cents: memo.total_cents, applied_date: aug(3) }).await.unwrap();

    let tax = |r: &TaxLiabilityReport| r.tax_collected_cents;
    assert_eq!(tax(&tax_liability(&pool, d(1), d(31)).await.unwrap()), -1_000, "July: the memo on its date");
    assert_eq!(tax(&tax_liability(&pool, aug(1), aug(31)).await.unwrap()), 1_000, "August: the invoice, settled by the credit");
    let year = tax_liability(&pool, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()).await.unwrap();
    assert_eq!((tax(&year), year.owed_cents), (0, 0), "nothing owed either way");
    assert_eq!(tax_liability_on(&pool, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(), Basis::Accrual).await.unwrap().owed_cents, 0);
}

#[test]
fn a_tax_payment_amount_arrives_as_a_bare_integer() {
    let input: SalesTaxPaymentInput =
        serde_json::from_str(r#"{"accountId":3,"paymentDate":"2025-07-31","amountCents":1237,"reference":null}"#).unwrap();
    assert_eq!(input.amount_cents, Cents::from(1_237));
    assert!(serde_json::from_str::<SalesTaxPaymentInput>(r#"{"accountId":3,"paymentDate":"2025-07-31","amountCents":12.37,"reference":null}"#).is_err());
}

#[tokio::test]
async fn discounts_lower_taxable_sales_on_the_liability_report_and_the_return() {
    let pool = test_pool().await;
    let services = account(&pool, "4100").await;
    let rate = create_tax_rate(&pool, TaxRateInput { name: "State 8%".into(), rate_ppm: 80_000, agency_name: Some("State".into()), liability_account_id: None }).await.unwrap();
    // A $100 taxable + $100 non-taxable invoice with $20 off the whole form: $10 of the discount is
    // off the taxable half, so $90 is taxed ($7.20).
    let invoice: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
        "customerName": "Acme", "issueDate": "2026-07-01", "dueDate": "2026-07-31", "taxRateId": rate.id, "discountCents": 2_000,
        "lines": [
            { "description": "Widgets", "quantityE4": 10_000, "rateCents": 10_000, "incomeAccountId": services, "isTaxable": true },
            { "description": "Labor", "quantityE4": 10_000, "rateCents": 10_000, "incomeAccountId": services, "isTaxable": false }
        ]
    }))
    .unwrap();
    assert_eq!(crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap().total_cents, 18_720);
    // A $100 taxable credit memo with 10% off: $90 taxable, $7.20 tax given back.
    let memo = crate::credits::create_credit_memo(
        &pool,
        crate::credits::CreditMemoInput {
            credit_number: None,
            customer_name: "Acme".into(),
            credit_date: d(3),
            memo: None,
            refund_account_id: None,
            tax_rate_id: Some(rate.id),
            lines: vec![crate::sales::SalesLineInput { product_service_id: None, description: "Return".into(), quantity_e4: 10_000, rate_cents: Cents(10_000), income_account_id: services, class_id: None, is_taxable: true, service_date: None }],
            header: Default::default(),
            form_lines: vec![crate::item_types::FormLineInput::discount(1, services, Some(100_000), None, true)],
        },
    )
    .await
    .unwrap();
    assert_eq!(memo.total_cents, 9_720);

    let report = tax_liability_on(&pool, d(1), d(31), Basis::Accrual).await.unwrap();
    let row = report.rows.iter().find(|r| r.tax_rate_id == Some(rate.id)).unwrap();
    assert_eq!((row.taxable_sales_cents, row.non_taxable_sales_cents, row.tax_collected_cents), (9_000 - 9_000, 9_000, 0), "invoice $90 taxable less the memo's $90; $90 non-taxable");

    let r = combined::sales_tax_return(&pool, d(1), d(31), Basis::Accrual).await.unwrap();
    assert_eq!(r.agencies[0].lines[0].taxable_sales_cents, 0, "the invoice's $90 less the credit memo's $90");
    let memo_only = combined::sales_tax_return(&pool, d(2), d(31), Basis::Accrual).await.unwrap();
    assert_eq!(memo_only.agencies[0].lines[0].taxable_sales_cents, -9_000);
}
