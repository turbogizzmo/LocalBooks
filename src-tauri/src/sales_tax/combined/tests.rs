use super::*;
use crate::db::test_pool;
use crate::ledger::Cents;
use crate::sales_tax::{create_tax_rate, pay_sales_tax, update_tax_rate, SalesTaxPaymentInput, TaxRateInput};

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn new_liability(pool: &SqlitePool, number: &str, name: &str) -> i64 {
    sqlx::query_scalar("INSERT INTO chart_of_accounts (account_number, name, account_type) VALUES (?1, ?2, 'OTHER_CURRENT_LIABILITY') RETURNING id").bind(number).bind(name).fetch_one(pool).await.unwrap()
}

async fn owed(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(credit_cents - debit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(account).fetch_one(pool).await.unwrap()
}

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

fn part(id: i64, ppm: i64) -> Component {
    Component { id, name: format!("r{id}"), rate_ppm: ppm, liability_account_id: id, is_active: true }
}

#[test]
fn components_share_the_combined_tax_to_the_cent() {
    // $10.01 at 6.25% + 1.25% + 1%: the combined 8.5% is 85¢; alone they round to 63 + 13 + 10 = 86¢.
    let total = super::super::tax_on(1_001, 85_000).unwrap();
    let parts = split(total, 1_001, &[part(1, 62_500), part(2, 12_500), part(3, 10_000)]).unwrap();
    assert_eq!(parts.iter().map(|p| p.cents).collect::<Vec<_>>(), vec![62, 13, 10]);
    assert_eq!(parts.iter().map(|p| p.cents).sum::<i64>(), total);
    // Even shares stay even when nothing is left over.
    let even = split(200, 1_000, &[part(1, 100_000), part(2, 100_000)]).unwrap();
    assert_eq!(even.iter().map(|p| p.cents).collect::<Vec<_>>(), vec![100, 100]);
}

#[tokio::test]
async fn a_combined_rate_posts_by_agency_and_files_a_return_per_agency() {
    let pool = test_pool().await;
    let (checking, services) = (account(&pool, "1010").await, account(&pool, "4100").await);
    let state_payable = account(&pool, "2200").await;
    let local_payable = new_liability(&pool, "2210", "Local Sales Tax Payable").await;
    let rate = |name: &str, ppm: i64, agency: &str, account: i64| TaxRateInput { name: name.into(), rate_ppm: ppm, agency_name: Some(agency.into()), liability_account_id: Some(account) };
    let state = create_tax_rate(&pool, rate("State", 60_000, "State Revenue Dept", state_payable)).await.unwrap();
    let county = create_tax_rate(&pool, rate("County", 15_000, "County Treasurer", local_payable)).await.unwrap();
    let city = create_tax_rate(&pool, rate("City", 10_000, "County Treasurer", local_payable)).await.unwrap();

    assert!(save_combined_rate(&pool, None, &CombinedRateInput { name: "Lone".into(), component_ids: vec![state.id] }).await.unwrap_err().starts_with("TAX_COMBINED_PARTS"));
    assert!(save_combined_rate(&pool, None, &CombinedRateInput { name: "Twice".into(), component_ids: vec![state.id, county.id, state.id] }).await.unwrap_err().starts_with("TAX_COMBINED_PARTS"), "a rate used twice, not next to itself");
    let combined = save_combined_rate(&pool, None, &CombinedRateInput { name: "Harbor City 8.5%".into(), component_ids: vec![state.id, county.id, city.id] }).await.unwrap();
    assert_eq!((combined.rate_ppm, combined.is_group, combined.component_ids.clone()), (85_000, true, vec![state.id, county.id, city.id]));
    assert!(save_combined_rate(&pool, None, &CombinedRateInput { name: "Nested".into(), component_ids: vec![combined.id, state.id] }).await.unwrap_err().starts_with("TAX_COMBINED_PARTS"));
    assert!(update_tax_rate(&pool, combined.id, rate("Harbor City 8.5%", 90_000, "x", state_payable)).await.unwrap_err().starts_with("TAX_IS_COMBINED"));

    // A $1,000.00 taxable invoice: $85.00 of tax, posted $60 to the state's account and $25 to the local one.
    let invoice: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
        "customerName": "Brightwater Cafe", "issueDate": "2026-07-01", "dueDate": "2026-07-31", "taxRateId": combined.id,
        "lines": [{ "description": "Catering", "quantityE4": 10_000, "rateCents": 100_000, "incomeAccountId": services, "isTaxable": true }]
    }))
    .unwrap();
    let inv = crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap();
    assert_eq!(inv.total_cents, 108_500);
    assert_eq!((owed(&pool, state_payable).await, owed(&pool, local_payable).await), (6_000, 2_500));

    // The state's rate goes up later: the combined rate follows, the posted invoice doesn't.
    update_tax_rate(&pool, state.id, rate("State", 65_000, "State Revenue Dept", state_payable)).await.unwrap();
    let rates = crate::sales_tax::list_tax_rates(&pool).await.unwrap();
    assert_eq!(rates.iter().find(|r| r.id == combined.id).unwrap().rate_ppm, 90_000);

    pay_sales_tax(&pool, SalesTaxPaymentInput { tax_rate_id: Some(state.id), liability_account_id: None, account_id: checking, payment_date: d(7, 31), amount_cents: Cents::from(6_000), reference: None }).await.unwrap();

    let r = sales_tax_return(&pool, d(7, 1), d(7, 31), Basis::Accrual).await.unwrap();
    let agencies: Vec<(&str, i64, i64)> = r.agencies.iter().map(|a| (a.agency_name.as_str(), a.tax_cents, a.paid_cents)).collect();
    assert_eq!(agencies, vec![("County Treasurer", 2_500, 0), ("State Revenue Dept", 6_000, 6_000)]);
    let county_lines: Vec<(&str, i64, i64)> = r.agencies[0].lines.iter().map(|l| (l.name.as_str(), l.taxable_sales_cents, l.tax_cents)).collect();
    assert_eq!(county_lines, vec![("City", 100_000, 1_000), ("County", 100_000, 1_500)]);
    assert_eq!((r.tax_cents, r.paid_cents), (8_500, 6_000));

    // On cash, half paid in August counts half of each part in August, nothing in July.
    let cash_july = sales_tax_return(&pool, d(7, 1), d(7, 31), Basis::Cash).await.unwrap();
    assert_eq!(cash_july.tax_cents, 0);
    let pay = crate::invoices::PaymentInput { discount_cents: 0, invoice_id: inv.id, deposit_account_id: checking, payment_date: d(8, 10), amount_cents: 54_250, payment_method: "CHECK".into(), reference: None, exchange_rate_e6: None, payment_method_id: None };
    crate::invoices::record_payment(&pool, pay).await.unwrap();
    let cash_aug = sales_tax_return(&pool, d(8, 1), d(8, 31), Basis::Cash).await.unwrap();
    let parts: Vec<(&str, i64)> = cash_aug.agencies.iter().map(|a| (a.agency_name.as_str(), a.tax_cents)).collect();
    assert_eq!(parts, vec![("County Treasurer", 1_250), ("State Revenue Dept", 3_000)]);
    assert_eq!(cash_aug.agencies[1].lines[0].taxable_sales_cents, 50_000);
}

