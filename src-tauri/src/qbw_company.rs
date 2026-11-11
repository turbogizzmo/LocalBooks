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

/// Transaction kinds by table stem (lines in `<stem>_lineitem`, headers in `<stem>_header`),
/// with a readable name. Payroll keeps its paychecks and liability payments in tables of its own.
const TRANSACTION_TABLES: [(&str, &str); 16] = [
    ("abmc_invoice", "Invoice"),
    ("abmc_credit_memo", "Credit memo"),
    ("abmc_sales_receipt", "Sales receipt"),
    ("abmc_received_payment", "Payment"),
    ("abmc_deposit", "Deposit"),
    ("abmc_transfer", "Transfer"),
    ("abmc_check", "Check"),
    ("abmc_credit_cardcharge", "Credit card charge"),
    ("abmc_bill", "Bill"),
    ("abmc_bill_check", "Bill payment"),
    ("abmc_bill_ccard", "Bill payment (card)"),
    ("abmc_customer_refund", "Refund"),
    ("abmc_general_journal", "Journal entry"),
    ("abmc_inventory_adjust", "Inventory adjustment"),
    ("es_paycheck", "Paycheck"),
    ("es_liability_payment", "Payroll liability payment"),
];

/// Where the cost of inventory sold on a form is kept, and the form's line table. Each sold
/// line has a pair of cost rows, cost of goods sold and the inventory asset it came out of,
/// keyed by the form line's `target_id` rather than by transaction. A bill has them too, when it
/// changes the cost of stock already sold (the cost moves between the two accounts).
const INVENTORY_COST_TABLES: [(&str, &str); 4] = [
    ("abmc_invoice_inventory_lineitem", "abmc_invoice_lineitem"),
    ("abmc_bill_inventory_lineitem", "abmc_bill_lineitem"),
    ("abmc_sale_receipt_inventory_lineitem", "abmc_sales_receipt_lineitem"),
    ("abmc_credit_memo_inventory_lineitem", "abmc_credit_memo_lineitem"),
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
    /// The income tax line the file assigns, as LocalBooks keeps an imported one it cannot match
    /// (see `accountant_reports::imported_tax_line`).
    pub tax_line: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLine {
    pub account_id: i64,
    /// Negative = debit, positive = credit.
    pub cents: i64,
    pub memo: Option<String>,
    /// The line's class as a full `Parent:Child` path (see [`class_path`]), when it has one.
    pub class: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTransaction {
    /// Document details for the kinds a full import recreates natively (see `qbw_documents`).
    pub document: Option<crate::qbw_documents::SourceDocument>,
    pub id: i64,
    pub kind: &'static str,
    pub date: NaiveDate,
    pub doc_num: Option<String>,
    pub name: Option<String>,
    pub memo: Option<String>,
    pub lines: Vec<SourceLine>,
}

impl SourceTransaction {
    /// The sum of the lines, zero when the transaction balances. Lines come from a file that may
    /// be damaged, so the sum is checked; one that overflows is `i64::MAX`, which is never zero,
    /// so such a transaction counts as unbalanced and is never posted.
    pub fn imbalance_cents(&self) -> i64 {
        checked_total(self.lines.iter().map(|l| l.cents)).unwrap_or(i64::MAX)
    }
}

/// A memorized transaction is a template the file keeps for entering the same transaction again;
/// its rows sit in the same tables as real ones, marked on the line or on its header, and post
/// nothing.
fn memorized(line: &Row, header: Option<&Row>) -> bool {
    line.get("is_memorized_transaction_bool").as_int() == Some(1) || header.is_some_and(|h| h.get("is_memorized_transaction_bool").as_int() == Some(1))
}

/// Some files keep one movement of money as two transactions that each hold half of it: a
/// customer payment deposited straight to the bank holds only its receivables line, and a deposit
/// on the same day holds only the bank line. Neither balances alone. Two unbalanced transactions
/// on the same date whose differences cancel exactly, each the other's only such match, are read
/// as one: the lines go to the one that names a customer, vendor or number (else the lower id).
/// Returns how many pairs were joined.
fn join_split_halves(transactions: &mut BTreeMap<i64, SourceTransaction>) -> usize {
    let off: Vec<(i64, NaiveDate, i64)> =
        transactions.values().map(|t| (t.id, t.date, t.imbalance_cents())).filter(|(_, _, d)| *d != 0 && *d != i64::MAX).collect();
    let partners = |id: i64, date: NaiveDate, d: i64| off.iter().filter(move |(o, od, odiff)| *o != id && *od == date && odiff.checked_neg() == Some(d)).map(|(o, _, _)| *o);
    let mut pairs = Vec::new();
    for &(id, date, d) in &off {
        let mine: Vec<i64> = partners(id, date, d).collect();
        if let [other] = mine[..] {
            let theirs: Vec<i64> = partners(other, date, -d).collect();
            if theirs == [id] && id < other {
                pairs.push((id, other));
            }
        }
    }
    for &(a, b) in &pairs {
        let named = |t: &SourceTransaction| t.name.is_some() || t.doc_num.is_some();
        let keep_b = transactions.get(&b).is_some_and(named) && !transactions.get(&a).is_some_and(named);
        let (keep, drop) = if keep_b { (b, a) } else { (a, b) };
        if let Some(gone) = transactions.remove(&drop) {
            if let Some(t) = transactions.get_mut(&keep) {
                t.lines.extend(gone.lines);
                t.memo = t.memo.take().or(gone.memo);
            }
        }
    }
    pairs.len()
}

/// The checked sum of some amounts, or `None` when it overflows.
pub(crate) fn checked_total(amounts: impl IntoIterator<Item = i64>) -> Option<i64> {
    amounts.into_iter().try_fold(0i64, |sum, c| sum.checked_add(c))
}

/// Counts that say how complete the read was.
#[derive(Debug, Clone, Default)]
pub struct LedgerQuality {
    pub pages: DecodeStats,
    #[cfg_attr(not(test), allow(dead_code))]
    pub refined_pages: usize,
    /// Rows of line-item tables that could not be decoded.
    pub unreadable_lines: usize,
    /// Lines read from rows damaged near their end (memo lost), kept because they make their
    /// transaction balance exactly.
    pub recovered_lines: usize,
    /// Pairs of transactions read as one (see [`join_split_halves`]).
    pub joined_transactions: usize,
    /// Decoded lines rejected by the checks (unknown account, odd amount, impossible date).
    pub rejected_lines: usize,
    pub rejected_by_reason: BTreeMap<&'static str, usize>,
    /// Accounts whose total from the decoded lines equals the balance the file stores.
    pub accounts_matching_stored: usize,
    pub accounts_with_stored_balance: usize,
    /// Stored amounts that are not a whole number of cents, each described ("Invoice line,
    /// transaction 812: 10.005"). They are left out of what is imported (a stored balance counts as
    /// zero, a line or payment link is skipped), so the summary lists them.
    pub fractional_amounts: Vec<String>,
}

/// The amount columns the import reads as cents, and what each row is, for [`fractional_amounts`].
const AMOUNT_COLUMNS: &[(&str, &str, &str, &str)] = &[
    ("abmc_account_internal", "ending_balance_amt", "account_id", "Stored balance of account"),
    ("abmc_customer_internal", "end_balance_amt", "customer_id", "Stored balance of customer"),
    ("abmc_vendor_internal", "end_balance_amt", "vendor_id", "Stored balance of vendor"),
    ("abmc_invoice_lineitem", "amount_amt", "transaction_id", "Invoice line, transaction"),
    ("abmc_credit_memo_lineitem", "amount_amt", "transaction_id", "Credit memo line, transaction"),
    ("abmc_sales_receipt_lineitem", "amount_amt", "transaction_id", "Sales receipt line, transaction"),
    ("abmc_bill_lineitem", "amount_amt", "transaction_id", "Bill line, transaction"),
    ("abmc_estimate_lineitem", "amount_amt", "transaction_id", "Estimate line, transaction"),
    ("abmc_purchase_order_lineitem", "amount_amt", "transaction_id", "Purchase order line, transaction"),
    ("abmc_transaction_link", "amount_amt", "transaction_id", "Payment applied, transaction"),
];

/// Every amount the import reads as cents that the file stores as a double with a fraction of a
/// cent: described, in table order, so the import summary can count and list them.
pub(crate) fn fractional_amounts(pages: &[LogicalPage], catalog: &Catalog) -> Vec<String> {
    let mut found = Vec::new();
    for (table, column, id_column, what) in AMOUNT_COLUMNS {
        for r in rows_of(pages, catalog, table) {
            if let Some(value) = r.get(column).fractional_amount() {
                let id = r.get(id_column).as_int().map(|i| i.to_string()).unwrap_or_else(|| "?".into());
                found.push(format!("{what} {id}: {value}"));
            }
        }
    }
    found
}

/// What became of a line row that did not decode in full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOutcome {
    /// Its leading columns made its transaction balance, so the line was used (memo lost).
    Recovered,
    /// It carries no amount (a blank or text line), so nothing is missing from the books.
    NoAmount,
    /// It could not be used; its transaction is unbalanced unless other lines make up for it.
    Unreadable,
}

impl LineOutcome {
    pub fn code(self) -> &'static str {
        match self {
            LineOutcome::Recovered => "RECOVERED",
            LineOutcome::NoAmount => "NO_AMOUNT",
            LineOutcome::Unreadable => "UNREADABLE",
        }
    }
}

