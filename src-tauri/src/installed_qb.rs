//! Export from an installed QuickBooks (Preview): for people who still have QuickBooks running,
//! LocalBooks asks QuickBooks itself for the company's data through Intuit's public interfaces,
//! and the answers go through the report-export reader ([`report_import`]) like files would.
//!
//! - **QuickBooks Desktop for Mac** answers qbXML through its AppleScript `executeqbxml` command
//!   (`mac`). It runs list and transaction queries but no report queries, so the ledger comes from
//!   the Journal report the user leaves open in front, read with its `reportAsText` command.
//! - **QuickBooks Desktop for Windows** answers qbXML through the QuickBooks SDK's request
//!   processor, `QBXMLRP2.RequestProcessor`, over COM (`win`), including a Journal report query.
//!
//! Each answer is turned into the table QuickBooks would export (Account Listing, contact lists,
//! Item List, Journal), so the accounts, lists and transactions are read by the same code as a
//! report export and staged the same way.
//!
//! **Read-only.** Every request is built here from [`Query`], whose requests are all queries, and
//! [`ensure_read_only`] refuses any request that is not one of them before it is sent. Nothing in
//! this module can add, change or delete anything in QuickBooks.
//!
//! The platform side sits behind [`Transport`], so everything else is tested without QuickBooks:
//! none was available while this was written, so neither platform has been tried live.

// Where QuickBooks Desktop doesn't run, only the tests reach the export.
#![cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]

mod xml;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
mod win;

use std::collections::HashMap;

use crate::report_import::{self, ReportRead};
use xml::Node;

/// Which QuickBooks LocalBooks talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Mac,
    #[cfg_attr(not(windows), allow(dead_code))]
    Windows,
}

impl Platform {
    fn qbxml_version(self) -> &'static str {
        match self {
            // The version QuickBooks for Mac writes in its own requests.
            Platform::Mac => "10.0",
            // QuickBooks 2014 and later; it has every query below.
            Platform::Windows => "13.0",
        }
    }

    /// What is asked for, in order. QuickBooks for Mac implements only some queries: one query per
    /// item type, no terms and no reports.
    fn queries(self) -> &'static [Query] {
        use Query::*;
        match self {
            Platform::Mac => &[Company, Accounts, Customers, Vendors, ServiceItems, NonInventoryItems, InventoryItems, Classes, Invoices, Bills, BillPaymentChecks, BillPaymentCards],
            Platform::Windows => &[Company, Accounts, Customers, Vendors, Items, Terms, Classes, Invoices, Bills, BillPaymentChecks, BillPaymentCards, Journal],
        }
    }
}

/// A qbXML request LocalBooks sends. Every one is a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Query {
    Company,
    Accounts,
    Customers,
    Vendors,
    /// Every item type at once (Windows).
    Items,
    ServiceItems,
    NonInventoryItems,
    InventoryItems,
    Classes,
    Terms,
    Invoices,
    Bills,
    BillPaymentChecks,
    BillPaymentCards,
    /// The Journal report for all dates (Windows).
    Journal,
}

/// The Journal report for all dates, with the columns the report reader uses.
const JOURNAL_BODY: &str = "<GeneralDetailReportType>Journal</GeneralDetailReportType>\
<ReportDateMacro>All</ReportDateMacro>\
<IncludeColumn>TxnNumber</IncludeColumn>\
<IncludeColumn>TxnType</IncludeColumn>\
<IncludeColumn>Date</IncludeColumn>\
<IncludeColumn>RefNumber</IncludeColumn>\
<IncludeColumn>Name</IncludeColumn>\
<IncludeColumn>Memo</IncludeColumn>\
<IncludeColumn>Account</IncludeColumn>\
<IncludeColumn>Debit</IncludeColumn>\
<IncludeColumn>Credit</IncludeColumn>";

