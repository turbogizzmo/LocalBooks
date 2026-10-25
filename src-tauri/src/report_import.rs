//! Reads QuickBooks report exports (CSV, or Excel `.xlsx`/`.xls`) into a [`Ledger`], so they go
//! through the same staging, account mapping and journal or full import as an IIF file. Every
//! QuickBooks product can export its reports, including QuickBooks Desktop for Mac in view-only
//! mode after its license has lapsed, so this is the way in that needs no company-file access.
//!
//! What it reads, found by the row of column headings (QuickBooks puts title rows above it):
//! - the **Journal** report: every transaction with all its lines. Desktop (Windows and Mac) has a
//!   `Trans #` column, which groups the lines; QuickBooks Online has none, so consecutive lines with
//!   the same date, type and number are one transaction until its debits equal its credits.
//! - the **General Ledger** or **Transaction Detail by Account**: lines grouped under account
//!   headings, each with a `Split` column naming the other side. A transaction appears once under
//!   each account it touches; its lines are paired back together through `Split`.
//! - lists: the **Account Listing** (which gives account types), the **Customer** and **Vendor
//!   Contact Lists** and the **Item List**, alone or with a report.
//!
//! Report noise is skipped: title rows, blank and indenting columns, account headings, total rows
//! and the footer timestamp. Amounts come from the Debit and Credit columns only, as written
//! (`1,234.56`, `(45.00)`, `-$0.29`); one that does not read is reported, never guessed, and its
//! transaction is listed unbalanced and not posted. Each transaction is then handed to the IIF
//! reader's resolver (`iif_import::assemble`), so the QuickBooks transaction types become the same
//! LocalBooks documents and payments apply to open invoices and bills the same way.

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::Path;

use calamine::{Data, Reader};

use crate::bank::{detect_date_order, parse_amount_cents, parse_date_ordered, DateOrder};
use crate::iif_import::{assemble, Block, RawLine};
use crate::ledger::format_cents;
use crate::qbw_company::{Ledger, SourceItem};
use crate::text_import::{decode, terms_days, Accounts, Names};

/// File extensions read as report exports.
pub const EXTENSIONS: [&str; 3] = ["csv", "xlsx", "xls"];

/// How far down a file the row of column headings may be.
const HEADER_SEARCH_ROWS: usize = 40;

type Grid = Vec<Vec<String>>;

/// A column heading reduced to letters, digits and `#`, lowercased: `Trans #` is `trans#`,
/// `Memo/Description` is `memodescription`.
fn key(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric() || *c == '#').flat_map(char::to_lowercase).collect()
}

// Column names by product and version, as keys.
const TRANS_NO: &[&str] = &["trans#", "transno", "transnum", "transaction#", "transactionno", "transid", "txn#", "txnid"];
const TYPE: &[&str] = &["type", "transactiontype", "txntype", "transtype"];
const DATE: &[&str] = &["date", "transactiondate", "txndate", "transdate"];
const NUM: &[&str] = &["num", "no", "number", "num#", "ref", "ref#", "refno", "docnum", "checkno"];
const NAME: &[&str] = &["name", "payee", "receivedfrom", "namefullname"];
const MEMO: &[&str] = &["memo", "memodescription", "description", "linememo", "linedescription"];
const ACCOUNT: &[&str] = &["account", "accountname", "accountfullname", "fullaccountname", "distributionaccount"];
const SPLIT: &[&str] = &["split", "splitaccount"];
const DEBIT: &[&str] = &["debit", "debits", "debitamount", "dr"];
const CREDIT: &[&str] = &["credit", "credits", "creditamount", "cr"];
const AMOUNT: &[&str] = &["amount"];

const LIST_ACCOUNT: &[&str] = &["account", "accountname", "accountfullname", "fullname"];
const ACCOUNT_TYPE: &[&str] = &["type", "accounttype", "accnttype"];
const DETAIL_TYPE: &[&str] = &["detailtype"];
const BALANCE: &[&str] = &["balancetotal", "balance", "totalbalance", "quickbooksbalance"];
const STATUS: &[&str] = &["activestatus", "status"];
const CUSTOMER: &[&str] = &["customer", "customername", "customerfullname", "customerjob", "customerdisplayname"];
const VENDOR: &[&str] = &["vendor", "vendorname", "vendorfullname", "vendordisplayname"];
const ITEM: &[&str] = &["item", "itemname", "productservice", "productservicename", "productservicefullname"];
const COMPANY: &[&str] = &["company", "companyname"];
const EMAIL: &[&str] = &["mainemail", "email", "emailaddress"];
const PHONE: &[&str] = &["mainphone", "phone", "phonenumbers", "phonenumber", "workphone"];
const BILL_TO: &[&str] = &["billto", "billingaddress", "billaddress", "billfrom", "address", "vendoraddress"];
const TERMS: &[&str] = &["terms"];
const NOTES: &[&str] = &["notes", "note"];
const TAX_ID: &[&str] = &["taxid", "vendortaxid", "taxidnumber", "businessidno"];
const ELIGIBLE_1099: &[&str] = &["1099", "eligiblefor1099", "vendoreligiblefor1099", "track1099", "trackpaymentsfor1099"];
const ITEM_TYPE: &[&str] = &["type", "itemtype"];
const ITEM_DESCRIPTION: &[&str] = &["description", "salesdescription", "salesdesc"];
const PRICE: &[&str] = &["price", "salesprice", "rate", "salespricerate"];
const INCOME_ACCOUNT: &[&str] = &["account", "incomeaccount", "incomeaccnt"];
const TAX_CODE: &[&str] = &["salestaxcode", "taxcode", "taxable"];