/// A line row that did not decode in full, with whatever of it could be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnreadableLine {
    pub kind: &'static str,
    pub transaction_id: Option<i64>,
    pub date: Option<NaiveDate>,
    pub doc_num: Option<String>,
    pub name: Option<String>,
    pub account_id: Option<i64>,
    pub cents: Option<i64>,
    pub outcome: LineOutcome,
}

/// A customer or vendor from the file's name lists, with its current contact details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceParty {
    pub id: i64,
    /// Customers: the `Customer:Job` path; vendors: the name.
    pub name: String,
    pub parent_id: Option<i64>,
    pub company_name: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub terms_days: Option<i64>,
    /// Vendors only.
    pub tax_id: Option<String>,
    pub is_1099: bool,
    pub active: bool,
    /// What the party owes (customers) or is owed (vendors) per the file, in cents.
    pub balance_cents: Option<i64>,
    /// Details that have no field of their own (extra email addresses, alternate phones).
    pub notes: Option<String>,
    /// Customers: the default sales rep, as initials or "initials:name".
    pub sales_rep: Option<String>,
    /// Custom fields with a value, as (label, value).
    pub custom_fields: Vec<(String, String)>,
}

/// An item (service or product) from the file's item list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceItem {
    pub id: i64,
    /// `Parent:Item` path.
    pub name: String,
    pub description: Option<String>,
    pub rate_cents: i64,
    /// The account the item posts sales to (a source account id).
    pub income_account_id: Option<i64>,
    pub kind: &'static str,
    /// Its sales tax code is a taxable one.
    pub taxable: bool,
    pub active: bool,
    /// Custom fields with a value, as (label, value).
    pub custom_fields: Vec<(String, String)>,
    /// What purchase forms say for it, and what it costs to buy.
    pub purchase_description: Option<String>,
    pub purchase_cost_cents: Option<i64>,
    /// A percent discount or charge, in parts per million of the line it applies to.
    pub percent_ppm: Option<i64>,
}

/// The file's sales rep and shipping method lists, by name, with whether each is active. They are
/// also found or created from the customers and documents that use them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceSalesLists {
    /// Initials, or "initials:name".
    pub reps: Vec<(String, bool)>,
    pub ship_methods: Vec<(String, bool)>,
    /// Price levels; a per-item level's items name the file's item ids (`SourceItem::id`).
    pub price_levels: Vec<SourcePriceLevel>,
    /// Customers' default price levels: (`SourceParty::id`, level name).
    pub customer_price_levels: Vec<(i64, String)>,
    /// Item groups, by name.
    pub item_groups: Vec<SourceItemGroup>,
}

/// An item group from the file: items sold together, which a sales form lists under the group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceItemGroup {
    pub id: i64,
    /// `Parent:Child` path, as items have.
    pub name: String,
    pub description: Option<String>,
    /// True when printed forms list the group's items; false prints the group as one line.
    pub print_items: bool,
    pub active: bool,
    /// The group's items in the order it lists them: (`SourceItem::id`, quantity × 10,000).
    pub members: Vec<(i64, i64)>,
}

/// A price level from the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePriceLevel {
    pub name: String,
    pub active: bool,
    pub kind: crate::price_levels::LevelKind,
    /// Fixed % levels: thousandths of a percent.
    pub percent_milli: i64,
    pub items: Vec<crate::price_levels::LevelItem>,
}

impl SourceSalesLists {
    /// Adds a rep once per set of initials; a later mention with a name fills the name in.
    pub fn add_rep(&mut self, rep: &str, active: bool) {
        let initials = |r: &str| r.split(':').next().unwrap_or_default().trim().to_lowercase();
        match self.reps.iter_mut().find(|(r, _)| initials(r) == initials(rep)) {
            Some((existing, _)) if !existing.contains(':') && rep.contains(':') => *existing = rep.to_string(),
            Some(_) => {}
            None => self.reps.push((rep.to_string(), active)),
        }
    }

