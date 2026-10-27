//! Reads QuickBooks for Mac company files from about 2010 to 2012 (`.qb2010`, `.qb2011`,
//! `.qb2012`) into a [`Ledger`].
//!
//! Those files are plain Apple Core Data SQLite stores: the file starts with `SQLite format 3` and
//! is not encrypted, so it is opened read-only like any SQLite database. Later QuickBooks for Mac
//! files start with `QBMac` and are encrypted; they are refused with the steps to save a copy for
//! QuickBooks for Windows instead, and no attempt is made to decrypt them. Everything below was
//! worked out from the two sample companies in the QuickBooks for Mac 2012 installer and its
//! `QuickBooksDataModel.momd` entity names; no Intuit code or data is used.
//!
//! The tables mirror QuickBooks for Windows, with Core Data's `Z` prefix. A `...MO` column is a
//! relationship holding the other row's `Z_PK`.
//!
//! - `ZACCOUNTS`: `ZTYPE` uses the Windows numbering (0 bank … 14 other expense; 15 is the
//!   non-posting estimates and orders accounts, which are left out), `ZPARENTMO` the parent and
//!   `ZENDINGBAL` the stored balance (debits positive).
//! - `ZNAMES`: `ZTYPE` 0 customer (jobs have a parent customer), 1 vendor, 2 employee, 3 other.
//!   `ZTERMS` is the `ZRECNUM` of a `ZPAYMENTTERMS` row.
//! - `ZITEMS`: `ZTYPE` 0 service, 1 inventory part, 3 non-inventory part, 5 other charge; the
//!   others (subtotals, groups, discounts, payments, sales tax) are not things LocalBooks sells.
//! - `ZMASTERS` holds a transaction's header (date, ship date and Ship Via) and `ZTARGS` its lines:
//!   account, name, item, amount (debits positive), memo, quantity and price. `ZVIEW` on every line
//!   is the transaction type ([`kind`]), and the line with `ZISSOURCE` set carries the header's
//!   name, number, memo and due date. Subtotal lines have no account and post nothing, nor do lines
//!   with `ZNOPOST` (estimates and orders). An inventory item's line keeps its cost in
//!   `ZCOGSAMOUNT`, which also posts: to the item's cost of goods sold account, and the other way
//!   to its inventory asset account.
//! - `ZLINKS` holds payment applications in sibling pairs, as Windows' `abmc_transaction_link`
//!   does: the row with `ZTONEISSOURCE` set links a line of the paying transaction (`ZTONEMO`) to a
//!   line of what it paid (`ZTTWOMO`), with the amount, negative. `ZTYPE` 4 is a payment applied
//!   to an invoice, 6 a bill payment or check applied to a bill and 9 a payment deposited.
//! - Amounts are decimal text or SQLite numbers (money `12.34$`, quantities `2#`, rates `7.5%`)
//!   and are read exactly; one that is not a whole number of cents is not rounded. Dates are
//!   seconds since 2001-01-01, with year 1 for none.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{Datelike, Duration, NaiveDate};
use sqlx::sqlite::{SqliteConnectOptions, SqliteRow};
use sqlx::{ConnectOptions, Connection, Row, SqliteConnection};

use crate::qbw_company::{Ledger, LedgerQuality, LineOutcome, SourceAccount, SourceItem, SourceLine, SourceParty, SourceSalesLists, SourceTransaction, UnreadableLine};
use crate::qbw_documents::{payment_method, Applied, DocumentLine, SourceDocument, SourceSales};
use crate::text_import::blank_party;

const NOT_WHOLE_CENTS: &str = "amounts that are not whole cents";
const UNKNOWN_ACCOUNT: &str = "lines whose account is not in the file";
const NO_DATE: &str = "transactions without a date";
const DATE_OUT_OF_RANGE: &str = "date out of range";

/// The tables a QuickBooks for Mac company file has.
const REQUIRED_TABLES: [&str; 3] = ["ZMASTERS", "ZTARGS", "ZACCOUNTS"];

/// LocalBooks account type codes, indexed by `ZACCOUNTS.ZTYPE` (the Windows numbering).
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

pub const ENCRYPTED: &str = "QBMAC_ENCRYPTED: this QuickBooks for Mac company file is encrypted, so LocalBooks can't read it. \
In QuickBooks for Mac, choose File › Export › To QuickBooks for Windows, leave the password fields blank, and choose the .qbb file it saves here instead";

/// Whether a file extension is a QuickBooks for Mac company file's: `qb` and a year (`qb2012`).
pub fn is_mac_extension(ext: &str) -> bool {
    let ext = ext.to_ascii_lowercase();
    ext.len() == 6 && ext.starts_with("qb") && ext[2..].bytes().all(|b| b.is_ascii_digit())
}

/// Whether a path is a QuickBooks for Mac company file or a file inside one saved as a bundle.
pub fn is_mac_path(path: &Path) -> bool {
    path.ancestors().any(|p| p.extension().is_some_and(|e| is_mac_extension(&e.to_string_lossy())))
}

/// What a file is, from its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Sqlite,
    /// The encrypted container of QuickBooks for Mac 2016 and later.
    Encrypted,
}

pub fn sniff(head: &[u8]) -> Option<Format> {
    if head.starts_with(b"SQLite format 3\0") {
        Some(Format::Sqlite)
    } else if head.starts_with(b"QBMac") {
        Some(Format::Encrypted)
    } else {
        None
    }
}

fn head(path: &Path) -> Option<Format> {
    use std::io::Read;
    let mut head = [0u8; 16];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).ok()?;
    sniff(&head[..n])
}

