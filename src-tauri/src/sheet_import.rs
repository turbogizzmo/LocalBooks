//! Importing a spreadsheet the user laid out (Settings › Import & Migration › From a spreadsheet):
//! customers, vendors or invoices from columns the person maps to fields, or from the downloadable
//! template. Every row is checked first and the ones that would fail are listed with their row
//! numbers; only a sheet without problems becomes a [`Ledger`], which is staged and reviewed like
//! every other import (`qbw_ledger_import`) before anything posts. Invoices are handed to the IIF
//! reader's resolver as `INVOICE` blocks, so they become the same documents as any other import's.
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
        SheetKind::Invoices => vec![
            field("number", "Invoice no.", true, &["invoice", "invoicenumber", "invoiceno", "inv", "invno", "num", "number", "docnum"]),
            field("date", "Date", true, &["invoicedate", "txndate", "issuedate"]),
            field("customer", "Customer", true, &["customername", "name", "billto"]),
            field("dueDate", "Due date", false, &["due", "duedate"]),
            field("item", "Item", false, &["product", "productservice", "service", "itemname"]),
            field("description", "Description", false, &["desc", "lineitem", "linedescription"]),
            field("quantity", "Quantity", false, &["qty"]),
            field("rate", "Rate", false, &["price", "unitprice", "salesprice"]),
            field("amount", "Amount", false, &["lineamount", "total", "linetotal"]),
            field("account", "Income account", true, &["account", "incomeaccount", "salesaccount"]),
            field("memo", "Memo", false, &["message", "invoicememo"]),
        ],
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
    /// Customers or vendors, or invoices.
    pub records: usize,
    /// The total of the invoices, in cents.
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

