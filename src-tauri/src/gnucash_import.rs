//! Reads GnuCash books into a [`Ledger`], for GnuCash users moving to LocalBooks.
//!
//! GnuCash saves a book either as gzip-compressed XML (the default `.gnucash` file) or as an
//! SQLite database (a `.gnucash` file that starts with `SQLite format 3`). Both hold the same
//! objects, so each reader ([`sqlite`], [`xml`]) fills one [`Book`], and [`ledger`] turns that
//! into the shared ledger: mapping, journal or full import and the balance checks then work as
//! for any other file. References: the GnuCash documentation (the SQL schema on the GnuCash wiki
//! and the XML file format notes), and piecash's schema notes; no code is taken from either.
//!
//! - An account is a tree node with a type, a commodity, and the `placeholder` and `hidden` flags.
//!   The `Parent:Child` path leaves out the invisible root. A placeholder holds no transactions
//!   of its own and becomes an ordinary parent account. LocalBooks subaccounts share their
//!   parent's type, so an account whose type differs from its parent's (a bank account under
//!   GnuCash's `Assets` placeholder) moves to the top level.
//! - A transaction has a currency and splits. A split's `value` is in the transaction's currency
//!   and its `quantity` in its account's commodity. Amounts are exact fractions (`value_num` /
//!   `value_denom`, or `num/denom` in XML) and import only when they are whole cents.
//! - The home currency is the one most transactions use. A split's home amount is its value when
//!   the transaction is in the home currency, or else its quantity when its account is.
//!   Otherwise there is no home amount, and the transaction is listed rather than converted at a
//!   guessed rate.
//! - Stock and mutual fund accounts hold securities, which LocalBooks does not track. Their
//!   transactions are skipped and listed.
//! - Business features: a posted invoice, bill, credit note or expense voucher points at its
//!   posting transaction and at the lot that gathers its receivable or payable splits. A payment
//!   puts its receivable split in the invoice's lot, or (GnuCash 2.6 and later) in a lot of its
//!   own that a zero-sum "link" transaction ties to the invoice's lot. Following those lots says
//!   which invoice each payment paid. Customers' jobs become sub-customers.

mod sqlite;
mod xml;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use chrono::{Duration, NaiveDate, NaiveDateTime};

use crate::qbw_company::{Ledger, LedgerQuality, LineOutcome, SourceAccount, SourceLine, SourceParty, SourceTransaction, UnreadableLine};
use crate::qbw_documents::{payment_method, Applied, DocumentLine, SourceDocument, SourceSales};
use crate::qif_import::SKIPPED_INVESTMENT;
use crate::text_import::blank_party;

pub use sqlite::read_sqlite;
#[cfg(test)]
use xml::read_xml;

const NOT_WHOLE_CENTS: &str = "amounts that are not whole cents";
const NO_HOME_AMOUNT: &str = "transactions in another currency with no home-currency amount";
const UNKNOWN_ACCOUNT: &str = "splits whose account is not in the book";
const NO_DATE: &str = "transactions without a date";

/// The kind of file a GnuCash book is saved as, from its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Sqlite,
    /// Gzip-compressed XML (GnuCash's default).
    GzipXml,
    Xml,
}

pub fn sniff(head: &[u8]) -> Option<Format> {
    if head.starts_with(b"SQLite format 3\0") {
        Some(Format::Sqlite)
    } else if head.starts_with(&[0x1f, 0x8b]) {
        Some(Format::GzipXml)
    } else if head.trim_ascii_start().starts_with(b"<?xml") || head.trim_ascii_start().starts_with(b"<gnc-v2") {
        Some(Format::Xml)
    } else {
        None
    }
}

/// Reads a GnuCash book of either kind.
pub async fn read(path: &Path) -> Result<Ledger, String> {
    let mut head = [0u8; 16];
    let n = {
        use std::io::Read;
        let mut file = std::fs::File::open(path).map_err(|e| format!("GNUCASH_UNREADABLE: {}: {e}", path.display()))?;
        file.read(&mut head).map_err(|e| format!("GNUCASH_UNREADABLE: {}: {e}", path.display()))?
    };
    let book = match sniff(&head[..n]) {
        Some(Format::Sqlite) => read_sqlite(path).await?,
        Some(format) => {
            let path = path.to_path_buf();
            tokio::task::spawn_blocking(move || xml::open(&path, format))
                .await
                .map_err(|e| format!("GNUCASH_PARSE_FAILED: the book could not be read ({e})"))??
        }
        None => return Err("GNUCASH_NOT_GNUCASH: this file is neither a GnuCash XML book nor a GnuCash SQLite book".into()),
    };
    let ledger = ledger(&book);
    // A book of lists alone (customers or vendors, no accounts yet) imports as a lists-only batch.
    let nothing = ledger.accounts.is_empty() && ledger.customers.is_empty() && ledger.vendors.is_empty();
    if nothing && ledger.transactions.is_empty() && ledger.unbalanced.is_empty() && ledger.unreadable.is_empty() {
        return Err("GNUCASH_EMPTY: this book has no accounts, customers, vendors or transactions".into());
    }
    Ok(ledger)
}