/// Keys only one product's exports use.
const DESKTOP_KEYS: &[&str] = &["trans#", "adj", "accnt#", "billto", "billfrom", "mainphone", "mainemail", "balancetotal", "salestaxcode"];
const ONLINE_KEYS: &[&str] =
    &["transactiontype", "memodescription", "distributionaccount", "detailtype", "productservice", "phonenumbers", "customerfullname", "fullname", "salesprice"];

/// What a file holds, by its column headings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Journal,
    /// General Ledger or Transaction Detail by Account.
    ByAccount,
    Accounts,
    Customers,
    Vendors,
    Items,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Journal => "Journal",
            Kind::ByAccount => "General Ledger",
            Kind::Accounts => "Account Listing",
            Kind::Customers => "Customer list",
            Kind::Vendors => "Vendor list",
            Kind::Items => "Item list",
        }
    }
}

/// The row of column headings.
struct Header {
    keys: Vec<String>,
}

impl Header {
    fn find(&self, names: &[&str]) -> Option<usize> {
        names.iter().find_map(|n| self.keys.iter().position(|k| k == n))
    }

    fn has(&self, names: &[&str]) -> bool {
        self.find(names).is_some()
    }

    fn kind(&self) -> Result<Option<Kind>, String> {
        let money = self.has(DEBIT) && self.has(CREDIT);
        if self.has(DATE) && (self.has(ACCOUNT) || self.has(SPLIT)) {
            if !money {
                // A single Amount column is signed by each account's own side, which the report
                // does not say; reading it would mean guessing.
                return match self.has(AMOUNT) {
                    true => Err("it has an Amount column but no Debit and Credit columns. Customize the report, add the Debit and Credit columns, and export it again".into()),
                    false => Ok(None),
                };
            }
            return Ok(Some(if self.has(SPLIT) { Kind::ByAccount } else { Kind::Journal }));
        }
        if money || self.has(DATE) {
            return Ok(None);
        }
        let any = |lists: &[&[&str]]| lists.iter().any(|l| self.has(l));
        Ok(if self.has(CUSTOMER) && any(&[EMAIL, PHONE, BILL_TO, BALANCE, COMPANY, TERMS]) {
            Some(Kind::Customers)
        } else if self.has(VENDOR) && any(&[EMAIL, PHONE, BILL_TO, BALANCE, COMPANY, TERMS, TAX_ID]) {
            Some(Kind::Vendors)
        } else if self.has(ITEM) && any(&[ITEM_TYPE, PRICE, ITEM_DESCRIPTION]) {
            Some(Kind::Items)
        } else if self.has(LIST_ACCOUNT) && self.has(ACCOUNT_TYPE) {
            Some(Kind::Accounts)
        } else {
            None
        })
    }

    fn product(&self) -> &'static str {
        let uses = |keys: &[&str]| self.keys.iter().any(|k| keys.contains(&k.as_str()));
        if uses(DESKTOP_KEYS) {
            "QuickBooks Desktop"
        } else if uses(ONLINE_KEYS) {
            "QuickBooks Online"
        } else {
            "QuickBooks"
        }
    }
}

/// One recognised report or list.
struct Table {
    file: String,
    kind: Kind,
    product: &'static str,
    header: Header,
    rows: Grid,
    /// 1-based row of the headings in the file, for messages.
    header_row: usize,
}

impl Table {
    fn col(&self, names: &[&str]) -> Option<usize> {
        self.header.find(names)
    }
}

fn cell(row: &[String], col: Option<usize>) -> Option<&str> {
    row.get(col?).map(|v| v.trim()).filter(|v| !v.is_empty())
}

fn text(row: &[String], col: Option<usize>) -> Option<String> {
    cell(row, col).map(str::to_string)
}

/// A spreadsheet cell as text. Numbers print as the shortest decimal that reads back as the same
/// value, which is the text Excel was given for any amount QuickBooks writes; a value that is not
/// a whole number of cents fails to read as an amount and is reported.
fn cell_text(data: &Data) -> String {
    match data {
        Data::Empty => String::new(),
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => f.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(d) if d.is_datetime() => {
            let (y, m, day, ..) = d.to_ymd_hms_milli();
            format!("{y:04}-{m:02}-{day:02}")
        }
        Data::DateTime(d) => d.as_f64().to_string(),
        Data::Error(e) => format!("#{e:?}"),
    }
}

