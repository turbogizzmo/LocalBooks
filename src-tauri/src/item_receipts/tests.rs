use super::*;
use crate::db::test_pool;
use crate::inventory::{list_items, track_product, TrackInput};
use crate::money::Cents;
use serde_json::json;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn gl(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1")
        .bind(account)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 4, day).unwrap()
}

struct Shop {
    pool: SqlitePool,
    product: i64,
    item: i64,
    inventory: i64,
    cogs: i64,
    sales: i64,
    freight: i64,
    ap: i64,
    vendor: i64,
}

async fn shop() -> Shop {
    let pool = test_pool().await;
    let sales = account(&pool, "4000").await;
    let product = crate::sales::create_product(
        &pool,
        crate::sales::ProductInput { name: "Widget".into(), description: None, rate_cents: Cents(2_500), income_account_id: sales, is_taxable: false },
    )
    .await
    .unwrap()
    .id;
    let tracked = track_product(&pool, &TrackInput { product_id: product, sku: None, asset_account_id: None, cogs_account_id: None }).await.unwrap();
    let ap: i64 =
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'ACCOUNTS_PAYABLE' AND is_system = 1").fetch_one(&pool).await.unwrap();
    let freight = account(&pool, "6000").await;
    let mut tx = pool.begin().await.unwrap();
    let vendor = crate::sales::find_or_create_named(&mut tx, crate::sales::Party::Vendor, "Supplier").await.unwrap();
    tx.commit().await.unwrap();
    Shop { inventory: tracked.asset_account_id, cogs: tracked.cogs_account_id, item: tracked.id, product, sales, freight, ap, vendor, pool }
}

async fn stock(s: &Shop) -> (i64, i64) {
    let item = list_items(&s.pool).await.unwrap().into_iter().find(|i| i.id == s.item).unwrap();
    (item.qty_on_hand_e4, item.value_cents)
}

/// An order for 10 widgets at $10.00 and $20.00 freight.
async fn order(s: &Shop) -> (i64, Vec<OrderLineReceiving>) {
    let input: crate::workflows::PurchaseOrderInput = serde_json::from_value(json!({
        "vendorName": "Supplier", "poDate": "2026-04-01", "memo": null,
        "lines": [
            { "accountId": s.inventory, "productServiceId": s.product, "description": "Widget", "quantityE4": 100_000, "rateCents": 1_000, "amountCents": 10_000 },
            { "accountId": s.freight, "description": "Freight", "amountCents": 2_000 }
        ]
    }))
    .unwrap();
    let id = crate::workflows::create_purchase_order(&s.pool, input).await.unwrap();
    (id, order_receiving(&s.pool, id).await.unwrap())
}

fn widgets(s: &Shop, po_line: Option<i64>, qty: i64) -> ReceiptLineInput {
    ReceiptLineInput {
        purchase_order_line_id: po_line,
        account_id: s.inventory,
        product_service_id: Some(s.product),
        description: Some("Widget".into()),
        quantity_e4: Some(qty * 10_000),
        rate_cents: Some(Cents(1_000)),
        amount_cents: Cents(qty * 1_000),
        class_id: None,
    }
}

fn receive(s: &Shop, order: Option<i64>, date: NaiveDate, lines: Vec<ReceiptLineInput>) -> ReceiptInput {
    ReceiptInput { vendor_id: s.vendor, purchase_order_id: order, receipt_date: date, reference: Some("PS-77".into()), memo: None, lines }
}

async fn invoice(s: &Shop, qty: i64) {
    let input: crate::invoices::InvoiceInput = serde_json::from_value(json!({
        "customerName": "Customer", "issueDate": "2026-04-10", "dueDate": "2026-05-10",
        "lines": [{ "description": "Widget", "quantityE4": qty * 10_000, "rateCents": 2_500, "incomeAccountId": s.sales, "productServiceId": s.product }]
    }))
    .unwrap();
    crate::invoices::create_and_post_invoice(&s.pool, input).await.unwrap();
}