impl Query {
    pub const ALL: [Query; 15] = [
        Query::Company,
        Query::Accounts,
        Query::Customers,
        Query::Vendors,
        Query::Items,
        Query::ServiceItems,
        Query::NonInventoryItems,
        Query::InventoryItems,
        Query::Classes,
        Query::Terms,
        Query::Invoices,
        Query::Bills,
        Query::BillPaymentChecks,
        Query::BillPaymentCards,
        Query::Journal,
    ];

    /// The request element.
    pub fn element(self) -> &'static str {
        match self {
            Query::Company => "CompanyQueryRq",
            Query::Accounts => "AccountQueryRq",
            Query::Customers => "CustomerQueryRq",
            Query::Vendors => "VendorQueryRq",
            Query::Items => "ItemQueryRq",
            Query::ServiceItems => "ItemServiceQueryRq",
            Query::NonInventoryItems => "ItemNonInventoryQueryRq",
            Query::InventoryItems => "ItemInventoryQueryRq",
            Query::Classes => "ClassQueryRq",
            Query::Terms => "TermsQueryRq",
            Query::Invoices => "InvoiceQueryRq",
            Query::Bills => "BillQueryRq",
            Query::BillPaymentChecks => "BillPaymentCheckQueryRq",
            Query::BillPaymentCards => "BillPaymentCreditCardQueryRq",
            Query::Journal => "GeneralDetailReportQueryRq",
        }
    }

    fn body(self) -> &'static str {
        match self {
            Query::Company | Query::Invoices | Query::Bills | Query::BillPaymentChecks | Query::BillPaymentCards => "",
            Query::Journal => JOURNAL_BODY,
            // Lists: inactive records too, as QuickBooks' own exports include them.
            _ => "<ActiveStatus>All</ActiveStatus>",
        }
    }

    /// What it reads, for messages.
    fn label(self) -> &'static str {
        match self {
            Query::Company => "the company name",
            Query::Accounts => "the chart of accounts",
            Query::Customers => "the customer list",
            Query::Vendors => "the vendor list",
            Query::Items | Query::ServiceItems | Query::NonInventoryItems | Query::InventoryItems => "the item list",
            Query::Classes => "the class list",
            Query::Terms => "the terms list",
            Query::Invoices => "the invoices",
            Query::Bills => "the bills",
            Query::BillPaymentChecks | Query::BillPaymentCards => "the bill payments",
            Query::Journal => "the Journal report",
        }
    }

    /// The export stops without it; the others are reported and skipped.
    fn required(self) -> bool {
        matches!(self, Query::Accounts | Query::Journal)
    }
}

/// The qbXML request document for one query.
pub fn request(query: Query, platform: Platform) -> String {
    let (version, element, body) = (platform.qbxml_version(), query.element(), query.body());
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?><?qbxml version=\"{version}\"?><QBXML><QBXMLMsgsRq onError=\"stopOnError\"><{element}>{body}</{element}></QBXMLMsgsRq></QBXML>"
    )
}

/// Element-name endings of requests that change QuickBooks data.
const WRITE_ENDINGS: [&str; 8] = ["Add", "Mod", "Del", "Void", "AddRq", "ModRq", "DelRq", "VoidRq"];

/// Refuses a request unless every message in it is one of [`Query`]'s, so nothing that could
/// change QuickBooks data is ever sent, whatever builds the request.
pub fn ensure_read_only(request: &str) -> Result<(), String> {
    let refuse = |why: String| Err(format!("QB_NOT_READ_ONLY: LocalBooks only sends QuickBooks read-only queries, and refused {why}"));
    let root = xml::parse(request)?;
    let Some(msgs) = (root.name == "QBXML").then(|| root.child("QBXMLMsgsRq")).flatten() else {
        return refuse("a request that is not a qbXML message set".into());
    };
    if msgs.children.is_empty() || root.children.len() != 1 {
        return refuse("a request with no queries, or with something besides its message set".into());
    }
    for m in &msgs.children {
        if !m.name.ends_with("QueryRq") || !Query::ALL.iter().any(|q| q.element() == m.name) {
            return refuse(format!("a {} request", m.name));
        }
    }
    fn find_write(n: &Node) -> Option<&str> {
        if WRITE_ENDINGS.iter().any(|e| n.name.ends_with(e)) {
            return Some(&n.name);
        }
        n.children.iter().find_map(find_write)
    }
    match find_write(&root) {
        Some(name) => refuse(format!("a request containing {name}")),
        None => Ok(()),
    }
}

