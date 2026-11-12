//! Item quantities from a desktop company file (gap R3c).
//!
//! The file keeps each inventory item's quantity on hand and value in its item tables, and each
//! change to them on the lines of the transactions that made it. Every such line posts to the
//! item's inventory asset account and carries a quantity, signed the way amounts are (negative =
//! debit = into stock):
//!
//! - purchase lines (bills, item receipts, checks and card charges) and inventory adjustment lines;
//! - the cost rows of sales forms (invoices, sales receipts, credit memos), which are kept apart
//!   from the form line and linked to it by line id (see `qbw_company::add_inventory_cost`); a
//!   bill's cost rows move value only, for stock already sold;
//! - assembly builds, which take the components out of stock and put the assembly in, all within
//!   the inventory asset account. A build posts nothing, so the reader turns it into a
//!   transaction of its own whose lines cancel on that account.
//!
//! Each change becomes a [`SourceStockMove`] on the transaction that made it. The full import
//! records them as stock movements on the entries those transactions became, then compares each
//! item with the quantity and value the file stores for it and lists every item that differs.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::NaiveDate;

use crate::qbw_company::{rows_of, text, SourceLine, SourceTransaction};
use crate::qbw_pages::LogicalPage;
use crate::qbw_records::{Catalog, Row};

/// What the readable transaction kind of a build is.
pub const BUILD_KIND: &str = "Assembly build";

/// An item the file tracks in stock, with the quantity and value it stores for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceStockItem {
    /// The file's item id (`SourceItem::id`).
    pub item_id: i64,
    /// Source account ids.
    pub asset_account_id: i64,
    pub cogs_account_id: Option<i64>,
    /// Quantity on hand × 10,000 and its value, as the file stores them; `None` when unreadable.
    pub stored_qty_e4: Option<i64>,
    pub stored_value_cents: Option<i64>,
    /// For an assembly: how many the file has in builds still pending (× 10,000).
    pub pending_builds_e4: i64,
}

/// A change to an item's stock made by one transaction: positive quantity and value go into stock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceStockMove {
    pub transaction_id: i64,
    pub item_id: i64,
    pub qty_e4: i64,
    pub value_cents: i64,
}

/// The file's stock: the items it tracks and every change to them, in the file's order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceStock {
    pub items: Vec<SourceStockItem>,
    pub moves: Vec<SourceStockMove>,
    /// Sold lines of an item whose cost rows could not be used, by item: the stock they took out
    /// is missing from its moves.
    pub unread_sales: HashMap<i64, usize>,
}

impl SourceStock {
    /// The item a line in stock is for, when the line moves stock: an item the file tracks, on an
    /// inventory asset account.
    pub fn tracked(&self, item_id: Option<i64>, account_id: i64) -> Option<&SourceStockItem> {
        let item = self.items.iter().find(|i| Some(i.item_id) == item_id)?;
        self.is_asset(account_id).then_some(item)
    }

    /// Whether an account is the inventory asset account of an item the file tracks.
    pub fn is_asset(&self, account_id: i64) -> bool {
        self.items.iter().any(|i| i.asset_account_id == account_id)
    }

    /// Adds the change a line makes: `cents` and `quantity_e4` signed as the file signs them
    /// (negative = into stock). Lines that change neither are left out.
    pub fn add(&mut self, transaction_id: i64, item_id: i64, cents: i64, quantity_e4: i64) {
        if cents == 0 && quantity_e4 == 0 {
            return;
        }
        self.moves.push(SourceStockMove { transaction_id, item_id, qty_e4: -quantity_e4, value_cents: -cents });
    }

    /// Each item's quantity and value from its moves (only those of `keep` transactions).
    #[cfg(test)]
    pub fn totals(&self, keep: impl Fn(i64) -> bool) -> HashMap<i64, (i64, i64)> {
        let mut out: HashMap<i64, (i64, i64)> = HashMap::new();
        for m in self.moves.iter().filter(|m| keep(m.transaction_id)) {
            let t = out.entry(m.item_id).or_default();
            t.0 = t.0.saturating_add(m.qty_e4);
            t.1 = t.1.saturating_add(m.value_cents);
        }
        out
    }
}

/// A line's quantity × 10,000, signed as stored; zero when it has none.
pub fn quantity_e4(row: &Row) -> i64 {
    row.get("quantity_qnty").as_quantity_e4().unwrap_or(0)
}

/// The items the file tracks in stock (inventory parts and assemblies with an asset account),
/// with the quantity and value it stores for each (`abmc_item_internal`, signed negative for
/// stock on hand, as debits are).
pub fn read_items(pages: &[LogicalPage], catalog: &Catalog) -> SourceStock {
    let stored: HashMap<i64, (Option<i64>, Option<i64>, i64)> = rows_of(pages, catalog, "abmc_item_internal")
        .iter()
        .filter_map(|r| {
            let qty = r.get("on_hand_qnty").as_quantity_e4().and_then(i64::checked_neg);
            let value = r.get("value_on_hand_amt").as_cents().and_then(i64::checked_neg);
            let pending = r.get("to_be_built_by_pending_build_txns_qnty").as_quantity_e4().map(i64::abs).unwrap_or(0);
            Some((r.get("item_id").as_int()?, (qty, value, pending)))
        })
        .collect();
    let mut items: Vec<SourceStockItem> = rows_of(pages, catalog, "abmc_item_user")
        .iter()
        .filter(|r| matches!(r.get("item_type").as_int(), Some(1 | 2)))
        .filter_map(|r| {
            let item_id = r.get("item_id").as_int()?;
            let asset_account_id = r.get("asset_account_id").as_int().filter(|a| *a > 0)?;
            let (stored_qty_e4, stored_value_cents, pending_builds_e4) = stored.get(&item_id).copied().unwrap_or((None, None, 0));
            Some(SourceStockItem {
                item_id,
                asset_account_id,
                cogs_account_id: r.get("cogs_account_id").as_int().filter(|a| *a > 0),
                stored_qty_e4,
                stored_value_cents,
                pending_builds_e4,
            })
        })
        .collect();
    items.sort_by_key(|i| i.item_id);
    items.dedup_by_key(|i| i.item_id);
    SourceStock { items, ..Default::default() }
}