async fn bill_for(s: &Shop, receipt: &ReceiptView, amounts: &[i64]) -> Result<i64, String> {
    let lines = receipt_lines(&s.pool, receipt.id).await.unwrap();
    let lines = lines.iter().zip(amounts).map(|(l, a)| ReceiptBillLine { receipt_line_id: l.id, amount_cents: Cents(*a) }).collect();
    bill_received_items(
        &s.pool,
        ReceiptBillInput {
            item_receipt_id: receipt.id,
            bill_number: Some("INV-9".into()),
            bill_date: d(20),
            due_date: d(30),
            terms_id: None,
            memo: None,
            lines,
        },
    )
    .await
}

#[tokio::test]
async fn a_receipt_raises_stock_against_the_clearing_account_not_payables() {
    let s = shop().await;
    let (po, lines) = order(&s).await;
    let freight = ReceiptLineInput {
        purchase_order_line_id: Some(lines[1].id),
        account_id: s.freight,
        product_service_id: None,
        description: Some("Freight".into()),
        quantity_e4: None,
        rate_cents: None,
        amount_cents: Cents(2_000),
        class_id: None,
    };
    let r = receive_items(&s.pool, receive(&s, Some(po), d(5), vec![widgets(&s, Some(lines[0].id), 4), freight])).await.unwrap();
    assert_eq!((r.receipt_number, r.total_cents, r.po_number.is_some(), r.bill_id, r.is_voided), (1, 6_000, true, None, false));

    let clearing: i64 =
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = 'Items Received Not Billed' AND is_system = 1").fetch_one(&s.pool).await.unwrap();
    assert_eq!(gl(&s.pool, clearing).await, -6_000, "the receipt is owed in the clearing liability");
    assert_eq!(gl(&s.pool, s.ap).await, 0, "nothing reaches Accounts Payable");
    assert_eq!((gl(&s.pool, s.inventory).await, gl(&s.pool, s.freight).await), (4_000, 2_000));
    assert_eq!(stock(&s).await, (40_000, 4_000));
    assert_eq!(received_not_billed(&s.pool, d(30)).await.unwrap(), 6_000);
    assert_eq!(received_not_billed(&s.pool, d(4)).await.unwrap(), 0, "as of a date before the receipt");

    // Partly received: the order stays open, and shows what came in.
    let progress = order_receiving(&s.pool, po).await.unwrap();
    assert_eq!(progress.iter().map(|l| (l.received_quantity_e4, l.received_amount_cents)).collect::<Vec<_>>(), [(40_000, 4_000), (0, 2_000)]);
    let status = || sqlx::query_scalar::<_, String>("SELECT status FROM purchase_orders WHERE id = ?1").bind(po).fetch_one(&s.pool);
    assert_eq!(status().await.unwrap(), "OPEN");
    assert!(crate::workflows::bill_purchase_order(&s.pool, po, d(6), d(30), None).await.unwrap_err().starts_with("PO_HAS_RECEIPTS"));

    // The rest arrives: the order closes. Voiding that receipt reopens it and takes its stock back.
    let rest = receive_items(&s.pool, receive(&s, Some(po), d(8), vec![widgets(&s, Some(lines[0].id), 6)])).await.unwrap();
    assert_eq!(status().await.unwrap(), "CLOSED");
    assert!(receive_items(&s.pool, receive(&s, Some(po), d(9), vec![widgets(&s, Some(lines[0].id), 1)]))
        .await
        .unwrap_err()
        .starts_with("RECEIPT_ORDER_NOT_OPEN"));
    // Received in full, it can't be reopened to receive more than was ordered.
    let reopen = crate::workflows::set_purchase_order_closed(&s.pool, po, false).await.unwrap_err();
    assert!(reopen.starts_with("PO_RECEIVED_IN_FULL"), "{reopen}");
    assert_eq!(status().await.unwrap(), "CLOSED");
    crate::ledger::void_transaction(&s.pool, rest.journal_entry_id).await.unwrap();
    assert_eq!(status().await.unwrap(), "OPEN");
    assert_eq!(stock(&s).await, (40_000, 4_000));
    assert_eq!(list_receipts(&s.pool, true).await.unwrap().iter().map(|r| r.id).collect::<Vec<_>>(), [r.id], "a voided receipt isn't waiting for a bill");

    // A receipt's entry isn't edited as a journal entry.
    let entry = crate::ledger::JournalEntry {
        id: None,
        date: d(5),
        description: "x".into(),
        lines: vec![crate::ledger::JournalLine::debit(s.freight, 1), crate::ledger::JournalLine::credit(s.sales, 1)],
        currency: None,
    };
    assert!(crate::edits::replace_journal_entry(&s.pool, r.journal_entry_id, entry).await.unwrap_err().starts_with("EDIT_NOT_MANUAL"));
}

