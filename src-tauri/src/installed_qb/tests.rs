use super::*;

use chrono::NaiveDate;

use crate::qbw_company::{Ledger, SourceTransaction};
use crate::qbw_documents::SourceDocument;

// Replies in Intuit's documented qbXML response format: a `QBXMLMsgsRs` holding one `…Rs` per
// request with its status attributes, and the `…Ret` records whose elements QuickBooks for Mac's
// `qbxmlTemplates` and the qbXML schema list.

fn reply(rs: &str, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" ?><QBXML><QBXMLMsgsRs><{rs} statusCode=\"0\" statusSeverity=\"Info\" statusMessage=\"Status OK\">{body}</{rs}></QBXMLMsgsRs></QBXML>"
    )
}

const COMPANY: &str = "<CompanyRet><IsSampleCompany>false</IsSampleCompany><CompanyName>Rock Castle Construction</CompanyName><LegalCompanyName>Rock Castle Construction</LegalCompanyName></CompanyRet>";

const ACCOUNTS: &str = "\
<AccountRet><ListID>10000-1</ListID><Name>Checking</Name><FullName>Checking</FullName><IsActive>true</IsActive><Sublevel>0</Sublevel><AccountType>Bank</AccountType><Balance>5600.00</Balance><TotalBalance>5600.00</TotalBalance></AccountRet>
<AccountRet><ListID>10000-2</ListID><Name>Accounts Receivable</Name><FullName>Accounts Receivable</FullName><IsActive>true</IsActive><Sublevel>0</Sublevel><AccountType>AccountsReceivable</AccountType><SpecialAccountType>AccountsReceivable</SpecialAccountType><Balance>250.00</Balance><TotalBalance>250.00</TotalBalance></AccountRet>
<AccountRet><ListID>10000-3</ListID><Name>Undeposited Funds</Name><FullName>Undeposited Funds</FullName><IsActive>true</IsActive><Sublevel>0</Sublevel><AccountType>OtherCurrentAsset</AccountType><SpecialAccountType>UndepositedFunds</SpecialAccountType><Balance>0.00</Balance></AccountRet>
<AccountRet><ListID>10000-4</ListID><Name>Accounts Payable</Name><FullName>Accounts Payable</FullName><IsActive>true</IsActive><Sublevel>0</Sublevel><AccountType>AccountsPayable</AccountType><Balance>0.00</Balance></AccountRet>
<AccountRet><ListID>10000-5</ListID><Name>Opening Balance Equity</Name><FullName>Opening Balance Equity</FullName><IsActive>true</IsActive><Sublevel>0</Sublevel><AccountType>Equity</AccountType><Balance>5000.00</Balance></AccountRet>
<AccountRet><ListID>10000-6</ListID><Name>Construction Income</Name><FullName>Construction Income</FullName><IsActive>true</IsActive><Sublevel>0</Sublevel><AccountType>Income</AccountType><Balance>0.00</Balance><TotalBalance>1250.00</TotalBalance></AccountRet>
<AccountRet><ListID>10000-7</ListID><Name>Labor</Name><FullName>Construction Income:Labor</FullName><IsActive>true</IsActive><ParentRef><ListID>10000-6</ListID><FullName>Construction Income</FullName></ParentRef><Sublevel>1</Sublevel><AccountType>Income</AccountType><Balance>1250.00</Balance></AccountRet>
<AccountRet><ListID>10000-8</ListID><Name>Job Materials</Name><FullName>Job Materials</FullName><IsActive>true</IsActive><Sublevel>0</Sublevel><AccountType>CostOfGoodsSold</AccountType><Balance>400.00</Balance></AccountRet>
<AccountRet><ListID>10000-9</ListID><Name>Old Truck Loan</Name><FullName>Old Truck Loan</FullName><IsActive>false</IsActive><Sublevel>0</Sublevel><AccountType>LongTermLiability</AccountType><Balance>0.00</Balance></AccountRet>
<AccountRet><ListID>10000-10</ListID><Name>Purchase Orders</Name><FullName>Purchase Orders</FullName><IsActive>true</IsActive><Sublevel>0</Sublevel><AccountType>NonPosting</AccountType></AccountRet>";