    pub fn add_ship_method(&mut self, name: &str, active: bool) {
        let name = name.trim();
        if !name.is_empty() && !self.ship_methods.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)) {
            self.ship_methods.push((name.to_string(), active));
        }
    }
}

#[derive(Debug, Clone)]
pub struct Ledger {
    pub accounts: Vec<SourceAccount>,
    pub customers: Vec<SourceParty>,
    pub vendors: Vec<SourceParty>,
    pub items: Vec<SourceItem>,
    /// Transactions whose lines balance, ordered by date.
    pub transactions: Vec<SourceTransaction>,
    /// Transactions with a line missing or misread; they cannot be posted as they are.
    pub unbalanced: Vec<SourceTransaction>,
    /// Line rows that did not decode in full, and what became of each.
    pub unreadable: Vec<UnreadableLine>,
    pub quality: LedgerQuality,
    pub sales_lists: SourceSalesLists,
    /// The class list: full `Parent:Child` paths with whether each is active. Classes the lines
    /// use are on it too (see [`Ledger::add_classes_used`]).
    pub classes: Vec<(String, bool)>,
}

/// Deepest class path LocalBooks keeps (see `classes`).
const CLASS_LEVELS: usize = 5;

/// A class name from a file as a `Parent:Child` path: each level trimmed, empty levels dropped,
/// and levels past the fifth folded into the fifth. `None` when nothing is left.
pub fn class_path(name: &str) -> Option<String> {
    let mut parts: Vec<String> = name.split(':').map(str::trim).filter(|p| !p.is_empty()).map(str::to_string).collect();
    if parts.len() > CLASS_LEVELS {
        let rest = parts.split_off(CLASS_LEVELS - 1).join(" - ");
        parts.push(rest);
    }
    (!parts.is_empty()).then(|| parts.join(":"))
}

impl Ledger {
    /// Adds a class to the list once (names compare without case); an existing entry is kept.
    pub fn add_class(&mut self, name: &str, active: bool) {
        let Some(path) = class_path(name) else { return };
        if !self.classes.iter().any(|(c, _)| c.eq_ignore_ascii_case(&path)) {
            self.classes.push((path, active));
        }
    }

    /// Puts every class a posting or form line uses on the class list, active, and sorts it.
    pub fn add_classes_used(&mut self) {
        let mut used: Vec<String> = Vec::new();
        for t in self.transactions.iter().chain(&self.unbalanced) {
            used.extend(t.lines.iter().filter_map(|l| l.class.clone()));
            if let Some(doc) = &t.document {
                used.extend(doc.lines().iter().filter_map(|l| l.class.clone()));
            }
        }
        for class in used {
            self.add_class(&class, true);
        }
        self.classes.sort_by_key(|(c, _)| c.to_lowercase());
    }
}

/// Adds the cost of inventory sold to the forms it was sold on. `form_lines` maps a form line's
/// `target_id` to its transaction and class. A row read twice counts once. A line's cost rows are
/// used only together, and only as pairs (as many debits as credits) that net to zero, so they
/// never unbalance their transaction; others are rejected.
fn add_inventory_cost(
    transactions: &mut BTreeMap<i64, SourceTransaction>,
    form_lines: &HashMap<i64, (i64, Option<String>)>,
    cost_rows: &[Row],
    known_accounts: &HashSet<i64>,
    quality: &mut LedgerQuality,
) {
    // A row read twice (the same record from a re-solved page) is one row, not two. Only rows
    // that carry their record id can be told to be the same; rows without one are all kept.
    let mut seen: HashSet<(i64, Option<i64>, i64, i64, i64)> = HashSet::new();
    let mut by_line: BTreeMap<i64, Vec<(i64, i64)>> = BTreeMap::new();
    for r in cost_rows {
        let (Some(target), Some(account_id), Some(cents)) = (r.get("target_id").as_int(), r.get("account_id").as_int(), r.get("amount_amt").as_cents()) else {
            continue;
        };
        let first = match r.get("inventory_record_id").as_int() {
            Some(record) => seen.insert((record, r.get("target_type").as_int(), target, account_id, cents)),
            None => true,
        };
        if cents != 0 && first {
            by_line.entry(target).or_default().push((account_id, cents));
        }
    }
    for (target, rows) in by_line {
        // A sold line's cost comes in pairs (a debit and a credit), more than one when it drew on
        // several purchases; together they net to zero.
        let debits = rows.iter().filter(|(_, c)| *c < 0).count();
        let pairs = debits * 2 == rows.len() && checked_total(rows.iter().map(|(_, c)| *c)) == Some(0);
        let reason = if !pairs {
            "inventory cost is not pairs that net to zero"
        } else if rows.iter().any(|(a, _)| !known_accounts.contains(a)) {
            "unknown account"
        } else {
            match form_lines.get(&target).and_then(|(tid, class)| Some((transactions.get_mut(tid)?, class))) {
                Some((txn, class)) => {
                    txn.lines.extend(rows.iter().map(|&(account_id, cents)| SourceLine { account_id, cents, memo: None, class: class.clone() }));
                    continue;
                }
                None => "inventory cost of a form line that was not read",
            }
        };
        quality.rejected_lines += rows.len();
        *quality.rejected_by_reason.entry(reason).or_default() += rows.len();
    }
}

/// Decodes the file's pages, re-solving the last sector of pages whose rows do not decode.
pub fn read_pages(path: &Path) -> Result<(Vec<LogicalPage>, Catalog, DecodeStats, usize), String> {
    let mut decoder = PageDecoder::open(path)?;
    let (mut pages, stats) = decoder.decode_all();
    let (catalog, refined) = repair_pages(&mut pages, &|page, score| decoder.refine(page, score));
    Ok((pages, catalog, stats, refined))
}