#[tokio::test]
async fn the_bill_clears_the_receipt_and_posts_its_price_difference() {
    let s = shop().await;
    let clearing_of = async |s: &Shop| {
        let id: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = 'Items Received Not Billed'").fetch_one(&s.pool).await.unwrap();
        gl(&s.pool, id).await
    };
    // 10 widgets received at $10.00; 4 sold before the bill comes.
    let freight = ReceiptLineInput {
        purchase_order_line_id: None,
        account_id: s.freight,
        product_service_id: None,
        description: Some("Freight".into()),
        quantity_e4: None,
        rate_cents: None,
        amount_cents: Cents(2_000),
        class_id: None,
    };
    let r = receive_items(&s.pool, receive(&s, None, d(5), vec![widgets(&s, None, 10), freight])).await.unwrap();
    invoice(&s, 4).await;
    assert_eq!(stock(&s).await, (60_000, 6_000));
    let cogs_before = gl(&s.pool, s.cogs).await;
    assert_eq!(cogs_before, 4_000);

    // Billed at $10.50 a widget ($105.00, +$5.00) and $18.00 freight (-$2.00): of the $5.00, the six
    // widgets on hand take $3.00 and the four sold $2.00.
    let bill = bill_for(&s, &r, &[10_500, 1_800]).await.unwrap();
    assert_eq!(clearing_of(&s).await, 0, "the bill clears what the receipt owed");
    assert_eq!(gl(&s.pool, s.ap).await, -12_300, "A/P holds the bill's amount");
    assert_eq!(stock(&s).await, (60_000, 6_300));
    assert_eq!(gl(&s.pool, s.inventory).await, 6_300, "stock and Inventory Asset agree");
    assert_eq!(gl(&s.pool, s.cogs).await, cogs_before + 200);
    assert_eq!(gl(&s.pool, s.freight).await, 1_800);
    let view = receipt(&s.pool, r.id).await.unwrap();
    assert_eq!((view.bill_id, view.bill_number.as_deref()), (Some(bill), Some("INV-9")));
    assert!(list_receipts(&s.pool, true).await.unwrap().is_empty());
    let summary: (i64, String) = sqlx::query_as("SELECT total_cents, status FROM bills WHERE id = ?1").bind(bill).fetch_one(&s.pool).await.unwrap();
    assert_eq!(summary, (12_300, "UNPAID".to_string()));

    // Billed once; the receipt can't be voided under its bill, and the bill isn't edited.
    assert!(bill_for(&s, &r, &[10_000, 2_000]).await.unwrap_err().starts_with("RECEIPT_BILLED"));
    assert!(crate::ledger::void_transaction(&s.pool, r.journal_entry_id).await.unwrap_err().starts_with("VOID_RECEIPT_BILLED"));
    let edit: crate::expenses::BillInput = serde_json::from_value(json!({
        "vendorName": "Supplier", "billDate": "2026-04-20", "dueDate": "2026-04-30", "memo": null, "billNumber": null, "totalCents": 100,
        "lines": [{ "accountId": s.freight, "amountCents": 100, "memo": null }]
    }))
    .unwrap();
    assert!(crate::edits::replace_bill(&s.pool, bill, edit).await.unwrap_err().starts_with("EDIT_RECEIPT_BILL"));

    // Voiding the bill puts everything back as the receipt left it; then the receipt voids too.
    let bill_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM bills WHERE id = ?1").bind(bill).fetch_one(&s.pool).await.unwrap();
    crate::ledger::void_transaction(&s.pool, bill_entry).await.unwrap();
    assert_eq!((clearing_of(&s).await, gl(&s.pool, s.ap).await, stock(&s).await), (-12_000, 0, (60_000, 6_000)));
    assert_eq!(list_receipts(&s.pool, true).await.unwrap().len(), 1, "open again");
    // With the bill gone the receipt voids, as a voided bill would: its ten widgets leave stock
    // (four were sold, so stock goes below zero), and nothing is owed for them any more.
    crate::ledger::void_transaction(&s.pool, r.journal_entry_id).await.unwrap();
    assert_eq!((clearing_of(&s).await, stock(&s).await), (0, (-40_000, -4_000)));
    assert_eq!(gl(&s.pool, s.inventory).await, -4_000, "stock and Inventory Asset still agree");
}