/// How LocalBooks reaches QuickBooks: AppleScript on a Mac, the QuickBooks SDK on Windows.
pub trait Transport {
    /// Sends one qbXML request document and returns QuickBooks' reply document.
    fn execute(&mut self, request: &str) -> Result<String, String>;

    /// The Journal report as tab-separated text, for a QuickBooks whose qbXML runs no reports.
    fn journal_report(&mut self) -> Result<String, String> {
        Err("QB_NO_JOURNAL: this QuickBooks cannot send its Journal report".into())
    }
}

/// Sends a query, refusing it first unless it is read-only, and returns the records it found.
fn run(transport: &mut dyn Transport, query: Query, platform: Platform) -> Result<Vec<Node>, String> {
    let text = request(query, platform);
    ensure_read_only(&text)?;
    let reply = transport.execute(&text)?;
    parse_reply(&reply, query)
}

/// The records in QuickBooks' reply to a query (`AccountRet`s, a `ReportRet`, …). A status of 1,
/// "nothing found", is an empty list; any other non-zero status is an error with QuickBooks' text.
pub fn parse_reply(reply: &str, query: Query) -> Result<Vec<Node>, String> {
    let root = xml::parse(reply)?;
    let rs = query.element().trim_end_matches("Rq").to_string() + "Rs";
    let Some(found) = root.child("QBXMLMsgsRs").and_then(|m| m.child(&rs)) else {
        return Err(format!("QBXML_NO_REPLY: QuickBooks' reply has no answer to the {} request", query.element()));
    };
    match found.attr("statusCode").unwrap_or("0") {
        "0" => Ok(found.children.clone()),
        "1" => Ok(Vec::new()),
        code => Err(format!(
            "QBXML_STATUS_{code}: QuickBooks could not send {} ({})",
            query.label(),
            found.attr("statusMessage").unwrap_or("no reason given")
        )),
    }
}

// --- Turning records into the tables QuickBooks exports ---

fn csv_text(header: &[&str], rows: Vec<Vec<String>>) -> Vec<u8> {
    let mut w = csv::Writer::from_writer(Vec::new());
    w.write_record(header).expect("writing to memory");
    for r in rows {
        w.write_record(&r).expect("writing to memory");
    }
    w.into_inner().expect("writing to memory")
}

fn text(n: &Node, path: &str) -> String {
    n.text_at(path).unwrap_or_default().to_string()
}

fn active(n: &Node) -> String {
    if n.text_at("IsActive") == Some("false") { "Inactive" } else { "Active" }.into()
}

/// An address as lines: the address block QuickBooks prints, else its parts.
fn address(n: &Node, element: &str) -> String {
    let lines = |node: &Node, parts: &[&str]| parts.iter().filter_map(|p| node.text_at(p)).map(str::to_string).collect::<Vec<_>>();
    if let Some(block) = n.child(&format!("{element}Block")) {
        let l = lines(block, &["Addr1", "Addr2", "Addr3", "Addr4", "Addr5"]);
        if !l.is_empty() {
            return l.join("\n");
        }
    }
    let Some(a) = n.child(element) else { return String::new() };
    let mut l = lines(a, &["Addr1", "Addr2", "Addr3", "Addr4", "Addr5"]);
    let city = [a.text_at("City"), a.text_at("State"), a.text_at("PostalCode")].into_iter().flatten().collect::<Vec<_>>().join(" ");
    if !city.is_empty() {
        l.push(city);
    }
    l.extend(a.text_at("Country").map(str::to_string));
    l.join("\n")
}