#[tokio::test]
async fn a_credit_memo_at_a_combined_rate_takes_back_each_part() {
    let pool = test_pool().await;
    let services = account(&pool, "4100").await;
    let payable = account(&pool, "2200").await;
    let local = new_liability(&pool, "2210", "Local Sales Tax Payable").await;
    let a = create_tax_rate(&pool, TaxRateInput { name: "State".into(), rate_ppm: 50_000, agency_name: Some("State".into()), liability_account_id: Some(payable) }).await.unwrap();
    let b = create_tax_rate(&pool, TaxRateInput { name: "City".into(), rate_ppm: 20_000, agency_name: Some("City".into()), liability_account_id: Some(local) }).await.unwrap();
    let combined = save_combined_rate(&pool, None, &CombinedRateInput { name: "Both".into(), component_ids: vec![a.id, b.id] }).await.unwrap();
    crate::credits::create_credit_memo(
        &pool,
        crate::credits::CreditMemoInput {
            customer_name: "Acme".into(),
            credit_date: d(7, 3),
            memo: None,
            refund_account_id: None,
            tax_rate_id: Some(combined.id),
            lines: vec![crate::sales::SalesLineInput { product_service_id: None, description: "Return".into(), quantity_e4: 10_000, rate_cents: 10_000, income_account_id: services, class_id: None, is_taxable: true, service_date: None }],
            header: Default::default(),
            form_lines: Vec::new(),
        },
    )
    .await
    .unwrap();
    assert_eq!((owed(&pool, payable).await, owed(&pool, local).await), (-500, -200));
    let r = sales_tax_return(&pool, d(7, 1), d(7, 31), Basis::Accrual).await.unwrap();
    assert_eq!(r.agencies.iter().map(|x| x.tax_cents).collect::<Vec<_>>(), vec![-200, -500]);
    // A combined rate with an inactive component can't be charged.
    crate::sales_tax::set_tax_rate_active(&pool, b.id, false).await.unwrap();
    let mut tx = crate::ledger::begin_write(&pool).await.unwrap();
    let err = crate::sales_tax::document_tax(&mut tx, Some(combined.id), 1_000, "INV").await.unwrap_err();
    assert!(err.starts_with("INV_INACTIVE_TAX") && err.contains("City"), "{err}");
    drop(tx);
}