#[tokio::test]
async fn receipts_and_their_bills_are_checked() {
    let s = shop().await;
    let (po, lines) = order(&s).await;
    let other = {
        let mut tx = s.pool.begin().await.unwrap();
        let id = crate::sales::find_or_create_named(&mut tx, crate::sales::Party::Vendor, "Someone Else").await.unwrap();
        tx.commit().await.unwrap();
        id
    };
    let err = |r: Result<ReceiptView, String>| r.unwrap_err();
    assert!(err(receive_items(&s.pool, receive(&s, None, d(5), vec![])).await).starts_with("RECEIPT_NO_LINES"));
    assert!(err(receive_items(&s.pool, ReceiptInput { vendor_id: other, ..receive(&s, Some(po), d(5), vec![widgets(&s, Some(lines[0].id), 1)]) }).await)
        .starts_with("RECEIPT_OTHER_VENDOR"));
    assert!(err(receive_items(&s.pool, receive(&s, None, d(5), vec![widgets(&s, Some(lines[0].id), 1)])).await).starts_with("RECEIPT_BAD_ORDER_LINE"));
    let wrong_account = ReceiptLineInput { account_id: s.freight, ..widgets(&s, None, 1) };
    assert!(err(receive_items(&s.pool, receive(&s, None, d(5), vec![wrong_account])).await).starts_with("STOCK_PURCHASE_ACCOUNT"));
    let to_income = ReceiptLineInput { account_id: s.sales, product_service_id: None, ..widgets(&s, None, 1) };
    assert!(err(receive_items(&s.pool, receive(&s, None, d(5), vec![to_income])).await).starts_with("RECEIPT_BAD_CATEGORY"));
    assert!(list_receipts(&s.pool, false).await.unwrap().is_empty(), "nothing saved");

    // Receiving more than ordered is allowed; the order closes.
    let r = receive_items(
        &s.pool,
        receive(
            &s,
            Some(po),
            d(5),
            vec![
                widgets(&s, Some(lines[0].id), 12),
                ReceiptLineInput {
                    purchase_order_line_id: Some(lines[1].id),
                    account_id: s.freight,
                    product_service_id: None,
                    description: None,
                    quantity_e4: None,
                    rate_cents: None,
                    amount_cents: Cents(2_000),
                    class_id: None,
                },
            ],
        ),
    )
    .await
    .unwrap();
    assert_eq!(r.total_cents, 14_000);
    let receipt_line = receipt_lines(&s.pool, r.id).await.unwrap();
    // The bill names every receipt line once, with an amount above zero, dated no later than due.
    let bill = |lines: Vec<ReceiptBillLine>, due: NaiveDate| {
        bill_received_items(
            &s.pool,
            ReceiptBillInput { item_receipt_id: r.id, bill_number: None, bill_date: d(20), due_date: due, terms_id: None, memo: None, lines },
        )
    };
    assert!(bill(vec![ReceiptBillLine { receipt_line_id: receipt_line[0].id, amount_cents: Cents(12_000) }], d(30))
        .await
        .unwrap_err()
        .starts_with("RECEIPT_BILL_LINES"));
    let both = |a: i64| {
        vec![
            ReceiptBillLine { receipt_line_id: receipt_line[0].id, amount_cents: Cents(a) },
            ReceiptBillLine { receipt_line_id: receipt_line[1].id, amount_cents: Cents(2_000) },
        ]
    };
    assert!(bill(both(0), d(30)).await.unwrap_err().starts_with("RECEIPT_BAD_AMOUNT"));
    assert!(bill(both(12_000), d(19)).await.unwrap_err().starts_with("BILL_DUE_BEFORE_DATE"));
    let id = bill(both(12_000), d(30)).await.unwrap();
    let lines: Vec<(i64, Option<i64>, Option<i64>)> =
        sqlx::query_as("SELECT amount_cents, quantity_e4, rate_cents FROM bill_lines WHERE bill_id = ?1 ORDER BY line_number")
            .bind(id)
            .fetch_all(&s.pool)
            .await
            .unwrap();
    assert_eq!(lines, [(12_000, Some(120_000), Some(1_000)), (2_000, None, None)]);
}