const CUSTOMERS: &str = "\
<CustomerRet><ListID>20000-1</ListID><Name>Kristy Abercrombie</Name><FullName>Kristy Abercrombie</FullName><IsActive>true</IsActive><Sublevel>0</Sublevel><CompanyName>Abercrombie Homes</CompanyName>\
<BillAddress><Addr1>Kristy Abercrombie</Addr1><Addr2>5647 Cypress Hill Rd</Addr2><City>Bayshore</City><State>CA</State><PostalCode>94326</PostalCode></BillAddress>\
<BillAddressBlock><Addr1>Kristy Abercrombie</Addr1><Addr2>5647 Cypress Hill Rd</Addr2><Addr3>Bayshore CA 94326</Addr3></BillAddressBlock>\
<Phone>415-555-6579</Phone><Email>kristy@samplename.com</Email><TermsRef><ListID>30000-1</ListID><FullName>Net 30</FullName></TermsRef><Balance>250.00</Balance><Notes>Prefers email &amp; text</Notes></CustomerRet>
<CustomerRet><ListID>20000-2</ListID><Name>Kitchen</Name><FullName>Kristy Abercrombie:Kitchen</FullName><IsActive>false</IsActive><ParentRef><ListID>20000-1</ListID><FullName>Kristy Abercrombie</FullName></ParentRef><Sublevel>1</Sublevel></CustomerRet>";

const VENDORS: &str = "\
<VendorRet><ListID>40000-1</ListID><Name>Timberloft Lumber</Name><IsActive>true</IsActive><CompanyName>Timberloft Lumber</CompanyName>\
<VendorAddress><Addr1>Timberloft Lumber</Addr1><Addr2>8 Mill Rd</Addr2><City>Middlefield</City><State>CA</State><PostalCode>94482</PostalCode></VendorAddress>\
<Phone>415-555-1182</Phone><TermsRef><ListID>30000-2</ListID><FullName>Monthly</FullName></TermsRef><VendorTaxIdent>94-1234567</VendorTaxIdent><IsVendorEligibleFor1099>true</IsVendorEligibleFor1099><Balance>0.00</Balance></VendorRet>";

const TERMS: &str = "\
<StandardTermsRet><ListID>30000-1</ListID><Name>Net 30</Name><IsActive>true</IsActive><StdDueDays>30</StdDueDays></StandardTermsRet>
<StandardTermsRet><ListID>30000-2</ListID><Name>Monthly</Name><IsActive>true</IsActive><StdDueDays>30</StdDueDays></StandardTermsRet>
<DateDrivenTermsRet><ListID>30000-3</ListID><Name>15th of month</Name><IsActive>true</IsActive><DayOfMonthDue>15</DayOfMonthDue></DateDrivenTermsRet>";

const SERVICE_ITEM: &str = "<ItemServiceRet><ListID>50000-1</ListID><Name>Framing</Name><FullName>Framing</FullName><IsActive>true</IsActive><Sublevel>0</Sublevel>\
<SalesTaxCodeRef><FullName>Non</FullName></SalesTaxCodeRef><SalesOrPurchase><Desc>Framing labor</Desc><Price>55.00</Price><AccountRef><FullName>Construction Income:Labor</FullName></AccountRef></SalesOrPurchase></ItemServiceRet>";
const INVENTORY_ITEM: &str = "<ItemInventoryRet><ListID>50000-2</ListID><Name>Cabinet Pulls</Name><FullName>Cabinets:Cabinet Pulls</FullName><IsActive>true</IsActive><Sublevel>1</Sublevel>\
<SalesTaxCodeRef><FullName>Tax</FullName></SalesTaxCodeRef><SalesDesc>Cabinet pull, brass</SalesDesc><SalesPrice>3.00</SalesPrice><IncomeAccountRef><FullName>Construction Income</FullName></IncomeAccountRef>\
<PurchaseCost>1.50</PurchaseCost><COGSAccountRef><FullName>Job Materials</FullName></COGSAccountRef></ItemInventoryRet>";
const DISCOUNT_ITEM: &str = "<ItemDiscountRet><ListID>50000-3</ListID><Name>Prompt Pay</Name><FullName>Prompt Pay</FullName><IsActive>true</IsActive><DiscountRate>10.0</DiscountRate></ItemDiscountRet>";

const CLASSES: &str = "<ClassRet><ListID>60000-1</ListID><Name>Remodel</Name><FullName>Remodel</FullName><IsActive>true</IsActive><Sublevel>0</Sublevel></ClassRet>\
<ClassRet><ListID>60000-2</ListID><Name>New Construction</Name><FullName>New Construction</FullName><IsActive>true</IsActive><Sublevel>0</Sublevel></ClassRet>";

