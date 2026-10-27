use super::*;

use sqlx::{Connection, SqliteConnection};

/// The tables and columns the reader uses, as Core Data declares them.
const SCHEMA: &str = r"
CREATE TABLE ZACCOUNTS (Z_PK INTEGER PRIMARY KEY, Z_ENT INTEGER, ZTYPE INTEGER, ZPARENTMO INTEGER, ZHIDDEN INTEGER, ZQBDELETED INTEGER,
    ZTAXLINEID INTEGER, ZENDINGBAL DECIMAL, ZNAME VARCHAR);
CREATE TABLE ZNAMES (Z_PK INTEGER PRIMARY KEY, ZTYPE INTEGER, ZPARENTMO INTEGER, ZHIDDEN INTEGER, ZQBDELETED INTEGER, ZTERMS INTEGER,
    ZIS1099VENDOR INTEGER, ZENDINGBAL DECIMAL, ZNAME VARCHAR, ZCOMPANYNAME VARCHAR, ZFIRST VARCHAR, ZLAST VARCHAR, ZTEL1 VARCHAR, ZEMAIL VARCHAR,
    ZADDR0 VARCHAR, ZADDR1 VARCHAR, ZADDR2 VARCHAR, ZADDR3 VARCHAR, ZADDR4 VARCHAR, ZTAXID VARCHAR, ZNOTE VARCHAR);
CREATE TABLE ZITEMS (Z_PK INTEGER PRIMARY KEY, ZTYPE INTEGER, ZPARENTMO INTEGER, ZHIDDEN INTEGER, ZQBDELETED INTEGER, ZTAXABLE INTEGER,
    ZPOSTACCNTMO INTEGER, ZCOGSACCNTMO INTEGER, ZINVACCNTMO INTEGER, ZNAME VARCHAR, ZDESC VARCHAR, ZSALESPRICE VARCHAR);
CREATE TABLE ZMASTERS (Z_PK INTEGER PRIMARY KEY, ZRECNUM INTEGER, ZVIAMO INTEGER, ZDATE TIMESTAMP, ZSHIPDATE TIMESTAMP);
CREATE TABLE ZTARGS (Z_PK INTEGER PRIMARY KEY, ZMASTERMO INTEGER, ZRECNUM INTEGER, ZVIEW INTEGER, ZACCNTMO INTEGER, ZNAMEMO INTEGER,
    ZITEMMO INTEGER, ZISSOURCE INTEGER, ZNOPOST INTEGER, ZPAYMETHMO INTEGER, ZRECEIVED INTEGER, ZDATE TIMESTAMP, ZDUEDATE TIMESTAMP,
    ZAMOUNT DECIMAL, ZCOGSAMOUNT DECIMAL, ZQUANTITY DECIMAL, ZUNITPRICE VARCHAR, ZMEMO VARCHAR, ZDOCUMENT VARCHAR);
CREATE TABLE ZLINKS (Z_PK INTEGER PRIMARY KEY, ZTYPE INTEGER, ZTONEMO INTEGER, ZTTWOMO INTEGER, ZTONEISSOURCE INTEGER, ZAMOUNT VARCHAR);
CREATE TABLE ZPAYMENTTERMS (Z_PK INTEGER PRIMARY KEY, ZRECNUM INTEGER, ZTERMSTYPE INTEGER, ZDUEDAYS INTEGER, ZQBDELETED INTEGER, ZNAME VARCHAR);
CREATE TABLE ZPAYMETHODS (Z_PK INTEGER PRIMARY KEY, ZNAME VARCHAR);
CREATE TABLE ZSHIPTYPES (Z_PK INTEGER PRIMARY KEY, ZHIDDEN INTEGER, ZQBDELETED INTEGER, ZNAME VARCHAR);
";