#[tokio::test]
async fn a_lower_price_takes_stock_value_down_to_zero_and_the_rest_to_cogs() {
    let s = shop().await;
    // 90 widgets already on hand at a penny each in all ($1.00), then 10 received at $10.00: the
    // average is $1.01. Selling 95 leaves 5 worth $5.05.
    crate::inventory::adjust(
        &s.pool,
        &crate::inventory::AdjustInput {
            item_id: s.item,
            date: d(1),
            qty_change_e4: 900_000,
            total_cost_cents: Some(100),
            offset_account_id: s.freight,
            memo: None,
        },
    )
    .await
    .unwrap();
    let r = receive_items(&s.pool, receive(&s, None, d(5), vec![widgets(&s, None, 10)])).await.unwrap();
    invoice(&s, 95).await;
    assert_eq!(stock(&s).await, (50_000, 505));
    let cogs_before = gl(&s.pool, s.cogs).await;

    // Billed at a cent in all: -$99.99. Half the received units are on hand, so their share is
    // -$50.00, but stock is worth only $5.05: it goes to zero and COGS takes the other $94.94.
    bill_for(&s, &r, &[1]).await.unwrap();
    assert_eq!(stock(&s).await, (50_000, 0));
    assert_eq!(gl(&s.pool, s.inventory).await, 0, "stock and Inventory Asset agree");
    assert_eq!(gl(&s.pool, s.cogs).await, cogs_before - 9_494);
}

#[tokio::test]
async fn a_po_line_s_job_carries_through_the_receipt_to_the_bill() {
    let s = shop().await;
    let mut tx = s.pool.begin().await.unwrap();
    let job = crate::sales::find_or_create_named(&mut tx, crate::sales::Party::Customer, "Kitchen remodel").await.unwrap();
    tx.commit().await.unwrap();
    sqlx::query("INSERT OR REPLACE INTO system_preferences (key, value) VALUES (?1, 'true')")
        .bind(crate::preferences::tax_time::EXPENSES_BILLABLE)
        .execute(&s.pool)
        .await
        .unwrap();
    // Widgets for the job; freight for no one.
    let input: crate::workflows::PurchaseOrderInput = serde_json::from_value(json!({
        "vendorName": "Supplier", "poDate": "2026-04-01", "memo": null,
        "lines": [
            { "accountId": s.inventory, "productServiceId": s.product, "description": "Widget", "quantityE4": 100_000, "rateCents": 1_000, "amountCents": 10_000, "customerId": job },
            { "accountId": s.freight, "description": "Freight", "amountCents": 2_000 }
        ]
    }))
    .unwrap();
    let po = crate::workflows::create_purchase_order(&s.pool, input).await.unwrap();
    let lines = order_receiving(&s.pool, po).await.unwrap();
    assert_eq!(lines.iter().map(|l| (l.customer_id, l.customer_name.as_deref())).collect::<Vec<_>>(), vec![(Some(job), Some("Kitchen remodel")), (None, None)]);

    let freight = ReceiptLineInput {
        purchase_order_line_id: Some(lines[1].id),
        account_id: s.freight,
        product_service_id: None,
        description: Some("Freight".into()),
        quantity_e4: None,
        rate_cents: None,
        amount_cents: Cents(2_000),
        class_id: None,
    };
    let receipt = receive_items(&s.pool, receive(&s, Some(po), d(5), vec![widgets(&s, Some(lines[0].id), 10), freight])).await.unwrap();
    let received = receipt_lines(&s.pool, receipt.id).await.unwrap();
    assert_eq!(received.iter().map(|l| l.customer_id).collect::<Vec<_>>(), vec![Some(job), None]);

    let bill = bill_for(&s, &receipt, &[10_000, 2_000]).await.unwrap();
    let billed: Vec<(Option<i64>, bool)> =
        sqlx::query_as("SELECT customer_id, is_billable FROM bill_lines WHERE bill_id = ?1 ORDER BY line_number").bind(bill).fetch_all(&s.pool).await.unwrap();
    assert_eq!(billed, vec![(Some(job), true), (None, false)], "the job's line is billable by the company's preference; a line for no one never is");
}

