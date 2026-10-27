use super::*;
use crate::db::test_pool;
use crate::invoices::{create_and_post_invoice, InvoiceInput, InvoiceLineInput};
use chrono::NaiveDate;

fn texts(pages: &[Page]) -> Vec<String> {
    pages
        .iter()
        .flat_map(|p| p.ops.iter())
        .filter_map(|op| match op {
            Op::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn formats_like_the_app() {
    assert_eq!(format_money(123_456, None, "$"), "$1,234.56");
    assert_eq!(format_money(-99, None, "€"), "-€0.99");
    assert_eq!(format_money(100_000_000, Some("EUR"), "$"), "EUR 1,000,000.00");
    assert_eq!(format_quantity(15_000), "1.5");
    assert_eq!(format_quantity(12_340_000), "1,234");
    assert_eq!(format_quantity(1), "0.0001");
    assert_eq!(format_date("2026-10-23"), "10/23/2026");
}

#[test]
fn templates_validate_and_fill_defaults() {
    let t = FormTemplate { title: "  ".into(), accent_color: "#ABCDEF".into(), amount_label: "".into(), ..FormTemplate::default() };
    let t = t.validated(FormKind::Estimate).unwrap();
    assert_eq!((t.title.as_str(), t.accent_color.as_str(), t.amount_label.as_str()), ("Estimate", "#abcdef", "Amount"));
    assert!(FormTemplate { accent_color: "green".into(), ..FormTemplate::default() }.validated(FormKind::Invoice).is_err());
    assert!(FormTemplate { title: "x".repeat(41), ..FormTemplate::default() }.validated(FormKind::Invoice).is_err());
    // Older saved templates without newer fields still read.
    let partial: FormTemplate = serde_json::from_str(r##"{"title":"Bill","accentColor":"#000000"}"##).unwrap();
    assert_eq!((partial.title.as_str(), partial.show_rate), ("Bill", true));
}

#[test]
fn layout_follows_the_template() {
    let company = Company { name: "Acme Studio".into(), address: "1 Loop Rd".into(), email: "hi@acme.test".into(), currency_symbol: "$".into(), ..Company::default() };
    let doc = sample_doc(FormKind::Invoice, "$");
    let tpl = FormTemplate::default_for(FormKind::Invoice);
    let all = texts(&layout(&doc, &company, &tpl, Some((400, 100))));
    for expected in ["Acme Studio", "Invoice", "No. 1001", "BILL TO", "Sample Customer", "Qty", "Balance due", "Payment received 10/15/2026", "-$1,000.00"] {
        assert!(all.iter().any(|t| t == expected), "missing {expected:?} in {all:?}");
    }
    assert!(all.iter().any(|t| t.contains("Please reference invoice 1001")));

    let tpl = FormTemplate {
        title: "Tax Invoice".into(),
        show_quantity: false,
        show_rate: false,
        show_payments: false,
        show_company_email: false,
        footer: "Thanks from {company}".into(),
        terms: "Pay by bank transfer.".into(),
        ..FormTemplate::default_for(FormKind::Invoice)
    };
    let pages = layout(&doc, &company, &tpl, None);
    let all = texts(&pages);
    assert!(all.iter().any(|t| t == "Tax Invoice"));
    assert!(all.iter().any(|t| t == "Thanks from Acme Studio"));
    assert!(all.iter().any(|t| t == "Pay by bank transfer."));
    for absent in ["Qty", "Rate", "hi@acme.test", "Payment received 10/15/2026"] {
        assert!(!all.iter().any(|t| t == absent), "{absent:?} should be hidden");
    }
    assert!(!pages.iter().any(|p| p.ops.iter().any(|op| matches!(op, Op::Image { .. }))));
}

#[test]
fn header_and_totals_text_stays_clear_of_the_logo_and_amounts() {
    let company = Company { name: "Northwest Regional Landscaping and Snow Removal Cooperative".into(), address: "1234 Southeast Industrial Parkway, Building 7, Unit 12, Portland, OR 97214".into(), currency_symbol: "$".into(), ..Company::default() };
    let mut doc = sample_doc(FormKind::Invoice, "$");
    doc.totals.insert(0, TotalRow { label: "Sales tax (Multnomah County Combined Special District Rate)".into(), value: "$1,234.56".into(), grand: false, payment: false });
    let tpl = FormTemplate { logo_size: LogoSize::Large, ..FormTemplate::default_for(FormKind::Invoice) };
    let pages = layout(&doc, &company, &tpl, Some((1000, 200)));
    let (logo_x, logo_bottom) = pages[0].ops.iter().find_map(|op| if let Op::Image { x, y, h, .. } = op { Some((*x, y + h)) } else { None }).unwrap();
    for op in &pages[0].ops {
        if let Op::Text { x, y, text, family, bold, size, align: Align::Left, .. } = op {
            if *y < logo_bottom {
                assert!(x + pdf::text_width(*family, *bold, *size, text) <= logo_x, "{text:?} runs under the logo");
            }
        }
    }
    let tax = texts(&pages).into_iter().find(|t| t.starts_with("Sales tax")).unwrap();
    assert!(tax.ends_with('…'), "{tax:?} should be shortened");
}

#[test]
fn long_documents_paginate_and_stay_on_the_page() {
    let company = Company { name: "Acme".into(), currency_symbol: "$".into(), ..Company::default() };
    let mut doc = sample_doc(FormKind::Estimate, "$");
    doc.lines = (0..80).map(|i| FormLine { description: format!("Line {i} with a fairly long description that wraps onto a second line in the table"), quantity: "1".into(), rate: "$1.00".into(), amount: "$1.00".into(), details: Vec::new() }).collect();
    let tpl = FormTemplate { paper: Paper::A4, ..FormTemplate::default_for(FormKind::Estimate) };
    let pages = layout(&doc, &company, &tpl, None);
    assert!(pages.len() > 2);
    let all = texts(&pages);
    assert!(all.iter().any(|t| t == &format!("Page {} of {}", pages.len(), pages.len())));
    assert!(all.iter().any(|t| t == "Estimate 1001 (continued)"));
    assert!(all.iter().any(|t| t == "Line 79 with a fairly long description that wraps onto a"));
    for page in &pages {
        for op in &page.ops {
            if let Op::Text { y, text, .. } = op {
                assert!(*y > 0.0 && *y < page.height - MARGIN + 10.0, "{text:?} at {y} is off the page");
            }
        }
    }
}

#[tokio::test]
async fn real_invoices_render_saved_templates_to_pdf() {
    let pool = test_pool().await;
    let services: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(&pool).await.unwrap();
    let d = |day| NaiveDate::from_ymd_opt(2026, 10, day).unwrap();
    let invoice = create_and_post_invoice(
        &pool,
        InvoiceInput {
            customer_name: "Initech".into(),
            issue_date: d(1),
            due_date: d(31),
            lines: vec![InvoiceLineInput { description: "Design (hours)".into(), quantity_e4: 15_000, rate_cents: 10_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        },
    )
    .await
    .unwrap();
    sqlx::query("UPDATE customers SET billing_address = '9 Elm St\nAustin, TX', email = 'ap@initech.test' WHERE name = 'Initech'").execute(&pool).await.unwrap();

    assert_eq!(get_template(&pool, None, FormKind::Invoice).await.unwrap(), FormTemplate::default_for(FormKind::Invoice));
    let custom = FormTemplate { title: "Statement of Work".into(), show_customer_email: true, ..FormTemplate::default_for(FormKind::Invoice) };
    save_template(&pool, None, FormKind::Invoice, custom.clone()).await.unwrap();
    assert_eq!(get_template(&pool, None, FormKind::Invoice).await.unwrap().title, "Statement of Work");
    // Templates are per form type.
    assert_eq!(get_template(&pool, None, FormKind::Estimate).await.unwrap().title, "Estimate");

    let rendered = render(&pool, FormKind::Invoice, Some(invoice.id), None, None, false).await.unwrap();
    assert_eq!(rendered.form.title, format!("Statement of Work {} · Initech", invoice.invoice_number));
    assert_eq!(rendered.form.customer_email.as_deref(), Some("ap@initech.test"));
    let all = texts(&rendered.form.pages);
    for expected in ["Initech", "9 Elm St", "Austin, TX", "ap@initech.test", "1.5", "$100.00", "$150.00", "Net 30"] {
        assert!(all.iter().any(|t| t == expected), "missing {expected:?} in {all:?}");
    }

    let (name, bytes) = pdf_file_for(&pool, FormKind::Invoice, invoice.id, None).await.unwrap();
    assert_eq!(name, format!("Statement of Work {} · Initech.pdf", invoice.invoice_number));
    assert!(bytes.starts_with(b"%PDF-1.4"));

    let draft = email_draft(&pool, FormKind::Invoice, invoice.id, None).await.unwrap();
    assert_eq!(draft.to, "ap@initech.test");
    assert_eq!(draft.subject, format!("Invoice {} from {}", invoice.invoice_number, load_company(&pool).await.unwrap().name));
    assert!(draft.body.starts_with("Dear Initech,\n\nPlease find attached invoice"), "{}", draft.body);
    assert!(draft.body.contains("$150.00, due 10/31/2026"));
    assert_eq!(draft.file_name, format!("Statement of Work {} · Initech.pdf", invoice.invoice_number));

    assert_eq!(reset_template(&pool, None, FormKind::Invoice).await.unwrap().title, "Invoice");
    assert_eq!(get_template(&pool, None, FormKind::Invoice).await.unwrap().title, "Invoice");
    assert!(render(&pool, FormKind::Estimate, Some(999), None, None, false).await.is_err());
}

#[test]
fn file_names_are_safe() {
    assert_eq!(file_name("Invoice 1001"), "Invoice 1001.pdf");
    assert_eq!(file_name("A/B: \"x\""), "A-B- -x-.pdf");
}

#[test]
fn every_layout_preset_renders_every_form_on_one_page() {
    let company = Company { name: "Acme Studio".into(), address: "1 Loop Rd\nSpringfield".into(), currency_symbol: "$".into(), ..Company::default() };
    for layout_preset in [FormLayout::Classic, FormLayout::Modern, FormLayout::Compact] {
        for kind in FormKind::ALL {
            let tpl = FormTemplate { layout: layout_preset, footer: "Pay by ACH".into(), ..FormTemplate::default_for(kind) };
            let pages = layout(&sample_doc(kind, "$"), &company, &tpl, Some((400, 100)));
            assert_eq!(pages.len(), 1, "{layout_preset:?} {kind:?}");
            let all = texts(&pages);
            assert!(all.iter().any(|t| t == &tpl.title), "{layout_preset:?} {kind:?} has its title");
            assert!(all.iter().any(|t| t == "Pay by ACH"));
            // The Modern preset puts the title in a filled accent band.
            let band = pages[0].ops.iter().any(|op| matches!(op, Op::Rect { y, h, .. } if *y == MARGIN && *h == 46.0));
            assert_eq!(band, layout_preset == FormLayout::Modern);
        }
    }
    // Compact fits more lines on a page than Classic.
    let mut doc = sample_doc(FormKind::Invoice, "$");
    doc.lines = (0..60).map(|i| FormLine { description: format!("Line {i}"), quantity: "1".into(), rate: "$1.00".into(), amount: "$1.00".into(), details: Vec::new() }).collect();
    let pages = |preset| layout(&doc, &company, &FormTemplate { layout: preset, ..FormTemplate::default_for(FormKind::Invoice) }, None).len();
    assert!(pages(FormLayout::Compact) < pages(FormLayout::Classic));
}

#[test]
fn statements_print_transactions_and_aging() {
    let company = Company { name: "Acme".into(), currency_symbol: "$".into(), ..Company::default() };
    let doc = sample_doc(FormKind::Statement, "$");
    let tpl = FormTemplate::default_for(FormKind::Statement);
    let all = texts(&layout(&doc, &company, &tpl, None));
    for expected in ["Statement", "10/01/2026 – 10/31/2026", "STATEMENT FOR", "Balance forward", "Invoice #1001", "Payment #2045", "Amount due", "Over 90 days"] {
        assert!(all.iter().any(|t| t == expected), "missing {expected:?} in {all:?}");
    }
    let all = texts(&layout(&doc, &company, &FormTemplate { show_aging: false, ..tpl }, None));
    assert!(!all.iter().any(|t| t == "Over 90 days"));
}

#[tokio::test]
async fn styles_hold_their_own_templates_and_follow_the_customer() {
    let pool = test_pool().await;
    let styles = list_styles(&pool).await.unwrap();
    assert_eq!((styles.len(), styles[0].name.as_str(), styles[0].is_default), (1, "Standard", true));
    let standard = styles[0].id;
    save_template(&pool, None, FormKind::Invoice, FormTemplate { title: "Tax Invoice".into(), ..FormTemplate::default_for(FormKind::Invoice) }).await.unwrap();

    let modern = create_style(&pool, "Modern", Some(standard)).await.unwrap();
    assert!(!modern.is_default);
    // A copy starts with the source's templates; then it changes on its own.
    assert_eq!(get_template(&pool, Some(modern.id), FormKind::Invoice).await.unwrap().title, "Tax Invoice");
    save_template(&pool, Some(modern.id), FormKind::Invoice, FormTemplate { layout: FormLayout::Modern, title: "Bill".into(), ..FormTemplate::default_for(FormKind::Invoice) }).await.unwrap();
    assert_eq!(get_template(&pool, None, FormKind::Invoice).await.unwrap().title, "Tax Invoice");
    assert!(create_style(&pool, "modern", None).await.unwrap_err().starts_with("FORM_STYLE_DUPLICATE"));
    assert!(create_style(&pool, " ", None).await.unwrap_err().starts_with("FORM_STYLE_NAME"));
    assert_eq!(rename_style(&pool, modern.id, "Bold").await.unwrap().name, "Bold");

    let services: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(&pool).await.unwrap();
    let input = |customer: &str| -> InvoiceInput {
        serde_json::from_value(serde_json::json!({ "customerName": customer, "issueDate": "2026-10-01", "dueDate": "2026-10-31",
            "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": 5_000, "incomeAccountId": services }] }))
        .unwrap()
    };
    let styled = create_and_post_invoice(&pool, input("Initech")).await.unwrap();
    let plain = create_and_post_invoice(&pool, input("Globex")).await.unwrap();
    sqlx::query("UPDATE customers SET form_style_id = ?1 WHERE name = 'Initech'").bind(modern.id).execute(&pool).await.unwrap();
    let pool_ref = &pool;
    let title = |id| async move { render(pool_ref, FormKind::Invoice, Some(id), None, None, false).await.unwrap().form.title };
    assert_eq!(title(styled.id).await, format!("Bill {} · Initech", styled.invoice_number));
    assert_eq!(title(plain.id).await, format!("Tax Invoice {} · Globex", plain.invoice_number));
    assert_eq!(email_draft(&pool, FormKind::Invoice, styled.id, None).await.unwrap().file_name, format!("Bill {} · Initech.pdf", styled.invoice_number));

    assert!(delete_style(&pool, standard).await.unwrap_err().starts_with("FORM_STYLE_DEFAULT"));
    set_default_style(&pool, modern.id).await.unwrap();
    assert_eq!(title(plain.id).await, format!("Bill {} · Globex", plain.invoice_number));
    set_default_style(&pool, standard).await.unwrap();
    assert_eq!(list_styles(&pool).await.unwrap().iter().find(|s| s.id == modern.id).unwrap().customer_count, 1);
    delete_style(&pool, modern.id).await.unwrap();
    // Its customers fall back to the default style.
    assert_eq!(title(styled.id).await, format!("Tax Invoice {} · Initech", styled.invoice_number));
    assert_eq!(list_styles(&pool).await.unwrap().len(), 1);
}

#[tokio::test]
async fn sales_receipts_and_statements_render_from_the_books() {
    let pool = test_pool().await;
    let account = |n: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(&pool).await.unwrap() }
    };
    let (services, checking) = (account("4100").await, account("1010").await);
    let receipt = crate::sales::post_sales_receipt(
        &pool,
        serde_json::from_value(serde_json::json!({ "customerName": "Initech", "receiptDate": "2026-10-05", "depositAccountId": checking,
            "paymentMethod": "CHECK", "reference": "2045", "memo": "Thanks!",
            "lines": [{ "description": "Workshop", "quantityE4": 20_000, "rateCents": 7_500, "incomeAccountId": services }] }))
        .unwrap(),
    )
    .await
    .unwrap();
    let rendered = render(&pool, FormKind::SalesReceipt, Some(receipt.id), None, None, false).await.unwrap();
    assert_eq!(rendered.form.title, format!("Sales Receipt {} · Initech", receipt.receipt_number));
    let all = texts(&rendered.form.pages);
    for expected in ["SOLD TO", "Initech", "Check #2045", "Workshop", "2", "$150.00", "Thanks!"] {
        assert!(all.iter().any(|t| t == expected), "missing {expected:?} in {all:?}");
    }

    let invoice: InvoiceInput = serde_json::from_value(serde_json::json!({ "customerName": "Initech", "issueDate": "2026-10-01", "dueDate": "2026-10-31",
        "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": 40_000, "incomeAccountId": services }] }))
    .unwrap();
    let invoice = create_and_post_invoice(&pool, invoice).await.unwrap();
    let customer = invoice.customer_id.unwrap();
    let period = Period { start_date: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(), end_date: NaiveDate::from_ymd_opt(2026, 10, 31).unwrap() };
    let rendered = render(&pool, FormKind::Statement, Some(customer), Some(period), None, false).await.unwrap();
    assert_eq!(rendered.form.title, "Statement 2026-10-31 · Initech");
    let all = texts(&rendered.form.pages);
    for expected in ["10/01/2026 – 10/31/2026", "Balance forward", &format!("Invoice #{}", invoice.invoice_number), "$400.00", "Amount due"] {
        assert!(all.iter().any(|t| t == expected), "missing {expected:?} in {all:?}");
    }
    sqlx::query("UPDATE customers SET email = 'ap@initech.test' WHERE id = ?1").bind(customer).execute(&pool).await.unwrap();
    let receipt_mail = email_draft(&pool, FormKind::SalesReceipt, receipt.id, None).await.unwrap();
    assert_eq!((receipt_mail.to.as_str(), receipt_mail.file_name.clone()), ("ap@initech.test", format!("Sales Receipt {} · Initech.pdf", receipt.receipt_number)));
    let statement_mail = email_draft(&pool, FormKind::Statement, customer, Some(period)).await.unwrap();
    assert!(statement_mail.body.contains("The amount due is $400.00"), "{}", statement_mail.body);
    assert_eq!(statement_mail.file_name, "Statement 2026-10-31 · Initech.pdf");
    let (name, bytes) = pdf_file_for(&pool, FormKind::Statement, customer, Some(period)).await.unwrap();
    assert_eq!(name, "Statement 2026-10-31 · Initech.pdf");
    assert!(bytes.starts_with(b"%PDF-1.4"));
    let backwards = Period { start_date: period.end_date, end_date: period.start_date };
    assert!(render(&pool, FormKind::Statement, Some(customer), Some(backwards), None, false).await.is_err());
    assert_eq!(Period::last_month(NaiveDate::from_ymd_opt(2026, 3, 15).unwrap()), Period { start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(), end_date: NaiveDate::from_ymd_opt(2026, 2, 28).unwrap() });
}

