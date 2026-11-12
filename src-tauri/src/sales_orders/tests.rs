use super::*;
use crate::db::test_pool;
use crate::inventory::{adjust, track_product, AdjustInput, TrackInput};
use crate::invoices::InvoiceLineInput;
use crate::money::Cents;

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 5, day).unwrap()
}

struct Shop {
    pool: SqlitePool,
    sales: i64,
    widget: i64,
    service: i64,
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

/// A shop with a tracked Widget (7 on hand) and an untracked Setup service.
async fn shop() -> Shop {
    let pool = test_pool().await;
    let sales = account(&pool, "4000").await;
    let product =
        |name: &str, rate| crate::sales::ProductInput { name: name.into(), description: None, rate_cents: rate, income_account_id: sales, is_taxable: false };
    let widget = crate::sales::create_product(&pool, product("Widget", Cents(2_500))).await.unwrap().id;
    let service = crate::sales::create_product(&pool, product("Setup", Cents(10_000))).await.unwrap().id;
    let item = track_product(&pool, &TrackInput { product_id: widget, sku: None, asset_account_id: None, cogs_account_id: None }).await.unwrap().id;
    let obe: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = 'Opening Balance Equity'").fetch_one(&pool).await.unwrap();
    adjust(&pool, &AdjustInput { item_id: item, date: d(1), qty_change_e4: 70_000, total_cost_cents: Some(7_000), offset_account_id: obe, memo: None })
        .await
        .unwrap();
    Shop { pool, sales, widget, service }
}

fn line(s: &Shop, product: i64, qty: i64, rate: i64) -> SalesOrderLineInput {
    SalesOrderLineInput {
        id: None,
        line: SalesLineInput {
            product_service_id: Some(product),
            description: "ordered".into(),
            quantity_e4: qty * 10_000,
            rate_cents: Cents(rate),
            income_account_id: s.sales,
            class_id: None,
            is_taxable: false,
            service_date: None,
        },
    }
}

fn order(customer: &str, date: NaiveDate, lines: Vec<SalesOrderLineInput>) -> SalesOrderInput {
    SalesOrderInput {
        order_number: None,
        customer_name: customer.into(),
        order_date: date,
        expected_date: None,
        memo: None,
        lines,
        tax_rate_id: None,
        sales_rep_id: None,
        ship_method_id: None,
        customer_message_id: None,
        header: SalesHeader::default(),
    }
}

/// An invoice for `customer` taking `qty` of each given order line.
fn invoice(s: &Shop, customer: &str, takes: &[(i64, i64, i64)]) -> (InvoiceInput, Vec<Take>) {
    let lines = takes
        .iter()
        .map(|(_, product, qty)| InvoiceLineInput {
            description: "shipped".into(),
            quantity_e4: qty * 10_000,
            rate_cents: Cents(2_500),
            income_account_id: s.sales,
            product_service_id: Some(*product),
            class_id: None,
            is_taxable: false,
            service_date: None,
        })
        .collect();
    let input = InvoiceInput {
        invoice_number: None,
        customer_name: customer.into(),
        issue_date: d(10),
        due_date: d(30),
        lines,
        currency: None,
        tax_rate_id: None,
        discount_cents: Cents::ZERO,
        details: Default::default(),
    };
    (input, takes.iter().enumerate().map(|(i, (order_line_id, ..))| Take { order_line_id: *order_line_id, invoice_line: i }).collect())
}

async fn entries(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries").fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn an_order_posts_nothing_and_is_invoiced_in_parts() {
    let s = shop().await;
    let before = entries(&s.pool).await;
    let saved = create(&s.pool, &order("Acme", d(2), vec![line(&s, s.widget, 10, 2_500), line(&s, s.service, 1, 10_000)])).await.unwrap();
    assert_eq!((saved.order_number, saved.status.as_str(), saved.total_cents, saved.open_cents), (1, "OPEN", 35_000, 35_000));
    assert_eq!(entries(&s.pool).await, before, "a sales order posts nothing");
    let lines = detail(&s.pool, saved.id).await.unwrap().lines;
    let (widgets, setup) = (lines[0].id, lines[1].id);

    // 10 ordered, 7 on hand: 3 backordered.
    let open = open_lines(&s.pool).await.unwrap();
    assert_eq!(open.lines[0].backordered_qty_e4, 30_000);
    let stock = open.items.iter().find(|i| i.product_service_id == s.widget).unwrap();
    assert_eq!((stock.on_hand_e4, stock.on_sales_order_e4, stock.available_e4), (Some(70_000), 100_000, Some(-30_000)));
    assert_eq!(open.items.iter().find(|i| i.product_service_id == s.service).unwrap().available_e4, None, "an untracked item has no stock");
    let status = crate::report_center::stock_status(&s.pool, d(1), d(31)).await.unwrap();
    assert_eq!((status.rows[0].qty_on_sales_order_e4, status.rows[0].qty_available_e4), (100_000, -30_000));

    // Ship 7 widgets and the setup.
    let (input, takes) = invoice(&s, "Acme", &[(widgets, s.widget, 7), (setup, s.service, 1)]);
    invoice_from_order(&s.pool, saved.id, &input, &takes).await.unwrap();
    let after = detail(&s.pool, saved.id).await.unwrap();
    assert_eq!(after.summary.status, "PARTLY_INVOICED");
    assert_eq!((after.lines[0].invoiced_qty_e4, after.lines[0].left_qty_e4, after.lines[1].left_qty_e4), (70_000, 30_000, 0));
    assert_eq!(after.summary.open_cents, 7_500);
    assert_eq!(after.invoices.len(), 1);
    let on_hand: i64 = sqlx::query_scalar("SELECT qty_on_hand_e4 FROM inventory_items").fetch_one(&s.pool).await.unwrap();
    assert_eq!(on_hand, 0, "the invoice sold the stock");

    // Too many, then the rest.
    let (input, takes) = invoice(&s, "Acme", &[(widgets, s.widget, 4)]);
    assert!(invoice_from_order(&s.pool, saved.id, &input, &takes).await.unwrap_err().starts_with("SO_OVER_INVOICED"));
    let (input, takes) = invoice(&s, "Acme", &[(widgets, s.widget, 3)]);
    invoice_from_order(&s.pool, saved.id, &input, &takes).await.unwrap();
    let done = detail(&s.pool, saved.id).await.unwrap();
    assert_eq!((done.summary.status.as_str(), done.summary.open_cents, done.invoices.len()), ("CLOSED", 0, 2));
    assert!(open_lines(&s.pool).await.unwrap().lines.is_empty());
    assert!(delete(&s.pool, saved.id).await.unwrap_err().starts_with("SO_HAS_INVOICES"));
}

#[tokio::test]
async fn refused_invoices_change_nothing() {
    let s = shop().await;
    let saved = create(&s.pool, &order("Acme", d(2), vec![line(&s, s.widget, 5, 2_500)])).await.unwrap();
    let widgets = detail(&s.pool, saved.id).await.unwrap().lines[0].id;
    let before = entries(&s.pool).await;

    let (input, takes) = invoice(&s, "Globex", &[(widgets, s.widget, 1)]);
    assert!(invoice_from_order(&s.pool, saved.id, &input, &takes).await.unwrap_err().starts_with("SO_OTHER_CUSTOMER"));
    let (input, takes) = invoice(&s, "Acme", &[(widgets, s.service, 1)]);
    assert!(invoice_from_order(&s.pool, saved.id, &input, &takes).await.unwrap_err().starts_with("SO_ITEM_MISMATCH"));
    let (input, _) = invoice(&s, "Acme", &[(widgets, s.widget, 1)]);
    assert!(invoice_from_order(&s.pool, saved.id, &input, &[]).await.unwrap_err().starts_with("SO_NOTHING_TAKEN"));
    let other = create(&s.pool, &order("Acme", d(3), vec![line(&s, s.widget, 1, 2_500)])).await.unwrap();
    let (input, takes) = invoice(&s, "Acme", &[(widgets, s.widget, 1)]);
    assert!(invoice_from_order(&s.pool, other.id, &input, &takes).await.unwrap_err().starts_with("SO_BAD_TAKE"));

    close(&s.pool, saved.id).await.unwrap();
    let (input, takes) = invoice(&s, "Acme", &[(widgets, s.widget, 1)]);
    assert!(invoice_from_order(&s.pool, saved.id, &input, &takes).await.unwrap_err().starts_with("SO_CLOSED"));
    assert_eq!(entries(&s.pool).await, before, "nothing posted");
    assert_eq!(detail(&s.pool, saved.id).await.unwrap().summary.status, "CLOSED");
    assert_eq!(pending_sales(&s.pool).await.unwrap().total_cents, 2_500, "only the other order is pending");
    reopen(&s.pool, saved.id).await.unwrap();
    assert_eq!(detail(&s.pool, saved.id).await.unwrap().summary.status, "OPEN");
    delete(&s.pool, other.id).await.unwrap();

    // A job of the customer may be invoiced for the customer's order.
    let acme: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Acme'").fetch_one(&s.pool).await.unwrap();
    sqlx::query("INSERT INTO customers (name, parent_id) VALUES ('Acme:Warehouse', ?1)").bind(acme).execute(&s.pool).await.unwrap();
    let (input, takes) = invoice(&s, "Acme:Warehouse", &[(widgets, s.widget, 2)]);
    invoice_from_order(&s.pool, saved.id, &input, &takes).await.unwrap();
}

#[tokio::test]
async fn voiding_an_invoice_reopens_its_quantities_and_an_edit_keeps_them() {
    let s = shop().await;
    let saved = create(&s.pool, &order("Acme", d(2), vec![line(&s, s.widget, 5, 2_500)])).await.unwrap();
    let widgets = detail(&s.pool, saved.id).await.unwrap().lines[0].id;
    let (input, takes) = invoice(&s, "Acme", &[(widgets, s.widget, 2)]);
    let first = invoice_from_order(&s.pool, saved.id, &input, &takes).await.unwrap();

    // An edit replaces the invoice; the order still counts the 2 as invoiced, now by the new one.
    let (edited, _) = invoice(&s, "Acme", &[(widgets, s.widget, 2)]);
    let new_id = crate::edits::replace_invoice(&s.pool, first.id, edited).await.unwrap();
    let after_edit = detail(&s.pool, saved.id).await.unwrap();
    assert_eq!(after_edit.lines[0].invoiced_qty_e4, 20_000);
    assert_eq!(after_edit.invoices.iter().map(|i| (i.invoice_id, i.is_voided)).collect::<Vec<_>>(), vec![(new_id, false), (first.id, true)]);

    let entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM invoices WHERE id = ?1").bind(new_id).fetch_one(&s.pool).await.unwrap();
    crate::ledger::void_transaction(&s.pool, entry).await.unwrap();
    let after_void = detail(&s.pool, saved.id).await.unwrap();
    assert_eq!((after_void.lines[0].invoiced_qty_e4, after_void.lines[0].left_qty_e4, after_void.summary.status.as_str()), (0, 50_000, "OPEN"));
    assert!(after_void.summary.has_invoices, "voided invoices still came from it");

    // An edit that ships fewer, or moves the invoice to another customer, gives the rest back.
    let (input, takes) = invoice(&s, "Acme", &[(widgets, s.widget, 3)]);
    let third = invoice_from_order(&s.pool, saved.id, &input, &takes).await.unwrap();
    let (fewer, _) = invoice(&s, "Acme", &[(widgets, s.widget, 1)]);
    let fourth = crate::edits::replace_invoice(&s.pool, third.id, fewer).await.unwrap();
    assert_eq!(detail(&s.pool, saved.id).await.unwrap().lines[0].invoiced_qty_e4, 10_000);
    let (elsewhere, _) = invoice(&s, "Globex", &[(widgets, s.widget, 1)]);
    crate::edits::replace_invoice(&s.pool, fourth, elsewhere).await.unwrap();
    assert_eq!(detail(&s.pool, saved.id).await.unwrap().lines[0].invoiced_qty_e4, 0);
}

#[tokio::test]
async fn orders_take_priced_lines_once_each_and_a_tax_rate() {
    let s = shop().await;
    assert!(
        create(&s.pool, &order("Acme", d(2), vec![line(&s, s.widget, 1, 0)])).await.unwrap_err().starts_with("SO_BAD_RATE"),
        "a 0.00 line could never be invoiced"
    );
    let rate = crate::sales_tax::create_tax_rate(
        &s.pool,
        crate::sales_tax::TaxRateInput { name: "State".into(), rate_ppm: 50_000, agency_name: None, liability_account_id: None },
    )
    .await
    .unwrap();
    let mut taxed = order("Acme", d(2), vec![SalesOrderLineInput { line: SalesLineInput { is_taxable: true, ..line(&s, s.widget, 2, 2_500).line }, id: None }]);
    taxed.tax_rate_id = Some(rate.id);
    let saved = create(&s.pool, &taxed).await.unwrap();
    let found = detail(&s.pool, saved.id).await.unwrap();
    assert_eq!((found.tax_rate_id, found.lines[0].is_taxable), (Some(rate.id), true));

    let id = found.lines[0].id;
    let again = SalesOrderLineInput { id: Some(id), ..line(&s, s.widget, 2, 2_500) };
    assert!(update(&s.pool, saved.id, &order("Acme", d(2), vec![again.clone(), again])).await.unwrap_err().starts_with("SO_LINE_TWICE"));
}

#[tokio::test]
async fn editing_a_partly_invoiced_order_keeps_what_was_invoiced() {
    let s = shop().await;
    let saved = create(&s.pool, &order("Acme", d(2), vec![line(&s, s.widget, 5, 2_500), line(&s, s.service, 1, 10_000)])).await.unwrap();
    let lines = detail(&s.pool, saved.id).await.unwrap().lines;
    let (input, takes) = invoice(&s, "Acme", &[(lines[0].id, s.widget, 3)]);
    invoice_from_order(&s.pool, saved.id, &input, &takes).await.unwrap();

    let keep = |id, qty| SalesOrderLineInput { id: Some(id), ..line(&s, s.widget, qty, 2_500) };
    // Below the 3 invoiced, dropping the invoiced line, or another customer: refused.
    assert!(update(&s.pool, saved.id, &order("Acme", d(2), vec![keep(lines[0].id, 2)])).await.unwrap_err().starts_with("SO_BELOW_INVOICED"));
    assert!(update(&s.pool, saved.id, &order("Acme", d(2), vec![line(&s, s.service, 1, 10_000)])).await.unwrap_err().starts_with("SO_LINE_INVOICED"));
    assert!(update(&s.pool, saved.id, &order("Globex", d(2), vec![keep(lines[0].id, 5)])).await.unwrap_err().starts_with("SO_INVOICED_CUSTOMER"));

    // Keep the widgets at 4, drop the setup line, add a new one first.
    let changed = update(&s.pool, saved.id, &order("Acme", d(2), vec![line(&s, s.service, 2, 10_000), keep(lines[0].id, 4)])).await.unwrap();
    assert_eq!(changed.open_cents, 20_000 + 2_500);
    let now = detail(&s.pool, saved.id).await.unwrap().lines;
    assert_eq!(
        now.iter().map(|l| (l.line_number, l.product_service_id, l.left_qty_e4)).collect::<Vec<_>>(),
        vec![(1, Some(s.service), 20_000), (2, Some(s.widget), 10_000)]
    );

    let pending = pending_sales(&s.pool).await.unwrap();
    assert_eq!((pending.rows.len(), pending.rows[0].orders, pending.total_cents), (1, 1, 22_500));
}

#[tokio::test]
async fn an_order_prints_queues_for_email_and_has_its_reports() {
    let s = shop().await;
    let rate = crate::sales_tax::create_tax_rate(
        &s.pool,
        crate::sales_tax::TaxRateInput { name: "State".into(), rate_ppm: 50_000, agency_name: None, liability_account_id: None },
    )
    .await
    .unwrap();
    let mut taxed = order(
        "Acme",
        d(2),
        vec![SalesOrderLineInput { line: SalesLineInput { is_taxable: true, ..line(&s, s.widget, 10, 2_500).line }, id: None }, line(&s, s.service, 1, 10_000)],
    );
    taxed.tax_rate_id = Some(rate.id);
    taxed.header.po_number = Some("PO-77".into());
    let saved = create(&s.pool, &taxed).await.unwrap();

    // The form: lines as ordered, sales tax at the order's rate on the taxable line.
    let doc = crate::forms::document(&s.pool, crate::forms::FormKind::SalesOrder, Some(saved.id), None, "$").await.unwrap();
    assert_eq!((doc.number.as_str(), doc.lines.len(), doc.total.as_str()), ("1", 2, "$362.50"));
    assert!(doc.totals.iter().any(|t| t.label == "Sales tax (State)" && t.value == "$12.50"), "{:?}", doc.totals);
    crate::forms::render(&s.pool, crate::forms::FormKind::SalesOrder, Some(saved.id), None, None, false).await.expect("renders a PDF");
    crate::email_log::set_queued(&s.pool, crate::forms::FormKind::SalesOrder, saved.id, true).await.unwrap();
    let queued = crate::email_log::queue(&s.pool).await.unwrap();
    assert_eq!(
        (queued.len(), queued[0].kind, queued[0].number, queued[0].total_cents),
        (1, crate::forms::FormKind::SalesOrder, 1, 36_250),
        "the total the form prints, tax included"
    );

    // The Report Center's three reports add up to what is left to invoice.
    let today = d(20);
    for id in ["open-so-customer", "open-so-item", "pending-sales"] {
        let report = crate::list_reports::run(&s.pool, id, None, None, today).await.unwrap();
        let total = report.report.rows.last().unwrap().cells.last().unwrap().as_i64().unwrap();
        assert_eq!(total, 35_000, "{id}");
    }
    let by_item = crate::list_reports::run(&s.pool, "open-so-item", None, None, today).await.unwrap();
    let heading = by_item.report.rows.iter().find_map(|r| r.cells[0].as_str().filter(|t| t.starts_with("Widget"))).unwrap();
    assert_eq!(heading, "Widget · on hand 7 · on sales orders 10 · available -3");
}

#[tokio::test]
async fn an_estimate_becomes_an_order_once_and_is_then_invoiced_through_it() {
    let s = shop().await;
    let est_line = |product, qty: i64, rate| SalesLineInput {
        product_service_id: Some(product),
        description: "quoted".into(),
        quantity_e4: qty * 10_000,
        rate_cents: rate,
        income_account_id: s.sales,
        class_id: None,
        is_taxable: false,
        service_date: None,
    };
    let estimate = crate::estimates::create_estimate(
        &s.pool,
        crate::estimates::EstimateInput {
            estimate_number: None,
            customer_name: "Acme".into(),
            estimate_date: d(1),
            expiration_date: None,
            memo: Some("Spring order".into()),
            lines: vec![est_line(s.widget, 4, Cents(2_500)), est_line(s.service, 1, Cents(10_000))],
            tax_rate_id: None,
            discount_cents: Cents(1_000),
            details: Default::default(),
        },
    )
    .await
    .unwrap();
    let made = create_from_estimate(&s.pool, estimate.id, d(3)).await.unwrap();
    assert_eq!((made.order.customer_name.as_str(), made.order.total_cents, made.order.memo.as_deref()), ("Acme", 20_000, Some("Spring order")));
    assert_eq!(made.left_out, vec!["the $10.00 discount".to_string()]);
    let lines = detail(&s.pool, made.order.id).await.unwrap().lines;
    assert_eq!(lines.iter().map(|l| (l.product_service_id, l.quantity_e4)).collect::<Vec<_>>(), vec![(Some(s.widget), 40_000), (Some(s.service), 10_000)]);

    assert!(create_from_estimate(&s.pool, estimate.id, d(3)).await.unwrap_err().starts_with("EST_ORDERED"));
    assert!(crate::estimates::convert_to_invoice(&s.pool, estimate.id, d(5), d(30)).await.unwrap_err().starts_with("EST_ORDERED"));
    let listed = crate::estimates::list_estimates(&s.pool).await.unwrap();
    assert_eq!(listed[0].sales_order_number, Some(made.order.order_number));
    assert!(crate::estimates::set_estimate_status(&s.pool, estimate.id, "REJECTED").await.unwrap_err().starts_with("EST_ORDERED"));

    // Deleting the order (nothing invoiced) frees the estimate again.
    delete(&s.pool, made.order.id).await.unwrap();
    assert_eq!(crate::estimates::list_estimates(&s.pool).await.unwrap()[0].sales_order_number, None);
    let again = create_from_estimate(&s.pool, estimate.id, d(4)).await.unwrap();
    assert_eq!(again.order.order_number, 1, "the deleted order's number is free again");

    // With sales orders off, an estimate can't become one.
    crate::preferences::set(&s.pool, crate::sales_prefs::SALES_ORDERS, "false").await.unwrap();
    delete(&s.pool, again.order.id).await.unwrap();
    assert!(create_from_estimate(&s.pool, estimate.id, d(4)).await.unwrap_err().starts_with("SO_OFF"));
}