const INVOICES: &str = "<InvoiceRet><TxnID>1-1</TxnID><TxnNumber>1</TxnNumber><CustomerRef><FullName>Kristy Abercrombie</FullName></CustomerRef><TxnDate>2024-01-05</TxnDate><RefNumber>1001</RefNumber><Subtotal>1250.00</Subtotal><BalanceRemaining>250.00</BalanceRemaining></InvoiceRet>";
const BILLS: &str = "<BillRet><TxnID>4-1</TxnID><TxnNumber>4</TxnNumber><VendorRef><FullName>Timberloft Lumber</FullName></VendorRef><TxnDate>2024-01-08</TxnDate><AmountDue>400.00</AmountDue><RefNumber>A-77</RefNumber></BillRet>";
const BILL_PAYMENTS: &str = "<BillPaymentCheckRet><TxnID>5-1</TxnID><TxnNumber>5</TxnNumber><PayeeEntityRef><FullName>Timberloft Lumber</FullName></PayeeEntityRef><TxnDate>2024-01-25</TxnDate><Amount>400.00</Amount></BillPaymentCheckRet>";

/// A row of the Journal report: the values for columns 1 to 9 (blank ones left out, as QuickBooks does).
fn data_row(n: usize, cells: [&str; 9]) -> String {
    let cols: String = cells.iter().enumerate().filter(|(_, v)| !v.is_empty()).map(|(i, v)| format!("<ColData colID=\"{}\" value=\"{v}\" />", i + 1)).collect();
    format!("<DataRow rowNumber=\"{n}\">{cols}</DataRow>")
}

/// The Journal from `GeneralDetailReportQueryRq`: the transaction number, type, date and number
/// on each transaction's first row only, a subtotal row after each, and a total row at the end.
fn journal_report() -> String {
    let txns: [&[[&str; 9]]; 6] = [
        &[["1", "Invoice", "2024-01-05", "1001", "Kristy Abercrombie", "", "Accounts Receivable", "1250.00", ""], ["", "", "", "", "Kristy Abercrombie", "Framing labor", "Construction Income:Labor", "", "1250.00"]],
        &[["2", "Payment", "2024-01-20", "555", "Kristy Abercrombie", "", "Undeposited Funds", "1000.00", ""], ["", "", "", "", "Kristy Abercrombie", "", "Accounts Receivable", "", "1000.00"]],
        &[["3", "Deposit", "2024-01-21", "", "", "Deposit", "Checking", "1000.00", ""], ["", "", "", "", "Kristy Abercrombie", "Deposit", "Undeposited Funds", "", "1000.00"]],
        &[["4", "Bill", "2024-01-08", "A-77", "Timberloft Lumber", "", "Accounts Payable", "", "400.00"], ["", "", "", "", "Timberloft Lumber", "Lumber", "Job Materials", "400.00", ""]],
        &[["5", "Bill Pmt -Check", "2024-01-25", "240", "Timberloft Lumber", "", "Checking", "", "400.00"], ["", "", "", "", "Timberloft Lumber", "", "Accounts Payable", "400.00", ""]],
        &[["6", "General Journal", "2024-01-01", "JE-1", "", "Opening balance", "Checking", "5000.00", ""], ["", "", "", "", "", "Opening balance", "Opening Balance Equity", "", "5000.00"]],
    ];
    let mut rows = String::new();
    let mut n = 0;
    for t in txns {
        for cells in t {
            n += 1;
            rows.push_str(&data_row(n, *cells));
        }
        n += 1;
        rows.push_str(&format!("<SubtotalRow rowNumber=\"{n}\"><ColData colID=\"8\" value=\"x\" /><ColData colID=\"9\" value=\"x\" /></SubtotalRow>"));
    }
    rows.push_str("<TotalRow rowNumber=\"99\"><ColData colID=\"1\" value=\"TOTAL\" /><ColData colID=\"8\" value=\"9050.00\" /><ColData colID=\"9\" value=\"9050.00\" /></TotalRow>");
    let cols = [("TxnNumber", "Trans #"), ("TxnType", "Type"), ("Date", "Date"), ("RefNumber", "Num"), ("Name", "Name"), ("Memo", "Memo"), ("Account", "Account"), ("Debit", "Debit"), ("Credit", "Credit")];
    let desc: String = cols
        .iter()
        .enumerate()
        .map(|(i, (ty, title))| format!("<ColDesc colID=\"{}\" dataType=\"STRTYPE\"><ColTitle titleRow=\"1\" value=\"{title}\" /><ColType>{ty}</ColType></ColDesc>", i + 1))
        .collect();
    format!("<ReportRet><ReportTitle>Journal</ReportTitle><ReportSubtitle>All Transactions</ReportSubtitle><NumRows>{n}</NumRows><NumColumns>9</NumColumns><NumColTitleRows>1</NumColTitleRows>{desc}<ReportData>{rows}</ReportData></ReportRet>")
}