/// A small company: an invoice with an inventory item, sales tax and a subtotal, paid in part
/// and deposited; a bill and its payment; a check; opening balances; an estimate; and two
/// transactions that cannot be read (an amount of a fraction of a cent, and no date). Stored
/// balances are the file's, debits positive, and match what the readable transactions post.
const DATA: &str = r"
INSERT INTO ZACCOUNTS (Z_PK, ZTYPE, ZPARENTMO, ZHIDDEN, ZQBDELETED, ZTAXLINEID, ZENDINGBAL, ZNAME) VALUES
    (1, 0, NULL, 0, 0, 1535, '1037.50$', 'Checking'),
    (2, 1, NULL, 0, 0, 0, '135.00$', 'Accounts Receivable'),
    (3, 2, NULL, 0, 0, 0, '0.00$', 'Undeposited Funds'),
    (4, 2, NULL, 0, 0, 0, '-40.00$', 'Inventory Asset'),
    (5, 5, NULL, 0, 0, 0, '0.00$', 'Accounts Payable'),
    (6, 7, NULL, 0, 0, 0, '-5.00$', 'Sales Tax Payable'),
    (7, 9, NULL, 0, 0, 0, '-1000.00$', 'Opening Bal Equity'),
    (8, 10, NULL, 0, 0, 0, '-150.00$', 'Sales'),
    (9, 10, 8, 0, 0, 0, '-80.00$', 'Parts'),
    (10, 11, NULL, 0, 0, 0, '40.00$', 'Cost of Goods Sold'),
    (11, 12, NULL, 0, 0, 0, '62.50$', 'Office: Supplies'),
    (12, 15, NULL, 0, 0, 0, '0.00$', 'Estimates'),
    (13, 0, 8, 0, 0, 0, '0.00$', 'Cash Drawer'),
    (14, 12, NULL, 1, 0, 0, '0.00$', 'Old Expenses');
INSERT INTO ZNAMES (Z_PK, ZTYPE, ZPARENTMO, ZHIDDEN, ZQBDELETED, ZTERMS, ZIS1099VENDOR, ZENDINGBAL, ZNAME, ZCOMPANYNAME, ZFIRST, ZLAST,
        ZTEL1, ZEMAIL, ZADDR0, ZADDR1, ZTAXID, ZNOTE) VALUES
    (1, 0, NULL, 0, 0, 7, 0, '0.00$', 'Acme Builders', 'Acme Builders Inc', 'Ann', 'Cole', '555-0100', 'ap@acme.test', 'Acme Builders Inc', '1 Main St', NULL, 'Prefers email'),
    (2, 0, 1, 0, 0, 7, 0, '135.00$', 'Roof', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL),
    (3, 1, NULL, 0, 0, 7, 1, '0.00$', 'Supplies Co', NULL, 'Sam', 'Diaz', NULL, NULL, NULL, NULL, '12-3456789', NULL),
    (4, 2, NULL, 0, 0, 0, 0, '0.00$', 'Pat Lee', NULL, 'Pat', 'Lee', NULL, NULL, NULL, NULL, '123-45-6789', NULL),
    (5, 3, NULL, 0, 0, 0, 0, '0.00$', 'Owner', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL),
    (6, 1, NULL, 1, 0, 0, 0, '0.00$', 'Gone Vendor', NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL);
INSERT INTO ZITEMS (Z_PK, ZTYPE, ZPARENTMO, ZHIDDEN, ZQBDELETED, ZTAXABLE, ZPOSTACCNTMO, ZCOGSACCNTMO, ZINVACCNTMO, ZNAME, ZDESC, ZSALESPRICE) VALUES
    (1, 0, NULL, 0, 0, 0, 8, NULL, NULL, 'Labor', 'Hourly labor', '75.00$'),
    (2, 1, NULL, 0, 0, 1, 9, 10, 4, 'Widget', 'Blue widget', '20.00$'),
    (3, 6, NULL, 0, 0, 0, NULL, NULL, NULL, 'Subtotal', 'Subtotal', NULL),
    (4, 10, NULL, 0, 0, 0, 6, NULL, NULL, 'State tax', 'Sales tax', '7.5%'),
    (5, 0, 1, 1, 0, 0, 8, NULL, NULL, 'Install', NULL, '-10.0%');
INSERT INTO ZMASTERS (Z_PK, ZRECNUM, ZVIAMO, ZDATE, ZSHIPDATE) VALUES
    (1, 101, 1, 730944000, 731289600),
    (2, 102, NULL, 731750400, -63114076800),
    (3, 103, NULL, 731894400, NULL),
    (4, 104, NULL, 731289600, NULL),
    (5, 105, NULL, 732585600, NULL),
    (6, 106, NULL, 732672000, NULL),
    (7, 107, NULL, 725760000, NULL),
    (8, 108, NULL, 733622400, NULL),
    (9, 109, NULL, 733622400, NULL),
    (10, 110, NULL, -63114076800, NULL);
