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
const TRANSACTION_TABLES: [(&str, &str); 14] = [
    ("invoice", "Invoice"),
    ("credit_memo", "Credit memo"),
    ("sales_receipt", "Sales receipt"),
    ("received_payment", "Payment"),
    ("deposit", "Deposit"),
    ("transfer", "Transfer"),
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
    /// Lines read from rows damaged near their end (memo lost), kept because they make their
    /// transaction balance exactly.
    pub recovered_lines: usize,
    /// Decoded lines rejected by the checks (unknown account, odd amount, impossible date).
    pub rejected_lines: usize,
    pub rejected_by_reason: BTreeMap<&'static str, usize>,
    /// Accounts whose total from the decoded lines equals the balance the file stores.
    pub accounts_matching_stored: usize,
    pub accounts_with_stored_balance: usize,
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
}

/// The file's sales rep and shipping method lists, by name, with whether each is active. They are
/// also found or created from the customers and documents that use them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceSalesLists {
    /// Initials, or "initials:name".
    pub reps: Vec<(String, bool)>,
    pub ship_methods: Vec<(String, bool)>,
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
                tax_line: r.get("tax_line_id_num").as_int().filter(|n| *n > 0).map(|n| crate::accountant_reports::quickbooks_tax_line(n)),
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
            None if candidates.iter().map(|l| l.cents).sum::<i64>() == missing => candidates,
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

/// The sales rep list (`abmc_sales_rep`: initials and the name record they belong to) as
/// "initials" or "initials:name", with whether each is active; and the Ship Via list
/// (`abmc_shipping`). Keyed by their ids, as headers and customers refer to them.
pub(crate) fn read_sales_lists(pages: &[LogicalPage], catalog: &Catalog) -> (HashMap<i64, (String, bool)>, HashMap<i64, (String, bool)>) {
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
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
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
            Some(SourceItem {
                id,
                name: full.get(&id)?.clone(),
                description: text(r, "description"),
                rate_cents: r.get("sales_price_amt").as_cents().map(i64::abs).unwrap_or(0),
                income_account_id: r.get("post_account_id").as_int(),
                kind: match r.get("item_type").as_int() {
                    Some(0) => "Service",
                    Some(1) => "Inventory part",
                    Some(3) => "Non-inventory part",
                    Some(4) => "Other charge",
                    _ => "Item",
                },
                taxable: r.get("sales_tax_code_id").as_int().is_some_and(|c| taxable_codes.contains(&c)),
                active: r.get("is_hidden_bool").as_int() != Some(1) && r.get("is_deleted_bool").as_int() != Some(1),
                custom_fields: r.get("item_abm_id").as_text().and_then(|a| custom.get(&a).cloned()).unwrap_or_default(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
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
    // Lines whose row is damaged after its leading columns: used only when they make their
    // transaction balance to the cent.
    let mut salvage: HashMap<i64, Vec<SourceLine>> = HashMap::new();
    let mut unreadable: Vec<UnreadableLine> = Vec::new();
    for (stem, kind) in TRANSACTION_TABLES {
        let Some(line_table) = catalog.table_id(&format!("abmc_{stem}_lineitem")) else { continue };
        let headers: HashMap<i64, Row> = rows_of(&pages, &catalog, &format!("abmc_{stem}_header"))
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
                    salvage.entry(tid).or_default().push(SourceLine { account_id, cents, memo: None });
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
                txn.lines.push(SourceLine { account_id, cents, memo: text(&r, "memo") });
            }
        }
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
    let arap: HashSet<i64> =
        accounts.iter().filter(|a| matches!(a.account_type, "ACCOUNTS_RECEIVABLE" | "ACCOUNTS_PAYABLE")).map(|a| a.id).collect();
    let mut documents = crate::qbw_documents::read_documents(&pages, &catalog, &|a| arap.contains(&a));
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
    Ok(Ledger { accounts, customers, vendors, items, transactions: balanced, unbalanced, unreadable, quality, sales_lists })
}

#[cfg(test)]
mod tests;