/// The Journal as QuickBooks for Mac's `reportAsText` is expected to return it: tab-separated,
/// with the report's title rows above its column headings.
const MAC_JOURNAL_TEXT: &str = "Rock Castle Construction\nJournal\nAll Transactions\n\n\
Trans #\tType\tDate\tNum\tName\tMemo\tAccount\tDebit\tCredit\n\
1\tInvoice\t01/05/2024\t1001\tKristy Abercrombie\t\tAccounts Receivable\t1,250.00\t\n\
\t\t\t\tKristy Abercrombie\tFraming labor\tConstruction Income:Labor\t\t1,250.00\n\
\t\t\t\t\t\t\t1,250.00\t1,250.00\n\
4\tBill\t01/08/2024\tA-77\tTimberloft Lumber\t\tAccounts Payable\t\t400.00\n\
\t\t\t\tTimberloft Lumber\tLumber\tJob Materials\t400.00\t\n\
\t\t\t\t\t\t\t400.00\t400.00\n\
6\tGeneral Journal\t01/01/2024\tJE-1\t\tOpening balance\tChecking\t5,000.00\t\n\
\t\t\t\t\tOpening balance\tOpening Balance Equity\t\t5,000.00\n\
\t\t\t\t\t\t\t5,000.00\t5,000.00\n";

/// QuickBooks, played by canned replies keyed by request element. It records every request.
struct Fake {
    replies: HashMap<&'static str, String>,
    journal: Option<String>,
    sent: Vec<String>,
}

impl Transport for Fake {
    fn execute(&mut self, request: &str) -> Result<String, String> {
        self.sent.push(request.to_string());
        let element = Query::ALL.iter().map(|q| q.element()).find(|e| request.contains(&format!("<{e}>"))).expect("a known query");
        let rs = element.trim_end_matches("Rq").to_string() + "Rs";
        Ok(self.replies.get(element).cloned().unwrap_or_else(|| {
            format!("<QBXML><QBXMLMsgsRs><{rs} statusCode=\"3120\" statusSeverity=\"Error\" statusMessage=\"Request not supported\" /></QBXMLMsgsRs></QBXML>")
        }))
    }

    fn journal_report(&mut self) -> Result<String, String> {
        match &self.journal {
            Some(text) => Ok(text.clone()),
            None => Err("QB_NO_JOURNAL: no report window".into()),
        }
    }
}

fn windows_qb() -> Fake {
    let replies = HashMap::from([
        ("CompanyQueryRq", reply("CompanyQueryRs", COMPANY)),
        ("AccountQueryRq", reply("AccountQueryRs", ACCOUNTS)),
        ("CustomerQueryRq", reply("CustomerQueryRs", CUSTOMERS)),
        ("VendorQueryRq", reply("VendorQueryRs", VENDORS)),
        ("ItemQueryRq", reply("ItemQueryRs", &format!("{SERVICE_ITEM}{INVENTORY_ITEM}{DISCOUNT_ITEM}"))),
        ("TermsQueryRq", reply("TermsQueryRs", TERMS)),
        ("ClassQueryRq", reply("ClassQueryRs", CLASSES)),
        ("InvoiceQueryRq", reply("InvoiceQueryRs", INVOICES)),
        ("BillQueryRq", reply("BillQueryRs", BILLS)),
        ("BillPaymentCheckQueryRq", reply("BillPaymentCheckQueryRs", BILL_PAYMENTS)),
        (
            "BillPaymentCreditCardQueryRq",
            "<QBXML><QBXMLMsgsRs><BillPaymentCreditCardQueryRs statusCode=\"1\" statusSeverity=\"Info\" statusMessage=\"A query request did not find a matching object in QuickBooks\" /></QBXMLMsgsRs></QBXML>".into(),
        ),
        ("GeneralDetailReportQueryRq", reply("GeneralDetailReportQueryRs", &journal_report())),
    ]);
    Fake { replies, journal: None, sent: Vec::new() }
}

fn mac_qb() -> Fake {
    let replies = HashMap::from([
        ("CompanyQueryRq", reply("CompanyQueryRs", COMPANY)),
        ("AccountQueryRq", reply("AccountQueryRs", ACCOUNTS)),
        ("CustomerQueryRq", reply("CustomerQueryRs", CUSTOMERS)),
        ("VendorQueryRq", reply("VendorQueryRs", VENDORS)),
        ("ItemServiceQueryRq", reply("ItemServiceQueryRs", SERVICE_ITEM)),
        ("ItemNonInventoryQueryRq", reply("ItemNonInventoryQueryRs", "")),
        ("ItemInventoryQueryRq", reply("ItemInventoryQueryRs", INVENTORY_ITEM)),
        ("ClassQueryRq", reply("ClassQueryRs", "")),
        ("InvoiceQueryRq", reply("InvoiceQueryRs", INVOICES)),
        ("BillQueryRq", reply("BillQueryRs", BILLS)),
        ("BillPaymentCheckQueryRq", reply("BillPaymentCheckQueryRs", "")),
        ("BillPaymentCreditCardQueryRq", reply("BillPaymentCreditCardQueryRs", "")),
    ]);
    Fake { replies, journal: Some(MAC_JOURNAL_TEXT.into()), sent: Vec::new() }
}