/// Re-solves the last sector of pages whose rows do not read, and reads the catalog. Catalog
/// pages go first, scored by the catalog rows they yield: a column definition lost to a few
/// wrong bytes would otherwise leave its table's rows unreadable. Then each table page whose
/// rows in sector 7 do not walk, scored by the rows that walk. `refine` returns a better
/// decode of a page, or `None`. Returns the catalog and how many pages were re-solved.
pub(crate) fn repair_pages(pages: &mut [LogicalPage], refine: &Refine) -> (Catalog, usize) {
    let mut refined = 0;
    let ends_in_s7 = |r: &crate::qbw_pages::RowSpan| (r.start + r.len) as usize > S7_START;
    for page in pages.iter_mut().filter(|p| crate::qbw_records::is_catalog_page(p)) {
        let score = crate::qbw_records::catalog_rows_read;
        if page.rows.iter().any(|r| ends_in_s7(r) && !crate::qbw_records::catalog_row_reads(page, r)) {
            if let Some(better) = refine(page, &score) {
                *page = better;
                refined += 1;
            }
        }
    }
    let catalog = read_catalog(pages);
    for page in pages.iter_mut() {
        let Some(cols) = catalog.columns.get(&page.owner) else { continue };
        if page.owner < 3 || CHUNK_KINDS.contains(&page.kind) {
            continue;
        }
        let walks = |p: &LogicalPage| p.rows.iter().filter(|r| r.chained && walk_row(p.row(r), cols).is_some()).count();
        let failing_in_s7 = page.rows.iter().any(|r| ends_in_s7(r) && !(r.chained && walk_row(page.row(r), cols).is_some()));
        if failing_in_s7 {
            if let Some(better) = refine(page, &walks) {
                *page = better;
                refined += 1;
            }
        }
    }
    (catalog, refined)
}

/// Re-solves one page's last sector for a score (see `PageDecoder::refine`).
pub(crate) type Refine<'a> = dyn Fn(&LogicalPage, &dyn Fn(&LogicalPage) -> usize) -> Option<LogicalPage> + 'a;

