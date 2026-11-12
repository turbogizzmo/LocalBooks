use super::*;
use std::path::Path;

use crate::db::test_pool;
use crate::qbw_company::{Ledger, LedgerQuality, SourceAccount, SourceItem, SourceLine, SourceTransaction};
use crate::qbw_ledger_import::{auto_map_accounts, batch_summary, discard_batch, post_batch, stage, ImportMode, PostLedgerInput};
use crate::qbw_stock::{SourceStockItem, SourceStockMove, BUILD_KIND};

fn date(d: &str) -> NaiveDate {
    d.parse().unwrap()
}

fn change(item: i64, qty: i64, value: i64, entry: Option<i64>) -> Change {
    (item, qty, value, date("2024-01-10"), "Invoice".into(), entry)
}

/// Each change is recorded on its own entry while quantity and value keep the same sign; one
/// that would leave them apart is carried into the item's next change, and what is still carried
/// at the end is reported, as are changes of transactions that were not posted.
#[test]
fn changes_that_would_leave_quantity_and_value_apart_are_carried_forward() {
    let changes = [
        // Item 1: bought 2 for 10.00, sold both at 10.01 (the file's rounding), then bought 1.
        change(1, 20_000, 1_000, Some(100)),
        change(1, -20_000, -1_001, Some(101)),
        change(1, 10_000, 600, Some(102)),
        // Item 2: a sale before its purchase on the same day leaves -1 valued at 0, which may
        // stand (value zero); then a value-only change of a bill that would leave -1 at +1.00.
        change(2, -10_000, 0, Some(200)),
        change(2, 0, 100, Some(201)),
        // Item 3: a change of a transaction that was not posted.
        change(3, 10_000, 500, None),
        // Item 4: rounding left at the end that can't stand.
        change(4, 10_000, 500, Some(400)),
        change(4, -10_000, -499, Some(401)),
    ];
    let (planned, unposted, carried) = plan(&changes);
    let rows: Vec<(i64, i64, i64, Option<i64>)> = planned.iter().map(|(i, q, v, _, _, e)| (*i, *q, *v, *e)).collect();
    assert_eq!(
        rows,
        [
            (1, 20_000, 1_000, Some(100)),
            // The sale would leave 0 units at -0.01: it is carried into the purchase after it.
            (1, -10_000, -401, Some(102)),
            (2, -10_000, 0, Some(200)),
            (4, 10_000, 500, Some(400)),
        ]
    );
    assert_eq!(unposted, HashMap::from([(3, 1)]));
    assert_eq!(carried, HashMap::from([(2, (0, 100)), (4, (-10_000, -499))]));
}

#[test]
fn transaction_kinds_give_movement_kinds() {
    assert_eq!(movement_kind("Bill"), "PURCHASE");
    assert_eq!(movement_kind("Credit card charge"), "PURCHASE");
    assert_eq!(movement_kind("Sales receipt"), "SALE");
    assert_eq!(movement_kind("Credit memo"), "SALE");
    assert_eq!(movement_kind("Inventory adjustment"), "ADJUSTMENT");
    assert_eq!(movement_kind(BUILD_KIND), "ADJUSTMENT");
}

fn account(id: i64, name: &str, kind: &'static str) -> SourceAccount {
    SourceAccount {
        id,
        name: name.into(),
        full_name: name.into(),
        account_type: kind,
        parent_id: None,
        active: true,
        stored_balance_cents: None,
        tax_line: None,
    }
}

fn txn(id: i64, kind: &'static str, d: &str, lines: &[(i64, i64)]) -> SourceTransaction {
    SourceTransaction {
        document: None,
        id,
        kind,
        date: date(d),
        doc_num: Some(id.to_string()),
        name: None,
        memo: None,
        lines: lines.iter().map(|&(account_id, cents)| SourceLine { account_id, cents, memo: None, class: None }).collect(),
    }
}

fn item(id: i64, name: &str, kind: &'static str) -> SourceItem {
    SourceItem {
        id,
        name: name.into(),
        description: None,
        rate_cents: 2_000,
        income_account_id: Some(6),
        kind,
        taxable: false,
        active: true,
        custom_fields: Vec::new(),
        purchase_description: None,
        purchase_cost_cents: None,
        percent_ppm: None,
    }
}

fn stock_item(item_id: i64, qty_e4: i64, value_cents: i64) -> SourceStockItem {
    SourceStockItem {
        item_id,
        asset_account_id: 31,
        cogs_account_id: Some(30),
        stored_qty_e4: Some(qty_e4),
        stored_value_cents: Some(value_cents),
        pending_builds_e4: 0,
    }
}

fn moved(transaction_id: i64, item_id: i64, qty_e4: i64, value_cents: i64) -> SourceStockMove {
    SourceStockMove { transaction_id, item_id, qty_e4, value_cents }
}