fn by_kind<'a>(ledger: &'a Ledger, kind: &str) -> &'a SourceTransaction {
    ledger.transactions.iter().find(|t| t.kind == kind).unwrap_or_else(|| panic!("no {kind}: {:?}", ledger.transactions.iter().map(|t| t.kind).collect::<Vec<_>>()))
}

fn account_type<'a>(ledger: &'a Ledger, full_name: &str) -> &'a str {
    ledger.accounts.iter().find(|a| a.full_name == full_name).unwrap_or_else(|| panic!("no account {full_name}")).account_type
}

fn element_names(n: &Node, out: &mut Vec<String>) {
    out.push(n.name.clone());
    for c in &n.children {
        element_names(c, out);
    }
}

/// QuickBooks for Mac 2024's request processor implements these queries (its `…QueryRq:requestTemplate:`
/// handlers); it would answer any other with "unsupported request".
const MAC_QUERIES: [&str; 17] = [
    "AccountQueryRq",
    "BillPaymentCheckQueryRq",
    "BillPaymentCreditCardQueryRq",
    "BillQueryRq",
    "ClassQueryRq",
    "CompanyQueryRq",
    "CustomerQueryRq",
    "DataExtDefQueryRq",
    "EmployeeQueryRq",
    "InvoiceQueryRq",
    "ItemInventoryQueryRq",
    "ItemNonInventoryQueryRq",
    "ItemServiceQueryRq",
    "OtherNameQueryRq",
    "PaymentMethodQueryRq",
    "PreferencesQueryRq",
    "VendorQueryRq",
];

#[test]
fn every_request_built_is_a_read_only_query() {
    for platform in [Platform::Mac, Platform::Windows] {
        for q in Query::ALL {
            let text = request(q, platform);
            ensure_read_only(&text).unwrap_or_else(|e| panic!("{q:?}: {e}"));
            let root = xml::parse(&text).unwrap();
            let msgs = root.child("QBXMLMsgsRq").unwrap();
            assert_eq!(msgs.children.len(), 1, "{q:?}: one message per request");
            assert!(msgs.children[0].name.ends_with("QueryRq"), "{q:?}");
            let mut names = Vec::new();
            element_names(&root, &mut names);
            for name in names {
                for write in ["Add", "Mod", "Del", "Void"] {
                    assert!(!name.ends_with(write) && !name.ends_with(&format!("{write}Rq")), "{q:?} builds a {name} element");
                }
            }
            assert!(text.contains(&format!("<?qbxml version=\"{}\"?>", platform.qbxml_version())));
        }
    }
}

#[test]
fn only_queries_the_platform_answers_are_sent() {
    for q in Platform::Mac.queries() {
        assert!(MAC_QUERIES.contains(&q.element()), "QuickBooks for Mac has no {}", q.element());
    }
    for platform in [Platform::Mac, Platform::Windows] {
        let mut seen = std::collections::HashSet::new();
        assert!(platform.queries().iter().all(|q| seen.insert(*q)), "{platform:?} asks something twice");
        assert!(platform.queries().iter().all(|q| Query::ALL.contains(q)));
    }
}

#[test]
fn requests_that_could_change_quickbooks_are_refused() {
    let wrap = |inner: &str| format!("<?xml version=\"1.0\"?><?qbxml version=\"13.0\"?><QBXML><QBXMLMsgsRq onError=\"stopOnError\">{inner}</QBXMLMsgsRq></QBXML>");
    for bad in [
        wrap("<AccountAddRq><AccountAdd><Name>X</Name><AccountType>Bank</AccountType></AccountAdd></AccountAddRq>"),
        wrap("<CustomerModRq><CustomerMod><ListID>1</ListID></CustomerMod></CustomerModRq>"),
        wrap("<TxnDelRq><TxnDelType>Invoice</TxnDelType><TxnID>1</TxnID></TxnDelRq>"),
        wrap("<TxnVoidRq><TxnVoidType>Check</TxnVoidType><TxnID>1</TxnID></TxnVoidRq>"),
        wrap("<DataExtDefDelRq><OwnerID>0</OwnerID></DataExtDefDelRq>"),
        // A query alongside a change still changes something.
        wrap("<AccountQueryRq /><JournalEntryAddRq><JournalEntryAdd /></JournalEntryAddRq>"),
        // A query LocalBooks never builds, even a harmless one, is not sent.
        wrap("<EmployeeQueryRq />"),
        // A write hidden inside a query element.
        wrap("<AccountQueryRq><AccountAdd><Name>X</Name></AccountAdd></AccountQueryRq>"),
        wrap(""),
        "<QBXML><QBXMLMsgsRq><AccountQueryRq /></QBXMLMsgsRq><QBXMLMsgsRq><AccountAddRq /></QBXMLMsgsRq></QBXML>".into(),
        "<QBPOSXML><QBPOSXMLMsgsRq><ItemInventoryAddRq /></QBPOSXMLMsgsRq></QBPOSXML>".into(),
    ] {
        let err = ensure_read_only(&bad).expect_err(&bad);
        assert!(err.starts_with("QB_NOT_READ_ONLY") || err.starts_with("QBXML_UNREADABLE"), "{err}");
    }
    assert!(ensure_read_only("not xml <").is_err());
}