INSERT INTO ZTARGS (Z_PK, ZMASTERMO, ZRECNUM, ZVIEW, ZACCNTMO, ZNAMEMO, ZITEMMO, ZISSOURCE, ZNOPOST, ZPAYMETHMO, ZRECEIVED, ZDATE, ZDUEDATE,
        ZAMOUNT, ZCOGSAMOUNT, ZQUANTITY, ZUNITPRICE, ZMEMO, ZDOCUMENT) VALUES
    (11, 1, 1, 4, 2, 2, NULL, 1, 0, NULL, 0, 730944000, 733536000, 235, 0, NULL, NULL, 'Roof job', '1001'),
    (12, 1, 2, 4, 8, NULL, 1, 0, 0, NULL, 0, 730944000, NULL, -150.0, 0, -2, '75.00$', 'Labor', NULL),
    (13, 1, 3, 4, 9, NULL, 2, 0, 0, NULL, 0, 730944000, NULL, '-80', -40, -4, '20.00$', 'Widgets', NULL),
    (14, 1, 4, 4, NULL, NULL, 3, 0, 0, NULL, 0, 730944000, NULL, -230, 0, NULL, NULL, 'Subtotal', NULL),
    (15, 1, 5, 4, 6, NULL, 4, 0, 0, NULL, 0, 730944000, NULL, -5, 0, NULL, '2.1739%', 'Sales tax', NULL),
    (16, 1, 6, 4, NULL, NULL, NULL, 0, 0, NULL, 0, 730944000, NULL, 'NaN', 'NaN', 'NaN', NULL, 'Thank you', NULL),
    (21, 2, 1, 7, 3, 2, NULL, 1, 0, 1, 0, 731750400, NULL, 100, 0, NULL, NULL, NULL, '555'),
    (22, 2, 2, 7, 2, 2, NULL, 0, 0, NULL, 0, 731750400, NULL, -100, 0, NULL, NULL, NULL, NULL),
    (31, 3, 1, 2, 1, NULL, NULL, 1, 0, NULL, 0, 731894400, NULL, 100, 0, NULL, NULL, 'Deposit', NULL),
    (32, 3, 2, 2, 3, NULL, NULL, 0, 0, NULL, 0, 731894400, NULL, -100, 0, NULL, NULL, NULL, NULL),
    (41, 4, 1, 9, 5, 3, NULL, 1, 0, NULL, 0, 731289600, 733881600, -50, 0, NULL, NULL, NULL, 'B-7'),
    (42, 4, 2, 9, 11, NULL, NULL, 0, 0, NULL, 0, 731289600, NULL, 50, 0, NULL, NULL, 'Paper', NULL),
    (51, 5, 1, 14, 1, 3, NULL, 1, 0, NULL, 0, 732585600, NULL, -50, 0, NULL, NULL, NULL, '1042'),
    (52, 5, 2, 14, 5, 3, NULL, 0, 0, NULL, 0, 732585600, NULL, 50, 0, NULL, NULL, NULL, NULL),
    (61, 6, 1, 3, 1, 4, NULL, 1, 0, NULL, 0, 732672000, NULL, -12.5, 0, NULL, NULL, 'Mileage', '1043'),
    (62, 6, 2, 3, 11, NULL, NULL, 0, 0, NULL, 0, 732672000, NULL, 12.5, 0, NULL, NULL, NULL, NULL),
    (71, 7, 1, 8, 1, NULL, NULL, 1, 0, NULL, 0, 725760000, NULL, 1000, 0, NULL, NULL, 'Opening balance', NULL),
    (72, 7, 2, 8, 7, NULL, NULL, 0, 0, NULL, 0, 725760000, NULL, -1000, 0, NULL, NULL, NULL, NULL),
    (81, 8, 1, 0, 12, 1, NULL, 1, 1, NULL, 0, 733622400, 734832000, 300, 0, NULL, NULL, 'New roof', 'E-1'),
    (82, 8, 2, 0, 8, NULL, 1, 0, 1, NULL, 0, 733622400, NULL, -300, 0, -4, '75.00$', 'Labor', NULL),
    (91, 9, 1, 8, 11, NULL, NULL, 1, 0, NULL, 0, 733622400, NULL, 10.005, 0, NULL, NULL, 'Odd amount', NULL),
    (92, 9, 2, 8, 1, NULL, NULL, 0, 0, NULL, 0, 733622400, NULL, -10.005, 0, NULL, NULL, NULL, NULL),
    (101, 10, 1, 8, 11, 5, NULL, 1, 0, NULL, 0, -63114076800, NULL, 1, 0, NULL, NULL, NULL, NULL),
    (102, 10, 2, 8, 1, NULL, NULL, 0, 0, NULL, 0, -63114076800, NULL, -1, 0, NULL, NULL, NULL, NULL);
