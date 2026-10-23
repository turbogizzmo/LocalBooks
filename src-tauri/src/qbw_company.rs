//! Reads a company file's general ledger through the page decoder ([`crate::qbw_pages`]) and the
//! record layer ([`crate::qbw_records`]), for files openqbw cannot read.
//!
//! Every posting line of every transaction lives in one of the `abmc_*_lineitem` tables, with the
//! account it posts to and a signed amount (negative = debit, positive = credit, matching the
//! account balances the file stores). Grouped by transaction they form the ledger. Each line is
//! checked before it is used, each transaction must balance to the cent, and the per-account
//! totals are compared with the balances the file itself stores, so whatever could not be read is
//! reported rather than silently missing.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use chrono::{Datelike, NaiveDate};

use crate::qbw_pages::{DecodeStats, LogicalPage, PageDecoder, S7_START};
use crate::qbw_records::{read_catalog, table_rows, walk_row, Catalog, Row, Value, CHUNK_KINDS};

/// LocalBooks account type codes, indexed by the file's numeric account type.
const ACCOUNT_TYPES: [&str; 15] = [
    "BANK",
    "ACCOUNTS_RECEIVABLE",
    "OTHER_CURRENT_ASSET",
    "FIXED_ASSET",
    "OTHER_ASSET",
    "ACCOUNTS_PAYABLE",
    "CREDIT_CARD",
    "OTHER_CURRENT_LIABILITY",
    "LONG_TERM_LIABILITY",
    "EQUITY",
    "INCOME",
    "COST_OF_GOODS_SOLD",
    "EXPENSE",
    "OTHER_INCOME",
    "OTHER_EXPENSE",
];