/// The sheets of a file: one for CSV, each worksheet for a workbook.
fn grids(file: &str, bytes: &[u8]) -> Result<Vec<Grid>, String> {
    let ext = Path::new(file).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if ext == "csv" || ext == "txt" {
        let text = decode(bytes);
        let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default();
        let delimiter = if first.matches('\t').count() > first.matches(',').count() { b'\t' } else { b',' };
        let mut reader = csv::ReaderBuilder::new().has_headers(false).flexible(true).delimiter(delimiter).from_reader(text.as_bytes());
        let mut grid = Vec::new();
        for record in reader.records() {
            let record = record.map_err(|e| format!("REPORT_UNREADABLE: {file}: {e}"))?;
            grid.push(record.iter().map(|f| f.trim().to_string()).collect());
        }
        return Ok(vec![grid]);
    }
    let mut book = calamine::open_workbook_auto_from_rs(Cursor::new(bytes)).map_err(|e| format!("REPORT_UNREADABLE: {file}: not a workbook Excel can open ({e})"))?;
    let mut sheets = Vec::new();
    for n in 0..book.sheet_names().len() {
        let Some(Ok(range)) = book.worksheet_range_at(n) else { continue };
        sheets.push(range.rows().map(|r| r.iter().map(|c| cell_text(c).trim().to_string()).collect()).collect());
    }
    Ok(sheets)
}

/// The first sheet of a file that is a report or list LocalBooks reads.
fn table(file: &str, bytes: &[u8]) -> Result<Table, String> {
    let mut why = None;
    for mut grid in grids(file, bytes)? {
        for i in 0..grid.len().min(HEADER_SEARCH_ROWS) {
            let header = Header { keys: grid[i].iter().map(|c| key(c)).collect() };
            match header.kind() {
                Ok(Some(kind)) => {
                    let rows = grid.split_off(i + 1);
                    return Ok(Table { file: file.to_string(), kind, product: header.product(), header, rows, header_row: i + 1 });
                }
                Ok(None) => {}
                Err(e) => why = Some(e),
            }
        }
    }
    Err(match why {
        Some(why) => format!("REPORT_NEEDS_DEBIT_CREDIT: {file}: {why}"),
        None => format!(
            "REPORT_UNRECOGNISED: {file} is not a QuickBooks report LocalBooks reads: none of its first {HEADER_SEARCH_ROWS} rows has the column headings of a \
             Journal or General Ledger (Date, Account or Split, Debit, Credit), an Account Listing (Account, Type), a customer or vendor contact list or an item list"
        ),
    })
}

/// An account path as QuickBooks Desktop writes it with account numbers on (`6100 · Utilities:6110
/// · Gas`), without the numbers.
fn account_path(raw: &str) -> String {
    raw.split(':')
        .map(|segment| {
            let segment = segment.trim();
            match segment.split_once(" · ") {
                Some((number, name)) if !number.is_empty() && number.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-')) => name.trim(),
                _ => segment,
            }
        })
        .collect::<Vec<_>>()
        .join(":")
}

/// LocalBooks account type for a QuickBooks account type name (Desktop's `Other Current Asset`,
/// Online's `Accounts receivable (A/R)`); `None` for non-posting and unknown types.
fn account_type(name: &str) -> Option<&'static str> {
    let k = key(name);
    let starts = |p: &str| k.starts_with(p);
    Some(if starts("accountsreceivable") || k == "ar" {
        "ACCOUNTS_RECEIVABLE"
    } else if starts("accountspayable") || k == "ap" {
        "ACCOUNTS_PAYABLE"
    } else if starts("bank") {
        "BANK"
    } else if starts("creditcard") {
        "CREDIT_CARD"
    } else if starts("othercurrentasset") {
        "OTHER_CURRENT_ASSET"
    } else if starts("fixedasset") {
        "FIXED_ASSET"
    } else if starts("otherasset") {
        "OTHER_ASSET"
    } else if starts("othercurrentliabilit") {
        "OTHER_CURRENT_LIABILITY"
    } else if starts("longtermliabilit") {
        "LONG_TERM_LIABILITY"
    } else if starts("equity") {
        "EQUITY"
    } else if starts("costofgoodssold") || k == "cogs" {
        "COST_OF_GOODS_SOLD"
    } else if starts("otherincome") {
        "OTHER_INCOME"
    } else if starts("otherexpense") {
        "OTHER_EXPENSE"
    } else if starts("income") {
        "INCOME"
    } else if starts("expense") {
        "EXPENSE"
    } else {
        return None;
    })
}

/// Whether an account of this type normally carries a debit balance.
fn debit_side(account_type: &str) -> bool {
    matches!(account_type, "BANK" | "ACCOUNTS_RECEIVABLE" | "OTHER_CURRENT_ASSET" | "FIXED_ASSET" | "OTHER_ASSET" | "COST_OF_GOODS_SOLD" | "EXPENSE" | "OTHER_EXPENSE")
}

fn balance_sheet(account_type: &str) -> bool {
    !matches!(account_type, "INCOME" | "OTHER_INCOME" | "COST_OF_GOODS_SOLD" | "EXPENSE" | "OTHER_EXPENSE")
}

