use super::*;
use crate::db::test_pool;
use crate::invoices::{create_and_post_invoice, invoice_detail, InvoiceInput, InvoiceLineInput};
use crate::sales::{post_sales_receipt, SalesLineInput, SalesReceiptInput};
use crate::sales_lists::SalesDetails;
use chrono::NaiveDate;
use sqlx::SqlitePool;

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).expect("valid date")
}

fn item(amount_cents: i64, is_taxable: bool) -> ItemAmount {
    ItemAmount { amount_cents, is_taxable }
}

async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.expect("seeded account")
}

fn line(account: i64, rate_cents: i64, is_taxable: bool) -> InvoiceLineInput {
    InvoiceLineInput { description: "Work".into(), quantity_e4: 10_000, rate_cents, income_account_id: account, product_service_id: None, class_id: None, is_taxable }
}

fn with_form(form_lines: Vec<FormLineInput>) -> SalesDetails {
    SalesDetails { form_lines, ..Default::default() }
}

fn invoice(lines: Vec<InvoiceLineInput>, form_lines: Vec<FormLineInput>, tax_rate_id: Option<i64>, discount_cents: i64) -> InvoiceInput {
    InvoiceInput {
        customer_name: "Acme".into(),
        issue_date: d(1),
        due_date: d(31),
        lines,
        currency: None,
        tax_rate_id,
        discount_cents,
        details: with_form(form_lines),
    }
}

/// The entry balances, and its debit to `control` (A/R or the deposit account) is the total.
async fn assert_posts(pool: &SqlitePool, entry_id: i64, control: i64, total: i64) {
    let (debits, credits): (i64, i64) =
        sqlx::query_as("SELECT SUM(debit_cents), SUM(credit_cents) FROM journal_lines WHERE journal_entry_id = ?1").bind(entry_id).fetch_one(pool).await.unwrap();
    assert_eq!(debits, credits, "entry {entry_id} balances");
    let to_control: i64 = sqlx::query_scalar("SELECT SUM(debit_cents) - SUM(credit_cents) FROM journal_lines WHERE journal_entry_id = ?1 AND account_id = ?2")
        .bind(entry_id)
        .bind(control)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(to_control, total, "entry {entry_id} debits the document total");
}

// ------------------------------------------------------------------------------------------------
// Layout
// ------------------------------------------------------------------------------------------------

#[test]
fn subtotals_sum_back_to_the_previous_subtotal() {
    let items = [item(1_000, true), item(2_000, false), item(500, true)];
    let form = [FormLineInput::subtotal(2), FormLineInput::subtotal(3)];
    let layout = lay_out(&items, &form, "INV").unwrap();
    assert_eq!(
        layout.rows,
        vec![
            (1, Laid::Item(0)),
            (2, Laid::Item(1)),
            (3, Laid::Form { index: 0, amount_cents: 3_000 }),
            (4, Laid::Item(2)),
            (5, Laid::Form { index: 1, amount_cents: 500 }),
        ]
    );
    assert_eq!((layout.lines_cents, layout.taxable_cents), (3_500, 1_500));
    assert_eq!(layout.item_numbers(), vec![1, 2, 4]);
}

#[test]
fn a_subtotal_at_the_top_or_twice_in_a_row_is_zero() {
    let layout = lay_out(&[item(700, false)], &[FormLineInput::subtotal(0), FormLineInput::subtotal(1), FormLineInput::subtotal(1)], "INV").unwrap();
    let amounts: Vec<i64> = layout.form_rows().map(|(_, _, a)| a).collect();
    assert_eq!(amounts, vec![0, 700, 0]);
}

#[test]
fn a_form_line_past_the_last_item_is_refused() {
    let err = lay_out(&[item(700, false)], &[FormLineInput::subtotal(2)], "INV").unwrap_err();
    assert!(err.starts_with("INV_BAD_FORM_LINE"), "{err}");
}

// ------------------------------------------------------------------------------------------------
// Posting
// ------------------------------------------------------------------------------------------------