// ------------------------------------------------------------------------------------------------
// The book as either reader finds it
// ------------------------------------------------------------------------------------------------

/// An exact amount, `num / denom`, as GnuCash stores it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Num {
    pub num: i64,
    pub denom: i64,
}

impl Num {
    pub fn new(num: i64, denom: i64) -> Self {
        Num { num, denom }
    }

    /// `"-12345/100"` or `"7"`.
    pub fn parse(raw: &str) -> Option<Num> {
        let raw = raw.trim();
        let (num, denom) = raw.split_once('/').unwrap_or((raw, "1"));
        let num = Num { num: num.trim().parse().ok()?, denom: denom.trim().parse().ok()? };
        (num.denom > 0).then_some(num)
    }

    pub fn is_zero(self) -> bool {
        self.num == 0
    }

    /// The amount in cents, when it is a whole number of cents.
    pub fn cents(self) -> Option<i64> {
        self.scaled(100)
    }

    /// The amount times `scale`, when that is a whole number.
    fn scaled(self, scale: i128) -> Option<i64> {
        if self.denom <= 0 {
            return None;
        }
        let x = self.num as i128 * scale;
        (x % self.denom as i128 == 0).then(|| x / self.denom as i128).and_then(|v| i64::try_from(v).ok())
    }

    /// The amount in cents, rounded half away from zero, as GnuCash rounds an invoice line.
    fn rounded_cents(num: i128, denom: i128) -> Option<i64> {
        if denom <= 0 {
            return None;
        }
        let x = num * 100;
        let (q, r) = (x / denom, x % denom);
        let q = if 2 * r.abs() >= denom { q + x.signum() } else { q };
        i64::try_from(q).ok()
    }

    /// As a person would write it: `12.345`, or `1/3` when it has no decimal form.
    pub fn display(self) -> String {
        let mut denom = self.denom;
        let mut places = 0;
        while denom > 1 && denom % 10 == 0 {
            denom /= 10;
            places += 1;
        }
        if denom != 1 {
            return format!("{}/{}", self.num, self.denom);
        }
        if places == 0 {
            return self.num.to_string();
        }
        let digits = format!("{:0>width$}", self.num.unsigned_abs(), width = places + 1);
        let (whole, frac) = digits.split_at(digits.len() - places);
        format!("{}{whole}.{frac}", if self.num < 0 { "-" } else { "" })
    }
}

/// A commodity as `NAMESPACE:SYMBOL` (`CURRENCY:USD`, `NASDAQ:AAPL`); older books say `ISO4217`
/// for currencies.
pub fn commodity_key(space: &str, id: &str) -> String {
    let space = space.trim();
    let space = if space.eq_ignore_ascii_case("ISO4217") { "CURRENCY" } else { space };
    format!("{}:{}", space.to_ascii_uppercase(), id.trim())
}

fn currency_code(key: &str) -> &str {
    key.rsplit(':').next().unwrap_or(key)
}

