use super::*;
use crate::custom_fields::{create_custom_field, CustomFieldInput, FieldList};
use crate::db::test_pool;
use crate::invoices::{create_and_post_invoice, invoice_detail, InvoiceInput, InvoiceLineInput};

fn rep(initials: &str, name: Option<&str>) -> SalesRepInput {
    SalesRepInput { initials: initials.into(), name: name.map(Into::into) }
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

fn invoice(customer: &str, account: i64, cents: i64, details: SalesDetails) -> InvoiceInput {
    InvoiceInput {
        customer_name: customer.into(),
        issue_date: d(1),
        due_date: d(31),
        lines: vec![InvoiceLineInput {
            description: "Work".into(),
            quantity_e4: 10_000,
            rate_cents: cents,
            income_account_id: account,
            product_service_id: None,
            class_id: None,
            is_taxable: false,
        }],
        currency: None,
        tax_rate_id: None,
        discount_cents: 0,
        details,
    }
}

#[tokio::test]
async fn reps_are_unique_by_initials_and_kept_once_used() {
    let pool = test_pool().await;
    let js = create_sales_rep(&pool, rep(" JS ", Some(" Jane Smith "))).await.unwrap();
    assert_eq!((js.initials.as_str(), js.name.as_deref(), js.is_active, js.is_used), ("JS", Some("Jane Smith"), true, false));
    assert!(create_sales_rep(&pool, rep("js", None)).await.unwrap_err().starts_with("REP_DUPLICATE"));
    assert!(create_sales_rep(&pool, rep("", None)).await.unwrap_err().starts_with("REP_INITIALS_REQUIRED"));
    assert!(create_sales_rep(&pool, rep("ABCDEF", None)).await.unwrap_err().starts_with("REP_INITIALS_TOO_LONG"));

    let renamed = update_sales_rep(&pool, js.id, rep("JNS", None)).await.unwrap();
    assert_eq!((renamed.initials.as_str(), renamed.name), ("JNS", None));

    // An unused rep can be deleted; one on a customer cannot.
    let tmp = create_sales_rep(&pool, rep("TMP", None)).await.unwrap();
    delete_sales_rep(&pool, tmp.id).await.unwrap();
    let acme = crate::sales::create_customer(&pool, "Acme", None).await.unwrap();
    sqlx::query("UPDATE customers SET sales_rep_id = ?1 WHERE id = ?2").bind(js.id).bind(acme.id).execute(&pool).await.unwrap();
    assert!(delete_sales_rep(&pool, js.id).await.unwrap_err().starts_with("REP_IN_USE"));
    assert!(!set_sales_rep_active(&pool, js.id, false).await.unwrap().is_active);
    assert_eq!(list_sales_reps(&pool).await.unwrap().len(), 1);
}

#[tokio::test]
async fn ship_methods_are_unique_by_name() {
    let pool = test_pool().await;
    let ups = create_ship_method(&pool, " UPS Ground ").await.unwrap();
    assert_eq!(ups.name, "UPS Ground");
    assert!(create_ship_method(&pool, "ups ground").await.unwrap_err().starts_with("SHIP_DUPLICATE"));
    assert!(create_ship_method(&pool, &"x".repeat(32)).await.unwrap_err().starts_with("SHIP_NAME_TOO_LONG"));
    let fedex = create_ship_method(&pool, "FedEx").await.unwrap();
    assert!(rename_ship_method(&pool, fedex.id, "UPS Ground").await.unwrap_err().starts_with("SHIP_DUPLICATE"));
    assert_eq!(rename_ship_method(&pool, fedex.id, "FedEx 2Day").await.unwrap().name, "FedEx 2Day");
    let names: Vec<String> = list_ship_methods(&pool).await.unwrap().into_iter().map(|m| m.name).collect();
    assert_eq!(names, ["FedEx 2Day", "UPS Ground"]);
    delete_ship_method(&pool, fedex.id).await.unwrap();
}

#[tokio::test]
async fn an_invoice_keeps_its_rep_shipping_and_printed_fields() {
    let pool = test_pool().await;
    let services = account(&pool, "4100").await;
    let js = create_sales_rep(&pool, rep("JS", Some("Jane Smith"))).await.unwrap();
    let ups = create_ship_method(&pool, "UPS").await.unwrap();
    let po = create_custom_field(&pool, FieldList::Customer, CustomFieldInput { label: "PO number".into(), on_sales_forms: true }).await.unwrap();
    let details = SalesDetails {
        sales_rep_id: Some(js.id),
        ship_date: Some(d(3)),
        ship_method_id: Some(ups.id),
        custom_fields: vec![FieldValue { field_id: po.id, value: " 4471 ".into() }],
    };
    let inv = create_and_post_invoice(&pool, invoice("Acme", services, 50_000, details.clone())).await.unwrap();
    let detail = invoice_detail(&pool, inv.id).await.unwrap().details;
    assert_eq!(detail.sales_rep_initials.as_deref(), Some("JS"));
    assert_eq!(detail.ship_date.as_deref(), Some("2026-10-03"));
    assert_eq!(detail.ship_method_name.as_deref(), Some("UPS"));
    assert_eq!(detail.custom_fields.iter().map(|f| (f.label.as_str(), f.value.as_str())).collect::<Vec<_>>(), [("PO number", "4471")]);
    assert_eq!(detail.to_input(), SalesDetails { custom_fields: vec![FieldValue { field_id: po.id, value: "4471".into() }], ..details.clone() });
    assert!(list_sales_reps(&pool).await.unwrap()[0].is_used);

    // Inactive reps and methods cannot be chosen again; fields not on forms cannot be printed.
    set_sales_rep_active(&pool, js.id, false).await.unwrap();
    let err = create_and_post_invoice(&pool, invoice("Acme", services, 100, details.clone())).await.unwrap_err();
    assert!(err.starts_with("INV_INACTIVE_SALES_REP"), "{err}");
    set_sales_rep_active(&pool, js.id, true).await.unwrap();
    let hidden = create_custom_field(&pool, FieldList::Customer, CustomFieldInput { label: "Birthday".into(), on_sales_forms: false }).await.unwrap();
    let err = create_and_post_invoice(&pool, invoice("Acme", services, 100, SalesDetails { custom_fields: vec![FieldValue { field_id: hidden.id, value: "May".into() }], ..details }))
        .await
        .unwrap_err();
    assert!(err.starts_with("CF_NOT_ON_FORMS"), "{err}");
    // Nothing from the refused invoices was kept.
    assert_eq!(crate::invoices::list_invoices(&pool).await.unwrap().len(), 1);
}

#[tokio::test]
async fn an_estimate_carries_its_details_to_its_invoice() {
    let pool = test_pool().await;
    let services = account(&pool, "4100").await;
    let js = create_sales_rep(&pool, rep("JS", None)).await.unwrap();
    let ups = create_ship_method(&pool, "UPS").await.unwrap();
    let estimate = crate::estimates::create_estimate(
        &pool,
        crate::estimates::EstimateInput {
            customer_name: "Initech".into(),
            estimate_date: d(1),
            expiration_date: None,
            memo: None,
            lines: vec![crate::sales::SalesLineInput {
                product_service_id: None,
                description: "Audit".into(),
                quantity_e4: 10_000,
                rate_cents: 5_000,
                income_account_id: services,
                class_id: None,
                is_taxable: false,
            }],
            tax_rate_id: None,
            discount_cents: 0,
            details: SalesDetails { sales_rep_id: Some(js.id), ship_method_id: Some(ups.id), ..Default::default() },
        },
    )
    .await
    .unwrap();
    assert_eq!(crate::estimates::estimate_detail(&pool, estimate.id).await.unwrap().details.sales_rep_initials.as_deref(), Some("JS"));
    let converted = crate::estimates::convert_to_invoice(&pool, estimate.id, d(5), d(30)).await.unwrap();
    let invoice = invoice_detail(&pool, converted.converted_to_invoice_id.unwrap()).await.unwrap();
    assert_eq!((invoice.details.sales_rep_id, invoice.details.ship_method_id), (Some(js.id), Some(ups.id)));
}

#[tokio::test]
async fn sales_by_rep_totals_invoices_and_receipts() {
    let pool = test_pool().await;
    let services = account(&pool, "4100").await;
    let checking = account(&pool, "1010").await;
    let js = create_sales_rep(&pool, rep("JS", Some("Jane Smith"))).await.unwrap();
    let with_rep = SalesDetails { sales_rep_id: Some(js.id), ..Default::default() };
    create_and_post_invoice(&pool, invoice("Acme", services, 30_000, with_rep.clone())).await.unwrap();
    create_and_post_invoice(&pool, invoice("Acme", services, 5_000, SalesDetails::default())).await.unwrap();
    crate::sales::post_sales_receipt(
        &pool,
        crate::sales::SalesReceiptInput {
            customer_name: None,
            receipt_date: d(2),
            deposit_account_id: checking,
            payment_method: "CASH".into(),
            reference: None,
            memo: None,
            lines: vec![crate::sales::SalesLineInput {
                product_service_id: None,
                description: "Walk-in".into(),
                quantity_e4: 20_000,
                rate_cents: 1_000,
                income_account_id: services,
                class_id: None,
                is_taxable: false,
            }],
            tax_rate_id: None,
            discount_cents: 0,
            details: with_rep,
        },
    )
    .await
    .unwrap();
    let report = crate::report_center::sales_by_rep(&pool, d(1), d(31)).await.unwrap();
    let rows: Vec<(&str, i64)> = report.rows.iter().map(|r| (r.label.as_str(), r.amount_cents)).collect();
    assert_eq!(rows, [("JS (Jane Smith)", 32_000), ("No rep", 5_000)]);
    assert_eq!(report.total_cents, 37_000);
}

#[tokio::test]
async fn imports_find_reps_and_methods_by_name() {
    let pool = test_pool().await;
    let existing = create_sales_rep(&pool, rep("JS", None)).await.unwrap();
    let mut tx = crate::ledger::begin_write(&pool).await.unwrap();
    assert_eq!(find_or_create_rep(&mut tx, "js:Jane Smith").await.unwrap(), Some(existing.id));
    let new = find_or_create_rep(&mut tx, "RW:Rita Wong").await.unwrap().unwrap();
    assert_eq!(find_or_create_rep(&mut tx, " ").await.unwrap(), None);
    let ups = find_or_create_ship_method(&mut tx, "UPS").await.unwrap();
    assert_eq!(find_or_create_ship_method(&mut tx, "ups").await.unwrap(), ups);
    crate::ledger::commit_write(tx).await.unwrap();
    let reps = list_sales_reps(&pool).await.unwrap();
    assert_eq!(reps.iter().find(|r| r.id == new).unwrap().name.as_deref(), Some("Rita Wong"));
    // A rep already on file keeps its own name.
    assert_eq!(reps.iter().find(|r| r.id == existing.id).unwrap().name, None);
}

/// A rep, shipping method or printed field that was fine when the estimate (or invoice) was made
/// carries over even after the list changes; only newly chosen ones are checked.
#[tokio::test]
async fn carried_details_survive_later_list_changes() {
    let pool = test_pool().await;
    let services = account(&pool, "4100").await;
    let js = create_sales_rep(&pool, rep("JS", None)).await.unwrap();
    let rw = create_sales_rep(&pool, rep("RW", None)).await.unwrap();
    let ups = create_ship_method(&pool, "UPS").await.unwrap();
    let po = create_custom_field(&pool, FieldList::Customer, CustomFieldInput { label: "PO number".into(), on_sales_forms: true }).await.unwrap();
    let details = SalesDetails {
        sales_rep_id: Some(js.id),
        ship_date: Some(d(3)),
        ship_method_id: Some(ups.id),
        custom_fields: vec![FieldValue { field_id: po.id, value: "77".into() }],
    };
    let estimate_input = || crate::estimates::EstimateInput {
        customer_name: "Initech".into(),
        estimate_date: d(1),
        expiration_date: None,
        memo: None,
        lines: vec![crate::sales::SalesLineInput {
            product_service_id: None,
            description: "Audit".into(),
            quantity_e4: 10_000,
            rate_cents: 5_000,
            income_account_id: services,
            class_id: None,
            is_taxable: false,
        }],
        tax_rate_id: None,
        discount_cents: 0,
        details: details.clone(),
    };
    let first = crate::estimates::create_estimate(&pool, estimate_input()).await.unwrap();
    let second = crate::estimates::create_estimate(&pool, estimate_input()).await.unwrap();
    let posted = create_and_post_invoice(&pool, invoice("Initech", services, 9_000, details.clone())).await.unwrap();

    // Everything on them goes stale.
    set_sales_rep_active(&pool, js.id, false).await.unwrap();
    set_ship_method_active(&pool, ups.id, false).await.unwrap();
    crate::custom_fields::update_custom_field(&pool, po.id, CustomFieldInput { label: "PO number".into(), on_sales_forms: false }).await.unwrap();

    // Converting straight from the estimate carries it all over.
    let converted = crate::estimates::convert_to_invoice(&pool, first.id, d(5), d(30)).await.unwrap();
    let carried = invoice_detail(&pool, converted.converted_to_invoice_id.unwrap()).await.unwrap().details;
    assert_eq!(carried.to_input(), details);

    // So does the invoice form loaded from an estimate, as long as the details are left as they were...
    let form = invoice("Initech", services, 5_000, details.clone());
    let from_form = crate::estimates::invoice_from_estimate(&pool, second.id, form).await.unwrap();
    assert_eq!(invoice_detail(&pool, from_form.id).await.unwrap().details.to_input(), details);

    // ...and an edited invoice keeps the original's.
    let replaced = crate::edits::replace_invoice(&pool, posted.id, invoice("Initech", services, 9_500, details.clone())).await.unwrap();
    assert_eq!(invoice_detail(&pool, replaced).await.unwrap().details.to_input(), details);

    // A newly chosen inactive rep is still refused.
    set_sales_rep_active(&pool, rw.id, false).await.unwrap();
    let other = SalesDetails { sales_rep_id: Some(rw.id), ..details.clone() };
    let err = crate::edits::replace_invoice(&pool, replaced, invoice("Initech", services, 9_600, other)).await.unwrap_err();
    assert!(err.starts_with("INV_INACTIVE_SALES_REP"), "{err}");
}
