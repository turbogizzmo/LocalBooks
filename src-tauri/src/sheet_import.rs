//! Importing a spreadsheet the user laid out (Settings › Import & Migration › From a spreadsheet):
//! customers, vendors, sales and purchase transactions or journal entries from columns the person
//! maps to fields, or from the downloadable template. Every row is checked first and the ones that would fail are listed with their row
//! numbers; only a sheet without problems becomes a [`Ledger`], which is staged and reviewed like
//! every other import (`qbw_ledger_import`) before anything posts. Transactions are handed to the IIF
//! reader's resolver as blocks of their IIF type (`INVOICE`, `CASH SALE`, `BILL`, `CHECK`,
//! `GENERAL JOURNAL`...), so they become the same documents as any other import's.
//! The design is in `docs/plans/spreadsheet-import.md`.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::bank::{detect_date_order, parse_amount_cents, parse_date_ordered, DateOrder};
use crate::iif_import::{assemble, Block, RawLine};
use crate::ledger::format_cents;
use crate::qbw_company::Ledger;
use crate::text_import::{quantity_e4, terms_days, Accounts, Names};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SheetKind {
    Customers,
    Vendors,
    Invoices,
    SalesReceipts,
    CreditMemos,
    Deposits,
    Bills,
    BillCredits,
    Checks,
    CardCharges,
    CardCredits,
    JournalEntries,
}

impl SheetKind {
    /// What the sheet holds, as the screen and the template's file name say it.
    pub fn title(self) -> &'static str {
        match self {
            SheetKind::Customers => "Customers",
            SheetKind::Vendors => "Vendors",
            SheetKind::Invoices => "Invoices",
            SheetKind::SalesReceipts => "Sales receipts",
            SheetKind::CreditMemos => "Credit memos",
            SheetKind::Deposits => "Deposits",
            SheetKind::Bills => "Bills",
            SheetKind::BillCredits => "Bill credits",
            SheetKind::Checks => "Checks",
            SheetKind::CardCharges => "Card charges",
            SheetKind::CardCredits => "Card credits",
            SheetKind::JournalEntries => "Journal entries",
        }
    }
}

/// One field a column can be mapped to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub id: &'static str,
    pub label: &'static str,
    pub required: bool,
    /// Other headings that suggest this field (compared without case, spaces or punctuation).
    #[serde(skip)]
    pub aliases: &'static [&'static str],
}

const fn field(id: &'static str, label: &'static str, required: bool, aliases: &'static [&'static str]) -> Field {
    Field { id, label, required, aliases }
}

const NAME: Field = field("name", "Name", true, &["customer", "customername", "vendor", "vendorname", "displayname", "fullname"]);
const COMPANY: Field = field("company", "Company", false, &["companyname", "business"]);
const EMAIL: Field = field("email", "Email", false, &["emailaddress", "mail"]);
const PHONE: Field = field("phone", "Phone", false, &["telephone", "phonenumber", "tel", "mobile"]);
const TERMS: Field = field("terms", "Terms", false, &["paymentterms"]);
const NOTES: Field = field("notes", "Notes", false, &["note", "comments", "memo"]);
const ACTIVE: Field = field("active", "Active", false, &["status", "isactive"]);
const DATE: Field = field("date", "Date", true, &["txndate", "transactiondate", "invoicedate", "issuedate", "billdate", "checkdate", "entrydate"]);
const NUMBER_ALIASES: &[&str] = &["number", "num", "no", "docnum", "refno", "ref", "reference", "invoice", "invoiceno", "invoicenumber", "inv", "invno", "receiptno", "billno", "checkno", "checknumber", "creditno"];

/// Who a transaction is with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Party {
    Customer,
    Vendor,
    /// A check's or card charge's payee: a vendor, created if new.
    Payee,
}

impl Party {
    const fn field(self, required: bool) -> Field {
        match self {
            Party::Customer => field("customer", "Customer", required, &["customername", "name", "billto", "soldto", "receivedfrom"]),
            Party::Vendor => field("vendor", "Vendor", required, &["vendorname", "name", "supplier", "payee"]),
            Party::Payee => field("payee", "Payee", required, &["name", "paidto", "vendor", "merchant"]),
        }
    }
}