/// The IIF transaction type for a report's `Type`, as each product names it, and the kind to show
/// when it differs from the IIF one.
fn iif_type(name: &str) -> (String, Option<&'static str>) {
    let code = match key(name).as_str() {
        "invoice" => "INVOICE",
        "payment" | "receivepayment" | "customerpayment" => "PAYMENT",
        "salesreceipt" | "cashsale" => "CASH SALE",
        "creditmemo" => "CREDIT MEMO",
        "bill" => "BILL",
        // Desktop calls a vendor credit just "Credit".
        "credit" | "vendorcredit" | "billcredit" => "BILL REFUND",
        "billpmtcheck" | "billpaymentcheck" | "billpayment" | "billpmt" => "BILLPMT",
        "billpmtccard" | "billpmtcreditcard" | "billpaymentcreditcard" => "BILLCCARD",
        "check" => "CHECK",
        // Online's expense is paid from a bank or a card account: a check that is not a check.
        "expense" | "cashexpense" => return ("CHECK".into(), Some("Expense")),
        "creditcardcharge" | "creditcardexpense" | "ccardcharge" => "CREDIT CARD",
        "creditcardcredit" | "ccardcredit" | "creditcardrefund" => "CCARD REFUND",
        "deposit" => "DEPOSIT",
        "transfer" | "creditcardpayment" => "TRANSFER",
        "generaljournal" | "journalentry" | "journal" => "GENERAL JOURNAL",
        "itemreceipt" => "ITEM RECEIPT",
        "refund" | "refundreceipt" | "cashrefund" | "customerrefund" => "CASH REFUND",
        "statementcharge" => "STATEMENT CHARGE",
        "inventoryadjust" | "inventoryadjustment" | "inventoryqtyadjust" | "inventoryquantityadjustment" | "inventorystartingvalue" => "INVENTORY ADJUST",
        "paycheck" | "payrollcheck" | "liabilitycheck" | "liabcheck" | "payrollliabilitycheck" => "PAYCHECK",
        "estimate" => "ESTIMATES",
        "purchaseorder" => "PURCHORD",
        _ => return (name.trim().to_uppercase(), None),
    };
    (code.into(), None)
}

/// The side the transaction's own account is on (receivables for an invoice, the bank for a
/// check): `Some(true)` for a debit.
fn main_side(code: &str) -> Option<bool> {
    match code {
        "INVOICE" | "CASH SALE" | "PAYMENT" | "DEPOSIT" | "STATEMENT CHARGE" | "BILL REFUND" | "CCARD REFUND" => Some(true),
        "CREDIT MEMO" | "CASH REFUND" | "BILL" | "ITEM RECEIPT" | "BILLPMT" | "BILLCCARD" | "CHECK" | "CREDIT CARD" | "PAYCHECK" => Some(false),
        _ => None,
    }
}

/// Excel's serial day number (1900 date system) as a date.
fn serial_date(raw: &str) -> Option<String> {
    let (whole, frac) = raw.split_once('.').unwrap_or((raw, ""));
    if whole.is_empty() || whole.len() > 6 || !whole.chars().chain(frac.chars()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    let days: i64 = whole.parse().ok()?;
    // Excel counts 1900 as a leap year, so serials from March 1900 on are one day ahead.
    let epoch = chrono::NaiveDate::from_ymd_opt(1899, 12, 30)?;
    (61..=2_958_465).contains(&days).then(|| (epoch + chrono::Duration::days(days)).format("%Y-%m-%d").to_string())
}

/// Whether a cell holds a date, in either day/month order.
fn is_date(raw: &str) -> bool {
    let raw = date_part(raw);
    serial_date(&raw).is_some() || [DateOrder::Mdy, DateOrder::Dmy].into_iter().any(|o| parse_date_ordered(&raw, o).is_ok())
}

/// A date cell without any time of day (`1/15/2024 12:00:00 AM`).
fn date_part(raw: &str) -> String {
    raw.split_whitespace().take_while(|t| !t.contains(':')).collect::<Vec<_>>().join(" ")
}

/// Reads a file's date cells: Excel dates, Excel serial numbers, and text dates in the order the
/// whole column uses (day-first only when some date can only be read that way).
struct Dates {
    order: DateOrder,
}

impl Dates {
    fn of<'a>(cells: impl Iterator<Item = &'a str>) -> Self {
        let texts: Vec<String> = cells.filter(|c| serial_date(c).is_none()).map(date_part).collect();
        Dates { order: detect_date_order(texts.iter().map(String::as_str)).0 }
    }

    /// The date as `YYYY-MM-DD`, or the cell as written when it is not a date (it is then reported).
    fn read(&self, raw: &str) -> String {
        if let Some(date) = serial_date(raw) {
            return date;
        }
        parse_date_ordered(&date_part(raw), self.order).map_or_else(|_| raw.to_string(), |d| d.format("%Y-%m-%d").to_string())
    }
}

/// A report line: the account, and the amount as the IIF reader takes it (debits positive).
struct Entry {
    account: String,
    /// `None` when both columns are blank; text that does not read as an amount when one is bad.
    amount: Option<String>,
    /// The amount in cents, when it read (blank is zero).
    cents: Option<i64>,
    name: Option<String>,
    memo: Option<String>,
}

fn entry(account: String, debit: Option<&str>, credit: Option<&str>, name: Option<String>, memo: Option<String>) -> Entry {
    if debit.is_none() && credit.is_none() {
        return Entry { account, amount: None, cents: Some(0), name, memo };
    }
    let read = |v: Option<&str>| v.map(parse_amount_cents).transpose();
    let (amount, cents) = match (read(debit), read(credit)) {
        (Ok(d), Ok(c)) => {
            let cents = d.unwrap_or(0) - c.unwrap_or(0);
            (format_cents(cents), Some(cents))
        }
        // The text that did not read, so the line is reported as unreadable.
        (Err(_), _) => (debit.unwrap_or_default().to_string(), None),
        (_, Err(_)) => (credit.unwrap_or_default().to_string(), None),
    };
    Entry { account, amount: Some(amount), cents, name, memo }
}