#[tokio::test]
async fn sales_forms_print_rep_shipping_and_custom_fields() {
    use crate::custom_fields::{create_custom_field, save_record_values, CustomFieldInput, FieldList, FieldValue};
    use crate::sales_lists::{create_sales_rep, create_ship_method, SalesDetails, SalesRepInput};
    let pool = test_pool().await;
    let services: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(&pool).await.unwrap();
    let widget: i64 =
        sqlx::query_scalar("INSERT INTO products_services (name, rate_cents, income_account_id) VALUES ('Widget', 500, ?1) RETURNING id").bind(services).fetch_one(&pool).await.unwrap();
    let color = create_custom_field(&pool, FieldList::Item, CustomFieldInput { label: "Color".into(), on_sales_forms: true }).await.unwrap();
    let sku = create_custom_field(&pool, FieldList::Item, CustomFieldInput { label: "Bin".into(), on_sales_forms: false }).await.unwrap();
    save_record_values(&pool, FieldList::Item, widget, &[FieldValue { field_id: color.id, value: "Red".into() }, FieldValue { field_id: sku.id, value: "B7".into() }]).await.unwrap();
    let po = create_custom_field(&pool, FieldList::Customer, CustomFieldInput { label: "PO number".into(), on_sales_forms: true }).await.unwrap();
    let rep = create_sales_rep(&pool, SalesRepInput { initials: "JS".into(), name: None }).await.unwrap();
    let ups = create_ship_method(&pool, "UPS Ground").await.unwrap();
    let d = |day| NaiveDate::from_ymd_opt(2026, 10, day).unwrap();
    let invoice = create_and_post_invoice(
        &pool,
        InvoiceInput {
            customer_name: "Initech".into(),
            issue_date: d(1),
            due_date: d(31),
            lines: vec![InvoiceLineInput { description: "Widget".into(), quantity_e4: 20_000, rate_cents: 500, income_account_id: services, product_service_id: Some(widget), class_id: None, is_taxable: false }],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
            details: SalesDetails {
                sales_rep_id: Some(rep.id),
                ship_date: Some(d(2)),
                ship_method_id: Some(ups.id),
                custom_fields: vec![FieldValue { field_id: po.id, value: "PO-55".into() }],
            },
        },
    )
    .await
    .unwrap();
    let company = Company { name: "Acme".into(), currency_symbol: "$".into(), ..Company::default() };
    let doc = invoice_doc(&pool, invoice.id, "$").await.unwrap();
    let tpl = FormTemplate { message: "Shipped {ship_via} on {ship_date} by {rep}.".into(), ..FormTemplate::default_for(FormKind::Invoice) };
    let all = texts(&layout(&doc, &company, &tpl, None));
    for expected in ["Rep", "JS", "Ship date", "10/02/2026", "Ship via", "UPS Ground", "PO number", "PO-55", "Color: Red", "Shipped UPS Ground on 10/02/2026 by JS."] {
        assert!(all.iter().any(|t| t == expected), "missing {expected:?} in {all:?}");
    }
    assert!(!all.iter().any(|t| t.contains("B7")), "a field not marked for sales forms is not printed");

    let hidden = FormTemplate { show_sales_rep: false, show_shipping: false, show_custom_fields: false, show_item_fields: false, ..FormTemplate::default_for(FormKind::Invoice) };
    let all = texts(&layout(&doc, &company, &hidden, None));
    for absent in ["Rep", "Ship via", "UPS Ground", "PO-55", "Color: Red"] {
        assert!(!all.iter().any(|t| t == absent), "{absent:?} should be hidden");
    }
}