#[derive(Debug, Clone, Default)]
pub struct Account {
    pub guid: String,
    pub name: String,
    /// GnuCash's type: `BANK`, `CASH`, `ASSET`, `STOCK`, ..., `ROOT`.
    pub kind: String,
    pub commodity: String,
    pub parent: Option<String>,
    pub hidden: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Split {
    pub account: String,
    pub memo: Option<String>,
    pub value: Num,
    pub quantity: Num,
    pub lot: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Transaction {
    pub guid: String,
    pub currency: String,
    pub num: Option<String>,
    pub date: Option<NaiveDate>,
    pub description: Option<String>,
    /// The `trans-txn-type` slot: `I` invoice (or bill), `P` payment, `L` lot link.
    pub txn_type: Option<String>,
    /// The `trans-date-due` slot of an invoice's posting transaction.
    pub due: Option<NaiveDate>,
    pub notes: Option<String>,
    pub splits: Vec<Split>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerKind {
    Customer,
    Job,
    Vendor,
    Employee,
}

impl OwnerKind {
    /// The SQL backend's `owner_type` (`GncOwnerType`).
    pub fn from_code(code: i64) -> Option<Self> {
        Some(match code {
            2 => OwnerKind::Customer,
            3 => OwnerKind::Job,
            4 => OwnerKind::Vendor,
            5 => OwnerKind::Employee,
            _ => return None,
        })
    }

    /// The XML file's `owner:type` (`gncCustomer`, ...).
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name.trim() {
            "gncCustomer" => OwnerKind::Customer,
            "gncJob" => OwnerKind::Job,
            "gncVendor" => OwnerKind::Vendor,
            "gncEmployee" => OwnerKind::Employee,
            _ => return None,
        })
    }
}

pub type Owner = (OwnerKind, String);

#[derive(Debug, Clone, Default)]
pub struct Address {
    pub name: Option<String>,
    pub lines: Vec<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
}

/// A customer, vendor or employee.
#[derive(Debug, Clone, Default)]
pub struct Party {
    pub guid: String,
    pub name: String,
    pub notes: Option<String>,
    pub active: bool,
    pub address: Address,
    pub terms: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Job {
    pub guid: String,
    pub name: String,
    pub active: bool,
    pub owner: Option<Owner>,
}

#[derive(Debug, Clone, Default)]
pub struct Invoice {
    pub guid: String,
    /// The invoice number.
    pub id: Option<String>,
    pub owner: Option<Owner>,
    pub terms: Option<String>,
    pub post_txn: Option<String>,
    pub post_lot: Option<String>,
    pub credit_note: bool,
}

/// One line of an invoice or bill.
#[derive(Debug, Clone, Default)]
pub struct Entry {
    pub invoice: Option<String>,
    pub bill: Option<String>,
    pub description: Option<String>,
    pub quantity: Num,
    pub i_account: Option<String>,
    pub i_price: Num,
    pub i_discount: Num,
    /// `VALUE` or `PERCENT`.
    pub i_discount_type: Option<String>,
    pub b_account: Option<String>,
    pub b_price: Num,
}

#[derive(Debug, Clone, Default)]
pub struct Lot {
    pub owner: Option<Owner>,
    pub invoice: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Book {
    /// Every account of the book's tree, root included; scheduled-transaction templates excluded.
    pub accounts: Vec<Account>,
    pub transactions: Vec<Transaction>,
    pub customers: Vec<Party>,
    pub vendors: Vec<Party>,
    pub employees: Vec<Party>,
    pub jobs: Vec<Job>,
    pub invoices: Vec<Invoice>,
    pub entries: Vec<Entry>,
    /// Billing terms: guid → days until due.
    pub terms: HashMap<String, i64>,
    pub lots: HashMap<String, Lot>,
}

/// A date as GnuCash stores it: `2024-03-12 10:59:00` (SQLite, UTC), `20240312105900` (older
/// SQLite), `2024-03-12 10:59:00 +0000` (XML) or `2024-03-12` (a date-only slot).
///
/// GnuCash stores a posting date as 10:59 UTC since 2.6.10, and before that as midnight local
/// time, which in UTC can fall on the evening before. When the stored time has a UTC offset that
/// is not zero, the date as written is the local one; a UTC time is moved forward 12 hours, which
/// gives the local date for midnight anywhere from UTC-12 to UTC+12 and for 10:59 UTC.
pub fn parse_date(raw: &str) -> Option<NaiveDate> {
    let raw = raw.trim();
    if let Ok(d) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        return Some(d);
    }
    let (stamp, offset) = match raw.rsplit_once(' ') {
        Some((s, o)) if o.starts_with(['+', '-']) => (s, Some(o)),
        _ => (raw, None),
    };
    if offset.is_some_and(|o| o.trim_start_matches(['+', '-']).trim_start_matches('0').is_empty()) || offset.is_none() {
        let utc = NaiveDateTime::parse_from_str(stamp, "%Y-%m-%d %H:%M:%S").or_else(|_| NaiveDateTime::parse_from_str(stamp, "%Y%m%d%H%M%S")).ok()?;
        return Some((utc + Duration::hours(12)).date());
    }
    NaiveDate::parse_from_str(stamp.get(..10)?, "%Y-%m-%d").ok()
}

// ------------------------------------------------------------------------------------------------
// From the book to the shared ledger
// ------------------------------------------------------------------------------------------------

/// LocalBooks account type for a GnuCash account type; `None` for the root and for securities.
fn account_type(kind: &str) -> Option<&'static str> {
    Some(match kind.trim().to_ascii_uppercase().as_str() {
        "BANK" | "CASH" => "BANK",
        "ASSET" => "OTHER_CURRENT_ASSET",
        "CREDIT" => "CREDIT_CARD",
        "LIABILITY" => "OTHER_CURRENT_LIABILITY",
        "EQUITY" => "EQUITY",
        "INCOME" => "INCOME",
        "EXPENSE" => "EXPENSE",
        "RECEIVABLE" => "ACCOUNTS_RECEIVABLE",
        "PAYABLE" => "ACCOUNTS_PAYABLE",
        // GnuCash shows trading accounts with equity on the balance sheet.
        "TRADING" => "EQUITY",
        _ => return None,
    })
}

/// Stock, mutual fund and the old currency-trading accounts hold securities or foreign currency
/// units, which LocalBooks does not track.
fn is_investment(kind: &str) -> bool {
    matches!(kind.trim().to_ascii_uppercase().as_str(), "STOCK" | "MUTUAL" | "CURRENCY")
}

/// What a split's account became.
#[derive(Clone, Copy)]
enum Target {
    Account(i64),
    Investment,
    Root,
}

/// A party a business object belongs to, resolved to the ledger's lists.
#[derive(Clone, Copy, PartialEq)]
enum Payee {
    Customer(i64),
    Vendor(i64),
    Employee,
}

/// One transaction's state while the ledger is built.
struct Read {
    /// Index into `book.transactions`.
    index: usize,
    /// Source transaction id, for transactions that are staged.
    id: Option<i64>,
    /// Home-currency amount of each split (debits positive, as GnuCash signs them), when it has one.
    cents: Vec<Option<i64>>,
}

/// The book with its lookups built once.
struct Context<'a> {
    book: &'a Book,
    targets: HashMap<&'a str, Target>,
    /// LocalBooks type and GnuCash name of each source account, by source id − 1.
    kinds: Vec<&'static str>,
    names: Vec<&'a str>,
    parties: Parties,
    entries: HashMap<&'a str, Vec<&'a Entry>>,
    invoices: HashMap<&'a str, &'a Invoice>,
}