#[tokio::test]
async fn a_receipts_bill_takes_the_orders_discounts_and_charges_once() {
    let s = shop().await;
    // 10 widgets at $10.00 with 10% off, and $20.00 freight with a $5.00 handling charge.
    let input: crate::workflows::PurchaseOrderInput = serde_json::from_value(json!({
        "vendorName": "Supplier", "poDate": "2026-04-01", "memo": null,
        "lines": [
            { "accountId": s.inventory, "productServiceId": s.product, "description": "Widget", "quantityE4": 100_000, "rateCents": 1_000, "amountCents": 10_000 },
            { "accountId": s.freight, "description": "Freight", "amountCents": 2_000 }
        ],
        "formLines": [
            { "afterLine": 1, "kind": "DISCOUNT", "percentPpm": 100_000, "accountId": s.sales, "description": "Volume discount" },
            { "afterLine": 2, "kind": "CHARGE", "amountCents": 500, "accountId": s.freight, "description": "Handling" }
        ]
    }))
    .unwrap();
    let po = crate::workflows::create_purchase_order(&s.pool, input).await.unwrap();
    let lines = order_receiving(&s.pool, po).await.unwrap();
    let freight = |cents: i64| ReceiptLineInput {
        purchase_order_line_id: Some(lines[1].id),
        account_id: s.freight,
        product_service_id: None,
        description: Some("Freight".into()),
        quantity_e4: None,
        rate_cents: None,
        amount_cents: Cents(cents),
        class_id: None,
    };

    // First shipment: 4 widgets and half the freight. The bill takes 10% of $40.00 and the handling.
    let first = receive_items(&s.pool, receive(&s, Some(po), d(5), vec![widgets(&s, Some(lines[0].id), 4), freight(1_000)])).await.unwrap();
    let preview = receipt_bill_form_lines(&s.pool, first.id).await.unwrap();
    assert_eq!(preview.iter().map(|f| (f.line.after_line, f.line.description.as_str())).collect::<Vec<_>>(), [(1, "Volume discount"), (2, "Handling")]);
    let bill = bill_for(&s, &first, &[4_000, 1_000]).await.unwrap();
    let carried = crate::purchase_lines::lines_of(&s.pool, crate::purchase_lines::PurchaseDoc::Bill, bill).await.unwrap();
    assert_eq!(carried.iter().map(|f| (f.after_line, f.amount_cents)).collect::<Vec<_>>(), [(1, 400), (2, 500)]);
    let total: i64 = sqlx::query_scalar("SELECT total_cents FROM bills WHERE id = ?1").bind(bill).fetch_one(&s.pool).await.unwrap();
    assert_eq!(total, 4_000 + 1_000 - 400 + 500);
    assert_eq!(gl(&s.pool, s.ap).await, -5_100);
    assert_eq!(gl(&s.pool, s.sales).await, -400, "the discount is credited to its account");

    // The rest: 10% of $60.00 again, but the fixed handling charge is already on the first bill.
    let second = receive_items(&s.pool, receive(&s, Some(po), d(6), vec![widgets(&s, Some(lines[0].id), 6), freight(1_000)])).await.unwrap();
    assert_eq!(receipt_bill_form_lines(&s.pool, second.id).await.unwrap().len(), 1);
    let bill2 = bill_for(&s, &second, &[6_000, 1_000]).await.unwrap();
    let carried = crate::purchase_lines::lines_of(&s.pool, crate::purchase_lines::PurchaseDoc::Bill, bill2).await.unwrap();
    assert_eq!(carried.iter().map(|f| (f.kind.as_str(), f.amount_cents)).collect::<Vec<_>>(), [("DISCOUNT", 600)]);
    assert_eq!(gl(&s.pool, s.ap).await, -5_100 - 6_400);

    // A receipt not against an order takes nothing.
    let loose = receive_items(&s.pool, receive(&s, None, d(7), vec![widgets(&s, None, 1)])).await.unwrap();
    assert!(receipt_bill_form_lines(&s.pool, loose.id).await.unwrap().is_empty());
}