/// A transaction as the report has it.
struct Group {
    trans_no: Option<String>,
    kind: Option<String>,
    date: Option<String>,
    num: Option<String>,
    name: Option<String>,
    lines: Vec<Entry>,
    row: usize,
}

impl Group {
    /// Its debits less its credits, while every amount has read.
    fn balance(&self) -> Option<i64> {
        self.lines.iter().map(|l| l.cents).sum()
    }

    /// The transaction for the IIF resolver, with its own account's line first.
    fn block(mut self, dates: &Dates) -> (Block, Option<&'static str>) {
        let (code, label) = iif_type(self.kind.as_deref().unwrap_or_default());
        let main = match main_side(&code) {
            None => 0,
            Some(debit) => {
                let on_side = |l: &Entry| l.cents.is_some_and(|c| if debit { c > 0 } else { c < 0 });
                match self.lines.first() {
                    Some(first) if on_side(first) => 0,
                    _ => self
                        .lines
                        .iter()
                        .enumerate()
                        .filter(|(_, l)| on_side(l))
                        .max_by_key(|(i, l)| (l.cents.unwrap_or(0).abs(), std::cmp::Reverse(*i)))
                        .map_or(0, |(i, _)| i),
                }
            }
        };
        if main > 0 {
            let line = self.lines.remove(main);
            self.lines.insert(0, line);
        }
        let date = self.date.as_deref().map(|d| dates.read(d));
        let name = self.name.or_else(|| self.lines.iter().find_map(|l| l.name.clone()));
        let raw = |l: Entry, name: Option<String>| RawLine {
            trnstype: code.clone(),
            date: date.clone(),
            account: Some(l.account),
            name,
            amount: l.amount,
            doc_num: self.num.clone(),
            memo: l.memo,
            ..Default::default()
        };
        let mut lines = self.lines.into_iter();
        let trns = match lines.next() {
            Some(first) => raw(first, name),
            None => RawLine { trnstype: code.clone(), date: date.clone(), name, doc_num: self.num.clone(), ..Default::default() },
        };
        let splits = lines.map(|l| {
            let name = l.name.clone();
            raw(l, name)
        });
        let splits = splits.collect();
        (Block { trns, splits, file_line: self.row }, label)
    }
}

/// Reads the Journal report: a `Trans #` starts each transaction when there is one; otherwise a
/// line with its own date or type starts one when the open one balances or the date, type or
/// number changes. Total rows and blank rows close the open transaction.
fn journal(t: &Table) -> Vec<Group> {
    let col = |names| t.col(names);
    let (trans_no, kind, date, num, name, memo, account, debit, credit) =
        (col(TRANS_NO), col(TYPE), col(DATE), col(NUM), col(NAME), col(MEMO), col(ACCOUNT), col(DEBIT), col(CREDIT));
    let mut groups: Vec<Group> = Vec::new();
    let mut open: Option<Group> = None;
    for (n, row) in t.rows.iter().enumerate() {
        let Some(account_name) = cell(row, account) else {
            // A total, a title, a blank or the footer.
            groups.extend(open.take());
            continue;
        };
        let own = Group {
            trans_no: text(row, trans_no),
            kind: text(row, kind),
            date: text(row, date),
            num: text(row, num),
            name: text(row, name),
            lines: Vec::new(),
            row: t.header_row + n + 1,
        };
        let starts = match &open {
            None => true,
            Some(g) if trans_no.is_some() => own.trans_no.is_some() && own.trans_no != g.trans_no,
            Some(g) => {
                let changed = |a: &Option<String>, b: &Option<String>| a.is_some() && a != b;
                (own.date.is_some() || own.kind.is_some())
                    && (g.balance() == Some(0) || changed(&own.date, &g.date) || changed(&own.kind, &g.kind) || changed(&own.num, &g.num))
            }
        };
        if starts {
            groups.extend(open.take());
            open = Some(own.clone_head());
        }
        let g = open.as_mut().expect("opened above");
        g.lines.push(entry(account_path(account_name), cell(row, debit), cell(row, credit), own.name, text(row, memo)));
    }
    groups.extend(open);
    groups
}

impl Group {
    /// Its type, date, number and name, without its lines.
    fn clone_head(&self) -> Group {
        Group {
            trans_no: self.trans_no.clone(),
            kind: self.kind.clone(),
            date: self.date.clone(),
            num: self.num.clone(),
            name: self.name.clone(),
            lines: Vec::new(),
            row: self.row,
        }
    }
}

/// A line of a General Ledger, under its account's heading.
struct AccountLine {
    head: Group,
    split: Option<String>,
    line: Entry,
}