impl Context<'_> {
    fn kind(&self, id: i64) -> &'static str {
        self.kinds[id as usize - 1]
    }

    fn account(&self, guid: &str) -> Option<i64> {
        match self.targets.get(guid) {
            Some(Target::Account(id)) => Some(*id),
            _ => None,
        }
    }

    fn is_control(&self, id: i64) -> bool {
        matches!(self.kind(id), "ACCOUNTS_RECEIVABLE" | "ACCOUNTS_PAYABLE")
    }
}

fn reject(quality: &mut LedgerQuality, reason: &'static str) {
    quality.rejected_lines += 1;
    *quality.rejected_by_reason.entry(reason).or_default() += 1;
}

pub fn ledger(book: &Book) -> Ledger {
    let by_guid: HashMap<&str, &Account> = book.accounts.iter().map(|a| (a.guid.as_str(), a)).collect();
    let path = |a: &Account| -> String {
        let mut names = vec![a.name.trim()];
        let mut cur = a.parent.as_deref().and_then(|p| by_guid.get(p));
        // A parent loop in a damaged file ends the path rather than the import.
        let mut seen = HashSet::from([a.guid.as_str()]);
        while let Some(p) = cur {
            if p.kind.eq_ignore_ascii_case("ROOT") || !seen.insert(p.guid.as_str()) {
                break;
            }
            names.push(p.name.trim());
            cur = p.parent.as_deref().and_then(|g| by_guid.get(g));
        }
        names.reverse();
        names.join(":")
    };

    // Accounts in path order, so parents come before their children.
    let mut ordered: Vec<(String, &Account)> = book.accounts.iter().filter(|a| !a.kind.eq_ignore_ascii_case("ROOT")).map(|a| (path(a), a)).collect();
    ordered.sort_by_key(|a| a.0.to_lowercase());
    let mut targets: HashMap<&str, Target> = book.accounts.iter().filter(|a| a.kind.eq_ignore_ascii_case("ROOT")).map(|a| (a.guid.as_str(), Target::Root)).collect();
    let mut accounts: Vec<SourceAccount> = Vec::new();
    let mut names: Vec<&str> = Vec::new();
    let mut depth: Vec<usize> = Vec::new();
    for (full_name, a) in &ordered {
        let Some(kind) = account_type(&a.kind) else {
            if is_investment(&a.kind) {
                targets.insert(a.guid.as_str(), Target::Investment);
            }
            continue;
        };
        let id = accounts.len() as i64 + 1;
        // LocalBooks subaccounts share their parent's type and nest at most five levels deep.
        let parent_id = match a.parent.as_deref().and_then(|p| targets.get(p)) {
            Some(Target::Account(p)) if accounts[*p as usize - 1].account_type == kind && depth[*p as usize - 1] < 5 => Some(*p),
            _ => None,
        };
        depth.push(parent_id.map_or(1, |p| depth[p as usize - 1] + 1));
        targets.insert(a.guid.as_str(), Target::Account(id));
        names.push(a.name.trim());
        accounts.push(SourceAccount {
            id,
            name: a.name.trim().to_string(),
            full_name: full_name.clone(),
            account_type: kind,
            parent_id,
            active: !a.hidden,
            stored_balance_cents: None,
            tax_line: None,
        });
    }

    // The home currency: the one most transactions are in.
    let mut uses: BTreeMap<&str, usize> = BTreeMap::new();
    for t in &book.transactions {
        *uses.entry(t.currency.as_str()).or_default() += 1;
    }
    let home: String = uses
        .iter()
        .max_by_key(|(c, n)| (**n, std::cmp::Reverse(**c)))
        .map(|(c, _)| c.to_string())
        .or_else(|| book.accounts.iter().find(|a| a.commodity.starts_with("CURRENCY:")).map(|a| a.commodity.clone()))
        .unwrap_or_else(|| "CURRENCY:USD".into());

    let mut entries: HashMap<&str, Vec<&Entry>> = HashMap::new();
    for e in &book.entries {
        if let Some(doc) = e.invoice.as_deref().or(e.bill.as_deref()) {
            entries.entry(doc).or_default().push(e);
        }
    }
    let cx = Context {
        book,
        targets,
        kinds: accounts.iter().map(|a| a.account_type).collect(),
        names,
        parties: Parties::new(book),
        entries,
        invoices: book.invoices.iter().map(|i| (i.guid.as_str(), i)).collect(),
    };
    let invoice_by_txn: HashMap<&str, &Invoice> = book.invoices.iter().filter_map(|i| Some((i.post_txn.as_deref()?, i))).collect();

    let mut quality = LedgerQuality::default();
    let mut unreadable: Vec<UnreadableLine> = Vec::new();
    let mut order: Vec<usize> = (0..book.transactions.len()).collect();
    order.sort_by_key(|&i| (book.transactions[i].date, i));
    let mut reads: Vec<Read> = Vec::new();
    let mut transactions: Vec<SourceTransaction> = Vec::new();
    for index in order {
        let t = &book.transactions[index];
        let label = t.description.as_deref().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
        let mut cents: Vec<Option<i64>> = Vec::new();
        let mut problem: Option<(&'static str, String)> = None;
        for s in &t.splits {
            let target = cx.targets.get(s.account.as_str()).copied();
            let amount = if t.currency == home {
                Some(s.value)
            } else if by_guid.get(s.account.as_str()).is_some_and(|a| a.commodity == home) {
                Some(s.quantity)
            } else {
                None
            };
            let c = match amount {
                Some(n) => {
                    let c = n.cents();
                    if c.is_none() {
                        problem.get_or_insert((NOT_WHOLE_CENTS, format!("{} is not a whole number of cents", n.display())));
                    }
                    c
                }
                None if s.value.is_zero() && s.quantity.is_zero() => Some(0),
                None => {
                    problem.get_or_insert((NO_HOME_AMOUNT, format!("in {} with no {} amount", currency_code(&t.currency), currency_code(&home))));
                    None
                }
            };
            match target {
                Some(Target::Investment) => problem = Some((SKIPPED_INVESTMENT, "investment transaction: securities are not tracked".into())),
                Some(Target::Account(_)) => {}
                _ if c == Some(0) => {}
                _ => {
                    problem.get_or_insert((UNKNOWN_ACCOUNT, "a split's account is not in the book".into()));
                }
            }
            cents.push(c);
        }
        if t.date.is_none() {
            problem.get_or_insert((NO_DATE, "it has no date".into()));
        }
        let (Some(date), None) = (t.date, &problem) else {
            let (reason, detail) = problem.expect("a transaction without a date has a problem");
            reject(&mut quality, reason);
            let name = Some(match &label {
                Some(l) => format!("{l} ({detail})"),
                None => format!("({detail})"),
            });
            for (s, c) in t.splits.iter().zip(&cents) {
                let Some(account_id) = cx.account(&s.account) else { continue };
                unreadable.push(UnreadableLine {
                    kind: "Transaction",
                    transaction_id: None,
                    date: t.date,
                    doc_num: t.num.clone(),
                    name: name.clone(),
                    account_id: Some(account_id),
                    cents: c.map(|c| -c),
                    outcome: LineOutcome::Unreadable,
                });
            }
            reads.push(Read { index, id: None, cents });
            continue;
        };
        let mut lines: Vec<SourceLine> = Vec::new();
        for (s, c) in t.splits.iter().zip(&cents) {
            let (Some(account_id), Some(c)) = (cx.account(&s.account), *c) else { continue };
            if c != 0 {
                lines.push(SourceLine { account_id, cents: -c, memo: s.memo.as_deref().map(str::trim).filter(|m| !m.is_empty()).map(str::to_string) });
            }
        }
        // A transaction that leaves every account where it was (a lot link, a zero-value entry)
        // posts nothing.
        let mut net: BTreeMap<i64, i64> = BTreeMap::new();
        for l in &lines {
            *net.entry(l.account_id).or_default() += l.cents;
        }
        if net.values().all(|c| *c == 0) {
            reads.push(Read { index, id: None, cents });
            continue;
        }
        let id = transactions.len() as i64 + 1;
        let notes = t.notes.as_deref().map(str::trim).filter(|n| !n.is_empty());
        let memo = match (&label, notes) {
            (Some(d), Some(n)) => Some(format!("{d} - {n}")),
            (Some(d), None) => Some(d.clone()),
            (None, n) => n.map(str::to_string),
        };
        let doc_num = t.num.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
        let mut st = SourceTransaction { document: None, id, kind: "Journal entry", date, doc_num, name: label, memo, lines };
        match invoice_by_txn.get(t.guid.as_str()) {
            Some(invoice) => invoice_document(&cx, &mut st, t, invoice),
            None => plain_document(&cx, &mut st, t),
        }
        transactions.push(st);
        reads.push(Read { index, id: Some(id), cents });
    }
    quality.unreadable_lines = unreadable.len();

    apply_lots(&cx, &reads, &mut transactions);

    let (transactions, unbalanced): (Vec<_>, Vec<_>) = transactions.into_iter().partition(|t| t.imbalance_cents() == 0);
    let (customers, vendors) = cx.parties.into_lists();
    Ledger { accounts, customers, vendors, items: Vec::new(), transactions, unbalanced, unreadable, quality, sales_lists: Default::default() }
}

/// The book's customers (with their jobs as sub-customers) and vendors, as ledger parties.
struct Parties {
    customers: Vec<SourceParty>,
    vendors: Vec<SourceParty>,
    by_guid: HashMap<String, Payee>,
}

impl Parties {
    fn new(book: &Book) -> Self {
        let mut p = Parties { customers: Vec::new(), vendors: Vec::new(), by_guid: HashMap::new() };
        let mut next = 0;
        let party = |next: &mut i64, x: &Party| {
            *next += 1;
            let addr = &x.address;
            let lines: Vec<&str> = addr.lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
            let some = |v: &Option<String>| v.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
            SourceParty {
                company_name: some(&addr.name).filter(|n| n != x.name.trim()),
                email: some(&addr.email),
                phone: some(&addr.phone),
                address: (!lines.is_empty()).then(|| lines.join("\n")),
                terms_days: x.terms.as_ref().and_then(|t| book.terms.get(t)).copied(),
                active: x.active,
                notes: some(&x.notes),
                ..blank_party(*next, x.name.trim())
            }
        };
        for c in &book.customers {
            let sp = party(&mut next, c);
            p.by_guid.insert(c.guid.clone(), Payee::Customer(sp.id));
            p.customers.push(sp);
        }
        for v in &book.vendors {
            let sp = party(&mut next, v);
            p.by_guid.insert(v.guid.clone(), Payee::Vendor(sp.id));
            p.vendors.push(sp);
        }
        for e in &book.employees {
            p.by_guid.insert(e.guid.clone(), Payee::Employee);
        }
        for j in &book.jobs {
            match j.owner.as_ref().and_then(|(_, g)| p.by_guid.get(g)).copied() {
                Some(Payee::Customer(parent)) => {
                    let parent_name = p.customers.iter().find(|c| c.id == parent).map(|c| c.name.clone()).unwrap_or_default();
                    next += 1;
                    p.customers.push(SourceParty { parent_id: Some(parent), active: j.active, ..blank_party(next, &format!("{parent_name}:{}", j.name.trim())) });
                    p.by_guid.insert(j.guid.clone(), Payee::Customer(next));
                }
                // A vendor's job only groups bills; the bills belong to the vendor.
                Some(owner) => {
                    p.by_guid.insert(j.guid.clone(), owner);
                }
                None => {}
            }
        }
        p
    }

    fn get(&self, owner: Option<&Owner>) -> Option<Payee> {
        owner.and_then(|(_, g)| self.by_guid.get(g)).copied()
    }

    fn name(&self, payee: Payee) -> Option<String> {
        let list = match payee {
            Payee::Customer(id) => self.customers.iter().find(|c| c.id == id),
            Payee::Vendor(id) => self.vendors.iter().find(|c| c.id == id),
            Payee::Employee => None,
        };
        list.map(|p| p.name.clone())
    }

    fn into_lists(mut self) -> (Vec<SourceParty>, Vec<SourceParty>) {
        self.customers.sort_by_key(|p| p.name.to_lowercase());
        self.vendors.sort_by_key(|p| p.name.to_lowercase());
        (self.customers, self.vendors)
    }
}

/// A form line for a posted split, one unit at its amount.
fn split_line(l: &SourceLine) -> DocumentLine {
    DocumentLine { item_id: None, description: l.memo.clone(), quantity_e4: 10_000, rate_cents: l.cents.abs(), cents: l.cents, account_id: l.account_id }
}

/// The form lines of an invoice or bill: its entries when they add up, per account, to what the
/// posting transaction posted, otherwise one line per posted split. Lines to receivables or
/// payables are left out; tax lines stay (the full import reads them as tax).
fn form_lines(cx: &Context, st: &SourceTransaction, invoice: &Invoice) -> Vec<DocumentLine> {
    let posted: Vec<&SourceLine> = st.lines.iter().filter(|l| !cx.is_control(l.account_id)).collect();
    let from_splits = || posted.iter().map(|l| split_line(l)).collect::<Vec<_>>();
    let mut by_account: BTreeMap<i64, i64> = BTreeMap::new();
    for l in &posted {
        *by_account.entry(l.account_id).or_default() += l.cents;
    }
    let mut lines: Vec<DocumentLine> = Vec::new();
    let mut covered: BTreeMap<i64, i64> = BTreeMap::new();
    for e in cx.entries.get(invoice.guid.as_str()).into_iter().flatten() {
        let on_invoice = e.invoice.as_deref() == Some(invoice.guid.as_str());
        let (account, price) = if on_invoice { (e.i_account.as_deref(), e.i_price) } else { (e.b_account.as_deref(), e.b_price) };
        let discount = if on_invoice && !e.i_discount.is_zero() { Some(e.i_discount) } else { None };
        let Some(account_id) = account.and_then(|a| cx.account(a)) else { return from_splits() };
        let Some(&posted_total) = by_account.get(&account_id) else { return from_splits() };
        // quantity × price, less any discount, in cents.
        let (q, p) = (e.quantity, price);
        let (mut num, mut denom) = (q.num as i128 * p.num as i128, q.denom as i128 * p.denom as i128);
        if let Some(d) = discount {
            if e.i_discount_type.as_deref().is_some_and(|k| k.eq_ignore_ascii_case("PERCENT")) {
                num *= 100 * d.denom as i128 - d.num as i128;
                denom *= 100 * d.denom as i128;
            } else {
                num = num * d.denom as i128 - d.num as i128 * denom;
                denom *= d.denom as i128;
            }
        }
        let Some(amount) = Num::rounded_cents(num, denom) else { return from_splits() };
        if amount == 0 {
            continue;
        }
        // Lines carry the posting's sign: credits on an invoice, debits on a bill.
        let cents = amount.abs() * posted_total.signum();
        *covered.entry(account_id).or_default() += cents;
        let (quantity_e4, rate_cents) = match (q.scaled(10_000).map(i64::abs).filter(|q| *q > 0), p.cents().map(i64::abs), discount) {
            (Some(q), Some(r), None) => (q, r),
            _ => (10_000, amount.abs()),
        };
        let description = e.description.as_deref().map(str::trim).filter(|d| !d.is_empty()).map(str::to_string);
        lines.push(DocumentLine { item_id: None, description, quantity_e4, rate_cents, cents, account_id });
    }
    if lines.is_empty() || covered.iter().any(|(a, c)| by_account.get(a) != Some(c)) {
        return from_splits();
    }
    // Accounts no entry names directly (sales tax) keep their posted lines.
    lines.extend(posted.iter().filter(|l| !covered.contains_key(&l.account_id)).map(|l| split_line(l)));
    lines
}

/// An invoice's, bill's, credit note's or expense voucher's posting transaction.
fn invoice_document(cx: &Context, st: &mut SourceTransaction, t: &Transaction, invoice: &Invoice) {
    let number = invoice.id.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_string);
    if number.is_some() {
        st.doc_num = number.clone();
    }
    let due_date = t.due.or_else(|| Some(st.date + Duration::days(*cx.book.terms.get(invoice.terms.as_ref()?)?)));
    let lines = form_lines(cx, st, invoice);
    let payee = cx.parties.get(invoice.owner.as_ref());
    match payee {
        Some(Payee::Customer(customer_id)) if invoice.credit_note => {
            st.kind = "Credit memo";
            st.document = Some(SourceDocument::CreditMemo { customer_id, number, lines, applied_to: Vec::new() });
        }
        Some(Payee::Customer(customer_id)) => {
            st.kind = "Invoice";
            st.document = Some(SourceDocument::Invoice { customer_id, number, due_date, lines, sales: SourceSales::default() });
        }
        // A vendor credit note is a bill that debits payables; the full import tells them apart.
        Some(Payee::Vendor(vendor_id)) => {
            st.kind = if invoice.credit_note { "Vendor credit" } else { "Bill" };
            st.document = Some(SourceDocument::Bill { vendor_id, number, due_date, lines });
        }
        // Expense vouchers post as journal entries: LocalBooks has no employee expense claims.
        Some(Payee::Employee) => st.kind = "Expense voucher",
        None => {
            st.kind = match invoice.owner.as_ref().map(|o| o.0) {
                Some(OwnerKind::Vendor) => "Bill",
                Some(OwnerKind::Employee) => "Expense voucher",
                _ => "Invoice",
            }
        }
    }
    if let Some(name) = payee.and_then(|p| cx.parties.name(p)) {
        st.name = Some(name);
    }
}