/// Terms by name, with their days until due, from the terms list.
pub fn terms_days(terms: &[Node]) -> HashMap<String, i64> {
    terms
        .iter()
        .filter(|t| t.name == "StandardTermsRet")
        .filter_map(|t| Some((t.text_at("Name")?.to_string(), t.text_at("StdDueDays")?.parse().ok()?)))
        .collect()
}

/// A customer's or vendor's terms as text the report reader understands: the name, with the days
/// added when the name has none (`Monthly (Net 30)`).
fn terms(n: &Node, days: &HashMap<String, i64>) -> String {
    let Some(name) = n.text_at("TermsRef/FullName") else { return String::new() };
    match days.get(name) {
        Some(d) if crate::text_import::terms_days(name).is_none() => format!("{name} (Net {d})"),
        _ => name.to_string(),
    }
}

/// The chart of accounts as an Account Listing.
pub fn accounts_table(accounts: &[Node]) -> Vec<u8> {
    let rows = accounts
        .iter()
        .filter(|a| a.name == "AccountRet")
        .map(|a| {
            let balance = a.text_at("TotalBalance").or(a.text_at("Balance")).unwrap_or_default();
            vec![text(a, "FullName"), text(a, "AccountType"), balance.to_string(), text(a, "Desc"), active(a)]
        })
        .collect();
    csv_text(&["Account", "Type", "Balance Total", "Description", "Active Status"], rows)
}

/// The customer list as a Customer Contact List.
pub fn customers_table(customers: &[Node], days: &HashMap<String, i64>) -> Vec<u8> {
    let rows = customers
        .iter()
        .filter(|c| c.name == "CustomerRet")
        .map(|c| {
            vec![
                c.text_at("FullName").or(c.text_at("Name")).unwrap_or_default().to_string(),
                text(c, "CompanyName"),
                text(c, "Email"),
                text(c, "Phone"),
                address(c, "BillAddress"),
                terms(c, days),
                active(c),
                text(c, "Notes"),
            ]
        })
        .collect();
    csv_text(&["Customer", "Company", "Main Email", "Main Phone", "Bill to", "Terms", "Active Status", "Notes"], rows)
}

/// The vendor list as a Vendor Contact List.
pub fn vendors_table(vendors: &[Node], days: &HashMap<String, i64>) -> Vec<u8> {
    let rows = vendors
        .iter()
        .filter(|v| v.name == "VendorRet")
        .map(|v| {
            vec![
                text(v, "Name"),
                text(v, "CompanyName"),
                text(v, "Email"),
                text(v, "Phone"),
                address(v, "VendorAddress"),
                terms(v, days),
                active(v),
                text(v, "Notes"),
                text(v, "VendorTaxIdent"),
                text(v, "IsVendorEligibleFor1099"),
            ]
        })
        .collect();
    csv_text(&["Vendor", "Company", "Main Email", "Main Phone", "Bill from", "Terms", "Active Status", "Notes", "Tax ID", "Eligible for 1099"], rows)
}

/// The item types as the Item List names them; other items (discounts, groups, sales tax…) keep
/// their qbXML name, which the report reader skips.
fn item_type(element: &str) -> String {
    match element {
        "ItemServiceRet" => "Service".into(),
        "ItemNonInventoryRet" => "Non-inventory Part".into(),
        "ItemInventoryRet" => "Inventory Part".into(),
        "ItemInventoryAssemblyRet" => "Inventory Assembly".into(),
        "ItemOtherChargeRet" => "Other Charge".into(),
        other => other.trim_start_matches("Item").trim_end_matches("Ret").to_string(),
    }
}