#[tokio::test]
async fn an_invoice_with_subtotals_posts_balanced_and_reads_back_in_order() {
    let pool = test_pool().await;
    let (ar, services) = (account_id(&pool, "1200").await, account_id(&pool, "4100").await);
    let input = invoice(
        vec![line(services, 10_000, false), line(services, 5_000, false), line(services, 2_500, false)],
        vec![FormLineInput::subtotal(2), FormLineInput { description: "Parts subtotal".into(), ..FormLineInput::subtotal(3) }],
        None,
        1_000,
    );
    let inv = create_and_post_invoice(&pool, input).await.unwrap();
    assert_eq!(inv.total_cents, 16_500);
    assert_posts(&pool, inv.journal_entry_id.unwrap(), ar, 16_500).await;

    let detail = invoice_detail(&pool, inv.id).await.unwrap();
    assert_eq!(detail.items.iter().map(|i| i.line_number).collect::<Vec<_>>(), vec![1, 2, 4]);
    let form: Vec<(i64, &str, i64)> = detail.form_lines.iter().map(|f| (f.line_number, f.description.as_str(), f.amount_cents)).collect();
    assert_eq!(form, vec![(3, "Subtotal", 15_000), (5, "Parts subtotal", 2_500)]);
}

#[tokio::test]
async fn a_sales_receipt_with_a_subtotal_posts_balanced() {
    let pool = test_pool().await;
    let (checking, services) = (account_id(&pool, "1010").await, account_id(&pool, "4100").await);
    let sale = |rate_cents| SalesLineInput { product_service_id: None, description: "Visit".into(), quantity_e4: 20_000, rate_cents, income_account_id: services, class_id: None, is_taxable: false };
    let receipt = post_sales_receipt(
        &pool,
        SalesReceiptInput {
            customer_name: None,
            receipt_date: d(2),
            deposit_account_id: checking,
            payment_method: "CASH".into(),
            reference: None,
            memo: None,
            lines: vec![sale(4_000), sale(1_000)],
            tax_rate_id: None,
            discount_cents: 0,
            details: with_form(vec![FormLineInput::subtotal(2)]),
        },
    )
    .await
    .unwrap();
    assert_eq!(receipt.total_cents, 10_000);
    assert_posts(&pool, receipt.journal_entry_id.unwrap(), checking, 10_000).await;
    let stored: Vec<(i64, i64)> = sqlx::query_as("SELECT line_number, amount_cents FROM sales_receipt_form_lines WHERE sales_receipt_id = ?1")
        .bind(receipt.id)
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(stored, vec![(3, 10_000)]);
}

#[tokio::test]
async fn an_estimate_carries_its_subtotals_to_its_invoice() {
    let pool = test_pool().await;
    let (ar, services) = (account_id(&pool, "1200").await, account_id(&pool, "4100").await);
    let est_line = |rate_cents| SalesLineInput { product_service_id: None, description: "Phase".into(), quantity_e4: 10_000, rate_cents, income_account_id: services, class_id: None, is_taxable: false };
    let estimate = crate::estimates::create_estimate(
        &pool,
        crate::estimates::EstimateInput {
            customer_name: "Initech".into(),
            estimate_date: d(1),
            expiration_date: None,
            memo: None,
            lines: vec![est_line(30_000), est_line(20_000)],
            tax_rate_id: None,
            discount_cents: 0,
            details: with_form(vec![FormLineInput::subtotal(1), FormLineInput::subtotal(2)]),
        },
    )
    .await
    .unwrap();
    assert_eq!(estimate.total_cents, 50_000);
    let converted = crate::estimates::convert_to_invoice(&pool, estimate.id, d(5), d(30)).await.unwrap();
    let detail = invoice_detail(&pool, converted.converted_to_invoice_id.unwrap()).await.unwrap();
    assert_eq!(detail.form_lines.iter().map(|f| (f.line_number, f.amount_cents)).collect::<Vec<_>>(), vec![(2, 30_000), (4, 20_000)]);
    assert_posts(&pool, detail.invoice.journal_entry_id.unwrap(), ar, 50_000).await;
}