pub(crate) fn text(row: &Row, column: &str) -> Option<String> {
    row.get(column).as_text().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub(crate) fn rows_of(pages: &[LogicalPage], catalog: &Catalog, table: &str) -> Vec<Row> {
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
                // QuickBooks numbers its tax lines; 0 is unassigned. The file has no names for them.
                tax_line: r.get("tax_line_id_num").as_int().filter(|n| *n > 0).map(crate::accountant_reports::quickbooks_tax_line),
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

/// Adds lines read from damaged rows to the transactions they make balance, and returns the
/// lines used, by transaction. A transaction takes one candidate whose amount is exactly what it
/// lacks, or all its candidates when together they are; otherwise it is left as it is (and stays
/// unbalanced).
fn recover_damaged_lines(
    transactions: &mut BTreeMap<i64, SourceTransaction>,
    salvage: HashMap<i64, Vec<SourceLine>>,
) -> Vec<(i64, SourceLine)> {
    let mut used = Vec::new();
    for (tid, candidates) in salvage {
        let Some(txn) = transactions.get_mut(&tid) else { continue };
        let missing = -txn.imbalance_cents();
        if missing == 0 {
            continue;
        }
        let recovered: Vec<SourceLine> = match candidates.iter().position(|l| l.cents == missing) {
            Some(i) => vec![candidates[i].clone()],
            None if checked_total(candidates.iter().map(|l| l.cents)) == Some(missing) => candidates,
            None => continue,
        };
        used.extend(recovered.iter().map(|l| (tid, l.clone())));
        txn.lines.extend(recovered);
    }
    used
}

/// The current version of each name's contact record (rows carry an `expired_date` once replaced).
fn current_contacts(pages: &[LogicalPage], catalog: &Catalog) -> HashMap<i64, Row> {
    let mut by_name: HashMap<i64, Row> = HashMap::new();
    for r in rows_of(pages, catalog, "abmc_contacts") {
        let Some(id) = r.get("name_id").as_int() else { continue };
        let rank = |row: &Row| (matches!(row.get("expired_date"), Value::Null), row.get("db_modified_tms").as_date());
        if by_name.get(&id).is_none_or(|old| rank(&r) > rank(old)) {
            by_name.insert(id, r);
        }
    }
    by_name
}

/// A plausible single email address, as LocalBooks stores one.
fn plain_email(candidate: &str) -> Option<String> {
    let e = candidate.trim();
    let (local, domain) = e.split_once('@')?;
    (!local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !e.contains(char::is_whitespace)).then(|| e.to_string())
}

/// Parent paths for a list keyed by id: `Parent:Child`, following `parent_id` (cycles cut).
fn paths(names: &BTreeMap<i64, (String, Option<i64>)>) -> HashMap<i64, String> {
    names
        .keys()
        .map(|&id| {
            let mut parts = Vec::new();
            let mut seen = HashSet::new();
            let mut cur = Some(id);
            while let Some(c) = cur.filter(|c| seen.insert(*c)) {
                let Some((name, parent)) = names.get(&c) else { break };
                parts.push(name.replace(':', "-"));
                cur = parent.filter(|p| *p > 0);
            }
            parts.reverse();
            (id, parts.join(":"))
        })
        .collect()
}

/// Which list a custom field definition is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldOwner {
    Customer,
    Vendor,
    Item,
}

/// Labels and values of custom fields, from `abmc_custom_field_type` (one row per defined field:
/// its position, its name, and which lists it is on) and `abmc_custom_field` (one row per value:
/// the record's `abm_id`, the position and the text). Returns each record's (label, value) pairs
/// in position order, for the records of `owner`.
pub(crate) fn custom_values(
    definitions: &[(i64, String, [bool; 3])],
    values: &[(String, i64, String)],
    owner: FieldOwner,
) -> HashMap<String, Vec<(String, String)>> {
    let index = match owner {
        FieldOwner::Customer => 0,
        FieldOwner::Vendor => 1,
        FieldOwner::Item => 2,
    };
    let labels: HashMap<i64, &str> = definitions.iter().filter(|d| d.2[index]).map(|d| (d.0, d.1.as_str())).collect();
    let mut sorted: Vec<&(String, i64, String)> = values.iter().filter(|v| !v.2.trim().is_empty()).collect();
    sorted.sort_by_key(|v| v.1);
    let mut out: HashMap<String, Vec<(String, String)>> = HashMap::new();
    for (abm, position, value) in sorted {
        if let Some(label) = labels.get(position) {
            out.entry(abm.clone()).or_default().push((label.to_string(), value.trim().to_string()));
        }
    }
    out
}

fn custom_values_of(pages: &[LogicalPage], catalog: &Catalog, owner: FieldOwner) -> HashMap<String, Vec<(String, String)>> {
    let flag = |r: &Row, c: &str| r.get(c).as_int() == Some(1);
    let definitions: Vec<(i64, String, [bool; 3])> = rows_of(pages, catalog, "abmc_custom_field_type")
        .iter()
        .filter_map(|r| {
            let flags = [flag(r, "is_for_customer_bool"), flag(r, "is_for_vendor_bool"), flag(r, "is_for_item_bool")];
            Some((r.get("custom_field_position").as_int()?, text(r, "custom_field_name")?, flags))
        })
        .collect();
    if definitions.is_empty() {
        return HashMap::new();
    }
    let values: Vec<(String, i64, String)> = rows_of(pages, catalog, "abmc_custom_field")
        .iter()
        .filter_map(|r| Some((r.get("abm_id").as_text()?, r.get("position").as_int()?, text(r, "field")?)))
        .collect();
    custom_values(&definitions, &values, owner)
}

/// A list's entries by id: the name and whether it is active.
pub(crate) type NamedList = HashMap<i64, (String, bool)>;

/// The sales rep list (`abmc_sales_rep`: initials and the name record they belong to) as
/// "initials" or "initials:name", with whether each is active; and the Ship Via list
/// (`abmc_shipping`). Keyed by their ids, as headers and customers refer to them.
pub(crate) fn read_sales_lists(pages: &[LogicalPage], catalog: &Catalog) -> (NamedList, NamedList) {
    let active = |r: &Row| r.get("is_hidden_bool").as_int() != Some(1) && r.get("is_deleted_bool").as_int() != Some(1);
    let reps_rows = rows_of(pages, catalog, "abmc_sales_rep");
    let contacts = if reps_rows.is_empty() { HashMap::new() } else { current_contacts(pages, catalog) };
    let reps = reps_rows
        .iter()
        .filter_map(|r| {
            let initials = text(r, "initials")?;
            let contact = r.get("name_id").as_int().and_then(|n| contacts.get(&n));
            let name = contact.and_then(|c| {
                let person = [text(c, "first_name"), text(c, "last_name")].into_iter().flatten().collect::<Vec<_>>().join(" ");
                (!person.is_empty()).then_some(person).or_else(|| text(c, "company_name"))
            });
            let rep = match name {
                Some(n) => format!("{}:{}", initials.trim(), n.replace(':', "-")),
                None => initials.trim().to_string(),
            };
            Some((r.get("sales_rep_id").as_int()?, (rep, active(r))))
        })
        .collect();
    let ships = rows_of(pages, catalog, "abmc_shipping").iter().filter_map(|r| Some((r.get("shipping_id").as_int()?, (text(r, "name")?, active(r))))).collect();
    (reps, ships)
}

/// One price level as the file stores it (`abmc_price_level` and its `abmc_price_level_item` rows).
/// `price_level_type` 2 is per item (as qbXML's PriceLevelType numbers it; 1 is fixed %), and a
/// level of another type that lists items is taken as per item too. `value_pct` is the fixed
/// percentage (−10 = 10% off), here in hundredths of a percent. A per-item level's items keep the
/// price QuickBooks worked out for them (`computed_price_amt`) as a fixed price; items priced only as
/// a percentage are left off (they sell at standard price).
pub(crate) fn price_level_of(name: String, hidden: bool, level_type: Option<i64>, value_hundredths: Option<i64>, items: &[(i64, Option<i64>)]) -> SourcePriceLevel {
    use crate::price_levels::{ItemBasis, LevelItem, LevelKind};
    let per_item = level_type == Some(2) || (level_type != Some(1) && !items.is_empty());
    SourcePriceLevel {
        name,
        active: !hidden,
        kind: if per_item { LevelKind::PerItem } else { LevelKind::FixedPercent },
        percent_milli: if per_item { 0 } else { value_hundredths.unwrap_or(0).saturating_mul(10) },
        items: if per_item {
            items
                .iter()
                .filter_map(|(item, cents)| Some(LevelItem { product_id: *item, basis: ItemBasis::Fixed, price_cents: Some(cents.as_ref()?.abs()), percent_milli: None }))
                .collect()
        } else {
            Vec::new()
        },
    }
}

/// The file's price levels and each customer's default (`abmc_customer_user.price_level_id`).
fn read_price_levels(pages: &[LogicalPage], catalog: &Catalog) -> (Vec<SourcePriceLevel>, Vec<(i64, String)>) {
    let rows = rows_of(pages, catalog, "abmc_price_level");
    if rows.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut items: HashMap<i64, Vec<(i64, Option<i64>)>> = HashMap::new();
    for r in rows_of(pages, catalog, "abmc_price_level_item") {
        if let (Some(level), Some(item)) = (r.get("price_level_id").as_int(), r.get("item_id").as_int()) {
            items.entry(level).or_default().push((item, r.get("computed_price_amt").as_cents()));
        }
    }
    let mut names: HashMap<i64, String> = HashMap::new();
    let mut levels = Vec::new();
    for r in &rows {
        let (Some(id), Some(name)) = (r.get("price_level_id").as_int(), text(r, "name")) else { continue };
        if r.get("is_deleted_bool").as_int() == Some(1) {
            continue;
        }
        let hidden = r.get("is_hidden_bool").as_int() == Some(1);
        names.insert(id, name.clone());
        levels.push(price_level_of(name, hidden, r.get("price_level_type").as_int(), r.get("value_pct").as_cents(), items.get(&id).map_or(&[][..], Vec::as_slice)));
    }
    let customers = rows_of(pages, catalog, "abmc_customer_user")
        .iter()
        .filter_map(|r| Some((r.get("customer_id").as_int()?, names.get(&r.get("price_level_id").as_int()?)?.clone())))
        .collect();
    (levels, customers)
}

fn read_parties(pages: &[LogicalPage], catalog: &Catalog, vendor: bool) -> Vec<SourceParty> {
    let (user, internal, id_col) =
        if vendor { ("abmc_vendor_user", "abmc_vendor_internal", "vendor_id") } else { ("abmc_customer_user", "abmc_customer_internal", "customer_id") };
    let contacts = current_contacts(pages, catalog);
    let terms: HashMap<i64, i64> = rows_of(pages, catalog, "abmc_terms")
        .iter()
        .filter_map(|r| Some((r.get("terms_id").as_int()?, r.get("standard_due_days").as_int()?)))
        .filter(|(_, d)| (0..=365).contains(d))
        .collect();
    let balances: HashMap<i64, i64> = rows_of(pages, catalog, internal)
        .iter()
        .filter_map(|r| Some((r.get(id_col).as_int()?, r.get("end_balance_amt").as_cents().unwrap_or(0))))
        .collect();
    let users: Vec<Row> = rows_of(pages, catalog, user);
    let names: BTreeMap<i64, (String, Option<i64>)> = users
        .iter()
        .filter_map(|r| Some((r.get(id_col).as_int()?, (text(r, "name")?, if vendor { None } else { r.get("parent_id").as_int() }))))
        .collect();
    let full = paths(&names);
    let reps = if vendor { HashMap::new() } else { read_sales_lists(pages, catalog).0 };
    let custom = custom_values_of(pages, catalog, if vendor { FieldOwner::Vendor } else { FieldOwner::Customer });
    let abm_col = if vendor { "vendor_abm_id" } else { "customer_abm_id" };
    let mut out = Vec::new();
    for r in &users {
        let Some(id) = r.get(id_col).as_int() else { continue };
        let Some(name) = full.get(&id).cloned() else { continue };
        let contact = contacts.get(&id);
        let field = |c: &str| contact.and_then(|row| text(row, c));
        let mut notes = Vec::new();
        let mut email = None;
        for (i, candidate) in field("email").unwrap_or_default().split([',', ';']).filter(|s| !s.trim().is_empty()).enumerate() {
            match (i, plain_email(candidate)) {
                (0, Some(e)) => email = Some(e),
                _ => notes.push(format!("Email: {}", candidate.trim())),
            }
        }
        for (label, c) in [("Phone 2", "telephone_num2"), ("Mobile", "cellular_num"), ("Fax", "fax_num")] {
            if let Some(v) = field(c) {
                notes.push(format!("{label}: {v}"));
            }
        }
        let address: Vec<String> = (1..=5).filter_map(|n| field(&format!("address_line{n}"))).collect();
        let person = [field("first_name"), field("last_name")].into_iter().flatten().collect::<Vec<_>>().join(" ");
        let balance = balances.get(&id).copied();
        let hidden = r.get("is_hidden_bool").as_int() == Some(1) || r.get("is_deleted_bool").as_int() == Some(1);
        out.push(SourceParty {
            id,
            name,
            parent_id: if vendor { None } else { r.get("parent_id").as_int().filter(|p| *p > 0 && names.contains_key(p)) },
            company_name: field("company_name").or_else(|| (!person.is_empty()).then_some(person)),
            email,
            phone: field("telephone_num1"),
            address: (!address.is_empty()).then(|| address.join("\n")),
            terms_days: r.get("terms_id").as_int().and_then(|t| terms.get(&t).copied()),
            // Newer QuickBooks files hold this encrypted with QuickBooks' own key, which cannot be read.
            tax_id: if vendor { text(r, "tax_identifier").filter(|t| crate::tax_ids::looks_like_tax_id(t)) } else { None },
            is_1099: vendor && r.get("is_1099_vendor_bool").as_int() == Some(1),
            // As in Intuit's own conversion: an inactive name that still has a balance comes in active.
            active: !hidden || balance.is_some_and(|b| b != 0),
            balance_cents: balance,
            notes: (!notes.is_empty()).then(|| notes.join("\n")),
            sales_rep: r.get("sales_rep_id").as_int().and_then(|s| reps.get(&s)).map(|(rep, _)| rep.clone()),
            custom_fields: r.get(abm_col).as_text().and_then(|a| custom.get(&a).cloned()).unwrap_or_default(),
        });
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// An item's kind by the file's item type code, or `None` for the kinds that are not things
/// LocalBooks sells: subtotals (a form line, not an item), groups, payments, and sales tax items
/// and groups (sales tax rates come in with the tax settings).
pub(crate) fn item_kind(code: Option<i64>) -> Option<&'static str> {
    match code {
        Some(0) => Some("Service"),
        Some(1 | 2) => Some("Inventory part"),
        Some(3) => Some("Non-inventory part"),
        Some(4) => Some("Fixed asset"),
        Some(5) => Some("Other charge"),
        Some(8) => Some("Discount"),
        Some(6 | 7 | 9 | 10 | 11) => None,
        _ => Some("Item"),
    }
}

/// Ids of the file's subtotal items: a form line that sums the lines above it.
pub(crate) fn subtotal_items(pages: &[LogicalPage], catalog: &Catalog) -> HashSet<i64> {
    rows_of(pages, catalog, "abmc_item_user").iter().filter(|r| r.get("item_type").as_int() == Some(6)).filter_map(|r| r.get("item_id").as_int()).collect()
}

fn read_items(pages: &[LogicalPage], catalog: &Catalog) -> Vec<SourceItem> {
    let taxable_codes: HashSet<i64> = rows_of(pages, catalog, "abmc_sales_tax_code")
        .iter()
        .filter(|r| r.get("is_taxable_bool").as_int() == Some(1))
        .filter_map(|r| r.get("sales_tax_code_id").as_int())
        .collect();
    let users = rows_of(pages, catalog, "abmc_item_user");
    let names: BTreeMap<i64, (String, Option<i64>)> =
        users.iter().filter_map(|r| Some((r.get("item_id").as_int()?, (text(r, "name")?, r.get("parent_id").as_int())))).collect();
    let full = paths(&names);
    let custom = custom_values_of(pages, catalog, FieldOwner::Item);
    let mut out: Vec<SourceItem> = users
        .iter()
        .filter_map(|r| {
            let id = r.get("item_id").as_int()?;
            let kind = item_kind(r.get("item_type").as_int())?;
            Some(SourceItem {
                id,
                name: full.get(&id)?.clone(),
                description: text(r, "description"),
                rate_cents: r.get("sales_price_amt").as_cents().map(i64::abs).unwrap_or(0),
                income_account_id: r.get("post_account_id").as_int(),
                percent_ppm: if matches!(kind, "Discount" | "Other charge") { r.get("sales_price_pct").as_percent_ppm() } else { None },
                kind,
                taxable: r.get("sales_tax_code_id").as_int().is_some_and(|c| taxable_codes.contains(&c)),
                active: r.get("is_hidden_bool").as_int() != Some(1) && r.get("is_deleted_bool").as_int() != Some(1),
                custom_fields: r.get("item_abm_id").as_text().and_then(|a| custom.get(&a).cloned()).unwrap_or_default(),
                purchase_description: text(r, "purchase_description").or_else(|| text(r, "purchase_desc")),
                purchase_cost_cents: r.get("purchase_cost_amt").as_cents().map(i64::abs),
            })
        })
        .collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// The file's item groups (item type 7). Their items are rows of `abmc_grouped_items`: the group's
/// `item_id`, the member's `sub_item_id` and its `quantity_qnty` (stored negative, or left empty
/// when the quantity is typed on the form; such an item comes in at quantity 1). The table's
/// column 2 orders a group's items; some files lost its name from the catalog, so it is found by
/// id. The same table lists the parts of assembly items, which are not groups and are left out.
fn read_item_groups(pages: &[LogicalPage], catalog: &Catalog) -> Vec<SourceItemGroup> {
    let order_column = catalog
        .table_id("abmc_grouped_items")
        .and_then(|t| catalog.columns.get(&t))
        .and_then(|cols| cols.iter().find(|c| c.id == 2))
        // A position is a whole number that isn't an id; any other column 2 leaves the file's order.
        .filter(|c| matches!(c.domain, crate::qbw_records::MISSING_DOMAIN | 1 | 2 | 19 | 21 | 22) && !c.name.ends_with("_id"))
        .map(|c| c.name.clone());
    item_groups_from(&rows_of(pages, catalog, "abmc_item_user"), &rows_of(pages, catalog, "abmc_grouped_items"), order_column.as_deref())
}

/// The groups among the item rows, with their items from the grouped-items rows (see
/// [`read_item_groups`]); `order_column` names the column that orders a group's items.
fn item_groups_from(users: &[Row], grouped: &[Row], order_column: Option<&str>) -> Vec<SourceItemGroup> {
    let names: BTreeMap<i64, (String, Option<i64>)> =
        users.iter().filter_map(|r| Some((r.get("item_id").as_int()?, (text(r, "name")?, r.get("parent_id").as_int())))).collect();
    let full = paths(&names);
    let mut members: BTreeMap<i64, Vec<(i64, usize, i64, i64)>> = BTreeMap::new();
    for (n, r) in grouped.iter().enumerate() {
        // Item 0 is a blank line in the group.
        let (Some(group), Some(item)) = (r.get("item_id").as_int(), r.get("sub_item_id").as_int().filter(|i| *i != 0)) else {
            continue;
        };
        // An item listed with no quantity is one whose quantity is typed on the form; a LocalBooks
        // group needs one, so it comes in at 1.
        let quantity = r.get("quantity_qnty").as_quantity_e4().map(i64::saturating_abs).filter(|q| *q != 0).unwrap_or(crate::ledger::QUANTITY_SCALE);
        let position = order_column.and_then(|c| r.get(c).as_int()).unwrap_or(i64::MAX);
        members.entry(group).or_default().push((position, n, item, quantity));
    }
    let mut out: Vec<SourceItemGroup> = users
        .iter()
        .filter(|r| r.get("item_type").as_int() == Some(7) && r.get("is_deleted_bool").as_int() != Some(1))
        .filter_map(|r| {
            let id = r.get("item_id").as_int()?;
            let mut items = members.remove(&id).unwrap_or_default();
            items.sort_by_key(|m| (m.0, m.1));
            Some(SourceItemGroup {
                id,
                name: full.get(&id)?.clone(),
                description: text(r, "description"),
                print_items: r.get("print_grouped_bool").as_int() == Some(1),
                active: r.get("is_hidden_bool").as_int() != Some(1),
                members: items.into_iter().map(|(_, _, item, quantity)| (item, quantity)).collect(),
            })
        })
        .collect();
    out.sort_by_key(|g| g.name.to_lowercase());
    out
}

/// The class list from `abmc_class`: each class's `Parent:Child` path by id, and the list with
/// whether each class is active (deleted classes are left out).
pub(crate) fn read_classes(pages: &[LogicalPage], catalog: &Catalog) -> (HashMap<i64, String>, Vec<(String, bool)>) {
    let rows: Vec<Row> = rows_of(pages, catalog, "abmc_class").into_iter().filter(|r| r.get("is_deleted_bool").as_int() != Some(1)).collect();
    let names: BTreeMap<i64, (String, Option<i64>)> =
        rows.iter().filter_map(|r| Some((r.get("class_id").as_int()?, (text(r, "name")?, r.get("parent_id").as_int())))).collect();
    let full = paths(&names);
    let list = rows
        .iter()
        .filter_map(|r| {
            let path = full.get(&r.get("class_id").as_int()?)?;
            Some((path.clone(), r.get("is_hidden_bool").as_int() != Some(1)))
        })
        .collect();
    (full, list)
}

/// Reads the general ledger. Blocking and CPU-bound.
pub fn read_ledger(path: &Path) -> Result<Ledger, String> {
    let (pages, catalog, page_stats, refined) = read_pages(path)?;
    let mut quality = LedgerQuality { pages: page_stats, refined_pages: refined, ..Default::default() };
    let accounts = read_accounts(&pages, &catalog);
    let known_accounts: HashSet<i64> = accounts.iter().map(|a| a.id).collect();
    let customers = names(&pages, &catalog, "abmc_customer_user", "customer_id");
    let vendors = names(&pages, &catalog, "abmc_vendor_user", "vendor_id");
    let employees = names(&pages, &catalog, "abmc_employee", "employee_id");
    let (class_paths, class_list) = read_classes(&pages, &catalog);
    let class_of = |row: &Row| row.get("class_id").as_int().and_then(|c| class_paths.get(&c)).cloned();
    let earliest = NaiveDate::from_ymd_opt(1980, 1, 1).expect("valid date");
    let latest = chrono::Local::now().date_naive().with_year(chrono::Local::now().year() + 1).unwrap_or(NaiveDate::MAX);

    let mut transactions: BTreeMap<i64, SourceTransaction> = BTreeMap::new();
    // Lines whose row is damaged after its leading columns: used only when they make their
    // transaction balance to the cent.
    let mut salvage: HashMap<i64, Vec<SourceLine>> = HashMap::new();
    let mut unreadable: Vec<UnreadableLine> = Vec::new();
    for (stem, kind) in TRANSACTION_TABLES {
        let Some(line_table) = catalog.table_id(&format!("{stem}_lineitem")) else { continue };
        let headers: HashMap<i64, Row> = rows_of(&pages, &catalog, &format!("{stem}_header"))
            .into_iter()
            .filter_map(|r| Some((r.get("transaction_id").as_int()?, r)))
            .collect();
        let lines = table_rows(&pages, &catalog, line_table);
        quality.unreadable_lines += lines.unchained + lines.unwalkable;
        let party = |row: &Row| {
            row.get("customer_id")
                .as_int()
                .and_then(|id| customers.get(&id))
                .or_else(|| row.get("vendor_id").as_int().and_then(|id| vendors.get(&id)))
                .or_else(|| row.get("employee_id").as_int().and_then(|id| employees.get(&id)))
                .cloned()
        };
        for r in &lines.partial {
            let tid = r.get("transaction_id").as_int();
            let account_id = r.get("account_id").as_int().filter(|a| known_accounts.contains(a));
            let cents = r.get("amount_amt").as_cents();
            let header = tid.and_then(|t| headers.get(&t));
            unreadable.push(UnreadableLine {
                kind,
                transaction_id: tid,
                date: header.and_then(|h| h.get("transaction_date").as_date()).or_else(|| r.get("transaction_date").as_date()),
                doc_num: header.and_then(|h| text(h, "doc_num")),
                name: header.and_then(party),
                account_id,
                cents,
                outcome: if cents.unwrap_or(0) == 0 { LineOutcome::NoAmount } else { LineOutcome::Unreadable },
            });
            if let (Some(tid), Some(account_id), Some(cents)) = (tid, account_id, cents) {
                if cents != 0 && r.get("is_no_post_bool").as_int() != Some(1) {
                    salvage.entry(tid).or_default().push(SourceLine { account_id, cents, memo: None, class: class_of(r) });
                }
            }
        }
        // Rows whose length does not chain cannot be read at all.
        for _ in 0..lines.unchained {
            unreadable.push(UnreadableLine {
                kind,
                transaction_id: None,
                date: None,
                doc_num: None,
                name: None,
                account_id: None,
                cents: None,
                outcome: LineOutcome::Unreadable,
            });
        }
        for r in lines.rows {
            if r.get("is_no_post_bool").as_int() == Some(1) || memorized(&r, r.get("transaction_id").as_int().and_then(|t| headers.get(&t))) {
                continue;
            }
            let checked = (|| {
                let tid = r.get("transaction_id").as_int().ok_or("no transaction")?;
                let cents = match r.get("amount_amt") {
                    Value::Null => 0,
                    v => v.as_cents().ok_or("amount not in cents")?,
                };
                // Larger than any amount the books can hold: a damaged value, kept out so later
                // totals cannot overflow.
                if cents.unsigned_abs() > crate::ledger::MAX_LINE_CENTS.unsigned_abs() {
                    return Err("amount too large");
                }
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
                let name = party(header.unwrap_or(&r));
                SourceTransaction {
                    id: tid,
                    kind,
                    date,
                    doc_num: header.and_then(|h| text(h, "doc_num")).or_else(|| text(&r, "doc_num")),
                    name,
                    memo: header.and_then(|h| text(h, "memo")),
                    lines: Vec::new(),
                    document: None,
                }
            });
            if cents != 0 {
                txn.lines.push(SourceLine { account_id, cents, memo: text(&r, "memo"), class: class_of(&r) });
            }
        }
    }

    for (cost_table, line_table) in INVENTORY_COST_TABLES {
        let (Some(cost_table), Some(line_table)) = (catalog.table_id(cost_table), catalog.table_id(line_table)) else { continue };
        let form_lines: HashMap<i64, (i64, Option<String>)> = table_rows(&pages, &catalog, line_table)
            .rows
            .iter()
            .filter_map(|r| Some((r.get("target_id").as_int()?, (r.get("transaction_id").as_int()?, class_of(r)))))
            .collect();
        let costs = table_rows(&pages, &catalog, cost_table);
        // Not transaction lines, so not listed with them; a lost row leaves its pair out below.
        let lost = costs.unchained + costs.unwalkable;
        if lost > 0 {
            quality.rejected_lines += lost;
            *quality.rejected_by_reason.entry("inventory cost row not read").or_default() += lost;
        }
        add_inventory_cost(&mut transactions, &form_lines, &costs.rows, &known_accounts, &mut quality);
    }

    for (tid, line) in recover_damaged_lines(&mut transactions, salvage) {
        quality.recovered_lines += 1;
        if let Some(u) = unreadable.iter_mut().find(|u| {
            u.outcome == LineOutcome::Unreadable && u.transaction_id == Some(tid) && u.account_id == Some(line.account_id) && u.cents == Some(line.cents)
        }) {
            u.outcome = LineOutcome::Recovered;
        }
    }

    let mut totals: HashMap<i64, i64> = HashMap::new();
    for txn in transactions.values() {
        for line in &txn.lines {
            *totals.entry(line.account_id).or_default() += line.cents;
        }
    }
    quality.fractional_amounts = fractional_amounts(&pages, &catalog);
    for account in &accounts {
        if let Some(stored) = account.stored_balance_cents {
            quality.accounts_with_stored_balance += 1;
            quality.accounts_matching_stored += (totals.get(&account.id).copied().unwrap_or(0) == stored) as usize;
        }
    }

    quality.joined_transactions = join_split_halves(&mut transactions);
    let (mut balanced, mut unbalanced): (Vec<_>, Vec<_>) =
        transactions.into_values().filter(|t| !t.lines.is_empty()).partition(|t| t.imbalance_cents() == 0);
    balanced.sort_by_key(|t| (t.date, t.id));
    unbalanced.sort_by_key(|t| (t.date, t.id));
    let arap: HashSet<i64> =
        accounts.iter().filter(|a| matches!(a.account_type, "ACCOUNTS_RECEIVABLE" | "ACCOUNTS_PAYABLE")).map(|a| a.id).collect();
    let (mut documents, too_large) = crate::qbw_documents::read_documents(&pages, &catalog, &|a| arap.contains(&a), &class_paths);
    if too_large > 0 {
        quality.rejected_lines += too_large;
        *quality.rejected_by_reason.entry("form line amount too large").or_default() += too_large;
    }
    for t in &mut balanced {
        t.document = documents.remove(&t.id);
    }
    // Estimates and purchase orders post nothing: they come along as transactions without lines.
    for (id, document) in documents {
        let (kind, date, doc_num, name, memo) = match &document {
            crate::qbw_documents::SourceDocument::Estimate { customer_id, date, number, memo, .. } => {
                ("Estimate", *date, number.clone(), customers.get(customer_id).cloned(), memo.clone())
            }
            crate::qbw_documents::SourceDocument::PurchaseOrder { vendor_id, date, number, memo, .. } => {
                ("Purchase order", *date, number.clone(), vendors.get(vendor_id).cloned(), memo.clone())
            }
            _ => continue,
        };
        if (earliest..=latest).contains(&date) {
            balanced.push(SourceTransaction { id, kind, date, doc_num, name, memo, lines: Vec::new(), document: Some(document) });
        }
    }
    balanced.sort_by_key(|t| (t.date, t.id));
    unreadable.sort_by_key(|u| (u.date, u.transaction_id));
    let customers = read_parties(&pages, &catalog, false);
    let vendors = read_parties(&pages, &catalog, true);
    let items = read_items(&pages, &catalog);
    let (reps, ships) = read_sales_lists(&pages, &catalog);
    let mut sales_lists = SourceSalesLists::default();
    for (rep, active) in reps.into_values() {
        sales_lists.add_rep(&rep, active);
    }
    for (name, active) in ships.into_values() {
        sales_lists.add_ship_method(&name, active);
    }
    sales_lists.reps.sort();
    sales_lists.ship_methods.sort();
    (sales_lists.price_levels, sales_lists.customer_price_levels) = read_price_levels(&pages, &catalog);
    sales_lists.item_groups = read_item_groups(&pages, &catalog);
    let mut ledger = Ledger { accounts, customers, vendors, items, transactions: balanced, unbalanced, unreadable, quality, sales_lists, classes: Vec::new() };
    for (name, active) in class_list {
        ledger.add_class(&name, active);
    }
    ledger.add_classes_used();
    Ok(ledger)
}

#[cfg(test)]
mod tests;