#[tokio::test]
async fn paying_a_combined_rate_pays_each_part_and_its_agency() {
    let pool = test_pool().await;
    let (checking, services) = (account(&pool, "1010").await, account(&pool, "4100").await);
    let state_payable = account(&pool, "2200").await;
    let city_payable = new_liability(&pool, "2210", "City Sales Tax Payable").await;
    let county_payable = new_liability(&pool, "2220", "County Sales Tax Payable").await;
    let rate = |name: &str, ppm: i64, agency: &str, account: i64| TaxRateInput { name: name.into(), rate_ppm: ppm, agency_name: Some(agency.into()), liability_account_id: Some(account) };
    let state = create_tax_rate(&pool, rate("State", 60_000, "State DOR", state_payable)).await.unwrap();
    let city = create_tax_rate(&pool, rate("City", 15_000, "City Clerk", city_payable)).await.unwrap();
    let county = create_tax_rate(&pool, rate("County", 5_000, "County Treasurer", county_payable)).await.unwrap();
    let combined = save_combined_rate(&pool, None, &CombinedRateInput { name: "State+City+County".into(), component_ids: vec![state.id, city.id, county.id] }).await.unwrap();

    // A $100.00 taxable invoice at 8%: $6.00 state, $1.50 city, $0.50 county.
    let invoice: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
        "customerName": "Brightwater Cafe", "issueDate": "2026-07-01", "dueDate": "2026-07-31", "taxRateId": combined.id,
        "lines": [{ "description": "Catering", "quantityE4": 10_000, "rateCents": 10_000, "incomeAccountId": services, "isTaxable": true }]
    }))
    .unwrap();
    crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap();

    // Paying the combined rate's $8.00 pays each component's account, in one entry.
    let id = pay_sales_tax(&pool, SalesTaxPaymentInput { tax_rate_id: Some(combined.id), liability_account_id: None, account_id: checking, payment_date: d(7, 31), amount_cents: Cents::from(800), reference: Some("1001".into()) }).await.unwrap();
    assert_eq!((owed(&pool, state_payable).await, owed(&pool, city_payable).await, owed(&pool, county_payable).await), (0, 0, 0));
    let entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM sales_tax_payments WHERE id = ?1").bind(id).fetch_one(&pool).await.unwrap();
    let credits: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_lines WHERE journal_entry_id = ?1 AND credit_cents > 0").bind(entry).fetch_one(&pool).await.unwrap();
    assert_eq!(credits, 1, "one bank line for the one check");

    let r = sales_tax_return(&pool, d(7, 1), d(7, 31), Basis::Accrual).await.unwrap();
    let agencies: Vec<(&str, i64, i64)> = r.agencies.iter().map(|a| (a.agency_name.as_str(), a.tax_cents, a.paid_cents)).collect();
    assert_eq!(agencies, vec![("City Clerk", 150, 150), ("County Treasurer", 50, 50), ("State DOR", 600, 600)]);
    assert_eq!((r.tax_cents, r.paid_cents), (800, 800));

    // An amount that doesn't divide evenly still adds up to the cent: $1.00 → 75 + 19 + 6.
    pay_sales_tax(&pool, SalesTaxPaymentInput { tax_rate_id: Some(combined.id), liability_account_id: None, account_id: checking, payment_date: d(8, 1), amount_cents: Cents::from(100), reference: None }).await.unwrap();
    assert_eq!((owed(&pool, state_payable).await, owed(&pool, city_payable).await, owed(&pool, county_payable).await), (-75, -19, -6));

    // Voiding the payment takes it off every agency's return.
    crate::ledger::void_transaction(&pool, entry).await.unwrap();
    let r = sales_tax_return(&pool, d(7, 1), d(7, 31), Basis::Accrual).await.unwrap();
    assert_eq!(r.agencies.iter().map(|a| a.paid_cents).collect::<Vec<_>>(), vec![0, 0, 0]);
}