#[tokio::test]
async fn posted_form_lines_cannot_be_changed() {
    let pool = test_pool().await;
    let services = account_id(&pool, "4100").await;
    let inv = create_and_post_invoice(&pool, invoice(vec![line(services, 1_000, false)], vec![FormLineInput::subtotal(1)], None, 0)).await.unwrap();
    let err = sqlx::query("UPDATE invoice_form_lines SET amount_cents = 5 WHERE invoice_id = ?1").bind(inv.id).execute(&pool).await.unwrap_err();
    assert!(err.to_string().contains("INV_IMMUTABLE"), "{err}");
    let err = sqlx::query("DELETE FROM invoice_form_lines WHERE invoice_id = ?1").bind(inv.id).execute(&pool).await.unwrap_err();
    assert!(err.to_string().contains("INV_IMMUTABLE"), "{err}");
}

// ------------------------------------------------------------------------------------------------
// The same cases through the Rust layout and the form's (`src/lib/itemTypes.js`)
// ------------------------------------------------------------------------------------------------

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Case {
    name: String,
    items: Vec<(i64, bool)>,
    form: Vec<FormLineInput>,
    #[serde(default)]
    rows: Vec<serde_json::Value>,
    #[serde(default)]
    lines_cents: i64,
    #[serde(default)]
    taxable_cents: i64,
    #[serde(default)]
    error: Option<String>,
}

fn cases() -> (String, Vec<Case>) {
    let text = include_str!("cases.json").to_string();
    let cases = serde_json::from_str(&text).expect("cases.json reads");
    (text, cases)
}

fn rows_json(layout: &Layout) -> Vec<serde_json::Value> {
    layout
        .rows
        .iter()
        .map(|(n, laid)| match laid {
            Laid::Item(i) => serde_json::json!([n, "I", i]),
            Laid::Form { index, amount_cents } => serde_json::json!([n, "F", index, amount_cents]),
        })
        .collect()
}

#[test]
fn the_shared_cases_lay_out_as_expected() {
    for case in cases().1 {
        let items: Vec<ItemAmount> = case.items.iter().map(|(a, t)| item(*a, *t)).collect();
        match (lay_out(&items, &case.form, "T"), &case.error) {
            (Ok(layout), None) => {
                assert_eq!(rows_json(&layout), case.rows, "{}", case.name);
                assert_eq!((layout.lines_cents, layout.taxable_cents), (case.lines_cents, case.taxable_cents), "{}", case.name);
            }
            (Err(err), Some(code)) => assert!(err.starts_with(&format!("T_{code}")), "{}: {err}", case.name),
            (other, _) => panic!("{}: unexpected {other:?}", case.name),
        }
    }
}