/// Reads a General Ledger or Transaction Detail by Account into transactions. Each line sits
/// under its account's heading (sub-accounts nested, closed by their `Total` row); the lines of
/// one transaction share its type, date and number, and are paired through `Split`, the other
/// account (`-SPLIT-` when there are several). What cannot be paired stays together as one
/// transaction, listed unbalanced if it does not balance.
fn by_account(t: &Table) -> Vec<Group> {
    let col = |names| t.col(names);
    let (trans_no, kind, date, num, name, memo, account, split, debit, credit) =
        (col(TRANS_NO), col(TYPE), col(DATE), col(NUM), col(NAME), col(MEMO), col(ACCOUNT), col(SPLIT), col(DEBIT), col(CREDIT));
    // Open headings: (column, full path).
    let mut headings: Vec<(usize, String)> = Vec::new();
    let mut lines: Vec<AccountLine> = Vec::new();
    for (n, row) in t.rows.iter().enumerate() {
        let Some((first_col, first)) = row.iter().enumerate().find(|(_, c)| !c.trim().is_empty()) else { continue };
        if row.iter().any(|c| key(c) == "beginningbalance") {
            continue;
        }
        // Online puts an account's heading and total in the first column, which may be Date.
        let is_line = cell(row, kind).is_some() || cell(row, date).is_some_and(is_date);
        let has_money = cell(row, debit).is_some() || cell(row, credit).is_some();
        let lower = first.trim().to_lowercase();
        if !is_line && (lower == "total" || lower.starts_with("total ")) {
            let closed = account_path(first.trim()[5..].trim().trim_start_matches("for ").trim_start_matches("For "));
            match headings.iter().rposition(|(_, h)| h.eq_ignore_ascii_case(&closed) || h.rsplit(':').next().is_some_and(|last| last.eq_ignore_ascii_case(&closed))) {
                Some(i) => headings.truncate(i),
                None if closed.is_empty() => headings.clear(),
                None => {}
            }
            continue;
        }
        if !is_line {
            if !has_money {
                // An account heading: a sub-account sits in a later column or under its parent.
                while headings.last().is_some_and(|(c, _)| *c > first_col) {
                    headings.pop();
                }
                let name = account_path(first.trim());
                let path = match headings.last() {
                    Some((_, parent)) if !name.contains(':') => format!("{parent}:{name}"),
                    _ => name,
                };
                headings.push((first_col, path));
            }
            continue;
        }
        let Some(account_name) = cell(row, account).map(account_path).or_else(|| headings.last().map(|h| h.1.clone())) else { continue };
        let head = Group {
            trans_no: text(row, trans_no),
            kind: text(row, kind),
            date: text(row, date),
            num: text(row, num),
            name: text(row, name),
            lines: Vec::new(),
            row: t.header_row + n + 1,
        };
        let line = entry(account_name, cell(row, debit), cell(row, credit), head.name.clone(), text(row, memo));
        lines.push(AccountLine { head, split: cell(row, split).map(account_path), line });
    }

    // Lines of one transaction: by Trans # when the report has it, else by type, date and number.
    let mut clusters: Vec<Vec<AccountLine>> = Vec::new();
    let mut index: HashMap<(String, String, String), usize> = HashMap::new();
    for (n, l) in lines.into_iter().enumerate() {
        let id = match &l.head.trans_no {
            Some(t) => (t.clone(), String::new(), String::new()),
            None if trans_no.is_some() => (format!("row {n}"), String::new(), String::new()),
            None => (
                key(l.head.kind.as_deref().unwrap_or_default()),
                l.head.date.clone().unwrap_or_default(),
                l.head.num.clone().unwrap_or_default().to_lowercase(),
            ),
        };
        let i = *index.entry(id).or_insert_with(|| {
            clusters.push(Vec::new());
            clusters.len() - 1
        });
        clusters[i].push(l);
    }
    let mut groups: Vec<Group> = Vec::new();
    for cluster in clusters {
        let sets = if trans_no.is_some() { vec![(0..cluster.len()).collect()] } else { pair(&cluster) };
        let mut cluster: Vec<Option<AccountLine>> = cluster.into_iter().map(Some).collect();
        for set in sets {
            let mut members: Vec<AccountLine> = set.iter().filter_map(|&i| cluster[i].take()).collect();
            let first = members.remove(0);
            let mut g = first.head.clone_head();
            g.lines.push(first.line);
            g.lines.extend(members.into_iter().map(|m| m.line));
            groups.push(g);
        }
    }
    groups.sort_by_key(|g| g.row);
    groups
}

/// Splits the lines of one type, date and number into transactions (indexes into `lines`, each
/// set in report order).
fn pair(lines: &[AccountLine]) -> Vec<Vec<usize>> {
    let same = |a: &str, b: &str| a.eq_ignore_ascii_case(b);
    let named = |a: &Option<String>, b: &Option<String>| matches!((a, b), (Some(a), Some(b)) if same(a, b));
    let is_hub = |l: &AccountLine| l.split.as_deref().is_some_and(|s| key(s) == "split");
    let mut used = vec![false; lines.len()];
    let mut sets: Vec<Vec<usize>> = Vec::new();

    // A line split to several accounts: the lines in those accounts that name it as their split.
    for (h, hub) in lines.iter().enumerate() {
        let Some(total) = hub.line.cents.filter(|_| is_hub(hub)) else { continue };
        let candidates: Vec<usize> = (0..lines.len())
            .filter(|&i| !used[i] && i != h && !is_hub(&lines[i]) && lines[i].split.as_deref().is_some_and(|s| same(s, &hub.line.account)))
            .collect();
        let balances = |set: &[usize]| set.iter().map(|&i| lines[i].line.cents).sum::<Option<i64>>() == Some(-total);
        let same_name: Vec<usize> = candidates.iter().copied().filter(|&i| named(&lines[i].head.name, &hub.head.name)).collect();
        let chosen = if !candidates.is_empty() && balances(&candidates) {
            candidates
        } else if !same_name.is_empty() && balances(&same_name) {
            same_name
        } else {
            continue;
        };
        let mut set = vec![h];
        set.extend(chosen);
        set.sort_unstable();
        set.iter().for_each(|&i| used[i] = true);
        sets.push(set);
    }
    // Two lines, each split to the other's account, for the same amount.
    for a in 0..lines.len() {
        let (Some(split), Some(cents)) = (lines[a].split.as_deref(), lines[a].line.cents) else { continue };
        if used[a] || is_hub(&lines[a]) {
            continue;
        }
        let fits = |b: usize| {
            b != a
                && !used[b]
                && same(&lines[b].line.account, split)
                && lines[b].split.as_deref().is_some_and(|s| same(s, &lines[a].line.account))
                && lines[b].line.cents == Some(-cents)
        };
        let partner = (0..lines.len()).find(|&b| fits(b) && named(&lines[b].head.name, &lines[a].head.name)).or_else(|| (0..lines.len()).find(|&b| fits(b)));
        if let Some(b) = partner {
            used[a] = true;
            used[b] = true;
            sets.push(if a < b { vec![a, b] } else { vec![b, a] });
        }
    }
    let rest: Vec<usize> = (0..lines.len()).filter(|&i| !used[i]).collect();
    if !rest.is_empty() {
        sets.push(rest);
    }
    sets.sort_by_key(|s| s[0]);
    sets
}