/// The company file to read: the file itself, or for a bundle (a folder) the store inside it.
fn locate(path: &Path) -> Result<(PathBuf, Option<Format>), String> {
    if !path.is_dir() {
        return Ok((path.to_path_buf(), head(path)));
    }
    let mut found: Vec<(Format, PathBuf)> = Vec::new();
    let mut dirs = vec![(path.to_path_buf(), 0)];
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                if depth < 3 {
                    dirs.push((p, depth + 1));
                }
            } else if let Some(format) = head(&p) {
                found.push((format, p));
            }
        }
    }
    // A store that is not encrypted wins over an encrypted one; then the first by path.
    found.sort_by_key(|(f, p)| (*f != Format::Sqlite, p.clone()));
    found.into_iter().next().map(|(f, p)| (p, Some(f))).ok_or_else(|| format!("QBMAC_NOT_COMPANY: no QuickBooks company file was found inside {}", path.display()))
}

/// Reads a QuickBooks for Mac company file, or refuses one this reader cannot open.
pub async fn read(path: &Path) -> Result<Ledger, String> {
    let (file, format) = locate(path)?;
    match format {
        Some(Format::Sqlite) => {}
        Some(Format::Encrypted) => return Err(ENCRYPTED.into()),
        None => {
            return Err("QBMAC_OLD_FORMAT: this isn't a company file LocalBooks can read. QuickBooks for Mac 2009 and earlier use an older format: \
open the file in QuickBooks and choose File › Export › To QuickBooks for Windows, then choose the .qbb file it saves"
                .into())
        }
    }
    let options = SqliteConnectOptions::new().filename(&file).read_only(true).immutable(true).disable_statement_logging();
    let mut conn = SqliteConnection::connect_with(&options).await.map_err(|e| format!("QBMAC_UNREADABLE: {}: {e}", file.display()))?;
    let book = read_connection(&mut conn).await;
    let _ = conn.close().await;
    let ledger = ledger(&book?);
    if ledger.accounts.is_empty() && ledger.transactions.is_empty() && ledger.unbalanced.is_empty() {
        return Err("QBMAC_EMPTY: this company file has no accounts or transactions".into());
    }
    Ok(ledger)
}