INSERT INTO ZLINKS (Z_PK, ZTYPE, ZTONEMO, ZTTWOMO, ZTONEISSOURCE, ZAMOUNT) VALUES
    (1, 4, 22, 12, 1, '-60.00$'), (2, 4, 12, 22, 0, '60.00$'),
    (3, 4, 22, 13, 1, '-40.00$'), (4, 4, 13, 22, 0, '40.00$'),
    (5, 9, 21, 31, 1, '-100.00$'), (6, 9, 31, 21, 0, '100.00$'),
    (7, 6, 51, 41, 1, '-50.00$'), (8, 6, 41, 51, 0, '50.00$'),
    (9, 18, 81, 12, 1, '2#');
INSERT INTO ZPAYMENTTERMS (Z_PK, ZRECNUM, ZTERMSTYPE, ZDUEDAYS, ZQBDELETED, ZNAME) VALUES (1, 7, 0, 30, 0, 'Net 30'), (2, 8, 1, 10, 0, '10th');
INSERT INTO ZPAYMETHODS (Z_PK, ZNAME) VALUES (1, 'Check');
INSERT INTO ZSHIPTYPES (Z_PK, ZHIDDEN, ZQBDELETED, ZNAME) VALUES (1, 0, 0, 'UPS'), (2, 0, 1, 'Pony Express');
";

async fn fixture_book() -> Book {
    let mut conn = SqliteConnection::connect("sqlite::memory:").await.unwrap();
    sqlx::raw_sql(SCHEMA).execute(&mut conn).await.unwrap();
    sqlx::raw_sql(DATA).execute(&mut conn).await.unwrap();
    read_connection(&mut conn).await.unwrap()
}

async fn fixture() -> Ledger {
    ledger(&fixture_book().await)
}

/// Writes the fixture as a company file at `path`.
async fn write_fixture(path: &Path) {
    let options = SqliteConnectOptions::new().filename(path).create_if_missing(true);
    let mut conn = options.connect().await.unwrap();
    sqlx::raw_sql(SCHEMA).execute(&mut conn).await.unwrap();
    sqlx::raw_sql(DATA).execute(&mut conn).await.unwrap();
    conn.close().await.unwrap();
}

/// A fresh folder under the system temp folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("localbooks-qbmac-{name}-{}-{}", std::process::id(), chrono::Local::now().format("%H%M%S%f")));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn account<'a>(l: &'a Ledger, full_name: &str) -> &'a SourceAccount {
    l.accounts.iter().find(|a| a.full_name == full_name).unwrap_or_else(|| panic!("no account {full_name}"))
}

fn txn(l: &Ledger, id: i64) -> &SourceTransaction {
    l.transactions.iter().find(|t| t.id == id).unwrap_or_else(|| panic!("no transaction {id}"))
}

fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[test]
fn amounts_read_exactly() {
    assert_eq!(scaled("72.00$", 2), Ok(Some(7_200)));
    assert_eq!(scaled("-40203.88", 2), Ok(Some(-4_020_388)));
    assert_eq!(scaled("15435", 2), Ok(Some(1_543_500)));
    assert_eq!(scaled("-0.5", 2), Ok(Some(-50)));
    assert_eq!(scaled("10.0500", 2), Ok(Some(1_005)));
    assert_eq!(scaled("NaN", 2), Ok(None));
    assert_eq!(scaled("", 2), Ok(None));
    assert_eq!(scaled("10.005", 2), Err(()));
    assert_eq!(scaled("1.5e3", 2), Err(()));
    assert_eq!(scaled("7.5%", 2), Err(()));
    assert_eq!(scaled("99999999999999999999", 2), Err(()));
}

#[test]
fn dates_count_from_2001() {
    assert_eq!(core_data_date("730944000"), Some(ymd(2024, 3, 1)));
    // Midnight Pacific time, and midnight in Sydney, are still that day.
    assert_eq!(core_data_date("730972800"), Some(ymd(2024, 3, 1)));
    assert_eq!(core_data_date("730908000"), Some(ymd(2024, 3, 1)));
    assert_eq!(core_data_date("306681745.639438"), Some(ymd(2010, 9, 21)));
    assert_eq!(core_data_date("-63114076800"), None);
    assert_eq!(core_data_date("NaN"), None);
}

