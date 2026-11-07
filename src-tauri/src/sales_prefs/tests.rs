use super::*;
use crate::credits::{create_vendor_credit, ApplyCreditInput, VendorCreditInput, VendorCreditLineInput};
use crate::db::test_pool;
use crate::estimates::{convert_to_invoice, create_estimate, invoice_from_estimate, EstimateInput};
use crate::expenses::{post_vendor_bill, BillInput, BillLineInput, BillPaymentInput};
use crate::guards::{self, warnings_in, WARN_OVERDUE_CUSTOMER};
use crate::invoices::{create_and_post_invoice, invoice_detail, InvoiceInput, InvoiceLineInput};
use crate::sales::SalesLineInput;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.expect("seeded account")
}

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

fn open(id: i64, balance_cents: i64) -> Open {
    Open { id, balance_cents }
}

async fn invoice(pool: &SqlitePool, customer: &str, cents: i64, issued: NaiveDate, due: NaiveDate) -> i64 {
    let income = account(pool, "4100").await;
    let input = InvoiceInput {
        customer_name: customer.into(),
        issue_date: issued,
        due_date: due,
        lines: vec![InvoiceLineInput { description: "Work".into(), quantity_e4: 10_000, rate_cents: cents, income_account_id: income, product_service_id: None, class_id: None, is_taxable: false }],
        currency: None,
        tax_rate_id: None,
        discount_cents: 0,
        details: Default::default(),
    };
    create_and_post_invoice(pool, input).await.expect("invoice posts").id
}

async fn bill(pool: &SqlitePool, vendor: &str, cents: i64, date: NaiveDate) -> i64 {
    bill_on_terms(pool, vendor, cents, date, None).await
}

async fn bill_on_terms(pool: &SqlitePool, vendor: &str, cents: i64, date: NaiveDate, terms_id: Option<i64>) -> i64 {
    let expense = account(pool, "6300").await;
    let input = BillInput {
        vendor_name: vendor.into(),
        bill_number: None,
        bill_date: date,
        due_date: date,
        memo: None,
        total_cents: cents,
        terms_id,
        lines: vec![BillLineInput { account_id: expense, amount_cents: cents, memo: None, quantity_e4: None, rate_cents: None, product_service_id: None, class_id: None, customer_id: None, billable: None }],
    };
    post_vendor_bill(pool, input).await.expect("bill posts").id
}

#[test]
fn a_payment_goes_to_an_exact_match_or_else_oldest_first() {
    let invoices = [open(1, 10_000), open(2, 25_000), open(3, 5_000)];
    // Exact: the invoice whose balance matches takes it all, even when it isn't the oldest.
    assert_eq!(apply_payment(&invoices, 5_000, AutoApply::Exact), vec![(3, 5_000)]);
    assert_eq!(apply_payment(&invoices, 25_000, AutoApply::Exact), vec![(2, 25_000)]);
    // No match: oldest first, each paid in full before the next, the last one partly.
    assert_eq!(apply_payment(&invoices, 30_000, AutoApply::Exact), vec![(1, 10_000), (2, 20_000)]);
    assert_eq!(apply_payment(&invoices, 5_000, AutoApply::Oldest), vec![(1, 5_000)]);
    // More than is owed: every invoice is paid and the rest stays unapplied.
    assert_eq!(apply_payment(&invoices, 50_000, AutoApply::Oldest), vec![(1, 10_000), (2, 25_000), (3, 5_000)]);
    assert!(apply_payment(&invoices, 5_000, AutoApply::Off).is_empty(), "off: the person ticks invoices");
    assert!(apply_payment(&invoices, 0, AutoApply::Exact).is_empty());
    assert_eq!(AutoApply::parse("oldest"), AutoApply::Oldest);
    assert_eq!(AutoApply::parse("anything else"), AutoApply::Exact);
}