/// Transaction kinds by line-item table, with a readable name.
const TRANSACTION_TABLES: [(&str, &str); 13] = [
    ("invoice", "Invoice"),
    ("credit_memo", "Credit memo"),
    ("sales_receipt", "Sales receipt"),
    ("received_payment", "Payment"),
    ("deposit", "Deposit"),
    ("check", "Check"),
    ("credit_cardcharge", "Credit card charge"),
    ("bill", "Bill"),
    ("bill_check", "Bill payment"),
    ("bill_ccard", "Bill payment (card)"),
    ("customer_refund", "Refund"),
    ("general_journal", "Journal entry"),
    ("inventory_adjust", "Inventory adjustment"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceAccount {
    pub id: i64,
    pub name: String,
    /// `Parent:Child` path.
    pub full_name: String,
    /// LocalBooks account type code.
    pub account_type: &'static str,
    pub parent_id: Option<i64>,
    pub active: bool,
    /// Balance the file stores for the account (negative = debit), when readable.
    pub stored_balance_cents: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLine {
    pub account_id: i64,
    /// Negative = debit, positive = credit.
    pub cents: i64,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTransaction {
    pub id: i64,
    pub kind: &'static str,
    pub date: NaiveDate,
    pub doc_num: Option<String>,
    pub name: Option<String>,
    pub memo: Option<String>,
    pub lines: Vec<SourceLine>,
}

impl SourceTransaction {
    pub fn imbalance_cents(&self) -> i64 {
        self.lines.iter().map(|l| l.cents).sum()
    }
}

/// Counts that say how complete the read was.
#[derive(Debug, Clone, Default)]
pub struct LedgerQuality {
    pub pages: DecodeStats,
    #[cfg_attr(not(test), allow(dead_code))]
    pub refined_pages: usize,
    /// Rows of line-item tables that could not be decoded.
    pub unreadable_lines: usize,
    /// Decoded lines rejected by the checks (unknown account, odd amount, impossible date).
    pub rejected_lines: usize,
    pub rejected_by_reason: BTreeMap<&'static str, usize>,
    /// Accounts whose total from the decoded lines equals the balance the file stores.
    pub accounts_matching_stored: usize,
    pub accounts_with_stored_balance: usize,
}

#[derive(Debug, Clone)]
pub struct Ledger {
    pub accounts: Vec<SourceAccount>,
    /// Transactions whose lines balance, ordered by date.
    pub transactions: Vec<SourceTransaction>,
    /// Transactions with a line missing or misread; they cannot be posted as they are.
    pub unbalanced: Vec<SourceTransaction>,
    pub quality: LedgerQuality,
}

/// Decodes the file's pages, re-solving the last sector of pages whose rows do not decode.
pub fn read_pages(path: &Path) -> Result<(Vec<LogicalPage>, Catalog, DecodeStats, usize), String> {
    let mut decoder = PageDecoder::open(path)?;
    let (mut pages, stats) = decoder.decode_all();
    let catalog = read_catalog(&pages);
    let mut refined = 0;
    for page in pages.iter_mut() {
        let Some(cols) = catalog.columns.get(&page.owner) else { continue };
        if page.owner < 3 || CHUNK_KINDS.contains(&page.kind) {
            continue;
        }
        let walks = |p: &LogicalPage| p.rows.iter().filter(|r| r.chained && walk_row(p.row(r), cols).is_some()).count();
        let failing_in_s7 = page
            .rows
            .iter()
            .any(|r| (r.start + r.len) as usize > S7_START && !(r.chained && walk_row(page.row(r), cols).is_some()));
        if failing_in_s7 {
            if let Some(better) = decoder.refine(page, &walks) {
                *page = better;
                refined += 1;
            }
        }
    }
    Ok((pages, catalog, stats, refined))
}

fn text(row: &Row, column: &str) -> Option<String> {
    row.get(column).as_text().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn rows_of(pages: &[LogicalPage], catalog: &Catalog, table: &str) -> Vec<Row> {
    catalog.table_id(table).map(|id| table_rows(pages, catalog, id).rows).unwrap_or_default()
}

/// Names by id from a list table such as `abmc_customer_user`.
fn names(pages: &[LogicalPage], catalog: &Catalog, table: &str, id_column: &str) -> HashMap<i64, String> {
    rows_of(pages, catalog, table)
        .iter()
        .filter_map(|r| Some((r.get(id_column).as_int()?, text(r, "name")?)))
        .collect()
}

fn read_accounts(pages: &[LogicalPage], catalog: &Catalog) -> Vec<SourceAccount> {
    let stored: HashMap<i64, i64> = rows_of(pages, catalog, "abmc_account_internal")
        .iter()
        .filter_map(|r| Some((r.get("account_id").as_int()?, r.get("ending_balance_amt").as_cents().unwrap_or(0))))
        .collect();
    let mut accounts: BTreeMap<i64, SourceAccount> = BTreeMap::new();
    for r in rows_of(pages, catalog, "abmc_account_user") {
        let (Some(id), Some(name)) = (r.get("account_id").as_int(), text(&r, "name")) else { continue };
        let Some(account_type) = r.get("type").as_int().and_then(|t| ACCOUNT_TYPES.get(t as usize)) else { continue };
        let hidden = r.get("is_hidden_bool").as_int() == Some(1) || r.get("is_deleted_bool").as_int() == Some(1);
        accounts.insert(
            id,
            SourceAccount {
                id,
                full_name: name.clone(),
                name: name.replace(':', "-"),
                account_type,
                parent_id: r.get("parent_id").as_int().filter(|p| *p > 0),
                active: !hidden,
                stored_balance_cents: stored.get(&id).copied(),
            },
        );
    }
    // Full names follow the parent chain; a parent of another type or a missing one is dropped.
    let snapshot = accounts.clone();
    for account in accounts.values_mut() {
        let mut path = vec![account.name.clone()];
        let mut parent = account.parent_id;
        let mut seen = HashSet::new();
        while let Some(pid) = parent.filter(|p| seen.insert(*p)) {
            match snapshot.get(&pid) {
                Some(p) if p.account_type == account.account_type => {
                    path.push(p.name.clone());
                    parent = p.parent_id;
                }
                _ => break,
            }
        }
        if account.parent_id.is_some_and(|p| snapshot.get(&p).is_none_or(|p| p.account_type != account.account_type)) {
            account.parent_id = None;
        }
        path.reverse();
        account.full_name = path.join(":");
    }
    accounts.into_values().collect()
}

/// Reads the general ledger. Blocking and CPU-bound.
pub fn read_ledger(path: &Path) -> Result<Ledger, String> {
    let (pages, catalog, page_stats, refined) = read_pages(path)?;
    let mut quality = LedgerQuality { pages: page_stats, refined_pages: refined, ..Default::default() };
    let accounts = read_accounts(&pages, &catalog);
    let known_accounts: HashSet<i64> = accounts.iter().map(|a| a.id).collect();
    let customers = names(&pages, &catalog, "abmc_customer_user", "customer_id");
    let vendors = names(&pages, &catalog, "abmc_vendor_user", "vendor_id");
    let earliest = NaiveDate::from_ymd_opt(1980, 1, 1).expect("valid date");
    let latest = chrono::Local::now().date_naive().with_year(chrono::Local::now().year() + 1).unwrap_or(NaiveDate::MAX);

    let mut transactions: BTreeMap<i64, SourceTransaction> = BTreeMap::new();
    for (stem, kind) in TRANSACTION_TABLES {
        let Some(line_table) = catalog.table_id(&format!("abmc_{stem}_lineitem")) else { continue };
        let headers: HashMap<i64, Row> = rows_of(&pages, &catalog, &format!("abmc_{stem}_header"))
            .into_iter()
            .filter_map(|r| Some((r.get("transaction_id").as_int()?, r)))
            .collect();
        let lines = table_rows(&pages, &catalog, line_table);
        quality.unreadable_lines += lines.unchained + lines.unwalkable;
        for r in lines.rows {
            if r.get("is_no_post_bool").as_int() == Some(1) {
                continue;
            }
            let checked = (|| {
                let tid = r.get("transaction_id").as_int().ok_or("no transaction")?;
                let cents = match r.get("amount_amt") {
                    Value::Null => 0,
                    v => v.as_cents().ok_or("amount not in cents")?,
                };
                // Text and subtotal lines carry no account and no amount.
                let account_id = match r.get("account_id").as_int() {
                    Some(a) => a,
                    None if cents == 0 => return Err("no amount"),
                    None => return Err("amount without account"),
                };
                if !known_accounts.contains(&account_id) {
                    return Err("unknown account");
                }
                let header = headers.get(&tid);
                let date = header
                    .and_then(|h| h.get("transaction_date").as_date())
                    .or_else(|| r.get("transaction_date").as_date())
                    .ok_or("no date")?;
                if !(earliest..=latest).contains(&date) {
                    return Err("date out of range");
                }
                Ok((tid, account_id, cents, date, header))
            })();
            let (tid, account_id, cents, date, header) = match checked {
                Ok(v) => v,
                Err("no amount") => continue,
                Err(reason) => {
                    quality.rejected_lines += 1;
                    *quality.rejected_by_reason.entry(reason).or_default() += 1;
                    continue;
                }
            };
            let txn = transactions.entry(tid).or_insert_with(|| {
                let source = header.unwrap_or(&r);
                let name = source
                    .get("customer_id")
                    .as_int()
                    .and_then(|id| customers.get(&id))
                    .or_else(|| source.get("vendor_id").as_int().and_then(|id| vendors.get(&id)))
                    .cloned();
                SourceTransaction {
                    id: tid,
                    kind,
                    date,
                    doc_num: header.and_then(|h| text(h, "doc_num")).or_else(|| text(&r, "doc_num")),
                    name,
                    memo: header.and_then(|h| text(h, "memo")),
                    lines: Vec::new(),
                }
            });
            if cents != 0 {
                txn.lines.push(SourceLine { account_id, cents, memo: text(&r, "memo") });
            }
        }
    }

    let mut totals: HashMap<i64, i64> = HashMap::new();
    for txn in transactions.values() {
        for line in &txn.lines {
            *totals.entry(line.account_id).or_default() += line.cents;
        }
    }
    for account in &accounts {
        if let Some(stored) = account.stored_balance_cents {
            quality.accounts_with_stored_balance += 1;
            quality.accounts_matching_stored += (totals.get(&account.id).copied().unwrap_or(0) == stored) as usize;
        }
    }

    let (mut balanced, mut unbalanced): (Vec<_>, Vec<_>) =
        transactions.into_values().filter(|t| !t.lines.is_empty()).partition(|t| t.imbalance_cents() == 0);
    balanced.sort_by_key(|t| (t.date, t.id));
    unbalanced.sort_by_key(|t| (t.date, t.id));
    Ok(Ledger { accounts, transactions: balanced, unbalanced, quality })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore]
    fn lists_unreadable_line_rows() {
        let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
        let (pages, catalog, _, _) = read_pages(Path::new(&path)).unwrap();
        for page in &pages {
            let Some(name) = catalog.names.get(&page.owner) else { continue };
            if !name.ends_with("_lineitem") || CHUNK_KINDS.contains(&page.kind) {
                continue;
            }
            let cols = &catalog.columns[&page.owner];
            for r in &page.rows {
                let walks = r.chained && walk_row(page.row(r), cols).is_some();
                if !walks {
                    let zone = if r.start as usize >= S7_START { "s7" } else if (r.start + r.len) as usize > S7_START { "crosses" } else { "s0-6" };
                    println!("{name:<36} page {:>5} kind {} row {:>4}+{:<4} chained {:<5} {zone:<7} s7 {:?} q {} r7 {} rows on page {}", page.number, page.kind, r.start, r.len, r.chained, page.sector7, page.q, page.r7, page.rows.len());
                }
            }
        }
    }

    /// Ledger read of a real company file (read-only): set LOCALBOOKS_QBW_CHECK and run
    /// `cargo test --release -- --ignored reads_the_ledger_of_a_real --nocapture`.
    #[test]
    #[ignore]
    fn reads_the_ledger_of_a_real_file() {
        let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
        let started = std::time::Instant::now();
        let ledger = read_ledger(Path::new(&path)).unwrap();
        let q = &ledger.quality;
        println!("read in {:.1}s; refined pages {}", started.elapsed().as_secs_f64(), q.refined_pages);
        println!(
            "accounts {} (stored balances matched {}/{}), transactions {} balanced, {} unbalanced; unreadable lines {}, rejected {}",
            ledger.accounts.len(),
            q.accounts_matching_stored,
            q.accounts_with_stored_balance,
            ledger.transactions.len(),
            ledger.unbalanced.len(),
            q.unreadable_lines,
            q.rejected_lines
        );
        let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
        for t in &ledger.transactions {
            *by_kind.entry(t.kind).or_default() += 1;
        }
        println!("by kind: {by_kind:?}");
        println!("rejected: {:?}", q.rejected_by_reason);
        if let (Some(first), Some(last)) = (ledger.transactions.first(), ledger.transactions.last()) {
            println!("dates {} .. {}", first.date, last.date);
        }
        for t in &ledger.unbalanced {
            println!("  unbalanced {} #{:?} {} {:?} off by {}", t.kind, t.doc_num, t.date, t.name, crate::ledger::format_cents(t.imbalance_cents()));
        }
        let names: HashMap<i64, &str> = ledger.accounts.iter().map(|a| (a.id, a.full_name.as_str())).collect();
        let mut totals: HashMap<i64, i64> = HashMap::new();
        for t in &ledger.transactions {
            for l in &t.lines {
                *totals.entry(l.account_id).or_default() += l.cents;
            }
        }
        for a in ledger.accounts.iter().filter(|a| a.stored_balance_cents.is_some_and(|s| s != totals.get(&a.id).copied().unwrap_or(0))) {
            println!(
                "  differs: {:<40} stored {:>14} read {:>14}",
                names[&a.id],
                crate::ledger::format_cents(a.stored_balance_cents.unwrap()),
                crate::ledger::format_cents(totals.get(&a.id).copied().unwrap_or(0))
            );
        }
    }
}