#[test]
fn recognizes_mac_files() {
    assert!(is_mac_extension("qb2012") && is_mac_extension("QB2010") && is_mac_extension("qb2024"));
    assert!(!is_mac_extension("qbw") && !is_mac_extension("qbb") && !is_mac_extension("qb12"));
    assert!(is_mac_path(Path::new("/books/Company.qb2012")));
    assert!(is_mac_path(Path::new("/books/Company.qb2016/Contents/store")));
    assert!(!is_mac_path(Path::new("/books/Company.qbw")));
    assert_eq!(sniff(b"SQLite format 3\0"), Some(Format::Sqlite));
    assert_eq!(sniff(b"QBMac\0\x04\0"), Some(Format::Encrypted));
    assert_eq!(sniff(b"\0\0\0\0"), None);
}

#[test]
fn transaction_types_by_view() {
    let kinds: Vec<&str> = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 12, 14, 17, 26, 99].into_iter().map(|v| kind(Some(v))).collect();
    assert_eq!(
        kinds,
        [
            "Estimate",
            "Credit card charge",
            "Deposit",
            "Check",
            "Invoice",
            "Sales receipt",
            "Credit memo",
            "Payment",
            "Journal entry",
            "Bill",
            "Vendor credit",
            "Bill payment",
            "Purchase order",
            "Transfer",
            "Transaction"
        ]
    );
}

#[tokio::test]
async fn reads_accounts_with_their_types_hierarchy_and_balances() {
    let l = fixture().await;
    let names: Vec<&str> = l.accounts.iter().map(|a| a.full_name.as_str()).collect();
    // The non-posting Estimates account is left out; a bank account under an income account moves to the top.
    assert_eq!(
        names,
        [
            "Accounts Payable",
            "Accounts Receivable",
            "Cash Drawer",
            "Checking",
            "Cost of Goods Sold",
            "Inventory Asset",
            "Office- Supplies",
            "Old Expenses",
            "Opening Bal Equity",
            "Sales",
            "Sales Tax Payable",
            "Sales:Parts",
            "Undeposited Funds"
        ]
    );
    let parts = account(&l, "Sales:Parts");
    assert_eq!((parts.account_type, parts.parent_id, parts.name.as_str()), ("INCOME", Some(8), "Parts"));
    assert_eq!(account(&l, "Cash Drawer").parent_id, None);
    assert_eq!(account(&l, "Checking").stored_balance_cents, Some(-103_750));
    assert_eq!(account(&l, "Checking").tax_line.as_deref(), Some("QuickBooks tax line 1535"));
    assert_eq!(account(&l, "Sales Tax Payable").account_type, "OTHER_CURRENT_LIABILITY");
    assert!(!account(&l, "Old Expenses").active);
    // Every account's total from the transactions matches what the file stores.
    assert_eq!(l.quality.accounts_with_stored_balance, 13);
    assert_eq!(l.quality.accounts_matching_stored, 13);
}

#[tokio::test]
async fn reads_customers_jobs_vendors_and_items() {
    let l = fixture().await;
    let customers: Vec<&str> = l.customers.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(customers, ["Acme Builders", "Acme Builders:Roof"]);
    let acme = &l.customers[0];
    assert_eq!(acme.company_name.as_deref(), Some("Acme Builders Inc"));
    assert_eq!((acme.email.as_deref(), acme.phone.as_deref()), (Some("ap@acme.test"), Some("555-0100")));
    assert_eq!(acme.address.as_deref(), Some("Acme Builders Inc\n1 Main St"));
    assert_eq!((acme.terms_days, acme.notes.as_deref()), (Some(30), Some("Prefers email")));
    let roof = &l.customers[1];
    assert_eq!((roof.parent_id, roof.balance_cents), (Some(1), Some(13_500)));

    // Employees and other names are not list entries; a hidden vendor comes in inactive.
    let vendors: Vec<(&str, bool)> = l.vendors.iter().map(|v| (v.name.as_str(), v.active)).collect();
    assert_eq!(vendors, [("Gone Vendor", false), ("Supplies Co", true)]);
    let supplies = &l.vendors[1];
    assert_eq!((supplies.tax_id.as_deref(), supplies.is_1099, supplies.company_name.as_deref()), (Some("12-3456789"), true, Some("Sam Diaz")));

    let items: Vec<(&str, &str, i64, Option<i64>, bool)> = l.items.iter().map(|i| (i.name.as_str(), i.kind, i.rate_cents, i.income_account_id, i.active)).collect();
    assert_eq!(items, [("Labor", "Service", 7_500, Some(8), true), ("Labor:Install", "Service", 0, Some(8), false), ("Widget", "Inventory part", 2_000, Some(9), true)]);
    assert_eq!(l.sales_lists.ship_methods, [("UPS".to_string(), true)]);
}