/// The file's assembly builds as transactions: the assembly's line puts it into stock, the
/// component lines take them out, all on inventory asset accounts, so the lines cancel. Each
/// build line adds its change to `stock`. Dates outside `dates` are left out, as for other
/// transactions.
///
/// A pending build (one waiting for its components) moves nothing. The file marks no build as
/// pending; it keeps, on the assembly, how many are in pending builds. When the assembly's latest
/// builds add up to exactly that many, they are the pending ones and are left out (see
/// [`pending_builds`]).
pub fn read_builds(
    pages: &[LogicalPage],
    catalog: &Catalog,
    stock: &mut SourceStock,
    known_accounts: &HashSet<i64>,
    dates: &std::ops::RangeInclusive<NaiveDate>,
) -> Vec<SourceTransaction> {
    let headers: HashMap<i64, Row> =
        rows_of(pages, catalog, "abmc_item_assembly_header").into_iter().filter_map(|r| Some((r.get("transaction_id").as_int()?, r))).collect();
    // Per build: its transaction, and its lines that move stock as (item, cents, quantity).
    type Build = (SourceTransaction, Vec<(i64, i64, i64)>);
    let mut builds: BTreeMap<i64, Build> = BTreeMap::new();
    // Per build: the assembly built and how many.
    let mut built: HashMap<i64, (i64, i64)> = HashMap::new();
    for r in rows_of(pages, catalog, "abmc_item_assembly_lineitem") {
        let header = r.get("transaction_id").as_int().and_then(|t| headers.get(&t));
        if r.get("is_build_bool").as_int() != Some(1)
            || r.get("is_memorized_transaction_bool").as_int() == Some(1)
            || header.is_some_and(|h| h.get("is_memorized_transaction_bool").as_int() == Some(1))
        {
            continue;
        }
        let (Some(tid), Some(account_id), Some(cents)) = (r.get("transaction_id").as_int(), r.get("account_id").as_int(), r.get("amount_amt").as_cents())
        else {
            continue;
        };
        let Some(date) = header.and_then(|h| h.get("transaction_date").as_date()).or_else(|| r.get("transaction_date").as_date()) else { continue };
        if !known_accounts.contains(&account_id) || !dates.contains(&date) || cents.unsigned_abs() > crate::ledger::MAX_LINE_CENTS.unsigned_abs() {
            continue;
        }
        let (txn, moves) = builds.entry(tid).or_insert_with(|| {
            let txn = SourceTransaction {
                document: None,
                id: tid,
                kind: BUILD_KIND,
                date,
                doc_num: header.and_then(|h| text(h, "doc_num")).or_else(|| text(&r, "doc_num")),
                name: None,
                memo: header.and_then(|h| text(h, "memo")),
                lines: Vec::new(),
            };
            (txn, Vec::new())
        });
        if cents != 0 {
            txn.lines.push(SourceLine { account_id, cents, memo: text(&r, "memo"), class: None });
        }
        let item = r.get("item_id").as_int();
        if r.get("is_source_bool").as_int() == Some(1) {
            if let Some(item) = item {
                built.insert(tid, (item, quantity_e4(&r).saturating_neg()));
            }
        }
        if let Some(item) = stock.tracked(item, account_id) {
            moves.push((item.item_id, cents, quantity_e4(&r)));
        }
    }
    let order: Vec<(i64, NaiveDate)> = builds.values().map(|(t, _)| (t.id, t.date)).collect();
    let pending = pending_builds(&stock.items, &order, &built);
    let mut out = Vec::new();
    for (tid, (txn, moves)) in builds {
        if pending.contains(&tid) {
            continue;
        }
        for (item, cents, quantity) in moves {
            stock.add(tid, item, cents, quantity);
        }
        out.push(txn);
    }
    out
}

/// The builds that are pending: per assembly, the latest builds (by date, then id) whose
/// quantities add up to exactly what the file has in pending builds of it. When no run of latest
/// builds adds up to that, none is taken as pending, and the assembly's quantity will differ from
/// the file's and be listed.
pub fn pending_builds(items: &[SourceStockItem], builds: &[(i64, NaiveDate)], built: &HashMap<i64, (i64, i64)>) -> HashSet<i64> {
    let mut out = HashSet::new();
    for item in items.iter().filter(|i| i.pending_builds_e4 > 0) {
        let mut mine: Vec<(NaiveDate, i64, i64)> =
            builds.iter().filter_map(|&(tid, date)| built.get(&tid).filter(|(a, _)| *a == item.item_id).map(|&(_, qty)| (date, tid, qty))).collect();
        mine.sort_unstable_by(|a, b| b.cmp(a));
        let mut total = 0i64;
        let mut taken = Vec::new();
        for (_, tid, qty) in mine {
            total = total.saturating_add(qty);
            taken.push(tid);
            if total >= item.pending_builds_e4 {
                break;
            }
        }
        if total == item.pending_builds_e4 {
            out.extend(taken);
        }
    }
    out
}

#[cfg(test)]
mod tests;