/// One receipt with two lines from the same order line: a fixed discount too big for the first goes
/// after the second, once.
#[tokio::test]
async fn a_fixed_discount_goes_after_the_receipt_line_that_can_take_it() {
    let s = shop().await;
    let input: crate::workflows::PurchaseOrderInput = serde_json::from_value(json!({
        "vendorName": "Supplier", "poDate": "2026-04-01", "memo": null,
        "lines": [{ "accountId": s.inventory, "productServiceId": s.product, "description": "Widget", "quantityE4": 100_000, "rateCents": 1_000, "amountCents": 10_000 }],
        "formLines": [{ "afterLine": 1, "kind": "DISCOUNT", "amountCents": 2_500, "accountId": s.sales, "description": "Rebate" }]
    }))
    .unwrap();
    let po = crate::workflows::create_purchase_order(&s.pool, input).await.unwrap();
    let lines = order_receiving(&s.pool, po).await.unwrap();
    // 2 widgets ($20.00, less than the $25.00 rebate) and 8 more ($80.00) on one receipt.
    let r = receive_items(&s.pool, receive(&s, Some(po), d(5), vec![widgets(&s, Some(lines[0].id), 2), widgets(&s, Some(lines[0].id), 8)])).await.unwrap();
    assert_eq!(receipt_bill_form_lines(&s.pool, r.id).await.unwrap().iter().map(|f| (f.line.after_line, f.source)).collect::<Vec<_>>(), [(1, 0), (2, 0)]);
    let bill = bill_for(&s, &r, &[2_000, 8_000]).await.unwrap();
    let carried = crate::purchase_lines::lines_of(&s.pool, crate::purchase_lines::PurchaseDoc::Bill, bill).await.unwrap();
    assert_eq!(carried.iter().map(|f| (f.after_line, f.amount_cents)).collect::<Vec<_>>(), [(2, 2_500)]);
    let total: i64 = sqlx::query_scalar("SELECT total_cents FROM bills WHERE id = ?1").bind(bill).fetch_one(&s.pool).await.unwrap();
    assert_eq!(total, 10_000 - 2_500);
}

#[tokio::test]
async fn an_order_received_against_cant_be_edited_or_deleted_even_once_the_receipt_is_voided() {
    let s = shop().await;
    let (po, lines) = order(&s).await;
    let r = receive_items(&s.pool, receive(&s, Some(po), d(5), vec![widgets(&s, Some(lines[0].id), 4)])).await.unwrap();
    let edit = |pool: SqlitePool| async move {
        let input: crate::workflows::PurchaseOrderInput = serde_json::from_value(json!({
            "vendorName": "Supplier", "poDate": "2026-04-01", "memo": null,
            "lines": [{ "accountId": 1, "description": "Changed", "amountCents": 500 }]
        }))
        .unwrap();
        crate::workflows::update_purchase_order(&pool, po, input).await
    };
    assert!(edit(s.pool.clone()).await.unwrap_err().starts_with("PO_HAS_RECEIPTS"));
    // Voided, the receipt still points at the order's lines: a clear refusal, not a database error.
    crate::ledger::void_transaction(&s.pool, r.journal_entry_id).await.unwrap();
    let err = edit(s.pool.clone()).await.unwrap_err();
    assert!(err.starts_with("PO_HAS_RECEIPTS"), "{err}");
    let err = crate::workflows::delete_purchase_order(&s.pool, po).await.unwrap_err();
    assert!(err.starts_with("PO_HAS_RECEIPTS"), "{err}");
}

#[test]
fn receipt_and_purchase_form_amounts_are_bare_integers_in_json() {
    let line: ReceiptLineInput = serde_json::from_value(json!({"accountId": 5, "quantityE4": 20_000, "rateCents": 1_250, "amountCents": 2_500})).unwrap();
    assert_eq!((line.rate_cents, line.amount_cents), (Some(Cents(1_250)), Cents(2_500)));
    let billed: ReceiptBillLine = serde_json::from_value(json!({"receiptLineId": 7, "amountCents": 2_400})).unwrap();
    assert_eq!(billed.amount_cents, Cents(2_400));
    let form: crate::purchase_lines::PurchaseFormLineInput =
        serde_json::from_value(json!({"afterLine": 1, "kind": "DISCOUNT", "amountCents": 300, "accountId": 9})).unwrap();
    assert_eq!(form.amount_cents, Some(Cents(300)));
    assert_eq!(serde_json::to_value(&form).unwrap()["amountCents"], json!(300), "it goes back out as a bare integer");
    assert!(serde_json::from_value::<ReceiptBillLine>(json!({"receiptLineId": 7, "amountCents": 24.5})).is_err(), "a fractional amount is refused");
}