#[tokio::test]
async fn transactions_balance_and_carry_their_documents() {
    let l = fixture().await;
    assert!(l.unbalanced.is_empty());
    let kinds: Vec<(i64, &str)> = l.transactions.iter().map(|t| (t.id, t.kind)).collect();
    assert_eq!(
        kinds,
        [(7, "Journal entry"), (1, "Invoice"), (4, "Bill"), (2, "Payment"), (3, "Deposit"), (5, "Bill payment"), (6, "Check"), (8, "Estimate")]
    );

    // The invoice: debits negative, the subtotal and memo lines left out, the widget's cost posted.
    let invoice = txn(&l, 1);
    assert_eq!((invoice.date, invoice.doc_num.as_deref(), invoice.name.as_deref(), invoice.memo.as_deref()), (ymd(2024, 3, 1), Some("1001"), Some("Acme Builders:Roof"), Some("Roof job")));
    let lines: Vec<(i64, i64)> = invoice.lines.iter().map(|l| (l.account_id, l.cents)).collect();
    assert_eq!(lines, [(2, -23_500), (8, 15_000), (9, 8_000), (6, 500), (10, -4_000), (4, 4_000)]);
    let Some(SourceDocument::Invoice { customer_id, number, due_date, lines, sales }) = &invoice.document else { panic!("{:?}", invoice.document) };
    assert_eq!((*customer_id, number.as_deref(), *due_date), (2, Some("1001"), Some(ymd(2024, 3, 31))));
    assert_eq!((sales.ship_date, sales.ship_via.as_deref()), (Some(ymd(2024, 3, 5)), Some("UPS")));
    let form: Vec<(Option<i64>, i64, i64, i64)> = lines.iter().map(|l| (l.item_id, l.quantity_e4, l.rate_cents, l.cents)).collect();
    assert_eq!(form, [(Some(1), 20_000, 7_500, 15_000), (Some(2), 40_000, 2_000, 8_000), (Some(4), 10_000, 500, 500)]);

    // The payment paid the invoice through two line links; its deposit swept it.
    let payment = txn(&l, 2);
    assert_eq!(payment.date, ymd(2024, 3, 10));
    assert_eq!(
        payment.document,
        Some(SourceDocument::Payment { customer_id: 2, reference: Some("555".into()), method: "CHECK".into(), applied_to: vec![Applied { transaction_id: 1, cents: 10_000 }] })
    );
    assert_eq!(txn(&l, 3).document, Some(SourceDocument::Deposit { payments: vec![Applied { transaction_id: 2, cents: 10_000 }] }));
    assert_eq!(
        txn(&l, 5).document,
        Some(SourceDocument::BillPayment { vendor_id: Some(3), reference: Some("1042".into()), applied_to: vec![Applied { transaction_id: 4, cents: 5_000 }] })
    );
    let Some(SourceDocument::Bill { vendor_id: 3, due_date, .. }) = &txn(&l, 4).document else { panic!() };
    assert_eq!(*due_date, Some(ymd(2024, 4, 4)));
    // A check to an employee keeps the name but has no vendor.
    let check = txn(&l, 6);
    assert_eq!((check.name.as_deref(), &check.document), (Some("Pat Lee"), &Some(SourceDocument::Expense { vendor_id: None, number: Some("1043".into()), applied_to: Vec::new() })));

    let estimate = txn(&l, 8);
    assert!(estimate.lines.is_empty());
    let Some(SourceDocument::Estimate { customer_id: 1, number, expires, lines, .. }) = &estimate.document else { panic!("{:?}", estimate.document) };
    assert_eq!((number.as_deref(), *expires), (Some("E-1"), Some(ymd(2024, 4, 15))));
    assert_eq!(lines.iter().map(|l| (l.quantity_e4, l.rate_cents, l.cents)).collect::<Vec<_>>(), [(40_000, 7_500, 30_000)]);
}