// ------------------------------------------------------------------------------------------------
// The file's rows, as text
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct Account {
    pub pk: i64,
    pub kind: Option<i64>,
    pub name: String,
    pub parent: Option<i64>,
    pub hidden: bool,
    pub ending_balance: Option<String>,
    pub tax_line: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct Name {
    pub pk: i64,
    /// 0 customer, 1 vendor, 2 employee, 3 other.
    pub kind: Option<i64>,
    pub name: String,
    pub parent: Option<i64>,
    pub hidden: bool,
    pub company: Option<String>,
    pub first: Option<String>,
    pub last: Option<String>,
    pub phone: Option<String>,
    pub email: Option<String>,
    pub address: Vec<String>,
    /// `ZRECNUM` of the payment terms.
    pub terms: Option<i64>,
    pub tax_id: Option<String>,
    pub is_1099: bool,
    pub ending_balance: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Item {
    pub pk: i64,
    pub kind: Option<i64>,
    pub name: String,
    pub parent: Option<i64>,
    pub hidden: bool,
    pub description: Option<String>,
    pub price: Option<String>,
    pub taxable: bool,
    pub income_account: Option<i64>,
    pub cogs_account: Option<i64>,
    pub asset_account: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct Master {
    pub pk: i64,
    pub date: Option<NaiveDate>,
    pub ship_date: Option<NaiveDate>,
    pub ship_via: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct Targ {
    pub pk: i64,
    pub master: Option<i64>,
    pub recnum: Option<i64>,
    pub view: Option<i64>,
    pub account: Option<i64>,
    pub name: Option<i64>,
    pub item: Option<i64>,
    pub amount: Option<String>,
    pub cogs: Option<String>,
    pub quantity: Option<String>,
    pub unit_price: Option<String>,
    pub is_source: bool,
    pub no_post: bool,
    pub memo: Option<String>,
    pub document: Option<String>,
    pub date: Option<NaiveDate>,
    pub due: Option<NaiveDate>,
    pub pay_method: Option<i64>,
    pub received: bool,
}

#[derive(Debug, Clone, Default)]
pub struct Link {
    pub kind: Option<i64>,
    pub one: Option<i64>,
    pub two: Option<i64>,
    pub one_is_source: bool,
    pub amount: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Terms {
    pub recnum: i64,
    /// 0 standard (due a number of days after the date), 1 due on a day of the month.
    pub kind: Option<i64>,
    pub due_days: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct Book {
    pub accounts: Vec<Account>,
    pub names: Vec<Name>,
    pub items: Vec<Item>,
    pub masters: Vec<Master>,
    pub targs: Vec<Targ>,
    pub links: Vec<Link>,
    pub terms: Vec<Terms>,
    pub pay_methods: HashMap<i64, String>,
    /// Ship Via names by `Z_PK`, with whether each is in use.
    pub ship_types: HashMap<i64, (String, bool)>,
}

/// Seconds since 2001-01-01, Core Data's reference date. A day is taken 12 hours on, so a midnight
/// stored in any time zone from UTC-12 to UTC+12 gives the date it was entered on; year 1 (and
/// anything before 1900) means no date.
pub fn core_data_date(raw: &str) -> Option<NaiveDate> {
    let secs: f64 = raw.trim().parse().ok().filter(|s: &f64| s.is_finite())?;
    // Whole seconds are enough: flooring them first lands on the same day.
    let days = (secs.floor() as i64).checked_add(43_200)?.div_euclid(86_400);
    let date = NaiveDate::from_ymd_opt(2001, 1, 1)?.checked_add_signed(Duration::try_days(days)?)?;
    (date.year() >= 1900).then_some(date)
}

/// A decimal as the file writes it (`-40203.88`, `72.00$`, `15435`), times `scale`, when that is
/// a whole number. `Ok(None)` for no value (empty, `NaN`); `Err` when it has more places than
/// `scale` keeps or is not a number.
pub fn scaled(raw: &str, places: u32) -> Result<Option<i64>, ()> {
    let s = raw.trim().trim_end_matches(['$', '#']).trim();
    if s.is_empty() || s.eq_ignore_ascii_case("nan") {
        return Ok(None);
    }
    let (negative, digits) = match s.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (whole, frac) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty() && frac.is_empty() || !whole.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
        return Err(());
    }
    let kept = frac.get(..frac.len().min(places as usize)).unwrap_or("");
    if frac[kept.len()..].bytes().any(|b| b != b'0') {
        return Err(());
    }
    let mut value: i128 = 0;
    for b in whole.bytes().chain(kept.bytes()).chain(std::iter::repeat_n(b'0', places as usize - kept.len())) {
        value = value.checked_mul(10).and_then(|v| v.checked_add((b - b'0') as i128)).ok_or(())?;
    }
    let value = i64::try_from(value).map_err(|_| ())?;
    Ok(Some(if negative { -value } else { value }))
}

/// An amount in cents; see [`scaled`].
pub fn cents(raw: Option<&str>) -> Result<Option<i64>, ()> {
    raw.map_or(Ok(None), |r| scaled(r, 2))
}

/// The columns a table has, so a file whose model lacks one still reads (it reads as empty).
async fn columns(conn: &mut SqliteConnection, table: &str) -> Result<HashSet<String>, String> {
    let rows = sqlx::query(&format!("PRAGMA table_info({table})")).fetch_all(&mut *conn).await.map_err(|e| format!("QBMAC_PARSE_FAILED: table {table}: {e}"))?;
    Ok(rows.iter().filter_map(|r| r.try_get::<String, _>("name").ok()).map(|c| c.to_ascii_uppercase()).collect())
}

/// Every row of a table with the wanted columns as text (numbers in SQLite's own exact form);
/// a missing table reads as no rows.
async fn rows(conn: &mut SqliteConnection, table: &str, wanted: &[&str]) -> Result<Vec<SqliteRow>, String> {
    let have = columns(conn, table).await?;
    if have.is_empty() {
        return Ok(Vec::new());
    }
    let list: Vec<String> = wanted.iter().map(|c| if have.contains(*c) { format!("CAST({c} AS TEXT) AS {c}") } else { format!("NULL AS {c}") }).collect();
    sqlx::query(&format!("SELECT {} FROM {table}", list.join(", "))).fetch_all(&mut *conn).await.map_err(|e| format!("QBMAC_PARSE_FAILED: table {table}: {e}"))
}

fn text(row: &SqliteRow, column: &str) -> Option<String> {
    row.try_get::<Option<String>, _>(column).ok().flatten().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn int(row: &SqliteRow, column: &str) -> Option<i64> {
    let t = text(row, column)?;
    t.parse::<i64>().ok().or_else(|| t.parse::<f64>().ok().filter(|f| f.fract() == 0.0 && f.is_finite()).map(|f| f as i64))
}

fn flag(row: &SqliteRow, column: &str) -> bool {
    int(row, column).is_some_and(|v| v != 0)
}

fn date(row: &SqliteRow, column: &str) -> Option<NaiveDate> {
    core_data_date(&text(row, column)?)
}

/// Reads the tables the ledger needs.
pub async fn read_connection(conn: &mut SqliteConnection) -> Result<Book, String> {
    for table in REQUIRED_TABLES {
        if columns(conn, table).await?.is_empty() {
            return Err(format!("QBMAC_NOT_COMPANY: this SQLite file isn't a QuickBooks for Mac company file (it has no {table} table)"));
        }
    }
    let mut book = Book::default();
    let deleted = |r: &SqliteRow| flag(r, "ZQBDELETED");
    for r in rows(conn, "ZACCOUNTS", &["Z_PK", "ZTYPE", "ZNAME", "ZPARENTMO", "ZHIDDEN", "ZQBDELETED", "ZENDINGBAL", "ZTAXLINEID"]).await? {
        let (Some(pk), Some(name)) = (int(&r, "Z_PK"), text(&r, "ZNAME")) else { continue };
        book.accounts.push(Account {
            pk,
            kind: int(&r, "ZTYPE"),
            name,
            parent: int(&r, "ZPARENTMO"),
            hidden: flag(&r, "ZHIDDEN") || deleted(&r),
            ending_balance: text(&r, "ZENDINGBAL"),
            tax_line: int(&r, "ZTAXLINEID").filter(|n| *n > 0),
        });
    }
    let name_columns = [
        "Z_PK", "ZTYPE", "ZNAME", "ZPARENTMO", "ZHIDDEN", "ZQBDELETED", "ZCOMPANYNAME", "ZFIRST", "ZLAST", "ZTEL1", "ZEMAIL", "ZADDR0", "ZADDR1", "ZADDR2", "ZADDR3",
        "ZADDR4", "ZTERMS", "ZTAXID", "ZIS1099VENDOR", "ZENDINGBAL", "ZNOTE",
    ];
    for r in rows(conn, "ZNAMES", &name_columns).await? {
        let (Some(pk), Some(name)) = (int(&r, "Z_PK"), text(&r, "ZNAME")) else { continue };
        book.names.push(Name {
            pk,
            kind: int(&r, "ZTYPE"),
            name,
            parent: int(&r, "ZPARENTMO"),
            hidden: flag(&r, "ZHIDDEN") || deleted(&r),
            company: text(&r, "ZCOMPANYNAME"),
            first: text(&r, "ZFIRST"),
            last: text(&r, "ZLAST"),
            phone: text(&r, "ZTEL1"),
            email: text(&r, "ZEMAIL"),
            address: (0..5).filter_map(|i| text(&r, &format!("ZADDR{i}"))).collect(),
            terms: int(&r, "ZTERMS").filter(|t| *t > 0),
            tax_id: text(&r, "ZTAXID"),
            is_1099: flag(&r, "ZIS1099VENDOR"),
            ending_balance: text(&r, "ZENDINGBAL"),
            note: text(&r, "ZNOTE"),
        });
    }
    let item_columns = ["Z_PK", "ZTYPE", "ZNAME", "ZPARENTMO", "ZHIDDEN", "ZQBDELETED", "ZDESC", "ZSALESPRICE", "ZTAXABLE", "ZPOSTACCNTMO", "ZCOGSACCNTMO", "ZINVACCNTMO"];
    for r in rows(conn, "ZITEMS", &item_columns).await? {
        let (Some(pk), Some(name)) = (int(&r, "Z_PK"), text(&r, "ZNAME")) else { continue };
        book.items.push(Item {
            pk,
            kind: int(&r, "ZTYPE"),
            name,
            parent: int(&r, "ZPARENTMO"),
            hidden: flag(&r, "ZHIDDEN") || deleted(&r),
            description: text(&r, "ZDESC"),
            price: text(&r, "ZSALESPRICE"),
            taxable: flag(&r, "ZTAXABLE"),
            income_account: int(&r, "ZPOSTACCNTMO"),
            cogs_account: int(&r, "ZCOGSACCNTMO"),
            asset_account: int(&r, "ZINVACCNTMO"),
        });
    }
    for r in rows(conn, "ZMASTERS", &["Z_PK", "ZDATE", "ZSHIPDATE", "ZVIAMO"]).await? {
        let Some(pk) = int(&r, "Z_PK") else { continue };
        book.masters.push(Master { pk, date: date(&r, "ZDATE"), ship_date: date(&r, "ZSHIPDATE"), ship_via: int(&r, "ZVIAMO") });
    }
    let targ_columns = [
        "Z_PK", "ZMASTERMO", "ZRECNUM", "ZVIEW", "ZACCNTMO", "ZNAMEMO", "ZITEMMO", "ZAMOUNT", "ZCOGSAMOUNT", "ZQUANTITY", "ZUNITPRICE", "ZISSOURCE", "ZNOPOST", "ZMEMO",
        "ZDOCUMENT", "ZDATE", "ZDUEDATE", "ZPAYMETHMO", "ZRECEIVED",
    ];
    for r in rows(conn, "ZTARGS", &targ_columns).await? {
        let Some(pk) = int(&r, "Z_PK") else { continue };
        book.targs.push(Targ {
            pk,
            master: int(&r, "ZMASTERMO"),
            recnum: int(&r, "ZRECNUM"),
            view: int(&r, "ZVIEW"),
            account: int(&r, "ZACCNTMO"),
            name: int(&r, "ZNAMEMO"),
            item: int(&r, "ZITEMMO"),
            amount: text(&r, "ZAMOUNT"),
            cogs: text(&r, "ZCOGSAMOUNT"),
            quantity: text(&r, "ZQUANTITY"),
            unit_price: text(&r, "ZUNITPRICE"),
            is_source: flag(&r, "ZISSOURCE"),
            no_post: flag(&r, "ZNOPOST"),
            memo: text(&r, "ZMEMO"),
            document: text(&r, "ZDOCUMENT"),
            date: date(&r, "ZDATE"),
            due: date(&r, "ZDUEDATE"),
            pay_method: int(&r, "ZPAYMETHMO"),
            received: flag(&r, "ZRECEIVED"),
        });
    }
    for r in rows(conn, "ZLINKS", &["ZTYPE", "ZTONEMO", "ZTTWOMO", "ZTONEISSOURCE", "ZAMOUNT"]).await? {
        book.links.push(Link { kind: int(&r, "ZTYPE"), one: int(&r, "ZTONEMO"), two: int(&r, "ZTTWOMO"), one_is_source: flag(&r, "ZTONEISSOURCE"), amount: text(&r, "ZAMOUNT") });
    }
    for r in rows(conn, "ZPAYMENTTERMS", &["ZRECNUM", "ZTERMSTYPE", "ZDUEDAYS", "ZQBDELETED"]).await? {
        let Some(recnum) = int(&r, "ZRECNUM") else { continue };
        if !deleted(&r) {
            book.terms.push(Terms { recnum, kind: int(&r, "ZTERMSTYPE"), due_days: int(&r, "ZDUEDAYS") });
        }
    }
    for r in rows(conn, "ZPAYMETHODS", &["Z_PK", "ZNAME"]).await? {
        if let (Some(pk), Some(name)) = (int(&r, "Z_PK"), text(&r, "ZNAME")) {
            book.pay_methods.insert(pk, name);
        }
    }
    for r in rows(conn, "ZSHIPTYPES", &["Z_PK", "ZNAME", "ZHIDDEN", "ZQBDELETED"]).await? {
        if let (Some(pk), Some(name)) = (int(&r, "Z_PK"), text(&r, "ZNAME")) {
            if !deleted(&r) {
                book.ship_types.insert(pk, (name, !flag(&r, "ZHIDDEN")));
            }
        }
    }
    Ok(book)
}

// ------------------------------------------------------------------------------------------------
// From the file to the shared ledger
// ------------------------------------------------------------------------------------------------

/// The transaction type for a line's `ZVIEW`.
pub fn kind(view: Option<i64>) -> &'static str {
    match view {
        Some(0) => "Estimate",
        Some(1) => "Credit card charge",
        Some(2) => "Deposit",
        Some(3) => "Check",
        Some(4) => "Invoice",
        Some(5) => "Sales receipt",
        Some(6) => "Credit memo",
        Some(7) => "Payment",
        Some(8) => "Journal entry",
        Some(9) => "Bill",
        Some(12) => "Vendor credit",
        Some(14) => "Bill payment",
        Some(16) => "Sales tax payment",
        Some(17) => "Purchase order",
        Some(18) => "Inventory adjustment",
        Some(19) => "Item receipt",
        Some(20) => "Paycheck",
        Some(21) => "Liability check",
        Some(24) => "Sales order",
        Some(26) => "Transfer",
        _ => "Transaction",
    }
}

/// Link types (`ZLINKS.ZTYPE`) the full import uses.
const LINK_PAYMENT: i64 = 4;
const LINK_BILL_PAYMENT: i64 = 6;
const LINK_DEPOSIT: i64 = 9;

fn reject(quality: &mut LedgerQuality, reason: &'static str) {
    quality.rejected_lines += 1;
    *quality.rejected_by_reason.entry(reason).or_default() += 1;
}

/// `Parent:Child` paths for a list, following `parent` (cycles and missing parents cut).
fn paths<'a>(rows: impl Iterator<Item = (i64, &'a str, Option<i64>)>) -> HashMap<i64, String> {
    let by: HashMap<i64, (&str, Option<i64>)> = rows.map(|(pk, name, parent)| (pk, (name, parent))).collect();
    by.keys()
        .map(|&pk| {
            let mut parts = Vec::new();
            let mut seen = HashSet::new();
            let mut cur = Some(pk);
            while let Some(c) = cur.filter(|c| seen.insert(*c)) {
                let Some((name, parent)) = by.get(&c) else { break };
                parts.push(name.trim().replace(':', "-"));
                cur = *parent;
            }
            parts.reverse();
            (pk, parts.join(":"))
        })
        .collect()
}

fn accounts(book: &Book) -> Vec<SourceAccount> {
    let typed: HashMap<i64, (&Account, &'static str)> =
        book.accounts.iter().filter_map(|a| Some((a.pk, (a, *ACCOUNT_TYPES.get(usize::try_from(a.kind?).ok()?)?)))).collect();
    // A subaccount shares its parent's type in LocalBooks; one under a parent of another type
    // moves to the top level.
    let parent = |a: &Account, t: &str| a.parent.filter(|p| typed.get(p).is_some_and(|(_, pt)| *pt == t));
    let full = paths(typed.values().map(|(a, t)| (a.pk, a.name.as_str(), parent(a, t))));
    let mut out: Vec<SourceAccount> = typed
        .values()
        .map(|(a, t)| SourceAccount {
            id: a.pk,
            name: a.name.trim().replace(':', "-"),
            full_name: full[&a.pk].clone(),
            account_type: t,
            parent_id: parent(a, t),
            active: !a.hidden,
            // The file stores debits positive; the ledger, credits.
            stored_balance_cents: cents(a.ending_balance.as_deref()).ok().flatten().map(|c| -c),
            tax_line: a.tax_line.map(crate::accountant_reports::quickbooks_tax_line),
        })
        .collect();
    out.sort_by(|a, b| a.full_name.to_lowercase().cmp(&b.full_name.to_lowercase()).then(a.id.cmp(&b.id)));
    out
}

/// Customers (with their jobs) and vendors.
fn parties(book: &Book) -> (Vec<SourceParty>, Vec<SourceParty>) {
    let terms: HashMap<i64, i64> =
        book.terms.iter().filter(|t| t.kind.unwrap_or(0) == 0).filter_map(|t| Some((t.recnum, t.due_days?))).filter(|(_, d)| (0..=365).contains(d)).collect();
    let customers: HashSet<i64> = book.names.iter().filter(|n| n.kind == Some(0)).map(|n| n.pk).collect();
    let full = paths(book.names.iter().filter(|n| n.kind == Some(0)).map(|n| (n.pk, n.name.as_str(), n.parent.filter(|p| customers.contains(p)))));
    let (mut cs, mut vs) = (Vec::new(), Vec::new());
    for n in &book.names {
        let vendor = match n.kind {
            Some(0) => false,
            Some(1) => true,
            _ => continue,
        };
        let person = [n.first.as_deref(), n.last.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" ");
        let name = if vendor { n.name.trim().replace(':', "-") } else { full[&n.pk].clone() };
        let balance = cents(n.ending_balance.as_deref()).ok().flatten().map(|c| if vendor { -c } else { c });
        let email = n.email.as_deref().filter(|e| e.contains('@') && !e.contains(char::is_whitespace)).map(str::to_string);
        let party = SourceParty {
            parent_id: if vendor { None } else { n.parent.filter(|p| customers.contains(p)) },
            company_name: n.company.clone().filter(|c| c.trim() != n.name.trim()).or_else(|| (!person.is_empty()).then_some(person)),
            email,
            phone: n.phone.clone(),
            address: (!n.address.is_empty()).then(|| n.address.join("\n")),
            terms_days: n.terms.and_then(|t| terms.get(&t).copied()),
            // Employees' tax IDs are their social security numbers and stay behind.
            tax_id: if vendor { n.tax_id.clone().filter(|t| crate::tax_ids::looks_like_tax_id(t)) } else { None },
            is_1099: vendor && n.is_1099,
            // As in Intuit's own conversion: an inactive name that still has a balance comes in active.
            active: !n.hidden || balance.is_some_and(|b| b != 0),
            balance_cents: balance,
            notes: n.note.clone(),
            ..blank_party(n.pk, &name)
        };
        if vendor {
            vs.push(party);
        } else {
            cs.push(party);
        }
    }
    cs.sort_by_key(|p| p.name.to_lowercase());
    vs.sort_by_key(|p| p.name.to_lowercase());
    (cs, vs)
}

fn items(book: &Book, known: &HashSet<i64>) -> Vec<SourceItem> {
    let full = paths(book.items.iter().map(|i| (i.pk, i.name.as_str(), i.parent)));
    let mut out: Vec<SourceItem> = book
        .items
        .iter()
        .filter_map(|i| {
            let kind = match i.kind? {
                0 => "Service",
                1 => "Inventory part",
                3 => "Non-inventory part",
                5 => "Other charge",
                _ => return None,
            };
            // A percentage price (`-10.0%`) is not a rate.
            let rate_cents = i.price.as_deref().filter(|p| !p.ends_with('%')).and_then(|p| scaled(p, 2).ok().flatten()).map_or(0, i64::abs);
            Some(SourceItem {
                id: i.pk,
                name: full[&i.pk].clone(),
                description: i.description.clone(),
                rate_cents,
                income_account_id: i.income_account.filter(|a| known.contains(a)),
                kind,
                taxable: i.taxable,
                active: !i.hidden,
                custom_fields: Vec::new(),
            })
        })
        .collect();
    out.sort_by_key(|i| i.name.to_lowercase());
    out
}

/// Quantity (×10,000) and rate of a form line when they give its amount, else one at the amount.
#[allow(clippy::float_arithmetic, reason = "scales a quantity read from the file; the amount it must reproduce is checked in integers")]
fn quantity_and_rate(t: &Targ, amount: i64) -> (i64, i64) {
    let quantity = t.quantity.as_deref().and_then(|q| q.trim_end_matches('#').trim().parse::<f64>().ok()).map(f64::abs).filter(|q| q.is_finite() && *q > 0.0);
    let rate = t.unit_price.as_deref().filter(|p| !p.ends_with('%')).and_then(|p| scaled(p, 2).ok().flatten()).map(i64::abs).filter(|r| *r > 0);
    if let (Some(q), Some(r)) = (quantity, rate) {
        let q_e4 = (q * 10_000.0).round() as i64;
        if q_e4 > 0 && (q_e4 as i128 * r as i128 + 5_000) / 10_000 == amount.abs() as i128 {
            return (q_e4, r);
        }
    }
    (10_000, amount.abs())
}

/// One transaction's lines, in the order of the form, and the line that carries its header.
struct Read<'a> {
    master: Option<&'a Master>,
    targs: Vec<&'a Targ>,
    source: Option<&'a Targ>,
}

pub fn ledger(book: &Book) -> Ledger {
    let accounts = accounts(book);
    let types: HashMap<i64, &'static str> = accounts.iter().map(|a| (a.id, a.account_type)).collect();
    let known: HashSet<i64> = types.keys().copied().collect();
    let (customers, vendors) = parties(book);
    let customer_ids: HashSet<i64> = customers.iter().map(|c| c.id).collect();
    let vendor_ids: HashSet<i64> = vendors.iter().map(|v| v.id).collect();
    let customer_names: HashMap<i64, String> = customers.iter().map(|c| (c.id, c.name.clone())).collect();
    let other_names: HashMap<i64, String> = book.names.iter().map(|n| (n.pk, n.name.trim().replace(':', "-"))).collect();
    let name_of = |pk: Option<i64>| pk.and_then(|p| customer_names.get(&p).or_else(|| other_names.get(&p))).cloned();
    let items_by: HashMap<i64, &Item> = book.items.iter().map(|i| (i.pk, i)).collect();
    let masters: HashMap<i64, &Master> = book.masters.iter().map(|m| (m.pk, m)).collect();
    let is_arap = |a: i64| matches!(types.get(&a).copied(), Some("ACCOUNTS_RECEIVABLE" | "ACCOUNTS_PAYABLE"));

    let mut grouped: BTreeMap<i64, Vec<&Targ>> = BTreeMap::new();
    for t in &book.targs {
        if let Some(m) = t.master {
            grouped.entry(m).or_default().push(t);
        }
    }
    let mut reads: BTreeMap<i64, Read> = BTreeMap::new();
    for (pk, mut targs) in grouped {
        targs.sort_by_key(|t| (t.recnum.unwrap_or(i64::MAX), t.pk));
        let source = targs.iter().find(|t| t.is_source).or(targs.first()).copied();
        reads.insert(pk, Read { master: masters.get(&pk).copied(), targs, source });
    }

    // Applications, from the paying side of each link pair: (type, paying master) → applied.
    let targ_master: HashMap<i64, i64> = book.targs.iter().filter_map(|t| Some((t.pk, t.master?))).collect();
    let mut sums: BTreeMap<(i64, i64, i64), i64> = BTreeMap::new();
    for l in book.links.iter().filter(|l| l.one_is_source) {
        let (Some(kind), Some(from), Some(to)) = (l.kind, l.one.and_then(|t| targ_master.get(&t)), l.two.and_then(|t| targ_master.get(&t))) else { continue };
        // Quantity links (an order's items received or billed) end in `#`; amounts in `$`.
        let Some(amount) = l.amount.as_deref().filter(|a| !a.ends_with('#')) else { continue };
        if let Ok(Some(c)) = scaled(amount, 2) {
            if from != to {
                *sums.entry((kind, *from, *to)).or_default() -= c;
            }
        }
    }
    let mut applied: HashMap<(i64, i64), Vec<Applied>> = HashMap::new();
    for ((kind, from, to), cents) in sums {
        if cents > 0 {
            applied.entry((kind, from)).or_default().push(Applied { transaction_id: to, cents });
        }
    }
    let mut deposits: HashMap<i64, Vec<Applied>> = HashMap::new();
    for ((kind, from), list) in &applied {
        if *kind == LINK_DEPOSIT {
            for a in list {
                deposits.entry(a.transaction_id).or_default().push(Applied { transaction_id: *from, cents: a.cents });
            }
        }
    }
    for list in deposits.values_mut() {
        list.sort_by_key(|a| a.transaction_id);
    }

    let earliest = NaiveDate::from_ymd_opt(1980, 1, 1).expect("valid date");
    let today = chrono::Local::now().date_naive();
    let latest = today.with_year(today.year() + 1).unwrap_or(NaiveDate::MAX);
    let mut quality = LedgerQuality::default();
    let mut unreadable: Vec<UnreadableLine> = Vec::new();
    let mut transactions: Vec<SourceTransaction> = Vec::new();
    for (&pk, read) in &reads {
        let Some(source) = read.source else { continue };
        let view = source.view;
        let kind = kind(view);
        let date = read.master.and_then(|m| m.date).or(source.date);
        let doc_num = source.document.clone();
        let party = source.name;
        let name = name_of(party);
        let memo = source.memo.clone();
        let customer = party.filter(|p| customer_ids.contains(p));
        let vendor = party.filter(|p| vendor_ids.contains(p));

        // Estimates and orders post nothing; estimates and purchase orders come along as documents.
        if read.targs.iter().all(|t| t.no_post) {
            let Some(date) = date.filter(|d| (earliest..=latest).contains(d)) else { continue };
            let lines: Vec<DocumentLine> = read
                .targs
                .iter()
                .filter(|t| !t.is_source)
                .filter_map(|t| {
                    let account_id = t.account.filter(|a| known.contains(a))?;
                    let c = cents(t.amount.as_deref()).ok().flatten().filter(|c| *c != 0)?.abs();
                    let (quantity_e4, rate_cents) = quantity_and_rate(t, c);
                    Some(DocumentLine { item_id: t.item, description: t.memo.clone(), quantity_e4, rate_cents, cents: c, account_id })
                })
                .collect();
            let document = match (view, customer, vendor) {
                (Some(0), Some(customer_id), _) => {
                    SourceDocument::Estimate { customer_id, date, number: doc_num.clone(), expires: source.due, memo: memo.clone(), lines, sales: SourceSales::default() }
                }
                (Some(17), _, Some(vendor_id)) => SourceDocument::PurchaseOrder {
                    vendor_id,
                    date,
                    number: doc_num.clone(),
                    expected: source.due,
                    memo: memo.clone(),
                    received: source.received,
                    lines,
                },
                _ => continue,
            };
            transactions.push(SourceTransaction { document: Some(document), id: pk, kind, date, doc_num, name, memo, lines: Vec::new() });
            continue;
        }

        // Posting lines, with each line's ledger amount (credits positive) kept for the forms.
        let mut problem: Option<(&'static str, String)> = None;
        let mut posted: Vec<(&Targ, i64, i64)> = Vec::new();
        let mut cost: Vec<SourceLine> = Vec::new();
        for t in read.targs.iter().filter(|t| !t.no_post) {
            let amount = match cents(t.amount.as_deref()) {
                Ok(c) => c.unwrap_or(0),
                Err(()) => {
                    problem.get_or_insert((NOT_WHOLE_CENTS, format!("{} is not a whole number of cents", t.amount.as_deref().unwrap_or_default())));
                    continue;
                }
            };
            match t.account {
                Some(a) if known.contains(&a) => {
                    if amount != 0 {
                        posted.push((t, a, -amount));
                    }
                }
                // Subtotal and text lines have no account; what they show is in the other lines.
                None => {}
                Some(_) if amount == 0 => {}
                Some(_) => {
                    problem.get_or_insert((UNKNOWN_ACCOUNT, "a line's account is not in the file".into()));
                }
            }
            match cents(t.cogs.as_deref()) {
                Ok(None | Some(0)) => {}
                Ok(Some(c)) => {
                    let item = t.item.and_then(|i| items_by.get(&i));
                    // The cost moves the item's inventory asset account by the cost (in the file's
                    // signs) and cost of goods sold the other way. An item without both accounts
                    // posts no cost, which shows as a balance that differs from the file's.
                    if let Some((Some(cogs), Some(asset))) = item.map(|i| (i.cogs_account.filter(|a| known.contains(a)), i.asset_account.filter(|a| known.contains(a)))) {
                        cost.push(SourceLine { account_id: cogs, cents: c, memo: t.memo.clone() });
                        cost.push(SourceLine { account_id: asset, cents: -c, memo: t.memo.clone() });
                    }
                }
                Err(()) => {
                    problem.get_or_insert((NOT_WHOLE_CENTS, format!("an inventory cost of {} is not a whole number of cents", t.cogs.as_deref().unwrap_or_default())));
                }
            }
        }
        match date {
            None => {
                problem.get_or_insert((NO_DATE, "it has no date".into()));
            }
            Some(d) if !(earliest..=latest).contains(&d) => {
                problem.get_or_insert((DATE_OUT_OF_RANGE, format!("its date {d} is out of range")));
            }
            Some(_) => {}
        }
        let (Some(date), None) = (date, &problem) else {
            let (reason, detail) = problem.expect("a transaction without a date has a problem");
            reject(&mut quality, reason);
            let label = Some(match &name {
                Some(n) => format!("{n} ({detail})"),
                None => format!("({detail})"),
            });
            for t in read.targs.iter().filter(|t| !t.no_post) {
                let Some(account_id) = t.account.filter(|a| known.contains(a)) else { continue };
                let c = cents(t.amount.as_deref());
                if matches!(c, Ok(None | Some(0))) {
                    continue;
                }
                unreadable.push(UnreadableLine {
                    kind,
                    transaction_id: Some(pk),
                    date,
                    doc_num: doc_num.clone(),
                    name: label.clone(),
                    account_id: Some(account_id),
                    cents: c.ok().flatten().map(|c| -c),
                    outcome: LineOutcome::Unreadable,
                });
            }
            continue;
        };
        let mut lines: Vec<SourceLine> = posted.iter().map(|(t, a, c)| SourceLine { account_id: *a, cents: *c, memo: t.memo.clone() }).collect();
        lines.extend(cost);
        if lines.is_empty() {
            continue;
        }

        // Form lines: what the form showed, without receivables or payables.
        let form = |all: bool| -> Vec<DocumentLine> {
            posted
                .iter()
                .filter(|(t, a, _)| !is_arap(*a) && (all || !t.is_source))
                .map(|(t, a, c)| {
                    let (quantity_e4, rate_cents) = quantity_and_rate(t, *c);
                    DocumentLine { item_id: t.item.filter(|i| items_by.contains_key(i)), description: t.memo.clone(), quantity_e4, rate_cents, cents: *c, account_id: *a }
                })
                .collect()
        };
        let method = || payment_method(source.pay_method.and_then(|m| book.pay_methods.get(&m)).map(String::as_str)).to_string();
        let applied_to = |link: i64| applied.get(&(link, pk)).cloned().unwrap_or_default();
        let sales = || SourceSales {
            rep: None,
            ship_date: read.master.and_then(|m| m.ship_date),
            ship_via: read.master.and_then(|m| m.ship_via).and_then(|v| book.ship_types.get(&v)).map(|(n, _)| n.clone()),
        };
        let document = match view {
            Some(4) => customer.map(|customer_id| SourceDocument::Invoice { customer_id, number: doc_num.clone(), due_date: source.due, lines: form(true), sales: sales() }),
            Some(6) => customer.map(|customer_id| SourceDocument::CreditMemo { customer_id, number: doc_num.clone(), lines: form(true), applied_to: Vec::new() }),
            Some(5) => Some(SourceDocument::SalesReceipt { customer_id: customer, number: doc_num.clone(), method: method(), lines: form(true), sales: sales() }),
            Some(7) => customer.map(|customer_id| SourceDocument::Payment { customer_id, reference: doc_num.clone(), method: method(), applied_to: applied_to(LINK_PAYMENT) }),
            Some(2) => Some(SourceDocument::Deposit { payments: deposits.get(&pk).cloned().unwrap_or_default() }),
            Some(9 | 12) => vendor.map(|vendor_id| SourceDocument::Bill { vendor_id, number: doc_num.clone(), due_date: source.due, lines: form(true) }),
            Some(14) => Some(SourceDocument::BillPayment { vendor_id: vendor, reference: doc_num.clone(), applied_to: applied_to(LINK_BILL_PAYMENT) }),
            Some(1 | 3) => Some(SourceDocument::Expense { vendor_id: vendor, number: doc_num.clone(), applied_to: applied_to(LINK_BILL_PAYMENT) }),
            Some(26) => Some(SourceDocument::Transfer),
            _ => None,
        };
        transactions.push(SourceTransaction { document, id: pk, kind, date, doc_num, name, memo, lines });
    }
    quality.unreadable_lines = unreadable.len();

    let mut totals: HashMap<i64, i64> = HashMap::new();
    for line in transactions.iter().flat_map(|t| &t.lines) {
        *totals.entry(line.account_id).or_default() += line.cents;
    }
    for account in &accounts {
        if let Some(stored) = account.stored_balance_cents {
            quality.accounts_with_stored_balance += 1;
            quality.accounts_matching_stored += (totals.get(&account.id).copied().unwrap_or(0) == stored) as usize;
        }
    }

    transactions.sort_by_key(|t| (t.date, t.id));
    unreadable.sort_by_key(|u| (u.date, u.transaction_id));
    let (transactions, unbalanced): (Vec<_>, Vec<_>) = transactions.into_iter().partition(|t| t.imbalance_cents() == 0);
    let items = items(book, &known);
    let mut sales_lists = SourceSalesLists::default();
    for (name, active) in book.ship_types.values() {
        sales_lists.add_ship_method(name, *active);
    }
    sales_lists.ship_methods.sort();
    Ledger { accounts, customers, vendors, items, transactions, unbalanced, unreadable, quality, sales_lists }
}

#[cfg(test)]
mod tests;