#[tokio::test]
async fn receive_payments_applies_by_the_company_preference() {
    let pool = test_pool().await;
    let older = invoice(&pool, "Acme", 10_000, d(9, 1), d(9, 30)).await;
    let newer = invoice(&pool, "Acme", 4_000, d(10, 1), d(10, 31)).await;
    invoice(&pool, "Globex", 4_000, d(9, 1), d(9, 30)).await;
    let applied = |id, cents| AppliedAmount { document_id: id, amount_cents: cents };

    assert_eq!(preferences::get(&pool, AUTO_APPLY).await.unwrap(), "exact", "on, with exact matches first, by default");
    assert_eq!(payment_application(&pool, "acme", None, 4_000, None).await.unwrap(), vec![applied(newer, 4_000)]);
    assert_eq!(payment_application(&pool, "Acme", None, 12_000, None).await.unwrap(), vec![applied(older, 10_000), applied(newer, 2_000)]);
    preferences::set(&pool, AUTO_APPLY, "oldest").await.unwrap();
    assert_eq!(payment_application(&pool, "Acme", None, 4_000, None).await.unwrap(), vec![applied(older, 4_000)]);
    preferences::set(&pool, AUTO_APPLY, "off").await.unwrap();
    assert!(payment_application(&pool, "Acme", None, 4_000, None).await.unwrap().is_empty());
    assert_eq!(payment_application(&pool, "Acme", None, 4_000, Some("exact")).await.unwrap(), vec![applied(newer, 4_000)], "the Auto Apply button still applies");
    assert!(payment_application(&pool, "Acme", Some("EUR"), 4_000, None).await.unwrap().is_empty(), "only invoices in the payment's currency");
    assert!(preferences::set(&pool, AUTO_APPLY, "newest").await.unwrap_err().starts_with("PREF_BAD_AUTO_APPLY"));
}