#[test]
fn a_refused_request_never_reaches_quickbooks() {
    struct Writer(Vec<String>);
    impl Transport for Writer {
        fn execute(&mut self, request: &str) -> Result<String, String> {
            self.0.push(request.into());
            Ok(String::new())
        }
    }
    // The only way to send is through `run`, which checks first; check that it does.
    let mut t = Writer(Vec::new());
    let _ = run(&mut t, Query::Accounts, Platform::Windows);
    assert_eq!(t.0.len(), 1);
    assert!(ensure_read_only(&t.0[0]).is_ok());
}

#[test]
fn reply_status_codes() {
    let none = "<QBXML><QBXMLMsgsRs><CustomerQueryRs statusCode=\"1\" statusSeverity=\"Info\" statusMessage=\"A query request did not find a matching object in QuickBooks\" /></QBXMLMsgsRs></QBXML>";
    assert!(parse_reply(none, Query::Customers).unwrap().is_empty(), "nothing found is an empty list");
    let failed = "<QBXML><QBXMLMsgsRs><CustomerQueryRs statusCode=\"3200\" statusSeverity=\"Error\" statusMessage=\"The provided edit sequence is out-of-date.\" /></QBXMLMsgsRs></QBXML>";
    let err = parse_reply(failed, Query::Customers).unwrap_err();
    assert!(err.starts_with("QBXML_STATUS_3200") && err.contains("edit sequence"), "{err}");
    assert!(parse_reply("<QBXML><QBXMLMsgsRs /></QBXML>", Query::Customers).unwrap_err().starts_with("QBXML_NO_REPLY"));
    let ok = reply("AccountQueryRs", ACCOUNTS);
    assert_eq!(parse_reply(&ok, Query::Accounts).unwrap().len(), 10);
}

#[test]
fn records_become_the_lists_quickbooks_exports() {
    let records = |rs: &str, body: &str| xml::parse(&reply(rs, body)).unwrap().child("QBXMLMsgsRs").unwrap().children[0].children.clone();
    let days = terms_days(&records("TermsQueryRs", TERMS));
    assert_eq!(days, HashMap::from([("Net 30".to_string(), 30), ("Monthly".to_string(), 30)]));

    let customers = String::from_utf8(customers_table(&records("CustomerQueryRs", CUSTOMERS), &days)).unwrap();
    let mut lines = customers.lines();
    assert_eq!(lines.next(), Some("Customer,Company,Main Email,Main Phone,Bill to,Terms,Active Status,Notes"));
    assert!(customers.contains("\"Kristy Abercrombie\n5647 Cypress Hill Rd\nBayshore CA 94326\""), "the address block, one line each: {customers}");
    assert!(customers.contains("Prefers email & text"), "entities are read");
    assert!(customers.contains("Kristy Abercrombie:Kitchen,,,,,,Inactive,"));

    let vendors = String::from_utf8(vendors_table(&records("VendorQueryRs", VENDORS), &days)).unwrap();
    assert!(vendors.contains("Monthly (Net 30)"), "terms without days in their name get them: {vendors}");
    assert!(vendors.contains("\"Timberloft Lumber\n8 Mill Rd\nMiddlefield CA 94482\""), "an address without a block is built from its parts");

    let items = String::from_utf8(items_table(&records("ItemQueryRs", &format!("{SERVICE_ITEM}{INVENTORY_ITEM}{DISCOUNT_ITEM}")))).unwrap();
    assert_eq!(
        items.lines().collect::<Vec<_>>(),
        [
            "Item,Description,Type,Price,Account,Sales Tax Code,Active Status",
            "Framing,Framing labor,Service,55.00,Construction Income:Labor,Non,Active",
            "Cabinets:Cabinet Pulls,\"Cabinet pull, brass\",Inventory Part,3.00,Construction Income,Tax,Active",
            "Prompt Pay,,Discount,,,,Active",
        ]
    );

    let report = records("GeneralDetailReportQueryRs", &journal_report());
    let journal = String::from_utf8(report_table(&report[0]).unwrap()).unwrap();
    let mut lines = journal.lines();
    assert_eq!(lines.next(), Some("Trans #,Type,Date,Num,Name,Memo,Account,Debit,Credit"));
    assert_eq!(lines.next(), Some("1,Invoice,2024-01-05,1001,Kristy Abercrombie,,Accounts Receivable,1250.00,"));
    assert_eq!(lines.next(), Some(",,,,Kristy Abercrombie,Framing labor,Construction Income:Labor,,1250.00"));
    assert_eq!(lines.next(), Some(",,,,,,,,"), "a subtotal row ends the transaction");
}