/// Any other transaction: a payment when it settles a receivable or payable lot, otherwise what
/// its bank or card line says it is, as the QIF reader names register entries.
fn plain_document(cx: &Context, st: &mut SourceTransaction, t: &Transaction) {
    let kinds: Vec<&str> = st.lines.iter().map(|l| cx.kind(l.account_id)).collect();
    let has = |k: &str| kinds.contains(&k);

    // A payment's receivable or payable split is in a lot that belongs to a customer or vendor:
    // a lot of its own, or the invoice's.
    let lot_payee = t.splits.iter().find_map(|s| {
        if !cx.is_control(cx.account(&s.account)?) {
            return None;
        }
        let lot = cx.book.lots.get(s.lot.as_deref()?)?;
        let owner = lot.owner.as_ref().or_else(|| cx.invoices.get(lot.invoice.as_deref()?)?.owner.as_ref());
        cx.parties.get(owner)
    });
    let is_payment = t.txn_type.as_deref() == Some("P") || lot_payee.is_some();
    if is_payment && has("ACCOUNTS_RECEIVABLE") {
        st.kind = "Payment";
        if let Some(payee @ Payee::Customer(customer_id)) = lot_payee {
            st.name = cx.parties.name(payee).or(st.name.take());
            st.document = Some(SourceDocument::Payment { customer_id, reference: st.doc_num.clone(), method: payment_method(None).to_string(), applied_to: Vec::new() });
        }
        return;
    }
    if is_payment && has("ACCOUNTS_PAYABLE") {
        st.kind = "Bill payment";
        let vendor_id = match lot_payee {
            Some(payee @ Payee::Vendor(v)) => {
                st.name = cx.parties.name(payee).or(st.name.take());
                Some(v)
            }
            _ => None,
        };
        st.document = Some(SourceDocument::BillPayment { vendor_id, reference: st.doc_num.clone(), applied_to: Vec::new() });
        return;
    }

    if st.lines.iter().any(|l| cx.kind(l.account_id) == "EQUITY" && cx.names[l.account_id as usize - 1].to_lowercase().starts_with("opening balance")) {
        st.kind = "Opening balance";
        return;
    }
    let balance_sheet = |k: &str| matches!(k, "BANK" | "CREDIT_CARD" | "OTHER_CURRENT_ASSET" | "OTHER_CURRENT_LIABILITY" | "LONG_TERM_LIABILITY");
    if st.lines.len() == 2 && kinds.iter().all(|k| balance_sheet(k)) && kinds.iter().any(|k| matches!(*k, "BANK" | "CREDIT_CARD")) {
        st.kind = "Transfer";
        st.document = Some(SourceDocument::Transfer);
        return;
    }
    let banks: Vec<&SourceLine> = st.lines.iter().filter(|l| cx.kind(l.account_id) == "BANK").collect();
    let cards: Vec<&SourceLine> = st.lines.iter().filter(|l| cx.kind(l.account_id) == "CREDIT_CARD").collect();
    let check_number = st.doc_num.clone().filter(|n| n.parse::<u64>().is_ok());
    // Ledger signs: a credit (money out of the bank, a charge on the card) is positive.
    match (banks.as_slice(), cards.as_slice()) {
        ([bank], []) if bank.cents > 0 => {
            st.kind = if check_number.is_some() { "Check" } else { "Withdrawal" };
            st.document = Some(SourceDocument::Expense { vendor_id: None, number: check_number, applied_to: Vec::new() });
        }
        ([_], []) => {
            st.kind = "Deposit";
            st.document = Some(SourceDocument::Deposit { payments: Vec::new() });
        }
        ([], [card]) if card.cents > 0 => {
            st.kind = "Credit card charge";
            st.document = Some(SourceDocument::Expense { vendor_id: None, number: None, applied_to: Vec::new() });
        }
        ([], [_]) => st.kind = "Credit card credit",
        _ => {}
    }
}