/// What a report export read held, for the wizard to say.
pub struct ReportRead {
    pub ledger: Ledger,
    /// Each file and what it was read as (`Journal.csv: QuickBooks Online Journal`).
    pub sources: Vec<String>,
    /// The file the import is named after: the report, or the first list.
    pub main: usize,
}

/// Reads report exports and lists, given as (file name, contents), into one ledger. Fails, saying
/// why, when a file is not a report LocalBooks reads, or when a Journal and a General Ledger of
/// the same books are given together (each holds every transaction).
pub fn read_files(files: &[(String, Vec<u8>)]) -> Result<ReportRead, String> {
    let mut tables: Vec<Table> = files.iter().map(|(name, bytes)| table(name, bytes)).collect::<Result<_, _>>()?;
    let reports = |k: Kind| tables.iter().filter(|t| t.kind == k).count();
    if reports(Kind::Journal) > 0 && reports(Kind::ByAccount) > 0 {
        return Err(
            "REPORT_OVERLAP: a Journal and a General Ledger both hold every transaction, so importing both would count each twice. Choose one (the Journal is the better source)"
                .into(),
        );
    }
    let sources = tables.iter().map(|t| format!("{}: {} {}", t.file, t.product, t.kind.label())).collect();
    let main = tables.iter().position(|t| matches!(t.kind, Kind::Journal | Kind::ByAccount)).unwrap_or(0);

    let mut accounts = Accounts::default();
    let mut nonposting: Vec<String> = Vec::new();
    let mut names = Names::default();
    let mut items: Vec<SourceItem> = Vec::new();
    // Lists first, so the reports find their account types.
    tables.sort_by_key(|t| match t.kind {
        Kind::Accounts => 0,
        Kind::Customers | Kind::Vendors | Kind::Items => 1,
        Kind::Journal | Kind::ByAccount => 2,
    });
    let mut balances: Vec<(i64, i64)> = Vec::new();
    let mut blocks: Vec<Block> = Vec::new();
    let mut labels: Vec<Option<&'static str>> = Vec::new();
    let mut seen_trans: HashSet<String> = HashSet::new();
    for t in &tables {
        match t.kind {
            Kind::Accounts => read_accounts(t, &mut accounts, &mut nonposting, &mut balances),
            Kind::Customers => read_parties(t, &mut names, true),
            Kind::Vendors => read_parties(t, &mut names, false),
            Kind::Items => read_items(t, &mut accounts, &mut items),
            Kind::Journal | Kind::ByAccount => {
                let groups = if t.kind == Kind::Journal { journal(t) } else { by_account(t) };
                let dates = Dates::of(groups.iter().filter_map(|g| g.date.as_deref()));
                for g in groups {
                    // Journals of overlapping periods share transactions; Trans # tells them apart.
                    if let Some(n) = &g.trans_no {
                        if !seen_trans.insert(n.clone()) {
                            continue;
                        }
                    }
                    let (block, label) = g.block(&dates);
                    blocks.push(block);
                    labels.push(label);
                }
            }
        }
    }

    let mut ledger = assemble(accounts, nonposting, names, items, Default::default(), &blocks);
    // The resolver numbers transactions by block, from 1.
    let label = |id: i64| usize::try_from(id - 1).ok().and_then(|i| labels.get(i).copied().flatten());
    for t in ledger.transactions.iter_mut().chain(ledger.unbalanced.iter_mut()) {
        if let Some(l) = label(t.id) {
            t.kind = l;
        }
    }
    for u in ledger.unreadable.iter_mut() {
        if let Some(l) = u.transaction_id.and_then(label) {
            u.kind = l;
        }
    }

    // The Account Listing's balances, to check the import against (leaf accounts only: a
    // parent's balance includes its sub-accounts').
    let parents: HashSet<i64> = ledger.accounts.iter().filter_map(|a| a.parent_id).collect();
    let mut read: HashMap<i64, i64> = HashMap::new();
    for l in ledger.transactions.iter().flat_map(|t| &t.lines) {
        *read.entry(l.account_id).or_default() += l.cents;
    }
    for (id, balance) in balances {
        if parents.contains(&id) {
            continue;
        }
        if let Some(a) = ledger.accounts.iter_mut().find(|a| a.id == id) {
            let stored = if debit_side(a.account_type) { -balance } else { balance };
            a.stored_balance_cents = Some(stored);
            ledger.quality.accounts_with_stored_balance += 1;
            if read.get(&id).copied().unwrap_or(0) == stored {
                ledger.quality.accounts_matching_stored += 1;
            }
        }
    }
    Ok(ReportRead { ledger, sources, main })
}