#[tokio::test]
async fn amounts_of_a_fraction_of_a_cent_and_missing_dates_are_listed() {
    let l = fixture().await;
    assert_eq!(l.quality.rejected_by_reason.get(NOT_WHOLE_CENTS), Some(&1));
    assert_eq!(l.quality.rejected_by_reason.get(NO_DATE), Some(&1));
    let listed: Vec<(Option<i64>, Option<i64>, Option<i64>)> = l.unreadable.iter().map(|u| (u.transaction_id, u.account_id, u.cents)).collect();
    assert_eq!(listed, [(Some(10), Some(11), Some(-100)), (Some(10), Some(1), Some(100)), (Some(9), Some(11), None), (Some(9), Some(1), None)]);
    assert!(l.unreadable[2].name.as_deref().unwrap().contains("10.005 is not a whole number of cents"));
    assert_eq!(l.quality.unreadable_lines, 4);
}

#[tokio::test]
async fn reads_a_file_and_a_bundle_and_refuses_what_it_cannot_read() {
    let dir = Scratch::new("files");
    let file = dir.path().join("Company.qb2012");
    write_fixture(&file).await;
    assert_eq!(read(&file).await.unwrap().transactions.len(), 8);

    // A bundle: the store is somewhere inside the folder.
    let bundle = dir.path().join("Bundle.qb2012");
    std::fs::create_dir_all(bundle.join("Contents")).unwrap();
    std::fs::write(bundle.join("Contents").join("Info.plist"), "<plist/>").unwrap();
    write_fixture(&bundle.join("Contents").join("store")).await;
    assert_eq!(read(&bundle).await.unwrap().transactions.len(), 8);

    let encrypted = dir.path().join("Newer.qb2016");
    std::fs::write(&encrypted, b"QBMac\0\x04\0 ciphertext").unwrap();
    let err = read(&encrypted).await.unwrap_err();
    assert!(err.starts_with("QBMAC_ENCRYPTED") && err.contains("To QuickBooks for Windows"), "{err}");

    let old = dir.path().join("Old.qb2008");
    std::fs::write(&old, b"\0\x01legacy").unwrap();
    assert!(read(&old).await.unwrap_err().starts_with("QBMAC_OLD_FORMAT"));

    let other = dir.path().join("Other.qb2012");
    let mut conn = SqliteConnectOptions::new().filename(&other).create_if_missing(true).connect().await.unwrap();
    sqlx::raw_sql("CREATE TABLE accounts (guid TEXT)").execute(&mut conn).await.unwrap();
    conn.close().await.unwrap();
    assert!(read(&other).await.unwrap_err().starts_with("QBMAC_NOT_COMPANY"));
}

/// The fixture goes through staging and a full import as native documents.
#[tokio::test]
async fn a_full_import_creates_native_documents() {
    use crate::qbw_ledger_import::{auto_map_accounts, batch_summary, post_batch, stage, ImportMode, PostLedgerInput};
    let pool = crate::db::test_pool().await;
    let l = fixture().await;
    let batch = stage(&pool, Path::new("/books/Company.qb2012"), 1, &l).await.unwrap();
    let summary = batch_summary(&pool, batch).await.unwrap();
    assert_eq!(summary.source_format, "QBMAC");
    auto_map_accounts(&pool, batch).await.unwrap();
    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap();
    let full = posted.full.unwrap();
    let summary = batch_summary(&pool, batch).await.unwrap();
    assert!(summary.fallbacks.is_empty(), "{:?}", summary.fallbacks.iter().map(|f| (&f.kind, &f.reason)).collect::<Vec<_>>());
    assert_eq!((full.invoices, full.payments, full.bills, full.bill_payments, full.checks, full.journal_entries), (1, 1, 1, 1, 1, 1), "{full:?}");
    let (status, total, paid): (String, i64, i64) = sqlx::query_as("SELECT status, total_cents, amount_paid_cents FROM invoices").fetch_one(&pool).await.unwrap();
    assert_eq!((status.as_str(), total, paid), ("PARTIAL", 23_500, 10_000));
    let bill_status: String = sqlx::query_scalar("SELECT status FROM bills").fetch_one(&pool).await.unwrap();
    assert_eq!(bill_status, "PAID");
    let deposited: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM deposit_lines WHERE invoice_payment_id IS NOT NULL").fetch_one(&pool).await.unwrap();
    assert_eq!(deposited, 1);
}