/// The account a transaction's total posts to.
#[derive(Debug, Clone, Copy)]
enum Header {
    /// Always this account, of this type.
    Fixed(&'static str, &'static str),
    /// The account a column names, of this type; `default` (an Undeposited Funds account) when
    /// the column is empty or not in the sheet, or required when there is no default.
    Column { label: &'static str, account_type: &'static str, default: Option<&'static str>, aliases: &'static [&'static str] },
}

/// How one kind of transaction is laid out: its IIF type, who it is with, the account its total
/// posts to and the accounts its lines post to.
#[derive(Debug, Clone, Copy)]
struct Doc {
    trnstype: &'static str,
    /// "invoice", for messages.
    noun: &'static str,
    number_label: &'static str,
    number_required: bool,
    party: Option<(Party, bool)>,
    header: Header,
    line_label: &'static str,
    line_type: &'static str,
    /// 1 when the total is a debit to the header account (an invoice's A/R), -1 when a credit.
    sign: i64,
    due: bool,
    method: bool,
}

const BANK_ALIASES: &[&str] = &["bank", "bankaccount", "account", "depositto", "paidfrom", "fromaccount", "checking"];

fn doc(kind: SheetKind) -> Option<Doc> {
    let sales = |trnstype, noun, number_label, number_required, party, header, sign| Doc {
        trnstype,
        noun,
        number_label,
        number_required,
        party,
        header,
        line_label: "Income account",
        line_type: "INCOME",
        sign,
        due: false,
        method: false,
    };
    let purchase = |trnstype, noun, number_label, party, header, sign| Doc {
        trnstype,
        noun,
        number_label,
        number_required: false,
        party,
        header,
        line_label: "Expense account",
        line_type: "EXPENSE",
        sign,
        due: false,
        method: false,
    };
    let ar = Header::Fixed("Accounts Receivable", "ACCOUNTS_RECEIVABLE");
    let ap = Header::Fixed("Accounts Payable", "ACCOUNTS_PAYABLE");
    let bank = |label| Header::Column { label, account_type: "BANK", default: None, aliases: BANK_ALIASES };
    let card = Header::Column { label: "Card account", account_type: "CREDIT_CARD", default: None, aliases: &["card", "creditcard", "account", "cardname", "paidwith"] };
    Some(match kind {
        SheetKind::Customers | SheetKind::Vendors | SheetKind::JournalEntries => return None,
        SheetKind::Invoices => Doc { due: true, ..sales("INVOICE", "invoice", "Invoice no.", true, Some((Party::Customer, true)), ar, 1) },
        SheetKind::SalesReceipts => Doc {
            method: true,
            ..sales(
                "CASH SALE",
                "sales receipt",
                "Receipt no.",
                false,
                Some((Party::Customer, false)),
                Header::Column { label: "Deposit to", account_type: "BANK", default: Some("Undeposited Funds"), aliases: BANK_ALIASES },
                1,
            )
        },
        SheetKind::CreditMemos => sales("CREDIT MEMO", "credit memo", "Credit no.", true, Some((Party::Customer, true)), ar, -1),
        SheetKind::Deposits => Doc { line_label: "From account", ..sales("DEPOSIT", "deposit", "Deposit no.", false, None, bank("Bank account"), 1) },
        SheetKind::Bills => Doc { due: true, ..purchase("BILL", "bill", "Bill no.", Some((Party::Vendor, true)), ap, -1) },
        SheetKind::BillCredits => purchase("BILL REFUND", "bill credit", "Credit no.", Some((Party::Vendor, true)), ap, 1),
        SheetKind::Checks => purchase("CHECK", "check", "Check no.", Some((Party::Payee, false)), bank("Bank account"), -1),
        SheetKind::CardCharges => purchase("CREDIT CARD", "card charge", "Ref no.", Some((Party::Payee, false)), card, -1),
        SheetKind::CardCredits => purchase("CCARD REFUND", "card credit", "Ref no.", Some((Party::Payee, false)), card, 1),
    })
}

/// The fields a kind of sheet takes, in the order the template has them.
pub fn fields(kind: SheetKind) -> Vec<Field> {
    match kind {
        SheetKind::Customers => vec![NAME, COMPANY, EMAIL, PHONE, field("address", "Billing address", false, &["address", "billto", "billingaddress"]), TERMS, NOTES, ACTIVE],
        SheetKind::Vendors => vec![
            NAME,
            COMPANY,
            EMAIL,
            PHONE,
            field("address", "Address", false, &["vendoraddress", "remitto"]),
            TERMS,
            field("taxId", "Tax ID", false, &["ein", "taxid", "ssn", "tin"]),
            field("is1099", "1099", false, &["eligible1099", "vendor1099", "1099vendor"]),
            NOTES,
            ACTIVE,
        ],
        SheetKind::JournalEntries => vec![
            field("number", "Entry no.", true, &["entry", "entrynumber", "journalno", "jeno", "je", "number", "num", "refno", "ref"]),
            DATE,
            field("account", "Account", true, &["accountname", "glaccount", "fullname"]),
            field("debit", "Debit", false, &["dr", "debitamount"]),
            field("credit", "Credit", false, &["cr", "creditamount"]),
            field("memo", "Memo", false, &["description", "desc", "lineitem"]),
            field("name", "Name", false, &["customer", "vendor", "payee", "customervendor"]),
        ],
        _ => {
            let d = doc(kind).expect("a transaction kind");
            let mut out = vec![field("number", d.number_label, d.number_required, NUMBER_ALIASES), DATE];
            if let Some((party, required)) = d.party {
                out.push(party.field(required));
            }
            if let Header::Column { label, default, aliases, .. } = d.header {
                out.push(field("bank", label, default.is_none(), aliases));
            }
            if d.due {
                out.push(field("dueDate", "Due date", false, &["due", "duedate"]));
            }
            if d.method {
                out.push(field("method", "Payment method", false, &["paymentmethod", "paidby", "method", "paymenttype"]));
            }
            out.extend([
                field("item", "Item", false, &["product", "productservice", "service", "itemname"]),
                field("description", "Description", false, &["desc", "lineitem", "linedescription"]),
                field("quantity", "Quantity", false, &["qty"]),
                field("rate", "Rate", false, &["price", "unitprice", "salesprice", "cost", "unitcost"]),
                field("amount", "Amount", false, &["lineamount", "total", "linetotal"]),
                field("account", d.line_label, true, &["account", "incomeaccount", "salesaccount", "expenseaccount", "category", "fromaccount"]),
                field("memo", "Memo", false, &["message", "note"]),
            ]);
            out
        }
    }
}

fn key(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

/// A field id → column index mapping.
pub type Mapping = HashMap<String, usize>;

/// The mapping headings suggest: each field to the first heading that matches its label, id or an
/// alias; no heading used twice.
pub fn suggest(kind: SheetKind, headers: &[String]) -> Mapping {
    let keys: Vec<String> = headers.iter().map(|h| key(h)).collect();
    let mut used = HashSet::new();
    let mut out = Mapping::new();
    // Exact label or id first for every field, then aliases, so "Name" isn't taken by a
    // field whose alias is "name" before the Name field claims it.
    for pass in 0..2 {
        for f in fields(kind) {
            if out.contains_key(f.id) {
                continue;
            }
            let wanted: Vec<String> = if pass == 0 { vec![key(f.label), key(f.id)] } else { f.aliases.iter().map(|a| key(a)).collect() };
            if let Some(i) = keys.iter().enumerate().position(|(i, k)| !k.is_empty() && !used.contains(&i) && wanted.contains(k)) {
                used.insert(i);
                out.insert(f.id.to_string(), i);
            }
        }
    }
    out
}

/// The sheet: its headings, and the rows under them with their row numbers in the file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sheet {
    pub headers: Vec<String>,
    /// (row number in the spreadsheet, cells)
    pub rows: Vec<(usize, Vec<String>)>,
}

/// Reads the first sheet of a CSV or workbook: the heading row is the first with two or more
/// non-empty cells; blank rows after it are skipped.
pub fn read(file: &str, bytes: &[u8]) -> Result<Sheet, String> {
    // Each row with its row number in the spreadsheet. A workbook's rows include its blank ones;
    // the CSV reader skips blank lines, so its rows take their line numbers from the reader.
    let numbered: Vec<(usize, Vec<String>)> = if is_text(file) {
        let text = crate::text_import::decode(bytes);
        let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default();
        let delimiter = if first.matches('\t').count() > first.matches(',').count() { b'\t' } else { b',' };
        let mut reader = csv::ReaderBuilder::new().has_headers(false).flexible(true).delimiter(delimiter).from_reader(text.as_bytes());
        let mut rows = Vec::new();
        let mut record = csv::StringRecord::new();
        loop {
            let line = reader.position().line();
            match reader.read_record(&mut record) {
                Ok(false) => break,
                Ok(true) => rows.push((usize::try_from(record.position().map_or(line, |p| p.line())).unwrap_or(0), record.iter().map(|f| f.trim().to_string()).collect())),
                Err(e) => return Err(format!("SHEET_UNREADABLE: {file}: {e}")),
            }
        }
        rows
    } else {
        let grid = crate::report_import::grids(file, bytes)?.into_iter().next().unwrap_or_default();
        grid.into_iter().enumerate().map(|(i, r)| (i + 1, r)).collect()
    };
    let Some(head) = numbered.iter().position(|(_, r)| r.iter().filter(|c| !c.trim().is_empty()).count() >= 2) else {
        return Err("SHEET_EMPTY: the spreadsheet has no row of column headings".into());
    };
    let headers = numbered[head].1.iter().map(|c| c.trim().to_string()).collect();
    let rows = numbered.into_iter().skip(head + 1).filter(|(_, r)| r.iter().any(|c| !c.trim().is_empty())).collect();
    Ok(Sheet { headers, rows })
}

fn is_text(file: &str) -> bool {
    let ext = std::path::Path::new(file).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    ext == "csv" || ext == "txt"
}

/// The file kinds a spreadsheet import reads.
pub const EXTENSIONS: &[&str] = &["csv", "txt", "xlsx", "xls", "xlsm", "ods"];

/// A row that would fail, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub row: usize,
    pub message: String,
}

/// The result of checking a sheet: how many records it holds, and the problems (none means it can
/// be staged).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub rows: usize,
    /// Customers or vendors, or transactions.
    pub records: usize,
    /// The total of the transactions (a journal entry's debits), in cents.
    pub total_cents: i64,
    pub problems: Vec<Problem>,
}