/// The items, of every type, as an Item List.
pub fn items_table(items: &[Node]) -> Vec<u8> {
    let rows = items
        .iter()
        .filter(|i| i.name.starts_with("Item") && i.name.ends_with("Ret"))
        .map(|i| {
            // Sold-only items keep their details in SalesOrPurchase, bought-and-sold ones in
            // SalesAndPurchase, inventory items at the top level.
            let first = |paths: &[&str]| paths.iter().find_map(|p| i.text_at(p)).unwrap_or_default().to_string();
            let price = match i.text_at("SalesOrPurchase/PricePercent") {
                Some(p) => format!("{p}%"),
                None => first(&["SalesOrPurchase/Price", "SalesAndPurchase/SalesPrice", "SalesPrice"]),
            };
            let taxable = match i.text_at("SalesTaxCodeRef/FullName") {
                Some(code) => code.to_string(),
                // QuickBooks for Mac answers whether the item is taxable here.
                None if i.text_at("IsTaxIncluded") == Some("true") => "Tax".into(),
                None => String::new(),
            };
            vec![
                text(i, "FullName"),
                first(&["SalesOrPurchase/Desc", "SalesAndPurchase/SalesDesc", "SalesDesc"]),
                item_type(&i.name),
                price,
                first(&["SalesOrPurchase/AccountRef/FullName", "SalesAndPurchase/IncomeAccountRef/FullName", "IncomeAccountRef/FullName"]),
                taxable,
                active(i),
            ]
        })
        .collect();
    csv_text(&["Item", "Description", "Type", "Price", "Account", "Sales Tax Code", "Active Status"], rows)
}

/// The column heading the report reader looks for, by the report column's type.
fn column_heading(col_type: &str) -> Option<&'static str> {
    Some(match col_type {
        "TxnNumber" => "Trans #",
        "TxnType" => "Type",
        "Date" => "Date",
        "RefNumber" => "Num",
        "Name" => "Name",
        "Memo" => "Memo",
        "Account" => "Account",
        "Debit" => "Debit",
        "Credit" => "Credit",
        "Class" => "Class",
        _ => return None,
    })
}

/// A report sent by a report query (`ReportRet`) as the CSV QuickBooks would export: its column
/// headings, then one row per data row. Subtotal and total rows become blank rows, which end the
/// open transaction the way they do in an export.
pub fn report_table(report: &Node) -> Result<Vec<u8>, String> {
    let mut columns: Vec<(String, String)> = Vec::new();
    for c in report.children("ColDesc") {
        let id = c.attr("colID").unwrap_or_default().to_string();
        let title = c.children("ColTitle").filter_map(|t| t.attr("value")).last().unwrap_or_default();
        let heading = c.text_at("ColType").and_then(column_heading).map(str::to_string).unwrap_or_else(|| title.to_string());
        columns.push((id, heading));
    }
    if columns.is_empty() {
        return Err("QBXML_NO_COLUMNS: QuickBooks sent the Journal report without its columns".into());
    }
    let mut rows = Vec::new();
    for row in report.child("ReportData").map(|d| d.children.as_slice()).unwrap_or_default() {
        if row.name != "DataRow" {
            rows.push(vec![String::new(); columns.len()]);
            continue;
        }
        let mut cells = vec![String::new(); columns.len()];
        for d in row.children("ColData") {
            if let (Some(id), Some(v)) = (d.attr("colID"), d.attr("value")) {
                if let Some(i) = columns.iter().position(|(c, _)| c == id) {
                    cells[i] = v.to_string();
                }
            }
        }
        rows.push(cells);
    }
    let header: Vec<&str> = columns.iter().map(|(_, h)| h.as_str()).collect();
    Ok(csv_text(&header, rows))
}

/// A report as tab-separated text (QuickBooks for Mac's `reportAsText`) as CSV. Done here rather
/// than by the report reader's own guess, which goes by the first line: a report's title rows
/// have no tabs. A blank line stays blank.
pub fn report_text_table(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    for line in text.lines().map(|l| l.trim_end_matches('\r')) {
        if line.is_empty() {
            // The CSV writer refuses an empty record.
            out.push(b'\n');
            continue;
        }
        let mut w = csv::Writer::from_writer(&mut out);
        w.write_record(line.split('\t')).expect("writing to memory");
        w.flush().expect("writing to memory");
    }
    out
}