/// The form computes the same layout. Runs where Node.js is installed (CI and development).
#[test]
fn the_form_lays_out_the_shared_cases_the_same_way() {
    let Ok(version) = std::process::Command::new("node").arg("--version").output() else {
        eprintln!("node is not installed; skipping the form's layout check");
        return;
    };
    assert!(version.status.success());
    let module = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/lib/itemTypes.js");
    let script = String::from(
        "import { pathToFileURL } from 'node:url';
         const { layOut } = await import(pathToFileURL(process.argv[2]).href);
         const cases = JSON.parse(process.argv[1]);
         const out = cases.map((c) => {
           const r = layOut(c.items.map(([amountCents, isTaxable]) => ({ amountCents, isTaxable })), c.form);
           if (r.error) return { error: r.error };
           return { rows: r.rows.map((x) => x.item !== undefined ? [x.number, 'I', x.item] : [x.number, 'F', x.form, x.amountCents]), linesCents: r.linesCents, taxableCents: r.taxableCents };
         });
         console.log(JSON.stringify(out));"
    );
    let (text, cases) = cases();
    let output = std::process::Command::new("node").args(["--input-type=module", "-e", &script, &text, &module.to_string_lossy()]).output().expect("run node");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let results: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).expect("node prints JSON");
    for (case, got) in cases.iter().zip(results) {
        match &case.error {
            Some(code) => assert_eq!(got["error"], serde_json::json!(code), "{}", case.name),
            None => {
                assert_eq!(got["rows"], serde_json::json!(case.rows), "{}", case.name);
                assert_eq!(got["linesCents"], case.lines_cents, "{}", case.name);
                assert_eq!(got["taxableCents"], case.taxable_cents, "{}", case.name);
            }
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Discounts
// ------------------------------------------------------------------------------------------------

async fn discounts_given(pool: &SqlitePool) -> i64 {
    // The account the document discount uses, made on first use.
    match sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = 'Discounts Given'").fetch_optional(pool).await.unwrap() {
        Some(id) => id,
        None => sqlx::query_scalar("INSERT INTO chart_of_accounts (name, account_type) VALUES ('Discounts Given', 'INCOME') RETURNING id").fetch_one(pool).await.unwrap(),
    }
}

#[tokio::test]
async fn an_invoice_with_discounts_and_tax_posts_balanced_and_taxes_the_discounted_amount() {
    let pool = test_pool().await;
    let (ar, services) = (account_id(&pool, "1200").await, account_id(&pool, "4100").await);
    let discounts = discounts_given(&pool).await;
    let rate = crate::sales_tax::create_tax_rate(
        &pool,
        crate::sales_tax::TaxRateInput { name: "State".into(), rate_ppm: 80_000, agency_name: None, liability_account_id: None },
    )
    .await
    .unwrap();
    let item = crate::sales::create_item(
        &pool,
        crate::sales::ProductInput { name: "Loyalty 10%".into(), description: None, rate_cents: 0, income_account_id: discounts, is_taxable: true },
        crate::sales::ItemKind::discount(Some(100_000)),
    )
    .await
    .unwrap();
    assert_eq!((item.item_type.as_str(), item.percent_ppm), ("DISCOUNT", Some(100_000)));

    // 100.00 taxable less 10% (taxable), 50.00 untaxed, subtotal, 5.00 off it (not taxable),
    // then a 2.00 document discount.
    let input = invoice(
        vec![line(services, 10_000, true), line(services, 5_000, false)],
        vec![
            FormLineInput { product_service_id: Some(item.id), description: "Loyalty".into(), ..FormLineInput::discount(1, discounts, Some(100_000), None, true) },
            FormLineInput::subtotal(2),
            FormLineInput::discount(2, discounts, None, Some(500), false),
        ],
        Some(rate.id),
        200,
    );
    let inv = create_and_post_invoice(&pool, input).await.unwrap();
    // Lines 150.00 − 10.00 − 5.00 = 135.00; taxable 90.00 less its share of the 2.00 discount
    // (90/135 × 2.00 = 1.33) = 88.67; tax 8% = 7.09; total 135.00 − 2.00 + 7.09 = 140.09.
    assert_eq!(inv.tax_cents, 709);
    assert_eq!(inv.total_cents, 14_009);
    assert_posts(&pool, inv.journal_entry_id.unwrap(), ar, 14_009).await;
    let to_discounts: i64 = sqlx::query_scalar("SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = ?1 AND account_id = ?2")
        .bind(inv.journal_entry_id)
        .bind(discounts)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(to_discounts, 1_000 + 500 + 200);

    let form: Vec<(i64, String, i64, Option<i64>)> =
        invoice_detail(&pool, inv.id).await.unwrap().form_lines.into_iter().map(|f| (f.line_number, f.kind, f.amount_cents, f.percent_ppm)).collect();
    assert_eq!(form, vec![(2, "DISCOUNT".into(), -1_000, Some(100_000)), (4, "SUBTOTAL".into(), 14_000, None), (5, "DISCOUNT".into(), -500, None)]);

    // The discount item is its own row in Sales by Product/Service.
    let report = crate::report_center::sales_by_product(&pool, d(1), d(31)).await.unwrap();
    let row = report.rows.iter().find(|r| r.label == "Loyalty 10%").expect("discount row");
    assert_eq!(row.amount_cents, -1_000);
}

#[tokio::test]
async fn a_sales_receipt_with_a_discount_posts_balanced() {
    let pool = test_pool().await;
    let (checking, services) = (account_id(&pool, "1010").await, account_id(&pool, "4100").await);
    let discounts = discounts_given(&pool).await;
    let sale = SalesLineInput { product_service_id: None, description: "Visit".into(), quantity_e4: 10_000, rate_cents: 8_000, income_account_id: services, class_id: None, is_taxable: false };
    let receipt = post_sales_receipt(
        &pool,
        SalesReceiptInput {
            customer_name: None,
            receipt_date: d(2),
            deposit_account_id: checking,
            payment_method: "CASH".into(),
            reference: None,
            memo: None,
            lines: vec![sale],
            tax_rate_id: None,
            discount_cents: 0,
            details: with_form(vec![FormLineInput::discount(1, discounts, Some(250_000), None, false)]),
        },
    )
    .await
    .unwrap();
    assert_eq!(receipt.total_cents, 6_000);
    assert_posts(&pool, receipt.journal_entry_id.unwrap(), checking, 6_000).await;
}

#[tokio::test]
async fn a_discount_must_post_to_an_active_income_account_from_a_discount_item() {
    let pool = test_pool().await;
    let (checking, services) = (account_id(&pool, "1010").await, account_id(&pool, "4100").await);
    let err = create_and_post_invoice(&pool, invoice(vec![line(services, 1_000, false)], vec![FormLineInput::discount(1, checking, None, Some(100), false)], None, 0))
        .await
        .unwrap_err();
    assert!(err.starts_with("INV_NOT_INCOME_ACCOUNT"), "{err}");
    let plain = crate::sales::create_product(
        &pool,
        crate::sales::ProductInput { name: "Hour".into(), description: None, rate_cents: 100, income_account_id: services, is_taxable: false },
    )
    .await
    .unwrap();
    let discounts = discounts_given(&pool).await;
    let err = create_and_post_invoice(
        &pool,
        invoice(vec![line(services, 1_000, false)], vec![FormLineInput { product_service_id: Some(plain.id), ..FormLineInput::discount(1, discounts, None, Some(100), false) }], None, 0),
    )
    .await
    .unwrap_err();
    assert!(err.starts_with("INV_NOT_A_DISCOUNT"), "{err}");
}

#[tokio::test]
async fn discount_items_cannot_be_stocked_or_price_levelled() {
    let pool = test_pool().await;
    let discounts = discounts_given(&pool).await;
    let item = crate::sales::create_item(
        &pool,
        crate::sales::ProductInput { name: "Promo".into(), description: None, rate_cents: 500, income_account_id: discounts, is_taxable: false },
        crate::sales::ItemKind::discount(None),
    )
    .await
    .unwrap();
    let err = sqlx::query("INSERT INTO price_levels (name, kind) VALUES ('VIP', 'PER_ITEM')").execute(&pool).await.and(
        sqlx::query("INSERT INTO price_level_items (price_level_id, product_id, basis, price_cents) VALUES ((SELECT id FROM price_levels), ?1, 'FIXED', 1)")
            .bind(item.id)
            .execute(&pool)
            .await,
    );
    assert!(err.unwrap_err().to_string().contains("PRICE_LEVEL_DISCOUNT"));
    let bad = crate::sales::create_item(
        &pool,
        crate::sales::ProductInput { name: "Odd".into(), description: None, rate_cents: 0, income_account_id: discounts, is_taxable: false },
        crate::sales::ItemKind { item_type: "ITEM".into(), percent_ppm: Some(5) },
    )
    .await
    .unwrap_err();
    assert!(bad.starts_with("ITEM_BAD_PERCENT"), "{bad}");
}

#[test]
fn percents_print_without_trailing_zeros() {
    assert_eq!(percent_text(100_000), "10%");
    assert_eq!(percent_text(25_000), "2.5%");
    assert_eq!(percent_text(82_500), "8.25%");
    assert_eq!(percent_text(1), "0.0001%");
}