#[test]
fn windows_export_reads_into_the_ledger() {
    let mut qb = windows_qb();
    let mut steps = Vec::new();
    let export = export(&mut qb, Platform::Windows, &mut |done, total| steps.push((done, total))).unwrap();
    assert_eq!(export.company.as_deref(), Some("Rock Castle Construction"));
    assert_eq!(steps.first(), Some(&(0, 14)));
    assert_eq!(steps.last(), Some(&(14, 14)));

    // Everything sent was a query this platform answers.
    let elements: Vec<&str> = Platform::Windows.queries().iter().map(|q| q.element()).collect();
    assert_eq!(qb.sent.len(), elements.len());
    for (sent, element) in qb.sent.iter().zip(&elements) {
        ensure_read_only(sent).unwrap();
        assert!(sent.contains(&format!("<{element}>")));
    }

    let ledger = &export.read.ledger;
    assert!(ledger.unbalanced.is_empty() && ledger.unreadable.is_empty(), "{:?}", ledger.unreadable);
    let kinds: Vec<&str> = ledger.transactions.iter().map(|t| t.kind).collect();
    assert_eq!(kinds, ["Journal entry", "Invoice", "Bill", "Payment", "Deposit", "Bill payment"], "in date order");
    let invoice = by_kind(ledger, "Invoice");
    assert_eq!((invoice.date, invoice.doc_num.as_deref()), (NaiveDate::from_ymd_opt(2024, 1, 5).unwrap(), Some("1001")));
    assert_eq!(invoice.lines.iter().map(|l| l.cents).collect::<Vec<_>>(), [-125_000, 125_000]);
    let Some(SourceDocument::Payment { applied_to, .. }) = &by_kind(ledger, "Payment").document else { panic!("payment") };
    assert_eq!((applied_to[0].transaction_id, applied_to[0].cents), (invoice.id, 100_000), "applied to the open invoice");

    // Account types come from the chart of accounts, inactive and non-posting accounts as
    // QuickBooks' own Account Listing would give them.
    assert_eq!(account_type(ledger, "Undeposited Funds"), "OTHER_CURRENT_ASSET");
    assert_eq!(account_type(ledger, "Job Materials"), "COST_OF_GOODS_SOLD");
    assert_eq!(account_type(ledger, "Old Truck Loan"), "LONG_TERM_LIABILITY");
    assert!(!ledger.accounts.iter().find(|a| a.full_name == "Old Truck Loan").unwrap().active);
    assert!(ledger.accounts.iter().all(|a| a.full_name != "Purchase Orders"), "non-posting accounts are left out");
    let labor = ledger.accounts.iter().find(|a| a.full_name == "Construction Income:Labor").unwrap();
    assert!(labor.parent_id.is_some());
    // Balance-sheet balances check the import: every leaf's matches what the Journal adds up to.
    assert_eq!(ledger.quality.accounts_matching_stored, ledger.quality.accounts_with_stored_balance);
    assert!(ledger.quality.accounts_with_stored_balance >= 5);

    let kristy = ledger.customers.iter().find(|c| c.name == "Kristy Abercrombie").unwrap();
    assert_eq!((kristy.terms_days, kristy.email.as_deref(), kristy.company_name.as_deref()), (Some(30), Some("kristy@samplename.com"), Some("Abercrombie Homes")));
    assert!(!ledger.customers.iter().find(|c| c.name == "Kristy Abercrombie:Kitchen").unwrap().active);
    let lumber = ledger.vendors.iter().find(|v| v.name == "Timberloft Lumber").unwrap();
    assert_eq!((lumber.terms_days, lumber.is_1099, lumber.tax_id.as_deref()), (Some(30), true, Some("94-1234567")));
    assert_eq!(ledger.items.iter().map(|i| (i.name.as_str(), i.kind, i.rate_cents, i.taxable)).collect::<Vec<_>>(), [
        ("Cabinets:Cabinet Pulls", "Inventory part", 300, true),
        ("Framing", "Service", 5_500, false),
    ]);

    assert_eq!(export.read.sources.len(), 5);
    assert!(export.read.sources.iter().any(|s| s == "Journal (from QuickBooks).csv: QuickBooks Desktop Journal"), "{:?}", export.read.sources);
    assert_eq!(export.notes, ["QuickBooks has 2 classes; this export does not bring classes in, so set them up under Lists › Classes"], "counts agree");
}