// --- The export ---

/// What an export read: the ledger and its sources, the company's name, and anything the user
/// should know about how complete it is.
pub struct Export {
    pub read: ReportRead,
    pub company: Option<String>,
    pub notes: Vec<String>,
    /// Bytes of the tables made from QuickBooks' answers.
    pub size: i64,
}

/// A report heading reduced to letters, digits and `#`, lowercased, as the report reader compares them.
fn key(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric() || *c == '#').flat_map(char::to_lowercase).collect()
}

/// How many transactions of each type a Journal (as CSV) holds, by `Type` reduced by [`key`], or
/// nothing when it has no Type column. A transaction is a `Trans #`, or without one, a row that
/// names its type.
pub fn journal_type_counts(csv_bytes: &[u8]) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    let rows: Vec<Vec<String>> = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .from_reader(csv_bytes)
        .records()
        .filter_map(Result::ok)
        .map(|r| r.iter().map(|f| f.trim().to_string()).collect())
        .collect();
    let Some((h, header)) = rows.iter().enumerate().take(40).find(|(_, r)| r.iter().any(|c| key(c) == "type") && r.iter().any(|c| key(c) == "account")) else {
        return counts;
    };
    let col = |name: &str| header.iter().position(|c| key(c) == name);
    let (Some(kind), trans) = (col("type"), col("trans#")) else { return counts };
    let mut seen = std::collections::HashSet::new();
    for row in &rows[h + 1..] {
        let Some(t) = row.get(kind).filter(|t| !t.is_empty()) else { continue };
        let fresh = match trans.and_then(|c| row.get(c)).filter(|n| !n.is_empty()) {
            Some(n) => seen.insert(n.clone()),
            None => trans.is_none(),
        };
        if fresh {
            *counts.entry(key(t)).or_insert(0) += 1;
        }
    }
    counts
}

/// Compares how many invoices, bills and bill payments QuickBooks lists with how many the Journal
/// holds, and says where they differ (usually a Journal that does not cover all dates).
fn cross_check(replies: &HashMap<Query, Vec<Node>>, journal: &HashMap<String, usize>) -> Vec<String> {
    if journal.is_empty() {
        return Vec::new();
    }
    let checks: [(&[Query], &str, &[&str]); 3] = [
        (&[Query::Invoices], "invoices", &["invoice"]),
        (&[Query::Bills], "bills", &["bill"]),
        (&[Query::BillPaymentChecks, Query::BillPaymentCards], "bill payments", &["billpmtcheck", "billpmtccard", "billpayment", "billpaymentcheck", "billpaymentcreditcard"]),
    ];
    let mut notes = Vec::new();
    for (queries, label, types) in checks {
        // Only when every query of the kind was answered.
        let Some(listed) = queries.iter().map(|q| replies.get(q).map(|r| r.iter().filter(|n| n.name.ends_with("Ret")).count())).sum::<Option<usize>>() else { continue };
        let held: usize = types.iter().filter_map(|t| journal.get(*t)).sum();
        if listed != held {
            notes.push(format!(
                "QuickBooks lists {listed} {label} but the Journal holds {held}: check that the Journal report covers all dates before posting"
            ));
        }
    }
    notes
}