fn inactive(row: &[String], col: Option<usize>) -> bool {
    cell(row, col).is_some_and(|s| key(s) == "inactive")
}

fn is_total(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower == "total" || lower.starts_with("total ")
}

fn read_accounts(t: &Table, accounts: &mut Accounts, nonposting: &mut Vec<String>, balances: &mut Vec<(i64, i64)>) {
    let (name, kind, detail, balance, status) = (t.col(LIST_ACCOUNT), t.col(ACCOUNT_TYPE), t.col(DETAIL_TYPE), t.col(BALANCE), t.col(STATUS));
    for row in &t.rows {
        let (Some(raw), Some(type_name)) = (cell(row, name), cell(row, kind)) else { continue };
        if is_total(raw) {
            continue;
        }
        let path = account_path(raw);
        let account_type = match (account_type(type_name), cell(row, detail)) {
            (Some(_), Some(d)) if key(d) == "undepositedfunds" => "OTHER_CURRENT_ASSET",
            (Some(t), _) => t,
            (None, _) if key(type_name).starts_with("nonposting") => {
                nonposting.push(path.to_lowercase());
                continue;
            }
            // A type this reader does not know: the account is typed from its use instead.
            (None, _) => continue,
        };
        let id = accounts.declare(&path, account_type, !inactive(row, status));
        if balance_sheet(account_type) {
            if let Some(Ok(cents)) = cell(row, balance).map(parse_amount_cents) {
                balances.push((id, cents));
            }
        }
    }
}

fn read_parties(t: &Table, names: &mut Names, customers: bool) {
    let name = t.col(if customers { CUSTOMER } else { VENDOR });
    let (company, email, phone, address, terms, status, notes, tax_id, eligible) =
        (t.col(COMPANY), t.col(EMAIL), t.col(PHONE), t.col(BILL_TO), t.col(TERMS), t.col(STATUS), t.col(NOTES), t.col(TAX_ID), t.col(ELIGIBLE_1099));
    for row in &t.rows {
        let Some(n) = cell(row, name).filter(|n| !is_total(n)) else { continue };
        let p = if customers {
            let id = names.ensure_customer(n);
            names.customers.iter_mut().find(|p| p.id == id)
        } else {
            let id = names.ensure_vendor(n);
            names.vendors.iter_mut().find(|p| p.id == id)
        };
        let p = p.expect("just ensured");
        p.company_name = text(row, company);
        p.email = text(row, email);
        p.phone = text(row, phone);
        p.address = text(row, address).map(|a| a.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n"));
        p.terms_days = cell(row, terms).and_then(terms_days);
        p.active = !inactive(row, status);
        p.notes = text(row, notes);
        if !customers {
            p.tax_id = text(row, tax_id);
            p.is_1099 = cell(row, eligible).is_some_and(|v| matches!(key(v).as_str(), "yes" | "y" | "true" | "x" | "1" | "eligible"));
        }
    }
}

fn item_kind(name: &str) -> Option<&'static str> {
    match key(name).as_str() {
        "service" => Some("Service"),
        "inventorypart" | "inventory" | "inventoryassembly" | "assembly" => Some("Inventory part"),
        "noninventorypart" | "noninventory" => Some("Non-inventory part"),
        "othercharge" => Some("Other charge"),
        // Discounts, subtotals, groups, payments and sales tax items are not things LocalBooks sells.
        _ => None,
    }
}

fn read_items(t: &Table, accounts: &mut Accounts, items: &mut Vec<SourceItem>) {
    let (name, kind, description, price, account, tax, status) =
        (t.col(ITEM), t.col(ITEM_TYPE), t.col(ITEM_DESCRIPTION), t.col(PRICE), t.col(INCOME_ACCOUNT), t.col(TAX_CODE), t.col(STATUS));
    for row in &t.rows {
        let (Some(n), Some(kind)) = (cell(row, name).filter(|n| !is_total(n)), cell(row, kind).and_then(item_kind)) else { continue };
        items.push(SourceItem {
            id: items.len() as i64 + 1,
            name: n.to_string(),
            description: text(row, description),
            rate_cents: cell(row, price).filter(|p| !p.ends_with('%')).and_then(|p| parse_amount_cents(p).ok()).map_or(0, i64::abs),
            income_account_id: cell(row, account).map(|a| accounts.ensure(&account_path(a), "INCOME")),
            kind,
            taxable: cell(row, tax).is_some_and(|v| matches!(key(v).as_str(), "tax" | "taxable" | "yes" | "y" | "true")),
            active: !inactive(row, status),
            custom_fields: Vec::new(),
        });
    }
}

#[cfg(test)]
mod tests;