struct Rows<'a> {
    mapping: &'a Mapping,
}

impl<'a> Rows<'a> {
    fn get(&self, row: &'a [String], field: &str) -> Option<&'a str> {
        let i = *self.mapping.get(field)?;
        row.get(i).map(|c| c.trim()).filter(|c| !c.is_empty())
    }
}

fn yes(value: &str) -> Option<bool> {
    match key(value).as_str() {
        "yes" | "y" | "true" | "1" | "x" | "active" | "eligible" => Some(true),
        "no" | "n" | "false" | "0" | "inactive" | "" => Some(false),
        _ => None,
    }
}

/// The date order to read the sheet's dates with: the one asked for, or what the column's values
/// make unambiguous.
fn date_order(sheet: &Sheet, mapping: &Mapping, asked: Option<DateOrder>) -> DateOrder {
    asked.unwrap_or_else(|| {
        let col = mapping.get("date").copied();
        detect_date_order(sheet.rows.iter().filter_map(|(_, r)| col.and_then(|c| r.get(c)).map(String::as_str)).filter(|v| !v.trim().is_empty())).0
    })
}

/// One line of a transaction as read.
struct Line {
    row: usize,
    /// The transaction's number, if the row gives one.
    number: Option<String>,
    /// Rows with the same key are one transaction: the number, or the row for a row without one.
    key: String,
    date: NaiveDate,
    party: Option<String>,
    /// The account the transaction's total posts to (a document's bank or card account).
    header: Option<String>,
    due: Option<NaiveDate>,
    method: Option<String>,
    item: Option<String>,
    description: Option<String>,
    quantity_e4: Option<i64>,
    rate_cents: Option<i64>,
    /// A document line: more than zero. A journal line: the debit less the credit.
    amount_cents: i64,
    account: String,
    memo: Option<String>,
}

/// Every problem in the sheet, and what it would import.
pub fn check(kind: SheetKind, sheet: &Sheet, mapping: &Mapping, order: Option<DateOrder>) -> Check {
    let (check, _) = read_rows(kind, sheet, mapping, order);
    check
}

fn missing_mapping(kind: SheetKind, mapping: &Mapping) -> Vec<Problem> {
    let mut out: Vec<Problem> = fields(kind)
        .into_iter()
        .filter(|f| f.required && !mapping.contains_key(f.id))
        .map(|f| Problem { row: 0, message: format!("Choose the column that holds {}", f.label) })
        .collect();
    if kind == SheetKind::JournalEntries && !mapping.contains_key("debit") && !mapping.contains_key("credit") {
        out.push(Problem { row: 0, message: "Choose the columns that hold Debit and Credit".into() });
    }
    out
}

fn capitalized(noun: &str) -> String {
    let mut c = noun.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// A number column's value: `None` when empty, `Some(None)` when it doesn't read.
fn amount_of(r: &Rows, cells: &[String], f: &str, problems: &mut Vec<Problem>, row: usize, label: &str) -> Option<i64> {
    let raw = r.get(cells, f)?;
    match parse_amount_cents(raw) {
        Ok(v) => Some(v),
        Err(_) => {
            problems.push(Problem { row, message: format!("The {label} \"{raw}\" doesn't read as a number") });
            None
        }
    }
}

fn read_rows(kind: SheetKind, sheet: &Sheet, mapping: &Mapping, order: Option<DateOrder>) -> (Check, Vec<Line>) {
    let mut problems = missing_mapping(kind, mapping);
    if !problems.is_empty() {
        return (Check { rows: sheet.rows.len(), records: 0, total_cents: 0, problems }, Vec::new());
    }
    let r = Rows { mapping };
    let mut lines = Vec::new();
    let mut records = 0;
    let mut total: i64 = 0;
    match kind {
        SheetKind::Customers | SheetKind::Vendors => {
            let mut seen: HashMap<String, usize> = HashMap::new();
            for (row, cells) in &sheet.rows {
                let Some(name) = r.get(cells, "name") else {
                    problems.push(Problem { row: *row, message: "The name is empty".into() });
                    continue;
                };
                if let Some(first) = seen.insert(name.to_lowercase(), *row) {
                    problems.push(Problem { row: *row, message: format!("{name} is also on row {first}") });
                }
                if let Some(t) = r.get(cells, "terms").filter(|t| terms_days(t).is_none()) {
                    problems.push(Problem { row: *row, message: format!("The terms \"{t}\" don't read as days (write Net 30 or 30)") });
                }
                for f in ["active", "is1099"] {
                    if let Some(v) = r.get(cells, f).filter(|v| yes(v).is_none()) {
                        problems.push(Problem { row: *row, message: format!("\"{v}\" isn't yes or no") });
                    }
                }
                if let Some(e) = r.get(cells, "email").filter(|e| !e.contains('@')) {
                    problems.push(Problem { row: *row, message: format!("\"{e}\" isn't an email address") });
                }
                records += 1;
            }
        }
        SheetKind::JournalEntries => {
            let order = date_order(sheet, mapping, order);
            for (row, cells) in &sheet.rows {
                let row = *row;
                let number = r.get(cells, "number");
                let when = r.get(cells, "date");
                let account = r.get(cells, "account");
                if number.is_none() {
                    problems.push(Problem { row, message: "The entry number is empty".into() });
                }
                if account.is_none() {
                    problems.push(Problem { row, message: "The account is empty".into() });
                }
                let date = read_date(when, order, "date", row, &mut problems);
                let debit = amount_of(&r, cells, "debit", &mut problems, row, "debit");
                let credit = amount_of(&r, cells, "credit", &mut problems, row, "credit");
                let (d, c) = (debit.unwrap_or(0), credit.unwrap_or(0));
                let unreadable = r.get(cells, "debit").is_some() && debit.is_none() || r.get(cells, "credit").is_some() && credit.is_none();
                let amount = if unreadable {
                    None
                } else if d < 0 || c < 0 {
                    problems.push(Problem { row, message: "Write debits and credits as amounts more than zero".into() });
                    None
                } else if d > 0 && c > 0 {
                    problems.push(Problem { row, message: "Give a debit or a credit on a row, not both".into() });
                    None
                } else if d == 0 && c == 0 {
                    problems.push(Problem { row, message: "Give the line's debit or credit".into() });
                    None
                } else {
                    Some(d - c)
                };
                if let (Some(number), Some(date), Some(account), Some(amount_cents)) = (number, date, account, amount) {
                    lines.push(Line {
                        row,
                        number: Some(number.to_string()),
                        key: number.to_lowercase(),
                        date,
                        party: r.get(cells, "name").map(String::from),
                        header: None,
                        due: None,
                        method: None,
                        item: None,
                        description: None,
                        quantity_e4: None,
                        rate_cents: None,
                        amount_cents,
                        account: account.to_string(),
                        memo: r.get(cells, "memo").map(String::from),
                    });
                }
            }
            // Each entry's rows agree on the date, and its debits equal its credits.
            let mut entries: Vec<(&Line, i64, i64)> = Vec::new();
            for l in &lines {
                match entries.iter_mut().find(|e| e.0.key == l.key) {
                    None => entries.push((l, l.amount_cents.max(0), (-l.amount_cents).max(0))),
                    Some(e) => {
                        if e.0.date != l.date {
                            problems.push(Problem { row: l.row, message: format!("Entry {} is dated {} on row {}", e.0.number.as_deref().unwrap_or_default(), e.0.date.format("%m/%d/%Y"), e.0.row) });
                        }
                        match (e.1.checked_add(l.amount_cents.max(0)), e.2.checked_add((-l.amount_cents).max(0))) {
                            (Some(dr), Some(cr)) => (e.1, e.2) = (dr, cr),
                            _ => problems.push(Problem { row: l.row, message: "The entries add up to more than LocalBooks can hold".into() }),
                        }
                    }
                }
            }
            for (first, debits, credits) in &entries {
                if debits != credits {
                    problems.push(Problem {
                        row: first.row,
                        message: format!("Entry {} doesn't balance: debits {}, credits {}", first.number.as_deref().unwrap_or_default(), format_cents(*debits), format_cents(*credits)),
                    });
                }
                match total.checked_add(*debits) {
                    Some(t) => total = t,
                    None => problems.push(Problem { row: first.row, message: "The entries add up to more than LocalBooks can hold".into() }),
                }
            }
            records = entries.len();
        }
        _ => {
            let d = doc(kind).expect("a transaction kind");
            let order = date_order(sheet, mapping, order);
            let party_label = d.party.map(|(p, _)| p.field(false).label.to_lowercase());
            for (row, cells) in &sheet.rows {
                let row = *row;
                let number = r.get(cells, "number");
                let party = d.party.and_then(|(p, _)| r.get(cells, p.field(false).id));
                let header = match d.header {
                    Header::Fixed(name, _) => Some(name),
                    Header::Column { default, .. } => r.get(cells, "bank").or(default),
                };
                let account = r.get(cells, "account");
                if d.number_required && number.is_none() {
                    problems.push(Problem { row, message: format!("The {} number is empty", d.noun) });
                }
                if let (Some((_, true)), None) = (d.party, party) {
                    problems.push(Problem { row, message: format!("The {} is empty", party_label.as_deref().unwrap_or_default()) });
                }
                if let (Header::Column { label, .. }, None) = (d.header, header) {
                    problems.push(Problem { row, message: format!("The {} is empty", label.to_lowercase()) });
                }
                if account.is_none() {
                    problems.push(Problem { row, message: format!("The {} is empty", d.line_label.to_lowercase()) });
                }
                let date = read_date(r.get(cells, "date"), order, "date", row, &mut problems);
                let due_raw = r.get(cells, "dueDate");
                let due = due_raw.and_then(|v| read_date(Some(v), order, "due date", row, &mut problems));
                let quantity = r.get(cells, "quantity").map(|q| (q, quantity_e4(q)));
                if let Some((raw, None)) = quantity {
                    problems.push(Problem { row, message: format!("The quantity \"{raw}\" doesn't read as a number") });
                }
                let rate = amount_of(&r, cells, "rate", &mut problems, row, "rate");
                let amount = amount_of(&r, cells, "amount", &mut problems, row, "amount");
                let q = quantity.and_then(|q| q.1);
                let from_rate = q.zip(rate).and_then(|(q, p)| crate::ledger::mul_div_round(q, p, crate::ledger::QUANTITY_SCALE));
                let amount_cents = match (amount, from_rate) {
                    (Some(a), Some(c)) if (a - c).abs() > 1 => {
                        problems.push(Problem { row, message: format!("The amount {} isn't the quantity times the rate ({})", format_cents(a), format_cents(c)) });
                        None
                    }
                    (Some(a), _) => Some(a),
                    (None, Some(c)) => Some(c),
                    (None, None) if r.get(cells, "amount").is_none() => {
                        problems.push(Problem { row, message: "Give the line's amount, or its quantity and rate".into() });
                        None
                    }
                    _ => None,
                };
                if amount_cents.is_some_and(|a| a <= 0) {
                    problems.push(Problem { row, message: format!("{} line's amount must be more than zero", capitalized(&format!("a {}", d.noun)).replace("A i", "An i")) });
                }
                if let (Some(date), Some(account), Some(amount_cents)) = (date, account, amount_cents.filter(|a| *a > 0)) {
                    if d.number_required && number.is_none() || d.party.is_some_and(|(_, req)| req) && party.is_none() || header.is_none() {
                        continue;
                    }
                    lines.push(Line {
                        row,
                        number: number.map(String::from),
                        key: number.map_or_else(|| format!("\u{0}{row}"), str::to_lowercase),
                        date,
                        party: party.map(String::from),
                        header: header.map(String::from),
                        due,
                        method: r.get(cells, "method").map(String::from),
                        item: r.get(cells, "item").map(String::from),
                        description: r.get(cells, "description").map(String::from),
                        quantity_e4: q,
                        rate_cents: rate,
                        amount_cents,
                        account: account.to_string(),
                        memo: r.get(cells, "memo").map(String::from),
                    });
                }
            }
            // A transaction's rows must agree on who it is with, its date and its account.
            let noun = capitalized(d.noun);
            let mut first: HashMap<&str, &Line> = HashMap::new();
            for l in &lines {
                let number = l.number.as_deref().unwrap_or_default();
                match first.get(l.key.as_str()) {
                    None => {
                        first.insert(&l.key, l);
                        records += 1;
                    }
                    Some(f) if !f.party.as_deref().unwrap_or_default().eq_ignore_ascii_case(l.party.as_deref().unwrap_or_default()) => problems.push(Problem {
                        row: l.row,
                        message: format!("{noun} {number} is for {} on row {}, not {}", f.party.as_deref().unwrap_or("no one"), f.row, l.party.as_deref().unwrap_or("no one")),
                    }),
                    Some(f) if f.date != l.date => problems.push(Problem { row: l.row, message: format!("{noun} {number} is dated {} on row {}", f.date.format("%m/%d/%Y"), f.row) }),
                    Some(f) if !f.header.as_deref().unwrap_or_default().eq_ignore_ascii_case(l.header.as_deref().unwrap_or_default()) => problems.push(Problem {
                        row: l.row,
                        message: format!("{noun} {number} uses {} on row {}, not {}", f.header.as_deref().unwrap_or_default(), f.row, l.header.as_deref().unwrap_or_default()),
                    }),
                    Some(_) => {}
                }
                match total.checked_add(l.amount_cents) {
                    Some(t) => total = t,
                    None => problems.push(Problem { row: l.row, message: format!("The {}s add up to more than LocalBooks can hold", d.noun) }),
                }
            }
        }
    }
    problems.sort_by_key(|p| p.row);
    (Check { rows: sheet.rows.len(), records, total_cents: total, problems }, lines)
}

/// A date column's value, or a problem saying it is empty or doesn't read.
fn read_date(raw: Option<&str>, order: DateOrder, label: &str, row: usize, problems: &mut Vec<Problem>) -> Option<NaiveDate> {
    let Some(raw) = raw else {
        problems.push(Problem { row, message: format!("The {label} is empty") });
        return None;
    };
    let parsed = parse_date_ordered(raw, order).ok();
    if parsed.is_none() {
        problems.push(Problem { row, message: format!("The {label} \"{raw}\" doesn't read") });
    }
    parsed
}

/// The lines in sheet order, grouped by transaction in the order each first appears.
fn grouped(lines: &[Line]) -> Vec<Vec<&Line>> {
    let mut order: Vec<&str> = Vec::new();
    let mut by_key: HashMap<&str, Vec<&Line>> = HashMap::new();
    for l in lines {
        if !by_key.contains_key(l.key.as_str()) {
            order.push(&l.key);
        }
        by_key.entry(&l.key).or_default().push(l);
    }
    order.into_iter().filter_map(|k| by_key.remove(k)).collect()
}

fn iso(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

/// A quantity as an IIF file writes it: with the sign of its line's amount.
fn signed_quantity(q: i64, negative: bool) -> String {
    format!("{}{}", if negative { "-" } else { "" }, crate::forms::format_quantity(q).replace(',', ""))
}

/// The sheet as a [`Ledger`] to stage, or the problems that stop it.
pub fn to_ledger(kind: SheetKind, sheet: &Sheet, mapping: &Mapping, order: Option<DateOrder>) -> Result<Ledger, Check> {
    let (check, lines) = read_rows(kind, sheet, mapping, order);
    if !check.problems.is_empty() {
        return Err(check);
    }
    let r = Rows { mapping };
    let mut accounts = Accounts::default();
    let mut names = Names::default();
    let mut blocks: Vec<Block> = Vec::new();
    match kind {
        SheetKind::Customers | SheetKind::Vendors => {
            for (_, cells) in &sheet.rows {
                let Some(name) = r.get(cells, "name") else { continue };
                let id = if kind == SheetKind::Customers { names.ensure_customer(name) } else { names.ensure_vendor(name) };
                let list = if kind == SheetKind::Customers { &mut names.customers } else { &mut names.vendors };
                let p = list.iter_mut().find(|p| p.id == id).expect("just ensured");
                p.company_name = r.get(cells, "company").map(String::from);
                p.email = r.get(cells, "email").map(String::from);
                p.phone = r.get(cells, "phone").map(String::from);
                p.address = r.get(cells, "address").map(|a| a.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n"));
                p.terms_days = r.get(cells, "terms").and_then(terms_days);
                p.notes = r.get(cells, "notes").map(String::from);
                p.active = r.get(cells, "active").and_then(yes).unwrap_or(true);
                if kind == SheetKind::Vendors {
                    p.tax_id = r.get(cells, "taxId").map(String::from);
                    p.is_1099 = r.get(cells, "is1099").and_then(yes).unwrap_or(false);
                }
            }
        }
        SheetKind::JournalEntries => {
            // Accounts the books don't have take a type from their name in the resolver; the
            // review matches each one to an account of the books.
            for ls in grouped(&lines) {
                let raw = |l: &Line| RawLine {
                    trnstype: "GENERAL JOURNAL".into(),
                    date: Some(iso(l.date)),
                    account: Some(l.account.clone()),
                    name: l.party.clone(),
                    amount: Some(crate::export_engine::plain_amount(l.amount_cents)),
                    doc_num: l.number.clone(),
                    memo: l.memo.clone(),
                    ..Default::default()
                };
                blocks.push(Block { trns: raw(ls[0]), splits: ls[1..].iter().map(|l| raw(l)).collect(), file_line: ls[0].row });
            }
        }
        _ => {
            let d = doc(kind).expect("a transaction kind");
            if let Header::Fixed(name, account_type) = d.header {
                accounts.declare(name, account_type, true);
            }
            for l in &lines {
                if let (Header::Column { account_type, default, .. }, Some(header)) = (d.header, l.header.as_deref()) {
                    accounts.ensure(header, if Some(header) == default { "OTHER_CURRENT_ASSET" } else { account_type });
                }
                accounts.ensure(&l.account, d.line_type);
                match (d.party, l.party.as_deref()) {
                    (Some((Party::Customer, _)), Some(p)) => {
                        names.ensure_customer(p);
                    }
                    (Some((Party::Vendor | Party::Payee, _)), Some(p)) => {
                        names.ensure_vendor(p);
                    }
                    _ => {}
                }
            }
            for ls in grouped(&lines) {
                let head = ls[0];
                // Checked when the rows were read: the transactions' sum fits, so each one's does.
                let total = ls.iter().try_fold(0_i64, |sum, l| sum.checked_add(l.amount_cents)).unwrap_or(0);
                let trns = RawLine {
                    trnstype: d.trnstype.into(),
                    date: Some(iso(head.date)),
                    account: head.header.clone(),
                    name: head.party.clone(),
                    amount: Some(crate::export_engine::plain_amount(d.sign * total)),
                    doc_num: head.number.clone(),
                    memo: ls.iter().find_map(|l| l.memo.clone()),
                    due_date: ls.iter().find_map(|l| l.due).map(iso),
                    pay_method: ls.iter().find_map(|l| l.method.clone()),
                    ..Default::default()
                };
                let splits = ls
                    .iter()
                    .map(|l| RawLine {
                        trnstype: d.trnstype.into(),
                        date: Some(iso(l.date)),
                        account: Some(l.account.clone()),
                        name: l.party.clone(),
                        amount: Some(crate::export_engine::plain_amount(-d.sign * l.amount_cents)),
                        doc_num: l.number.clone(),
                        memo: l.description.clone().or_else(|| l.item.clone()),
                        quantity: l.quantity_e4.map(|q| signed_quantity(q, d.sign > 0)),
                        price: l.rate_cents.map(crate::export_engine::plain_amount),
                        item: l.item.clone(),
                        ..Default::default()
                    })
                    .collect();
                blocks.push(Block { trns, splits, file_line: head.row });
            }
        }
    }
    let mut ledger = assemble(accounts, Vec::new(), names, Vec::new(), Default::default(), &blocks);
    ledger.add_classes_used();
    Ok(ledger)
}

/// The template's example rows: a value for each field, by id.
fn example(kind: SheetKind) -> Vec<Vec<(&'static str, &'static str)>> {
    let d = doc(kind);
    let sales = d.is_some_and(|d| d.line_type == "INCOME");
    let line: Vec<(&str, &str)> = vec![
        ("number", if kind == SheetKind::Invoices { "1001" } else { "101" }),
        ("date", "03/01/2026"),
        ("customer", "Rivera Landscaping"),
        ("vendor", "Paper Supply Co"),
        ("payee", "Paper Supply Co"),
        ("bank", match kind {
            SheetKind::CardCharges | SheetKind::CardCredits => "Visa",
            SheetKind::SalesReceipts => "Undeposited Funds",
            _ => "Checking",
        }),
        ("dueDate", "03/31/2026"),
        ("method", "Check"),
        ("item", if sales { "Lawn care" } else { "" }),
        ("description", match kind {
            SheetKind::Deposits => "Interest",
            _ if sales => "March mowing",
            _ => "Printer paper",
        }),
        ("quantity", "4"),
        ("rate", if sales { "60.00" } else { "12.50" }),
        ("amount", if sales { "240.00" } else { "50.00" }),
        ("account", match kind {
            SheetKind::Deposits => "Interest Income",
            _ if sales => "Sales",
            _ => "Office Supplies",
        }),
        ("memo", ""),
    ];
    match kind {
        SheetKind::Customers => vec![vec![
            ("name", "Rivera Landscaping"),
            ("company", "Rivera Landscaping LLC"),
            ("email", "jo@rivera.example"),
            ("phone", "555-0100"),
            ("address", "412 Orchard Lane\nSpringfield, OR 97477"),
            ("terms", "Net 30"),
            ("active", "Yes"),
        ]],
        SheetKind::Vendors => vec![vec![
            ("name", "Paper Supply Co"),
            ("email", "orders@paper.example"),
            ("phone", "555-0199"),
            ("address", "9 Mill Road\nSalem, OR 97301"),
            ("terms", "Net 15"),
            ("taxId", "12-3456789"),
            ("is1099", "No"),
            ("active", "Yes"),
        ]],
        SheetKind::JournalEntries => vec![
            vec![("number", "JE-1"), ("date", "03/31/2026"), ("account", "Depreciation Expense"), ("debit", "125.00"), ("memo", "March depreciation")],
            vec![("number", "JE-1"), ("date", "03/31/2026"), ("account", "Accumulated Depreciation"), ("credit", "125.00"), ("memo", "March depreciation")],
        ],
        _ => vec![line],
    }
}

/// The downloadable template: the fields as headings and example rows, as CSV.
pub fn template(kind: SheetKind) -> String {
    let fields = fields(kind);
    let mut out = csv::Writer::from_writer(Vec::new());
    let _ = out.write_record(fields.iter().map(|f| f.label));
    for row in example(kind) {
        let _ = out.write_record(fields.iter().map(|f| row.iter().find(|(id, _)| *id == f.id).map_or("", |(_, v)| *v)));
    }
    String::from_utf8(out.into_inner().unwrap_or_default()).unwrap_or_default()
}

#[cfg(test)]
mod tests;