/// Asks QuickBooks for the company's lists and Journal, and reads them into a ledger. `progress`
/// is told the steps done and the steps in all.
pub fn export(transport: &mut dyn Transport, platform: Platform, progress: &mut dyn FnMut(u64, u64)) -> Result<Export, String> {
    let queries = platform.queries();
    let total = queries.len() as u64 + 2;
    let mut replies: HashMap<Query, Vec<Node>> = HashMap::new();
    let mut notes = Vec::new();
    for (n, &q) in queries.iter().enumerate() {
        progress(n as u64, total);
        match run(transport, q, platform) {
            Ok(records) => {
                replies.insert(q, records);
            }
            Err(e) if q.required() => return Err(e),
            Err(e) => notes.push(format!("{} was not read ({e})", capitalise(q.label()))),
        }
    }

    progress(queries.len() as u64, total);
    let journal = match replies.get(&Query::Journal) {
        Some(records) => {
            let report = records.iter().find(|r| r.name == "ReportRet").ok_or("QBXML_NO_REPORT: QuickBooks answered the Journal request without a report")?;
            ("Journal (from QuickBooks).csv".to_string(), report_table(report)?)
        }
        None => ("Journal (from QuickBooks).csv".to_string(), report_text_table(&transport.journal_report()?)),
    };

    let empty = Vec::new();
    let get = |q: Query| replies.get(&q).unwrap_or(&empty);
    let days = terms_days(get(Query::Terms));
    let items: Vec<Node> = [Query::Items, Query::ServiceItems, Query::NonInventoryItems, Query::InventoryItems].into_iter().flat_map(|q| get(q).iter().cloned()).collect();
    let mut files = vec![("Accounts (from QuickBooks).csv".to_string(), accounts_table(get(Query::Accounts)))];
    if !get(Query::Customers).is_empty() {
        files.push(("Customers (from QuickBooks).csv".into(), customers_table(get(Query::Customers), &days)));
    }
    if !get(Query::Vendors).is_empty() {
        files.push(("Vendors (from QuickBooks).csv".into(), vendors_table(get(Query::Vendors), &days)));
    }
    if !items.is_empty() {
        files.push(("Items (from QuickBooks).csv".into(), items_table(&items)));
    }
    notes.extend(cross_check(&replies, &journal_type_counts(&journal.1)));
    files.push(journal);

    progress(total - 1, total);
    let size = files.iter().map(|(_, b)| b.len() as i64).sum();
    let read = report_import::read_files(&files)?;
    let classes = get(Query::Classes).iter().filter(|c| c.name == "ClassRet").count();
    if classes > 0 {
        notes.push(format!("QuickBooks has {classes} classes; this export does not bring classes in, so set them up under Lists › Classes"));
    }
    let company = get(Query::Company).iter().find_map(|c| c.text_at("CompanyName")).map(str::to_string);
    progress(total, total);
    Ok(Export { read, company, notes, size })
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// What LocalBooks can see of QuickBooks on this computer before asking it anything, so no
/// permission prompt appears yet.
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// `mac` or `windows`; none where LocalBooks cannot ask QuickBooks (Linux).
    pub platform: Option<&'static str>,
    /// QuickBooks apps found in the Applications folders (Mac).
    pub installed: Vec<String>,
    /// The QuickBooks that is running, by name (Mac).
    pub running: Option<String>,
    /// The QuickBooks SDK's request processor is registered (Windows).
    pub sdk_installed: bool,
}

/// Looks for QuickBooks on this computer.
pub fn status() -> Status {
    #[cfg(target_os = "macos")]
    return mac::status();
    #[cfg(windows)]
    return win::status();
    #[cfg(not(any(target_os = "macos", windows)))]
    Status::default()
}

/// Connects to the running QuickBooks and exports its open company.
pub fn export_installed(progress: &mut dyn FnMut(u64, u64)) -> Result<Export, String> {
    #[cfg(target_os = "macos")]
    return export(&mut mac::connect()?, Platform::Mac, progress);
    #[cfg(windows)]
    return export(&mut win::connect()?, Platform::Windows, progress);
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = progress;
        Err("QB_UNSUPPORTED_PLATFORM: QuickBooks Desktop runs on macOS and Windows; on this computer, export its reports instead".into())
    }
}

#[cfg(test)]
mod tests;
