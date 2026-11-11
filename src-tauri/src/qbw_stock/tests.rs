use super::*;

/// Compares each tracked item's quantity and value, from the stock moves of the transactions a
/// real file's ledger reads as balanced, with what the file stores for it. Set
/// LOCALBOOKS_QBW_CHECK and run with `--ignored --nocapture`.
#[test]
#[ignore]
fn compares_stock_of_a_real_file() {
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
    let ledger = crate::qbw_company::read_ledger(std::path::Path::new(&path)).unwrap();
    let balanced: HashMap<i64, (NaiveDate, &str)> = ledger.transactions.iter().map(|t| (t.id, (t.date, t.kind))).collect();
    let totals = ledger.stock.totals(|t| balanced.contains_key(&t));
    let all = ledger.stock.totals(|_| true);
    let names: HashMap<i64, &str> = ledger.items.iter().map(|i| (i.id, i.name.as_str())).collect();
    let (mut matching, mut differing) = (0, 0);
    for item in &ledger.stock.items {
        let (qty, value) = totals.get(&item.item_id).copied().unwrap_or((0, 0));
        let stored = (item.stored_qty_e4.unwrap_or(0), item.stored_value_cents.unwrap_or(0));
        if (qty, value) == stored {
            matching += 1;
            continue;
        }
        differing += 1;
        let with_unbalanced = all.get(&item.item_id).copied().unwrap_or((0, 0));
        println!(
            "item {} {:?}: file qty {:?} value {:?}; read qty {} value {}; with unbalanced qty {} value {}",
            item.item_id,
            names.get(&item.item_id),
            item.stored_qty_e4,
            item.stored_value_cents,
            qty,
            value,
            with_unbalanced.0,
            with_unbalanced.1
        );
    }
    // Transactions whose inventory asset lines don't add up to the value their stock moves carry:
    // stock read without its line, or a line without the stock it moved.
    let mut by_txn: HashMap<i64, i64> = HashMap::new();
    for m in &ledger.stock.moves {
        *by_txn.entry(m.transaction_id).or_default() += m.value_cents;
    }
    for t in &ledger.transactions {
        let asset: i64 = -t.lines.iter().filter(|l| ledger.stock.is_asset(l.account_id)).map(|l| l.cents).sum::<i64>();
        let moved = by_txn.get(&t.id).copied().unwrap_or(0);
        if asset != moved {
            println!("txn {} {} {}: inventory asset lines {asset}, stock moves {moved}", t.id, t.kind, t.date);
        }
    }
    // How often stock would pass through a state LocalBooks refuses (quantity and value of
    // different signs), taking the moves in date order.
    let mut moves: Vec<&SourceStockMove> = ledger.stock.moves.iter().filter(|m| balanced.contains_key(&m.transaction_id)).collect();
    moves.sort_by_key(|m| (balanced[&m.transaction_id].0, m.transaction_id));
    let mut state: HashMap<i64, (i64, i64)> = HashMap::new();
    let mut odd = 0;
    for m in &moves {
        let s = state.entry(m.item_id).or_default();
        s.0 += m.qty_e4;
        s.1 += m.value_cents;
        if !((s.0 > 0 && s.1 >= 0) || (s.0 == 0 && s.1 == 0) || (s.0 < 0 && s.1 <= 0)) {
            odd += 1;
        }
    }
    let mut kinds: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for m in &moves {
        *kinds.entry(balanced[&m.transaction_id].1).or_default() += 1;
    }
    println!(
        "tracked items {}, matching {matching}, differing {differing}; moves {} ({kinds:?}), states refused {odd}",
        ledger.stock.items.len(),
        moves.len()
    );
}

fn assembly(item_id: i64, pending_builds_e4: i64) -> SourceStockItem {
    SourceStockItem { item_id, asset_account_id: 31, cogs_account_id: Some(30), stored_qty_e4: None, stored_value_cents: None, pending_builds_e4 }
}

/// The file keeps only how many of an assembly are in pending builds; the latest builds that add
/// up to exactly that many are the pending ones. When none add up, none is taken as pending.
#[test]
fn the_latest_builds_that_add_up_to_the_pending_quantity_are_pending() {
    let day = |d: u32| NaiveDate::from_ymd_opt(2024, 12, d).unwrap();
    let builds = [(1, day(1)), (2, day(5)), (3, day(15)), (4, day(15)), (5, day(2)), (6, day(3))];
    let built: HashMap<i64, (i64, i64)> = [
        // Assembly 67: built 3, 4, then 5 and 10 on the same day; 15 pending.
        (1, (67, 30_000)),
        (2, (67, 40_000)),
        (3, (67, 50_000)),
        (4, (67, 100_000)),
        // Assembly 70: 2 built, 1 pending, which no latest run adds up to.
        (5, (70, 20_000)),
        // Assembly 71: nothing pending.
        (6, (71, 10_000)),
    ]
    .into_iter()
    .collect();
    let items = [assembly(67, 150_000), assembly(70, 10_000), assembly(71, 0)];
    let mut pending: Vec<i64> = pending_builds(&items, &builds, &built).into_iter().collect();
    pending.sort();
    assert_eq!(pending, [3, 4]);
}

/// A line moves stock when it is for an item the file tracks and posts to an inventory asset
/// account; its change has the opposite sign to the file's (negative there = into stock).
#[test]
fn lines_on_an_inventory_account_move_stock_with_the_sign_turned() {
    let mut stock = SourceStock { items: vec![assembly(67, 0)], ..Default::default() };
    assert!(stock.tracked(Some(67), 31).is_some());
    assert!(stock.tracked(Some(67), 26).is_none(), "the sale's income line moves nothing");
    assert!(stock.tracked(Some(68), 31).is_none(), "an item the file doesn't track");
    stock.add(9, 67, -5_400, -20_000);
    stock.add(9, 67, 0, 0);
    stock.add(10, 67, 2_700, 10_000);
    assert_eq!(stock.moves, [SourceStockMove { transaction_id: 9, item_id: 67, qty_e4: 20_000, value_cents: 5_400 }, SourceStockMove {
        transaction_id: 10,
        item_id: 67,
        qty_e4: -10_000,
        value_cents: -2_700
    }]);
    assert_eq!(stock.totals(|t| t == 9)[&67], (20_000, 5_400));
}