#[test]
fn mac_export_reads_the_journal_window() {
    let mut qb = mac_qb();
    let export = export(&mut qb, Platform::Mac, &mut |_, _| {}).unwrap();
    for sent in &qb.sent {
        ensure_read_only(sent).unwrap();
        assert!(sent.contains("<?qbxml version=\"10.0\"?>"));
        for unsupported in ["TermsQueryRq", "ItemQueryRq", "GeneralDetailReportQueryRq"] {
            assert!(!sent.contains(unsupported), "QuickBooks for Mac has no {unsupported}");
        }
    }
    let ledger = &export.read.ledger;
    assert!(ledger.unbalanced.is_empty(), "{:?}", ledger.unbalanced);
    assert_eq!(ledger.transactions.iter().map(|t| t.kind).collect::<Vec<_>>(), ["Journal entry", "Invoice", "Bill"]);
    assert_eq!(ledger.items.len(), 2, "the item types are asked one by one");
    assert_eq!(ledger.vendors[0].terms_days, None, "no terms list on a Mac, and \"Monthly\" says no days");
    assert!(export.read.sources.iter().any(|s| s == "Journal (from QuickBooks).csv: QuickBooks Desktop Journal"), "{:?}", export.read.sources);
    assert!(export.notes.is_empty(), "{:?}", export.notes);
}

#[test]
fn a_journal_missing_transactions_is_flagged() {
    let mut qb = mac_qb();
    qb.replies.insert("BillPaymentCheckQueryRq", reply("BillPaymentCheckQueryRs", BILL_PAYMENTS));
    qb.replies.insert("InvoiceQueryRq", reply("InvoiceQueryRs", &INVOICES.repeat(2)));
    let export = export(&mut qb, Platform::Mac, &mut |_, _| {}).unwrap();
    assert_eq!(export.notes, [
        "QuickBooks lists 2 invoices but the Journal holds 1: check that the Journal report covers all dates before posting",
        "QuickBooks lists 1 bill payments but the Journal holds 0: check that the Journal report covers all dates before posting",
    ]);
}

#[test]
fn missing_answers() {
    // A list QuickBooks will not send is reported and skipped.
    let mut qb = windows_qb();
    qb.replies.remove("VendorQueryRq");
    let export = export(&mut qb, Platform::Windows, &mut |_, _| {}).unwrap();
    assert!(export.notes[0].starts_with("The vendor list was not read (QBXML_STATUS_3120"), "{:?}", export.notes);
    assert_eq!(export.read.ledger.vendors.len(), 1, "still named by the Journal");

    // The chart of accounts and the Journal are needed.
    let mut qb = windows_qb();
    qb.replies.remove("AccountQueryRq");
    assert!(super::export(&mut qb, Platform::Windows, &mut |_, _| {}).err().unwrap().starts_with("QBXML_STATUS_3120"));
    let mut qb = mac_qb();
    qb.journal = None;
    assert!(super::export(&mut qb, Platform::Mac, &mut |_, _| {}).err().unwrap().starts_with("QB_NO_JOURNAL"));
}

#[test]
fn journal_type_counts_reads_both_layouts() {
    let mac = journal_type_counts(&report_text_table(MAC_JOURNAL_TEXT));
    assert_eq!(mac, HashMap::from([("invoice".into(), 1), ("bill".into(), 1), ("generaljournal".into(), 1)]));
    let online = "Date,Transaction Type,Num,Name,Memo/Description,Account,Debit,Credit\n01/03/2024,Invoice,1037,A,,AR,$1.00,\n,,,,x,Sales,,$1.00\n";
    assert!(journal_type_counts(online.as_bytes()).is_empty(), "no Type column by that name: nothing to compare");
    assert!(journal_type_counts(b"").is_empty());
}

#[test]
fn report_text_becomes_csv() {
    let csv = String::from_utf8(report_text_table("Rock Castle\nJournal\n\nDate\tMemo\tAccount\n01/05/2024\tSaid \"hi\", twice\tChecking\r\n")).unwrap();
    assert_eq!(csv, "Rock Castle\nJournal\n\nDate,Memo,Account\n01/05/2024,\"Said \"\"hi\"\", twice\",Checking\n");
}