/// A file with purchases, a sale, an assembly build and an adjustment, built for this test.
/// Signs as in the file: negative = debit.
fn books() -> Ledger {
    Ledger {
        accounts: vec![
            account(1, "Riverside Bank", "BANK"),
            account(4, "Accounts Receivable", "ACCOUNTS_RECEIVABLE"),
            account(6, "Sales", "INCOME"),
            account(12, "Shrinkage", "EXPENSE"),
            account(30, "Cost of Goods Sold", "COST_OF_GOODS_SOLD"),
            account(31, "Inventory Asset", "OTHER_CURRENT_ASSET"),
        ],
        customers: vec![],
        vendors: vec![],
        items: vec![
            item(200, "Widget", "Inventory part"),
            item(201, "Gadget", "Inventory part"),
            item(202, "Kit", "Inventory part"),
            item(203, "Spare", "Inventory part"),
            item(204, "Installation", "Service"),
        ],
        transactions: vec![
            // 10 widgets for 100.00 and 4 gadgets for 40.00, by check.
            txn(50, "Check", "2024-01-05", &[(31, -10_000), (1, 10_000)]),
            txn(51, "Check", "2024-01-06", &[(31, -4_000), (1, 4_000)]),
            // 3 widgets sold for 60.00; they cost 30.00.
            txn(52, "Invoice", "2024-01-10", &[(4, -6_000), (6, 6_000), (30, -3_000), (31, 3_000)]),
            // 2 kits built from 2 widgets and 2 gadgets: value moves within Inventory Asset.
            txn(53, BUILD_KIND, "2024-01-12", &[(31, -4_000), (31, 2_000), (31, 2_000)]),
            // A gadget lost.
            txn(54, "Inventory adjustment", "2024-01-15", &[(31, 1_000), (12, -1_000)]),
        ],
        // A spare bought on a check the reader could not read completely.
        unbalanced: vec![txn(55, "Check", "2024-01-16", &[(31, -500)])],
        unreadable: vec![],
        quality: LedgerQuality::default(),
        sales_lists: Default::default(),
        classes: Vec::new(),
        stock: SourceStock {
            items: vec![stock_item(200, 50_000, 5_000), stock_item(201, 10_000, 1_000), stock_item(202, 20_000, 4_000), stock_item(203, 10_000, 500)],
            moves: vec![
                moved(50, 200, 100_000, 10_000),
                moved(51, 201, 40_000, 4_000),
                moved(52, 200, -30_000, -3_000),
                moved(53, 202, 20_000, 4_000),
                moved(53, 200, -20_000, -2_000),
                moved(53, 201, -20_000, -2_000),
                moved(54, 201, -10_000, -1_000),
                moved(55, 203, 10_000, 500),
            ],
            unread_sales: HashMap::new(),
        },
    }
}

async fn on_hand(pool: &SqlitePool, name: &str) -> (i64, i64) {
    sqlx::query_as("SELECT i.qty_on_hand_e4, i.value_cents FROM inventory_items i JOIN products_services p ON p.id = i.product_id WHERE p.name = ?1")
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// After a full import each item the file tracks is tracked in LocalBooks with the quantity and
/// value the file stores; the item whose purchase could not be read is listed with the reason;
/// stock and the Inventory Asset account agree; and voiding an imported entry puts its stock back.
#[tokio::test]
async fn a_full_import_brings_in_quantities_on_hand() {
    let pool = test_pool().await;
    let batch = stage(&pool, Path::new("/books/Stock.qbw"), 1, &books()).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();

    // Before posting, the summary compares the file with what the import would bring in.
    let before = batch_summary(&pool, batch).await.unwrap().stock;
    assert_eq!((before.items, before.matching), (4, 3));
    assert_eq!(before.differences.len(), 1);
    let spare = &before.differences[0];
    assert_eq!((spare.name.as_str(), spare.expected_qty_e4, spare.imported_qty_e4), ("Spare", Some(10_000), 0));
    assert_eq!(spare.note.as_deref(), Some("1 transaction that changes its stock can't be posted; it is listed as not read completely"));

    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap();
    assert_eq!(posted.stock, Some(StockResult { items: 4, tracked: 4, matching: 3, movements: 7 }));
    assert_eq!(on_hand(&pool, "Widget").await, (50_000, 5_000));
    assert_eq!(on_hand(&pool, "Gadget").await, (10_000, 1_000));
    assert_eq!(on_hand(&pool, "Kit").await, (20_000, 4_000));
    assert_eq!(on_hand(&pool, "Spare").await, (0, 0));
    let untracked: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM inventory_items i JOIN products_services p ON p.id = i.product_id WHERE p.name = 'Installation'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(untracked, 0, "a service is not tracked");

    // Stock and the Inventory Asset account agree.
    let summary = batch_summary(&pool, batch).await.unwrap();
    let asset = summary.accounts.iter().find(|a| a.full_name == "Inventory Asset").and_then(|a| a.account_id).unwrap();
    let (asset_balance, stock_value): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COALESCE(SUM(l.debit_cents - l.credit_cents), 0) FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
                 WHERE l.account_id = ?1 AND e.status = 'POSTED'),
                (SELECT SUM(value_cents) FROM inventory_items WHERE asset_account_id = ?1)",
    )
    .bind(asset)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((asset_balance, stock_value), (10_000, 10_000));

    // After posting, the summary compares with what LocalBooks holds and keeps the reason.
    assert_eq!((summary.stock.items, summary.stock.matching), (4, 3));
    let spare = &summary.stock.differences[0];
    assert_eq!((spare.name.as_str(), spare.imported_qty_e4, spare.imported_value_cents), ("Spare", 0, 0));
    assert_eq!(spare.note.as_deref(), Some("1 transaction that changes its stock could not be posted; it is listed as not read completely"));
    assert_eq!(unposted_note(2), "2 transactions that change its stock could not be posted; they are listed as not read completely");

    // The build has an entry of its own whose lines cancel, and its stock moves with it.
    let build_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM ledger_import_transactions WHERE batch_id = ?1 AND source_id = 53")
        .bind(batch)
        .fetch_one(&pool)
        .await
        .unwrap();
    let kinds: Vec<String> =
        sqlx::query_scalar("SELECT kind FROM stock_ledger_entries WHERE journal_entry_id = ?1 ORDER BY id").bind(build_entry).fetch_all(&pool).await.unwrap();
    assert_eq!(kinds, ["ADJUSTMENT", "ADJUSTMENT", "ADJUSTMENT"]);

    // Voiding the adjustment puts the lost gadget back.
    let adjustment: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM ledger_import_transactions WHERE batch_id = ?1 AND source_id = 54")
        .bind(batch)
        .fetch_one(&pool)
        .await
        .unwrap();
    crate::ledger::void_transaction(&pool, adjustment).await.unwrap();
    assert_eq!(on_hand(&pool, "Gadget").await, (20_000, 2_000));
}