/// One invoice line as read.
struct Line {
    row: usize,
    number: String,
    date: NaiveDate,
    customer: String,
    due: Option<NaiveDate>,
    item: Option<String>,
    description: Option<String>,
    quantity_e4: Option<i64>,
    rate_cents: Option<i64>,
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
    fields(kind)
        .into_iter()
        .filter(|f| f.required && !mapping.contains_key(f.id))
        .map(|f| Problem { row: 0, message: format!("Choose the column that holds {}", f.label) })
        .collect()
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
        SheetKind::Invoices => {
            let order = date_order(sheet, mapping, order);
            let date = |v: &str| parse_date_ordered(v, order).ok();
            for (row, cells) in &sheet.rows {
                let row = *row;
                let mut bad = |m: String| problems.push(Problem { row, message: m });
                let number = r.get(cells, "number");
                let when = r.get(cells, "date");
                let customer = r.get(cells, "customer");
                let account = r.get(cells, "account");
                if number.is_none() {
                    bad("The invoice number is empty".into());
                }
                if customer.is_none() {
                    bad("The customer is empty".into());
                }
                if account.is_none() {
                    bad("The income account is empty".into());
                }
                let parsed_date = when.and_then(date);
                if when.is_none() {
                    bad("The date is empty".into());
                } else if parsed_date.is_none() {
                    bad(format!("The date \"{}\" doesn't read", when.unwrap_or_default()));
                }
                let due = r.get(cells, "dueDate");
                let parsed_due = due.and_then(date);
                if due.is_some() && parsed_due.is_none() {
                    bad(format!("The due date \"{}\" doesn't read", due.unwrap_or_default()));
                }
                let quantity = r.get(cells, "quantity").map(|q| (q, quantity_e4(q)));
                let rate = r.get(cells, "rate").map(|v| (v, parse_amount_cents(v).ok()));
                let amount = r.get(cells, "amount").map(|v| (v, parse_amount_cents(v).ok()));
                for (label, value) in [("quantity", quantity.map(|q| (q.0, q.1.is_some()))), ("rate", rate.map(|q| (q.0, q.1.is_some()))), ("amount", amount.map(|q| (q.0, q.1.is_some())))] {
                    if let Some((raw, false)) = value {
                        bad(format!("The {label} \"{raw}\" doesn't read as a number"));
                    }
                }
                let (q, p) = (quantity.and_then(|q| q.1), rate.and_then(|v| v.1));
                let from_rate = q.zip(p).and_then(|(q, p)| crate::ledger::mul_div_round(q, p, crate::ledger::QUANTITY_SCALE));
                let amount_cents = match (amount.and_then(|a| a.1), from_rate) {
                    (Some(a), Some(c)) if (a - c).abs() > 1 => {
                        bad(format!("The amount {} isn't the quantity times the rate ({})", format_cents(a), format_cents(c)));
                        None
                    }
                    (Some(a), _) => Some(a),
                    (None, Some(c)) => Some(c),
                    (None, None) if amount.is_none() => {
                        bad("Give the line's amount, or its quantity and rate".into());
                        None
                    }
                    _ => None,
                };
                if amount_cents.is_some_and(|a| a <= 0) {
                    bad("An invoice line's amount must be more than zero".into());
                }
                if let (Some(number), Some(date), Some(customer), Some(account), Some(amount_cents)) = (number, parsed_date, customer, account, amount_cents.filter(|a| *a > 0)) {
                    lines.push(Line {
                        row,
                        number: number.to_string(),
                        date,
                        customer: customer.to_string(),
                        due: parsed_due,
                        item: r.get(cells, "item").map(String::from),
                        description: r.get(cells, "description").map(String::from),
                        quantity_e4: q,
                        rate_cents: p,
                        amount_cents,
                        account: account.to_string(),
                        memo: r.get(cells, "memo").map(String::from),
                    });
                }
            }
            // An invoice's rows must agree on its customer and date.
            let mut first: HashMap<&str, &Line> = HashMap::new();
            for l in &lines {
                match first.get(l.number.as_str()) {
                    None => {
                        first.insert(&l.number, l);
                        records += 1;
                    }
                    Some(f) if !f.customer.eq_ignore_ascii_case(&l.customer) => {
                        problems.push(Problem { row: l.row, message: format!("Invoice {} is for {} on row {}, not {}", l.number, f.customer, f.row, l.customer) })
                    }
                    Some(f) if f.date != l.date => problems.push(Problem { row: l.row, message: format!("Invoice {} is dated {} on row {}", l.number, f.date.format("%m/%d/%Y"), f.row) }),
                    Some(_) => {}
                }
                match total.checked_add(l.amount_cents) {
                    Some(t) => total = t,
                    None => problems.push(Problem { row: l.row, message: "The invoices add up to more than LocalBooks can hold".into() }),
                }
            }
        }
    }
    problems.sort_by_key(|p| p.row);
    (Check { rows: sheet.rows.len(), records, total_cents: total, problems }, lines)
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
        SheetKind::Invoices => {
            accounts.declare("Accounts Receivable", "ACCOUNTS_RECEIVABLE", true);
            let mut order: Vec<&str> = Vec::new();
            let mut by_number: HashMap<&str, Vec<&Line>> = HashMap::new();
            for l in &lines {
                if !by_number.contains_key(l.number.as_str()) {
                    order.push(&l.number);
                }
                by_number.entry(&l.number).or_default().push(l);
                accounts.ensure(&l.account, "INCOME");
            }
            for number in order {
                let ls = &by_number[number];
                let head = ls[0];
                let iso = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
                // Checked when the rows were read: the invoices' sum fits, so each invoice's does.
                let total = ls.iter().try_fold(0_i64, |sum, l| sum.checked_add(l.amount_cents)).unwrap_or(0);
                let trns = RawLine {
                    trnstype: "INVOICE".into(),
                    date: Some(iso(head.date)),
                    account: Some("Accounts Receivable".into()),
                    name: Some(head.customer.clone()),
                    amount: Some(crate::export_engine::plain_amount(total)),
                    doc_num: Some(number.to_string()),
                    memo: ls.iter().find_map(|l| l.memo.clone()),
                    due_date: head.due.map(iso),
                    ..Default::default()
                };
                let splits = ls
                    .iter()
                    .map(|l| RawLine {
                        trnstype: "INVOICE".into(),
                        date: Some(iso(l.date)),
                        account: Some(l.account.clone()),
                        name: Some(l.customer.clone()),
                        amount: Some(crate::export_engine::plain_amount(-l.amount_cents)),
                        doc_num: Some(number.to_string()),
                        memo: l.description.clone().or_else(|| l.item.clone()),
                        // IIF writes a sale's quantity negative, as its amount.
                        quantity: l.quantity_e4.map(|q| format!("-{}", crate::forms::format_quantity(q).replace(',', ""))),
                        price: l.rate_cents.map(crate::export_engine::plain_amount),
                        item: l.item.clone(),
                        ..Default::default()
                    })
                    .collect();
                blocks.push(Block { trns, splits, file_line: head.row });
            }
            for l in &lines {
                names.ensure_customer(&l.customer);
            }
        }
    }
    let mut ledger = assemble(accounts, Vec::new(), names, Vec::new(), Default::default(), &blocks);
    ledger.add_classes_used();
    Ok(ledger)
}

/// The downloadable template: the fields as headings and one example row, as CSV.
pub fn template(kind: SheetKind) -> String {
    let headers: Vec<&str> = fields(kind).iter().map(|f| f.label).collect();
    let example: Vec<&str> = match kind {
        SheetKind::Customers => vec!["Rivera Landscaping", "Rivera Landscaping LLC", "jo@rivera.example", "555-0100", "412 Orchard Lane\nSpringfield, OR 97477", "Net 30", "", "Yes"],
        SheetKind::Vendors => vec!["Paper Supply Co", "", "orders@paper.example", "555-0199", "9 Mill Road\nSalem, OR 97301", "Net 15", "12-3456789", "No", "", "Yes"],
        SheetKind::Invoices => vec!["1001", "03/01/2026", "Rivera Landscaping", "03/31/2026", "Lawn care", "March mowing", "4", "60.00", "240.00", "Sales", ""],
    };
    let mut out = csv::Writer::from_writer(Vec::new());
    let _ = out.write_record(&headers);
    let _ = out.write_record(&example);
    String::from_utf8(out.into_inner().unwrap_or_default()).unwrap_or_default()
}

#[cfg(test)]
mod tests;