// ------------------------------------------------------------------------------------------------
// The sample companies in the QuickBooks for Mac 2012 installer
// ------------------------------------------------------------------------------------------------

/// The installer image: `LOCALBOOKS_QBMAC_DMG`, or `research-samples/QuickBooksProMac2012.dmg`
/// next to the repository's `src-tauri`.
fn installer() -> PathBuf {
    std::env::var_os("LOCALBOOKS_QBMAC_DMG")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../research-samples/QuickBooksProMac2012.dmg"))
}

/// Mounts the installer read-only for the length of a test, and detaches it after.
struct Mounted(Scratch);

impl Mounted {
    fn new() -> Self {
        let dmg = installer();
        assert!(dmg.exists(), "no installer at {} (set LOCALBOOKS_QBMAC_DMG)", dmg.display());
        let dir = Scratch::new("dmg");
        let status = std::process::Command::new("hdiutil")
            .args(["attach", "-readonly", "-nobrowse", "-noautoopen", "-mountpoint"])
            .arg(dir.path())
            .arg(&dmg)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "hdiutil could not mount {}", dmg.display());
        Mounted(dir)
    }

    fn sample(&self, name: &str) -> PathBuf {
        self.0.path().join("QuickBooks 2012.app/Contents/Resources/English.lproj/qbmd").join(format!("{name}.qb2012"))
    }
}

impl Drop for Mounted {
    fn drop(&mut self) {
        let _ = std::process::Command::new("hdiutil").args(["detach", "-quiet"]).arg(self.0.path()).status();
    }
}

/// Reads both sample companies: every transaction balances, nothing is unreadable, and every
/// account's total equals the balance the file stores. Prints the figures for the PR.
#[tokio::test]
#[ignore]
async fn reads_the_2012_sample_companies() {
    let mounted = Mounted::new();
    for name in ["Sample Product", "Sample Service"] {
        let l = read(&mounted.sample(name)).await.unwrap();
        let lines: usize = l.transactions.iter().map(|t| t.lines.len()).sum();
        let documents = l.transactions.iter().filter(|t| t.document.is_some()).count();
        let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
        for t in &l.transactions {
            *kinds.entry(t.kind).or_default() += 1;
        }
        println!(
            "{name}: {} transactions ({lines} lines, {documents} with documents), {} unbalanced, {} unreadable lines; accounts {}/{} match the stored balance; \
             {} customers, {} vendors, {} items; rejected {:?}",
            l.transactions.len(),
            l.unbalanced.len(),
            l.unreadable.len(),
            l.quality.accounts_matching_stored,
            l.quality.accounts_with_stored_balance,
            l.customers.len(),
            l.vendors.len(),
            l.items.len(),
            l.quality.rejected_by_reason,
        );
        println!("   by type: {kinds:?}");
        assert!(l.unbalanced.is_empty() && l.unreadable.is_empty(), "{name}");
        assert!(l.transactions.iter().all(|t| t.imbalance_cents() == 0));
        assert_eq!(l.quality.accounts_matching_stored, l.quality.accounts_with_stored_balance, "{name}");
        assert!(l.quality.accounts_with_stored_balance > 100);
    }
}

/// Full-imports both sample companies and prints what became native documents and what fell back.
#[tokio::test]
#[ignore]
async fn full_imports_the_2012_sample_companies() {
    use crate::qbw_ledger_import::{auto_map_accounts, batch_summary, post_batch, stage, ImportMode, PostLedgerInput};
    let mounted = Mounted::new();
    for name in ["Sample Product", "Sample Service"] {
        let pool = crate::db::test_pool().await;
        let path = mounted.sample(name);
        let l = read(&path).await.unwrap();
        let batch = stage(&pool, &path, 1, &l).await.unwrap();
        auto_map_accounts(&pool, batch).await.unwrap();
        let full = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap().full.unwrap();
        let summary = batch_summary(&pool, batch).await.unwrap();
        let mut reasons: BTreeMap<String, usize> = BTreeMap::new();
        for f in &summary.fallbacks {
            *reasons.entry(format!("{}: {}", f.kind, f.reason)).or_default() += 1;
        }
        println!("{name}: {full:?}");
        for (reason, n) in reasons {
            println!("   {n:>4} {reason}");
        }
        // The books balance after the import: the trial balance nets to zero.
        let net: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM journal_lines").fetch_one(&pool).await.unwrap();
        assert_eq!(net, 0);
    }
}