/// Says which invoice or bill each payment paid and which invoice each credit note was applied
/// to, by following the book's lots. A split in an invoice's lot from a payment pays that invoice;
/// one from a link transaction carries the payment (or credit note) whose lot the link's other
/// split is in.
fn apply_lots(cx: &Context, reads: &[Read], transactions: &mut [SourceTransaction]) {
    let book = cx.book;
    // Every split with a lot and a home amount: lot → (read, amount, debits positive).
    let mut lots: HashMap<&str, Vec<(usize, i64)>> = HashMap::new();
    for (r, read) in reads.iter().enumerate() {
        for (s, c) in book.transactions[read.index].splits.iter().zip(&read.cents) {
            if let (Some(lot), Some(c)) = (s.lot.as_deref(), *c) {
                if c != 0 {
                    lots.entry(lot).or_default().push((r, c));
                }
            }
        }
    }
    let position: HashMap<i64, usize> = transactions.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
    let read_of_txn: HashMap<&str, usize> = reads.iter().enumerate().map(|(r, read)| (book.transactions[read.index].guid.as_str(), r)).collect();

    // Invoices and bills (not credit notes), oldest first, with their lots.
    let mut charges: Vec<(&str, usize, i64)> = Vec::new();
    for invoice in &book.invoices {
        let (Some(lot), Some(&r)) = (invoice.post_lot.as_deref(), invoice.post_txn.as_deref().and_then(|t| read_of_txn.get(t))) else { continue };
        let Some(id) = reads[r].id else { continue };
        if matches!(transactions[position[&id]].document, Some(SourceDocument::Invoice { .. } | SourceDocument::Bill { .. })) && !invoice.credit_note {
            charges.push((lot, r, id));
        }
    }
    charges.sort_by_key(|(_, r, _)| (book.transactions[reads[*r].index].date, *r));

    // What each payment or credit note still has to apply from each lot it is in.
    let mut left: HashMap<(usize, &str), i64> = HashMap::new();
    for (lot, charge_read, charge_id) in charges {
        let Some(splits) = lots.get(lot) else { continue };
        let Some(&(_, charge)) = splits.iter().find(|(r, _)| *r == charge_read) else { continue };
        let mut payers: Vec<(usize, i64)> = Vec::new();
        for &(r, c) in splits.iter().filter(|(r, c)| *r != charge_read && c.signum() != charge.signum()) {
            if reads[r].id.is_some() {
                payers.push((r, c.abs()));
                continue;
            }
            // A link: whatever it moved here came from the lots its other splits are in.
            let link = &book.transactions[reads[r].index];
            let mut amount = c.abs();
            for (s, sc) in link.splits.iter().zip(&reads[r].cents) {
                let (Some(other), Some(sc)) = (s.lot.as_deref(), *sc) else { continue };
                if other == lot || sc.signum() != charge.signum() {
                    continue;
                }
                let mut from_split = sc.abs();
                for &(p, pc) in lots.get(other).into_iter().flatten() {
                    if amount == 0 || from_split == 0 {
                        break;
                    }
                    if reads[p].id.is_none() || pc.signum() == charge.signum() {
                        continue;
                    }
                    let remaining = left.entry((p, other)).or_insert(pc.abs());
                    let take = (*remaining).min(amount).min(from_split);
                    if take > 0 {
                        *remaining -= take;
                        amount -= take;
                        from_split -= take;
                        payers.push((p, take));
                    }
                }
            }
        }
        for (p, cents) in payers {
            let Some(id) = reads[p].id else { continue };
            if let Some(
                SourceDocument::Payment { applied_to, .. } | SourceDocument::BillPayment { applied_to, .. } | SourceDocument::CreditMemo { applied_to, .. },
            ) = &mut transactions[position[&id]].document
            {
                match applied_to.iter_mut().find(|a| a.transaction_id == charge_id) {
                    Some(a) => a.cents += cents,
                    None => applied_to.push(Applied { transaction_id: charge_id, cents }),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