#[tokio::test]
async fn selling_to_a_customer_with_overdue_invoices_warns_when_turned_on() {
    let pool = test_pool().await;
    let today = d(11, 6);
    invoice(&pool, "Acme", 10_000, d(9, 1), d(10, 1)).await;
    invoice(&pool, "Acme", 2_500, d(9, 15), d(10, 15)).await;
    invoice(&pool, "Acme", 9_900, d(11, 1), d(12, 1)).await; // not due yet
    invoice(&pool, "Globex", 5_000, d(11, 1), d(11, 30)).await;

    assert_eq!(guards::overdue_customer_warning(&pool, "Acme", today).await.unwrap(), None, "off by default");
    preferences::set(&pool, WARN_OVERDUE_CUSTOMER, "true").await.unwrap();
    let warning = guards::overdue_customer_warning(&pool, "acme", today).await.unwrap().expect("Acme is overdue");
    assert_eq!(warning.code, "CUSTOMER_OVERDUE");
    assert!(warning.message.contains("2 overdue invoices") && warning.message.contains("$125.00") && warning.message.contains("10/01/2026"), "{}", warning.message);
    assert_eq!(guards::overdue_customer_warning(&pool, "Globex", today).await.unwrap(), None, "nothing past due");
    assert_eq!(guards::overdue_customer_warning(&pool, "New Customer", today).await.unwrap(), None);
    // A job counts toward its customer, and the customer's invoices toward its jobs.
    assert!(guards::overdue_customer_warning(&pool, "Acme:Warehouse", today).await.unwrap().is_none(), "no such job yet");
    invoice(&pool, "Globex:Roof", 700, d(9, 1), d(9, 2)).await;
    assert!(guards::overdue_customer_warning(&pool, "Globex", today).await.unwrap().is_some());

    // Through the guards: a new sale asks, Save anyway goes ahead; an estimate is not a sale.
    let err = guards::confirm(guards::warnings_on(&pool, &guards::sale(today, "Acme"), today).await.unwrap(), None).unwrap_err();
    assert_eq!(warnings_in(&err).unwrap()[0].code, "CUSTOMER_OVERDUE");
    assert!(guards::confirm(guards::warnings_on(&pool, &guards::sale(today, "Acme"), today).await.unwrap(), Some(&["CUSTOMER_OVERDUE".to_string()])).is_ok());
    assert!(guards::warnings_on(&pool, &guards::dated(today), today).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_new_bill_is_due_after_the_vendors_terms_or_the_company_days() {
    let pool = test_pool().await;
    assert_eq!(preferences::get(&pool, BILL_DUE_DAYS).await.unwrap(), "10");
    assert_eq!(bill_due_date(&pool, "Not Yet A Vendor", d(10, 1)).await.unwrap(), d(10, 11));
    bill(&pool, "Harbor Hardware", 1_000, d(10, 1)).await;
    assert_eq!(bill_due_date(&pool, "Harbor Hardware", d(10, 25)).await.unwrap(), d(11, 4), "no terms: the company's 10 days");
    sqlx::query("UPDATE vendors SET terms_days = 30 WHERE name = 'Harbor Hardware'").execute(&pool).await.unwrap();
    assert_eq!(bill_due_date(&pool, "harbor hardware", d(10, 1)).await.unwrap(), d(10, 31), "the vendor's terms win");
    sqlx::query("UPDATE vendors SET terms_id = (SELECT id FROM terms WHERE name = 'Net 15') WHERE name = 'Harbor Hardware'").execute(&pool).await.unwrap();
    assert_eq!(bill_due_date(&pool, "Harbor Hardware", d(10, 1)).await.unwrap(), d(10, 16), "a term from the Terms list comes first");
    preferences::set(&pool, BILL_DUE_DAYS, "0").await.unwrap();
    assert_eq!(bill_due_date(&pool, "Someone Else", d(10, 1)).await.unwrap(), d(10, 1), "0 is due on receipt");
    for bad in ["-1", "366", "soon"] {
        assert!(preferences::set(&pool, BILL_DUE_DAYS, bad).await.unwrap_err().starts_with("PREF_BAD_DUE_DAYS"), "{bad}");
    }
}

#[test]
fn vendor_credits_cover_the_chosen_bills_oldest_credit_first() {
    let credits = [open(10, 3_000), open(11, 5_000)];
    let uses = plan_vendor_credits(&credits, &[open(1, 2_000), open(2, 4_000), open(3, 9_000)]);
    let used = |c, b, a| CreditUse { credit_id: c, bill_id: b, amount_cents: a };
    assert_eq!(uses, vec![used(10, 1, 2_000), used(10, 2, 1_000), used(11, 2, 3_000), used(11, 3, 2_000)]);
    assert!(plan_vendor_credits(&[], &[open(1, 2_000)]).is_empty());
}

#[tokio::test]
async fn pay_bills_uses_vendor_credits_when_turned_on() {
    let pool = test_pool().await;
    let (expense, bank) = (account(&pool, "6300").await, account(&pool, "1010").await);
    let first = bill(&pool, "Harbor Hardware", 6_000, d(10, 1)).await;
    let second = bill(&pool, "Harbor Hardware", 3_000, d(10, 5)).await;
    let other = bill(&pool, "Paper Co", 1_000, d(10, 5)).await;
    let credit = create_vendor_credit(
        &pool,
        VendorCreditInput {
            vendor_name: "Harbor Hardware".into(),
            reference: Some("RMA-7".into()),
            credit_date: d(10, 2),
            memo: None,
            lines: vec![VendorCreditLineInput { account_id: expense, amount_cents: 7_000, memo: None, class_id: None, customer_id: None }],
        },
    )
    .await
    .unwrap();

    assert_eq!(pay_bills_plan(&pool, &[first, second], d(10, 9)).await.unwrap(), PayBillsPlan::default(), "off by default");
    preferences::set(&pool, BILL_AUTO_CREDITS, "true").await.unwrap();
    let uses = pay_bills_plan(&pool, &[first, second, other], d(10, 9)).await.unwrap().credits;
    assert_eq!(
        uses,
        vec![CreditUse { credit_id: credit.id, bill_id: first, amount_cents: 6_000 }, CreditUse { credit_id: credit.id, bill_id: second, amount_cents: 1_000 }]
    );

    let credits: Vec<ApplyCreditInput> = uses.iter().map(|u| ApplyCreditInput { credit_id: u.credit_id, document_id: u.bill_id, amount_cents: u.amount_cents, applied_date: d(10, 9) }).collect();
    let pay = |bill_id, amount_cents| BillPaymentInput { bill_id, payment_account_id: bank, payment_date: d(10, 9), amount_cents, reference: None };
    // The first bill is paid off by the credit alone; the second needs 20.00 more.
    let too_much = pay_bills_with_credits(&pool, vec![pay(second, 3_000)], credits.clone(), Vec::new(), d(10, 9)).await.unwrap_err();
    assert!(too_much.starts_with("BILL_OVERPAYMENT"), "{too_much}");
    assert_eq!(pay_bills_with_credits(&pool, vec![pay(second, 2_000), pay(other, 1_000)], credits, Vec::new(), d(10, 9)).await.unwrap(), 3);
    let status: Vec<String> = sqlx::query_scalar("SELECT status FROM bills ORDER BY id").fetch_all(&pool).await.unwrap();
    assert_eq!(status, ["PAID", "PAID", "PAID"]);
    let left: i64 = sqlx::query_scalar("SELECT remaining_cents FROM v_vendor_credit_balances WHERE vendor_credit_id = ?1").bind(credit.id).fetch_one(&pool).await.unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
#[ignore = "waits for #151: estimate_lines is rebuilt to allow zero-amount lines on top of its migration 0067"]
async fn zero_amount_estimate_lines_stay_off_the_invoice() {
    let pool = test_pool().await;
    let income = account(&pool, "4100").await;
    let line = |what: &str, rate: i64| SalesLineInput { product_service_id: None, description: what.into(), quantity_e4: 10_000, rate_cents: rate, income_account_id: income, class_id: None, is_taxable: false };
    let estimate = |lines| EstimateInput { customer_name: "Initech".into(), estimate_date: d(10, 1), expiration_date: None, memo: None, lines, tax_rate_id: None, discount_cents: 0, details: Default::default() };
    let est = create_estimate(&pool, estimate(vec![line("Design", 50_000), line("Site visit (included)", 0), line("Build", 25_000)])).await.expect("a zero line is allowed");
    assert_eq!(est.total_cents, 75_000);
    assert!(create_estimate(&pool, estimate(vec![line("Nothing", 0)])).await.unwrap_err().starts_with("EST_AMOUNT_TOO_LARGE"), "the estimate itself needs an amount");

    assert_eq!(preferences::get(&pool, DROP_ZERO_LINES).await.unwrap(), "true");
    let converted = convert_to_invoice(&pool, est.id, d(10, 5), d(11, 4)).await.unwrap();
    let items = invoice_detail(&pool, converted.converted_to_invoice_id.unwrap()).await.unwrap().items;
    assert_eq!(items.iter().map(|i| i.description.as_str()).collect::<Vec<_>>(), ["Design", "Build"]);

    // Turned off, the zero line comes over and must be priced: the one-step conversion explains.
    preferences::set(&pool, DROP_ZERO_LINES, "false").await.unwrap();
    let second = create_estimate(&pool, estimate(vec![line("Design", 50_000), line("Extra", 0)])).await.unwrap();
    assert!(convert_to_invoice(&pool, second.id, d(10, 5), d(11, 4)).await.unwrap_err().starts_with("EST_ZERO_LINE: line 2"));

    // Through the invoice form: with the preference on, a zero line sent along is left off.
    preferences::set(&pool, DROP_ZERO_LINES, "true").await.unwrap();
    let form = InvoiceInput {
        customer_name: "Initech".into(),
        issue_date: d(10, 6),
        due_date: d(11, 5),
        lines: [("Design", 50_000), ("Extra", 0)]
            .into_iter()
            .map(|(what, rate)| InvoiceLineInput { description: what.into(), quantity_e4: 10_000, rate_cents: rate, income_account_id: income, product_service_id: None, class_id: None, is_taxable: false })
            .collect(),
        currency: None,
        tax_rate_id: None,
        discount_cents: 0,
        details: Default::default(),
    };
    let inv = invoice_from_estimate(&pool, second.id, form).await.unwrap();
    assert_eq!(inv.item_count, 1);

    assert_eq!(without_zero_lines(vec![0, 0], true, |v| *v), [0, 0], "never an empty invoice");
    assert_eq!(without_zero_lines(vec![5, 0], false, |v| *v), [5, 0]);
}

#[tokio::test]
async fn batch_four_preferences_validate() {
    let pool = test_pool().await;
    assert_eq!(preferences::set(&pool, DEFAULT_MARKUP, "12.50%").await.unwrap(), "12.5");
    assert_eq!(preferences::set(&pool, DEFAULT_MARKUP, "").await.unwrap(), "0");
    assert!(preferences::set(&pool, DEFAULT_MARKUP, "-5").await.unwrap_err().starts_with("PREF_BAD_MARKUP"));
    assert_eq!(preferences::set(&pool, DEFAULT_FOB, "  Our dock ").await.unwrap(), "Our dock");
    assert!(preferences::set(&pool, DEFAULT_FOB, &"x".repeat(31)).await.is_err());
    assert!(preferences::set(&pool, DEFAULT_SHIP_METHOD, "999").await.unwrap_err().starts_with("PREF_BAD_SHIP_METHOD"));
    let ups = crate::sales_lists::create_ship_method(&pool, "UPS").await.unwrap();
    assert_eq!(preferences::set(&pool, DEFAULT_SHIP_METHOD, &ups.id.to_string()).await.unwrap(), ups.id.to_string());
    assert_eq!(preferences::set(&pool, DEFAULT_SHIP_METHOD, "").await.unwrap(), "");
    assert_eq!(preferences::set(&pool, "job_status_awarded", " Won ").await.unwrap(), "Won");
    assert!(preferences::set(&pool, "job_status_closed", "").await.unwrap_err().starts_with("PREF_REQUIRED"));
    assert_eq!(job_status_labels(&pool).await.unwrap()["AWARDED"], "Won");
    assert_eq!(job_status_labels(&pool).await.unwrap()["NOT_AWARDED"], "Not awarded");
    assert!(preferences::set(&pool, DEFAULT_DELIVERY, "fax").await.is_err());
    for key in [PRICE_LEVELS, ESTIMATES, AUTO_CALCULATE, UNDEPOSITED_DEFAULT] {
        assert_eq!(preferences::get(&pool, key).await.unwrap(), "true", "{key}");
        assert!(preferences::set(&pool, key, "yes").await.is_err(), "{key}");
    }
    assert_eq!(format_percent_milli(40_125), "40.125");
}

#[tokio::test]
async fn new_customers_take_the_send_forms_default_and_jobs_keep_a_status() {
    let pool = test_pool().await;
    let before = crate::sales::create_customer(&pool, "Before", None).await.unwrap();
    assert_eq!(before.preferred_delivery, None);
    preferences::set(&pool, DEFAULT_DELIVERY, "email").await.unwrap();
    let after = crate::sales::create_customer(&pool, "After", None).await.unwrap();
    assert_eq!(after.preferred_delivery.as_deref(), Some("EMAIL"));
    invoice(&pool, "Typed On A Form", 1_000, d(10, 1), d(10, 31)).await;
    let typed: Option<String> = sqlx::query_scalar("SELECT preferred_delivery FROM customers WHERE name = 'Typed On A Form'").fetch_one(&pool).await.unwrap();
    assert_eq!(typed.as_deref(), Some("EMAIL"), "however a customer is added");

    let access = crate::tax_ids::Access::OPEN;
    let job = crate::sales::create_customer(&pool, "After:Kitchen", None).await.unwrap();
    let input = |status: &str, delivery: &str| crate::parties::PartyDetailsInput {
        name: "After:Kitchen".into(),
        company_name: None,
        email: None,
        phone: None,
        address: None,
        terms_days: None,
        terms_id: None,
        customer_type_id: None,
        job_type_id: None,
        vendor_type_id: None,
        notes: None,
        tax_id: None,
        keep_tax_id: true,
        is_1099: false,
        tax_rate_id: None,
        is_tax_exempt: false,
        tax_exemption: None,
        form_style_id: None,
        sales_rep_id: None,
        price_level_id: None,
        preferred_delivery: Some(delivery.into()),
        job_status: Some(status.into()),
        custom_fields: Vec::new(),
        ..Default::default()
    };
    let saved = crate::parties::update_party(&pool, "customer", job.id, input("AWARDED", "PRINT"), access).await.unwrap();
    assert_eq!((saved.job_status.as_deref(), saved.preferred_delivery.as_deref()), (Some("AWARDED"), Some("PRINT")));
    assert!(crate::parties::update_party(&pool, "customer", job.id, input("WON", "PRINT"), access).await.unwrap_err().starts_with("PARTY_BAD_JOB_STATUS"));
    assert!(crate::parties::update_party(&pool, "customer", job.id, input("", "FAX"), access).await.unwrap_err().starts_with("PARTY_BAD_DELIVERY"));
}

#[tokio::test]
async fn pay_bills_takes_early_payment_discounts_through_their_last_day() {
    let pool = test_pool().await;
    let (expense, bank) = (account(&pool, "6300").await, account(&pool, "1010").await);
    let terms: i64 = sqlx::query_scalar("INSERT INTO terms (name, net_days, discount_ppm, discount_days) VALUES ('2% 10 Net 30', 30, 20000, 10) RETURNING id")
        .fetch_one(&pool)
        .await
        .unwrap();
    let on_terms = bill_on_terms(&pool, "Harbor Hardware", 50_000, d(10, 1), Some(terms)).await;
    let plain = bill(&pool, "Harbor Hardware", 10_000, d(10, 1)).await;

    preferences::set(&pool, BILL_AUTO_DISCOUNTS, "true").await.unwrap();
    assert!(pay_bills_plan(&pool, &[on_terms], d(10, 5)).await.unwrap().discounts.is_empty(), "no discount account yet");
    assert!(preferences::set(&pool, BILL_DISCOUNT_ACCOUNT, &bank.to_string()).await.unwrap_err().starts_with("PREF_BAD_DISCOUNT_ACCOUNT"), "not a bank account");
    preferences::set(&pool, BILL_DISCOUNT_ACCOUNT, &expense.to_string()).await.unwrap();

    let plan = pay_bills_plan(&pool, &[on_terms, plain], d(10, 11)).await.unwrap();
    assert_eq!(plan.discounts, vec![DiscountUse { bill_id: on_terms, amount_cents: 1_000 }], "2% of 500.00, on the 10th day; none on a bill without a discount");
    assert!(pay_bills_plan(&pool, &[on_terms], d(10, 12)).await.unwrap().discounts.is_empty(), "the discount has ended");

    let pay = |bill_id, amount_cents| BillPaymentInput { bill_id, payment_account_id: bank, payment_date: d(10, 11), amount_cents, reference: None };
    let late = pay_bills_with_credits(&pool, vec![pay(on_terms, 49_000)], Vec::new(), plan.discounts.clone(), d(10, 12)).await.unwrap_err();
    assert!(late.starts_with("BILL_NO_DISCOUNT"), "{late}");
    assert_eq!(pay_bills_with_credits(&pool, vec![pay(on_terms, 49_000)], Vec::new(), plan.discounts, d(10, 11)).await.unwrap(), 1);
    let status: String = sqlx::query_scalar("SELECT status FROM bills WHERE id = ?1").bind(on_terms).fetch_one(&pool).await.unwrap();
    assert_eq!(status, "PAID");
    let (reference, total): (Option<String>, i64) = sqlx::query_as("SELECT reference, total_cents FROM vendor_credits").fetch_one(&pool).await.unwrap();
    assert_eq!((reference.as_deref(), total), (Some("Discount"), 1_000), "the discount is posted as a vendor credit to the discount account");
    assert!(pay_bills_plan(&pool, &[on_terms], d(10, 11)).await.unwrap().discounts.is_empty(), "never twice");
}