/// A journal import posts entries only: items are not tracked, and the summary still shows what
/// a full import would bring in. Discarding a batch removes its staged stock.
#[tokio::test]
async fn a_journal_import_leaves_stock_alone_and_discard_removes_the_staged_stock() {
    let pool = test_pool().await;
    let batch = stage(&pool, Path::new("/books/Stock.qbw"), 1, &books()).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap();
    assert_eq!(posted.stock, None);
    let tracked: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM inventory_items").fetch_one(&pool).await.unwrap();
    assert_eq!(tracked, 0);
    assert_eq!(batch_summary(&pool, batch).await.unwrap().stock.matching, 3);

    let other = stage(&pool, Path::new("/books/Other.qbw"), 1, &books()).await.unwrap();
    discard_batch(&pool, other).await.unwrap();
    let left: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM ledger_import_stock_items WHERE batch_id = ?1) + (SELECT COUNT(*) FROM ledger_import_stock_moves WHERE batch_id = ?1)",
    )
    .bind(other)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(left, 0);

    // An item whose own row in the file can't be read is listed, not compared with zero.
    let mut unread = books();
    unread.stock.items[2].stored_qty_e4 = None;
    let batch = stage(&pool, Path::new("/books/Unread.qbw"), 1, &unread).await.unwrap();
    let kit = batch_summary(&pool, batch).await.unwrap().stock.differences.into_iter().find(|d| d.name == "Kit").unwrap();
    assert!(kit.note.as_deref().is_some_and(|n| n.contains("could not be read")), "{kit:?}");

    // Sales whose cost rows the reader couldn't use are given as the reason an item is short.
    let mut short = books();
    short.stock.unread_sales.insert(201, 2);
    let batch = stage(&pool, Path::new("/books/Short.qbw"), 1, &short).await.unwrap();
    let gadget = batch_summary(&pool, batch).await.unwrap().stock.differences.into_iter().find(|d| d.name == "Gadget").unwrap();
    assert_eq!(gadget.note.as_deref(), Some("the cost of 2 sales of it could not be read, so the stock they took out is missing"));
}

/// An item LocalBooks already tracks before the import keeps its stock, and is listed.
#[tokio::test]
async fn an_item_already_tracked_keeps_its_stock_and_is_listed() {
    let pool = test_pool().await;
    let product: i64 = sqlx::query_scalar(
        "INSERT INTO products_services (name, rate_cents, income_account_id, item_type)
         VALUES ('Widget', 100, (SELECT id FROM chart_of_accounts WHERE account_type = 'INCOME' ORDER BY id LIMIT 1), 'ITEM') RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    crate::inventory::track_product(&pool, &crate::inventory::TrackInput { product_id: product, sku: None, asset_account_id: None, cogs_account_id: None })
        .await
        .unwrap();
    let batch = stage(&pool, Path::new("/books/Stock.qbw"), 1, &books()).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap().stock.unwrap();
    assert_eq!((posted.tracked, posted.matching), (3, 2));
    assert_eq!(on_hand(&pool, "Widget").await, (0, 0));
    let widget = batch_summary(&pool, batch).await.unwrap().stock.differences.into_iter().find(|d| d.name == "Widget").unwrap();
    assert!(widget.note.as_deref().is_some_and(|n| n.contains("already tracked")), "{widget:?}");
}
