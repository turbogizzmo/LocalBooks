use super::*;

use chrono::NaiveDate;

use crate::qbw_company::{LineOutcome, SourceTransaction};
use crate::qbw_documents::SourceDocument;

/// QuickBooks Desktop for Windows, Journal report exported to CSV: title rows, an indenting first
/// column, `Trans #`, type and date on a transaction's first line only, account numbers, a total
/// row after each transaction, a negative amount in parentheses, a grand total and a footer.
const DESKTOP_JOURNAL: &str = "Rock Castle Construction
Journal
All Transactions

,Trans #,Type,Date,Num,Adj,Name,Memo,Account,Class,Debit,Credit
,1,Invoice,01/05/2024,1001,,Kristy Abercrombie,,11000 · Accounts Receivable,,\"1,250.00\",
,,,,,,Kristy Abercrombie,Framing labor,40100 · Construction Income:40110 · Labor,,,\"1,250.00\"
,,,,,,,,,,\"1,250.00\",\"1,250.00\"

,2,Payment,01/20/2024,555,,Kristy Abercrombie,,12000 · Undeposited Funds,,\"1,000.00\",
,,,,,,Kristy Abercrombie,,11000 · Accounts Receivable,,,\"1,000.00\"
,,,,,,,,,,\"1,000.00\",\"1,000.00\"

,3,Deposit,01/21/2024,,,,Deposit,10100 · Checking,,\"1,000.00\",
,,,,,,Kristy Abercrombie,Deposit,12000 · Undeposited Funds,,,\"1,000.00\"
,,,,,,,,,,\"1,000.00\",\"1,000.00\"

,4,Bill,01/08/2024,A-77,,Timberloft Lumber,,20000 · Accounts Payable,,,400.00
,,,,,,Timberloft Lumber,Lumber,54000 · Job Expenses:54300 · Job Materials,,400.00,
,,,,,,,,,,400.00,400.00

,5,Bill Pmt -Check,01/25/2024,240,,Timberloft Lumber,,10100 · Checking,,,400.00
,,,,,,Timberloft Lumber,,20000 · Accounts Payable,,400.00,
,,,,,,,,,,400.00,400.00

,6,Check,01/26/2024,241,,City Water,Water bill,10100 · Checking,,,75.00
,,,,,,City Water,Water bill,65100 · Utilities:65130 · Water,,100.00,
,,,,,,City Water,Overcharge refunded,65100 · Utilities:65130 · Water,,(25.00),
,,,,,,,,,,75.00,75.00

,7,Credit Card Charge,01/27/2024,,,Office Depot,,20500 · QuickBooks Credit Card,,,52.10
,,,,,,Office Depot,Paper,64800 · Office Supplies,,52.10,
,,,,,,,,,,52.10,52.10

,8,General Journal,01/01/2024,JE-1,,,Opening balance,10100 · Checking,,\"5,000.00\",
,,,,,,,Opening balance,30000 · Opening Balance Equity,,,\"5,000.00\"
,,,,,,,,,,\"5,000.00\",\"5,000.00\"

,9,Credit Card Credit,01/29/2024,,,Office Depot,Returned paper,20500 · QuickBooks Credit Card,,10.00,
,,,,,,Office Depot,Returned paper,64800 · Office Supplies,,,10.00
,,,,,,,,,,10.00,10.00

,TOTAL,,,,,,,,,\"9,187.10\",\"9,187.10\"

11:42 AM
02/01/24
Accrual Basis
";

/// QuickBooks Online, Journal report exported to CSV: no `Trans #`, the date, type and name on the
/// first line of each transaction, `$` amounts, and transactions that follow each other with no
/// total row between them (told apart by their type, or by the open one balancing).
const ONLINE_JOURNAL: &str = "Craig's Design and Landscaping Services
Journal
All Dates

Date,Transaction Type,Num,Name,Memo/Description,Account,Debit,Credit
01/03/2024,Invoice,1037,Sonnenschein Family Store,,Accounts Receivable (A/R),$362.07,
,,,,Rock Fountain,Sales of Product Income,,$275.00
,,,,Fountain Pump,Sales of Product Income,,$12.75
,,,,Sales tax,Board of Equalization Payable,,$74.32
,,,,,,$362.07,$362.07

01/04/2024,Expense,,Hicks Hardware,,Mastercard,,$88.09
,,,,Supplies,Job Expenses:Job Materials,$88.09,
,,,,,,$88.09,$88.09

01/10/2024,Payment,,Sonnenschein Family Store,,Undeposited Funds,$362.07,
,,,,,Accounts Receivable (A/R),,$362.07
01/10/2024,Deposit,,,,Checking,$362.07,
,,,,,Undeposited Funds,,$362.07
01/12/2024,Journal Entry,5,,Owner investment,Checking,\"$1,000.00\",
,,,,Owner investment,Opening Balance Equity,,\"$1,000.00\"
01/15/2024,Deposit,,,Cash sale,Checking,$60.00,
,,,,Cash sale,Sales of Product Income,,$60.00
01/15/2024,Deposit,,,Cash sale,Checking,$40.00,
,,,,Cash sale,Sales of Product Income,,$40.00
TOTAL,,,,,,\"$2,274.30\",\"$2,274.30\"


Accrual basis Monday, February 5, 2024 10:24 AM GMT-08:00
";

const ONLINE_ACCOUNTS: &str = "Craig's Design and Landscaping Services
Account List

Full name,Type,Detail type,Description,Balance
Checking,Bank,Checking,,\"1,462.07\"
Mastercard,Credit Card,Credit Card,,88.09
Accounts Receivable (A/R),Accounts receivable (A/R),Accounts Receivable (A/R),,0.00
Undeposited Funds,Other Current Assets,Undeposited Funds,,0.00
Board of Equalization Payable,Other Current Liabilities,Sales Tax Payable,,74.32
Opening Balance Equity,Equity,Opening Balance Equity,,\"1,000.00\"
Sales of Product Income,Income,Sales of Product Income,,
Job Expenses,Expenses,Supplies & Materials,,
Job Expenses:Job Materials,Expenses,Supplies & Materials,,
Estimates,Non-Posting,Non-Posting,,
";

const ONLINE_CUSTOMERS: &str = "Craig's Design and Landscaping Services
Customer Contact List

Customer,Phone Numbers,Email,Full Name,Billing Address,Shipping Address
Sonnenschein Family Store,(650) 557-8463,Familiystore@intuit.com,Russ Sonnenschein,\"5647 Cypress Hill Ave.
Middlefield, CA 94303\",
Amy's Bird Sanctuary,(650) 555-3311,Birds@intuit.com,Amy Lauterbach,\"4581 Finch St.
Bayshore, CA 94326\",
";

/// A Desktop vendor contact list, to go with the Online journal (each list is read on its own).
const DESKTOP_VENDORS: &str = ",Vendor,Account No.,Bill from,Primary Contact,Main Phone,Tax ID,Eligible for 1099,Terms
,Hicks Hardware,,\"Hicks Hardware
42 Main St\",Geoff Hicks,555-0100,12-3456789,Yes,Net 15
,Total,,,,,,,
";

const ONLINE_ITEMS: &str = "Product/Service List

Product/Service,Type,Description,Price,Cost,Qty On Hand,Income account
Rock Fountain,Inventory,Rock Fountain,275,125,2,Sales of Product Income
Fountain Pump,Inventory,Fountain Pump,12.75,10,3,Sales of Product Income
Design,Service,Custom Design,75,,,Design income
Discount,Discount,Discount given,10%,,,
";

/// QuickBooks Desktop General Ledger: account headings (sub-accounts nested in the next column),
/// `Split`, `-SPLIT-` for a transaction with several lines, totals, and one line whose other
/// side is missing.
const DESKTOP_GENERAL_LEDGER: &str = "Rock Castle Construction
General Ledger
As of January 31, 2024

,,Type,Date,Num,Adj,Name,Memo,Split,Debit,Credit,Balance
10100 · Checking,,,,,,,,,,,0.00
,,Deposit,01/21/2024,,,,Deposit,12000 · Undeposited Funds,\"1,000.00\",,\"1,000.00\"
,,Bill Pmt -Check,01/25/2024,240,,Timberloft Lumber,,20000 · Accounts Payable,,400.00,600.00
,,Check,01/26/2024,241,,City Water,Water bill,-SPLIT-,,75.00,525.00
,,Check,01/30/2024,300,,Fix-It Repairs,,66000 · Repairs,,90.00,435.00
,,Deposit,01/31/2024,,,,Interest,70100 · Interest Income,5.00,,440.00
,,Deposit,01/31/2024,,,,Interest,70100 · Interest Income,7.00,,447.00
Total 10100 · Checking,,,,,,,,,\"1,012.00\",565.00,447.00
11000 · Accounts Receivable,,,,,,,,,,,0.00
,,Invoice,01/05/2024,1001,,Kristy Abercrombie,,40100 · Construction Income:40110 · Labor,\"1,250.00\",,\"1,250.00\"
,,Payment,01/20/2024,555,,Kristy Abercrombie,,12000 · Undeposited Funds,,\"1,000.00\",250.00
Total 11000 · Accounts Receivable,,,,,,,,,\"1,250.00\",\"1,000.00\",250.00
12000 · Undeposited Funds,,,,,,,,,,,0.00
,,Payment,01/20/2024,555,,Kristy Abercrombie,,11000 · Accounts Receivable,\"1,000.00\",,\"1,000.00\"
,,Deposit,01/21/2024,,,Kristy Abercrombie,Deposit,10100 · Checking,,\"1,000.00\",0.00
Total 12000 · Undeposited Funds,,,,,,,,,\"1,000.00\",\"1,000.00\",0.00
20000 · Accounts Payable,,,,,,,,,,,0.00
,,Bill,01/08/2024,A-77,,Timberloft Lumber,,54000 · Job Expenses:54300 · Job Materials,,400.00,400.00
,,Bill Pmt -Check,01/25/2024,240,,Timberloft Lumber,,10100 · Checking,400.00,,0.00
Total 20000 · Accounts Payable,,,,,,,,,400.00,400.00,0.00
40100 · Construction Income,,,,,,,,,,,0.00
,40110 · Labor,,,,,,,,,,0.00
,,Invoice,01/05/2024,1001,,Kristy Abercrombie,Framing labor,11000 · Accounts Receivable,,\"1,250.00\",\"1,250.00\"
,Total 40110 · Labor,,,,,,,,0.00,\"1,250.00\",\"1,250.00\"
Total 40100 · Construction Income,,,,,,,,,0.00,\"1,250.00\",\"1,250.00\"
54000 · Job Expenses,,,,,,,,,,,0.00
,54300 · Job Materials,,,,,,,,,,0.00
,,Bill,01/08/2024,A-77,,Timberloft Lumber,Lumber,20000 · Accounts Payable,400.00,,400.00
,Total 54300 · Job Materials,,,,,,,,,400.00,0.00,400.00
Total 54000 · Job Expenses,,,,,,,,,400.00,0.00,400.00
65100 · Utilities,,,,,,,,,,,0.00
,65130 · Water,,,,,,,,,,0.00
,,Check,01/26/2024,241,,City Water,Water bill,10100 · Checking,100.00,,100.00
,,Check,01/26/2024,241,,City Water,Overcharge refunded,10100 · Checking,(25.00),,75.00
,Total 65130 · Water,,,,,,,,,75.00,0.00,75.00
Total 65100 · Utilities,,,,,,,,,75.00,0.00,75.00
70100 · Interest Income,,,,,,,,,,,0.00
,,Deposit,01/31/2024,,,,Interest,10100 · Checking,,7.00,7.00
,,Deposit,01/31/2024,,,,Interest,10100 · Checking,,5.00,12.00
Total 70100 · Interest Income,,,,,,,,,0.00,12.00,12.00
TOTAL,,,,,,,,,\"4,137.00\",\"4,227.00\",-90.00
";

fn read_one(name: &str, text: &str) -> Ledger {
    read_files(&[(name.to_string(), text.as_bytes().to_vec())]).unwrap().ledger
}

fn by_kind<'a>(ledger: &'a Ledger, kind: &str) -> &'a SourceTransaction {
    ledger.transactions.iter().find(|t| t.kind == kind).unwrap_or_else(|| panic!("no {kind}: {:?}", ledger.transactions.iter().map(|t| t.kind).collect::<Vec<_>>()))
}

fn account<'a>(ledger: &'a Ledger, full_name: &str) -> &'a crate::qbw_company::SourceAccount {
    ledger.accounts.iter().find(|a| a.full_name == full_name).unwrap_or_else(|| panic!("no account {full_name}"))
}

/// Each account's total (negative = debit), by full name.
fn totals(ledger: &Ledger) -> HashMap<String, i64> {
    let mut totals = HashMap::new();
    for l in ledger.transactions.iter().flat_map(|t| &t.lines) {
        let name = ledger.accounts.iter().find(|a| a.id == l.account_id).unwrap().full_name.clone();
        *totals.entry(name).or_default() += l.cents;
    }
    totals
}

fn date(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[test]
fn reads_a_desktop_journal() {
    let read = read_files(&[("Journal.csv".into(), DESKTOP_JOURNAL.as_bytes().to_vec())]).unwrap();
    assert_eq!(read.sources, vec!["Journal.csv: QuickBooks Desktop Journal"]);
    let ledger = read.ledger;
    assert!(ledger.unbalanced.is_empty() && ledger.unreadable.is_empty(), "{:?}", ledger.unreadable);
    let kinds: Vec<&str> = ledger.transactions.iter().map(|t| t.kind).collect();
    assert_eq!(
        kinds,
        ["Journal entry", "Invoice", "Bill", "Payment", "Deposit", "Bill payment", "Check", "Credit card charge", "Credit card credit"],
        "in date order"
    );

    // Account numbers are dropped and sub-accounts keep their parent.
    let labor = account(&ledger, "Construction Income:Labor");
    assert_eq!(labor.parent_id, Some(account(&ledger, "Construction Income").id));
    assert_eq!(labor.account_type, "INCOME");
    assert_eq!(account(&ledger, "Accounts Receivable").account_type, "ACCOUNTS_RECEIVABLE");
    assert_eq!(account(&ledger, "Checking").account_type, "BANK");
    assert_eq!(account(&ledger, "QuickBooks Credit Card").account_type, "CREDIT_CARD");
    assert_eq!(account(&ledger, "Utilities:Water").account_type, "EXPENSE");

    // Trans #, type and date carry to the lines below them; (25.00) in Debit is a credit.
    let check = by_kind(&ledger, "Check");
    assert_eq!((check.date, check.doc_num.as_deref(), check.name.as_deref()), (date(2024, 1, 26), Some("241"), Some("City Water")));
    assert_eq!(check.lines.iter().map(|l| l.cents).collect::<Vec<_>>(), [7_500, -10_000, 2_500]);

    let invoice = by_kind(&ledger, "Invoice");
    assert_eq!(invoice.lines[0].cents, -125_000, "debits are negative in the ledger");
    let Some(SourceDocument::Payment { applied_to, .. }) = &by_kind(&ledger, "Payment").document else { panic!("payment") };
    assert_eq!((applied_to[0].transaction_id, applied_to[0].cents), (invoice.id, 100_000), "applied to the open invoice");
    let Some(SourceDocument::Deposit { payments }) = &by_kind(&ledger, "Deposit").document else { panic!("deposit") };
    assert_eq!(payments.len(), 1, "the deposit swept the payment");
    let Some(SourceDocument::BillPayment { applied_to, .. }) = &by_kind(&ledger, "Bill payment").document else { panic!("bill payment") };
    assert_eq!(applied_to[0].cents, 40_000);
    assert_eq!(ledger.customers.len(), 1);
    assert_eq!(ledger.vendors.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["Timberloft Lumber"]);

    // The trial balance matches the report's TOTAL row, where the (25.00) in the Debit column
    // counts against the debits; posted, it is a credit.
    let debits: i64 = ledger.transactions.iter().flat_map(|t| &t.lines).filter(|l| l.cents < 0).map(|l| -l.cents).sum();
    assert_eq!(debits - 2_500, 918_710);
}

#[test]
fn reads_an_online_journal_without_transaction_numbers() {
    let read = read_files(&[("Journal.csv".into(), ONLINE_JOURNAL.as_bytes().to_vec())]).unwrap();
    assert_eq!(read.sources, vec!["Journal.csv: QuickBooks Online Journal"]);
    let ledger = read.ledger;
    assert!(ledger.unbalanced.is_empty() && ledger.unreadable.is_empty());
    let kinds: Vec<&str> = ledger.transactions.iter().map(|t| t.kind).collect();
    assert_eq!(kinds, ["Invoice", "Expense", "Payment", "Deposit", "Journal entry", "Deposit", "Deposit"]);
    let invoice = by_kind(&ledger, "Invoice");
    assert_eq!(invoice.lines.len(), 4);
    assert_eq!(invoice.doc_num.as_deref(), Some("1037"));
    let Some(SourceDocument::Invoice { lines, .. }) = &invoice.document else { panic!("invoice") };
    assert_eq!(lines.iter().map(|l| l.description.as_deref().unwrap_or_default()).collect::<Vec<_>>(), ["Rock Fountain", "Fountain Pump", "Sales tax"]);
    // Online's expense is a check-or-card expense paid from the card here.
    let expense = by_kind(&ledger, "Expense");
    assert!(matches!(expense.document, Some(SourceDocument::Expense { .. })));
    assert_eq!(account(&ledger, "Mastercard").account_type, "CREDIT_CARD");
    // Two deposits on one day with nothing between them: each closes when it balances.
    let same_day: Vec<i64> = ledger.transactions.iter().filter(|t| t.date == date(2024, 1, 15)).map(|t| t.lines[0].cents).collect();
    assert_eq!(same_day, [-6_000, -4_000]);
    let debits: i64 = ledger.transactions.iter().flat_map(|t| &t.lines).filter(|l| l.cents < 0).map(|l| -l.cents).sum();
    assert_eq!(debits, 227_430, "the report's TOTAL row");
}

#[test]
fn a_report_with_its_lists_takes_their_types_details_and_balances() {
    let files: Vec<(String, Vec<u8>)> = [
        ("Customers.csv", ONLINE_CUSTOMERS),
        ("Journal.csv", ONLINE_JOURNAL),
        ("Account List.csv", ONLINE_ACCOUNTS),
        ("Vendors.csv", DESKTOP_VENDORS),
        ("Items.csv", ONLINE_ITEMS),
    ]
    .iter()
    .map(|(n, t)| (n.to_string(), t.as_bytes().to_vec()))
    .collect();
    let read = read_files(&files).unwrap();
    assert_eq!(read.main, 1, "named after the journal");
    assert_eq!(
        read.sources,
        [
            "Customers.csv: QuickBooks Online Customer list",
            "Journal.csv: QuickBooks Online Journal",
            "Account List.csv: QuickBooks Online Account Listing",
            "Vendors.csv: QuickBooks Desktop Vendor list",
            "Items.csv: QuickBooks Online Item list",
        ]
    );
    let ledger = read.ledger;
    assert!(ledger.unbalanced.is_empty());
    assert_eq!(account(&ledger, "Undeposited Funds").account_type, "OTHER_CURRENT_ASSET");
    assert_eq!(account(&ledger, "Board of Equalization Payable").account_type, "OTHER_CURRENT_LIABILITY");
    assert_eq!(account(&ledger, "Job Expenses:Job Materials").parent_id, Some(account(&ledger, "Job Expenses").id));
    assert!(!ledger.accounts.iter().any(|a| a.name == "Estimates"), "non-posting accounts are not imported");
    // Every balance-sheet balance in the listing matches the journal.
    assert_eq!(account(&ledger, "Checking").stored_balance_cents, Some(-146_207));
    assert_eq!(account(&ledger, "Mastercard").stored_balance_cents, Some(8_809));
    assert_eq!((ledger.quality.accounts_matching_stored, ledger.quality.accounts_with_stored_balance), (6, 6));

    assert_eq!(ledger.customers.len(), 2, "a customer with no transactions is kept");
    let store = ledger.customers.iter().find(|c| c.name == "Sonnenschein Family Store").unwrap();
    assert_eq!(store.email.as_deref(), Some("Familiystore@intuit.com"));
    assert_eq!(store.address.as_deref(), Some("5647 Cypress Hill Ave.\nMiddlefield, CA 94303"));
    let hicks = &ledger.vendors[0];
    assert_eq!((hicks.name.as_str(), hicks.tax_id.as_deref(), hicks.is_1099, hicks.terms_days), ("Hicks Hardware", Some("12-3456789"), true, Some(15)));
    let Some(SourceDocument::Expense { vendor_id, .. }) = by_kind(&ledger, "Expense").document else { panic!("expense") };
    assert_eq!(vendor_id, Some(hicks.id), "the payee is the listed vendor");

    assert_eq!(ledger.items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["Design", "Fountain Pump", "Rock Fountain"], "the discount is skipped");
    let pump = ledger.items.iter().find(|i| i.name == "Fountain Pump").unwrap();
    assert_eq!((pump.kind, pump.rate_cents), ("Inventory part", 1_275));
    assert_eq!(pump.income_account_id, Some(account(&ledger, "Sales of Product Income").id));
    assert_eq!(account(&ledger, "Design income").account_type, "INCOME");
}

#[test]
fn lists_alone_import_as_lists() {
    let ledger = read_one("Account List.csv", ONLINE_ACCOUNTS);
    assert!(ledger.transactions.is_empty());
    assert_eq!(ledger.accounts.len(), 9);
    assert_eq!(account(&ledger, "Accounts Receivable (A/R)").account_type, "ACCOUNTS_RECEIVABLE");
}

#[test]
fn rebuilds_transactions_from_a_general_ledger() {
    let read = read_files(&[("General Ledger.csv".into(), DESKTOP_GENERAL_LEDGER.as_bytes().to_vec())]).unwrap();
    assert_eq!(read.sources, vec!["General Ledger.csv: QuickBooks Desktop General Ledger"]);
    let ledger = read.ledger;
    let kinds: Vec<&str> = ledger.transactions.iter().map(|t| t.kind).collect();
    assert_eq!(kinds, ["Invoice", "Bill", "Payment", "Deposit", "Bill payment", "Check", "Deposit", "Deposit"]);
    // Check 300 names Repairs as its other side, but Repairs has no such line: it is listed.
    assert_eq!(ledger.unbalanced.len(), 1);
    assert_eq!((ledger.unbalanced[0].doc_num.as_deref(), ledger.unbalanced[0].imbalance_cents()), (Some("300"), 9_000));

    // The three-line check comes back together, its bank line first.
    let check = by_kind(&ledger, "Check");
    assert_eq!(check.lines.iter().map(|l| l.cents).collect::<Vec<_>>(), [7_500, -10_000, 2_500]);
    // Two interest deposits on one day pair up by amount.
    let interest: Vec<Vec<i64>> = ledger.transactions.iter().filter(|t| t.date == date(2024, 1, 31)).map(|t| t.lines.iter().map(|l| l.cents).collect()).collect();
    assert_eq!(interest, [vec![-500, 500], vec![-700, 700]]);
    assert_eq!(account(&ledger, "Construction Income:Labor").parent_id, Some(account(&ledger, "Construction Income").id));
    let Some(SourceDocument::Payment { applied_to, .. }) = &by_kind(&ledger, "Payment").document else { panic!("payment") };
    assert_eq!(applied_to[0].cents, 100_000);

    // Each account's balance is the report's, less the unpaired check.
    let totals = totals(&ledger);
    assert_eq!(totals["Checking"], -(100_000 - 40_000 - 7_500 + 500 + 700));
    assert_eq!(totals["Accounts Receivable"], -25_000);
    assert_eq!(totals["Undeposited Funds"], 0);
    assert_eq!(totals["Utilities:Water"], -7_500);
    assert_eq!(totals.values().sum::<i64>(), 0);
}

#[test]
fn the_journal_and_the_general_ledger_agree() {
    let journal = totals(&read_one("Journal.csv", DESKTOP_JOURNAL));
    let general = totals(&read_one("General Ledger.csv", DESKTOP_GENERAL_LEDGER));
    for name in ["Accounts Receivable", "Undeposited Funds", "Accounts Payable", "Construction Income:Labor", "Job Expenses:Job Materials", "Utilities:Water"] {
        assert_eq!(journal[name], general[name], "{name}");
    }
}

#[test]
fn an_unreadable_amount_is_reported_never_guessed() {
    let text = DESKTOP_JOURNAL.replace("(25.00)", "25.0O");
    let ledger = read_one("Journal.csv", &text);
    assert_eq!(ledger.unbalanced.len(), 1, "the check is listed, not posted");
    assert_eq!(ledger.unbalanced[0].doc_num.as_deref(), Some("241"));
    assert_eq!(ledger.unreadable.len(), 1);
    assert_eq!((ledger.unreadable[0].kind, ledger.unreadable[0].outcome), ("Check", LineOutcome::Unreadable));
    // Fractions of a cent are not rounded either.
    let ledger = read_one("Journal.csv", &DESKTOP_JOURNAL.replace("52.10,", "52.105,"));
    assert_eq!(ledger.unbalanced.len(), 1);
}

#[test]
fn day_first_dates_are_read_when_the_column_needs_it() {
    let text = ONLINE_JOURNAL.replace("01/03/2024", "03/01/2024").replace("01/15/2024", "15/01/2024").replace("01/10/2024", "10/01/2024").replace("01/12/2024", "12/01/2024").replace("01/04/2024", "04/01/2024");
    let ledger = read_one("Journal.csv", &text);
    assert_eq!(by_kind(&ledger, "Invoice").date, date(2024, 1, 3));
}

#[test]
fn refuses_files_that_are_not_reports() {
    let err = |name: &str, text: &str| read_files(&[(name.to_string(), text.as_bytes().to_vec())]).err().unwrap();
    assert!(err("Bank.csv", "Date,Description,Amount\n1/1/2024,Coffee,-5.00\n").starts_with("REPORT_UNRECOGNISED: Bank.csv"));
    assert!(err("Notes.csv", "").starts_with("REPORT_UNRECOGNISED"));
    let amount_only = "General Ledger\n\nDate,Transaction Type,Num,Name,Memo/Description,Split,Amount,Balance\n01/03/2024,Invoice,1037,Store,,Sales,362.07,362.07\n";
    let e = err("GL.csv", amount_only);
    assert!(e.starts_with("REPORT_NEEDS_DEBIT_CREDIT: GL.csv") && e.contains("add the Debit and Credit columns"), "{e}");
    assert!(err("Book.xlsx", "not a zip").starts_with("REPORT_UNREADABLE: Book.xlsx"));
    let both = read_files(&[("J.csv".into(), DESKTOP_JOURNAL.as_bytes().to_vec()), ("GL.csv".into(), DESKTOP_GENERAL_LEDGER.as_bytes().to_vec())]);
    assert!(both.err().unwrap().starts_with("REPORT_OVERLAP"));
}

#[test]
fn overlapping_desktop_journals_are_read_once() {
    let files = vec![("2024 a.csv".to_string(), DESKTOP_JOURNAL.as_bytes().to_vec()), ("2024 b.csv".to_string(), DESKTOP_JOURNAL.as_bytes().to_vec())];
    assert_eq!(read_files(&files).unwrap().ledger.transactions.len(), 9);
}

#[test]
fn excel_serial_dates_and_text_dates() {
    assert_eq!(serial_date("45296").as_deref(), Some("2024-01-05"));
    assert_eq!(serial_date("45296.5").as_deref(), Some("2024-01-05"));
    assert_eq!(serial_date("20240105"), None, "eight digits are a yyyymmdd date");
    let dates = Dates { order: DateOrder::Mdy };
    assert_eq!(dates.read("1/15/2024 12:00:00 AM"), "2024-01-15");
    assert_eq!(dates.read("Jan 15, 2024"), "2024-01-15");
    assert_eq!(dates.read("2024-01-15"), "2024-01-15");
}

/// A workbook with one sheet of the given rows: numbers are written as numbers, text as inline
/// strings, and `Date(serial)` as a number in a date format (as QuickBooks writes dates).
enum X {
    T(&'static str),
    N(&'static str),
    Date(u32),
}

fn workbook(rows: &[Vec<Option<X>>]) -> Vec<u8> {
    use std::io::Write;
    let mut sheet = String::from(r#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#);
    for (r, row) in rows.iter().enumerate() {
        sheet += &format!(r#"<row r="{}">"#, r + 1);
        for (c, value) in row.iter().enumerate() {
            let at = format!("{}{}", (b'A' + c as u8) as char, r + 1);
            match value {
                None => {}
                Some(X::T(t)) => sheet += &format!(r#"<c r="{at}" t="inlineStr"><is><t>{t}</t></is></c>"#),
                Some(X::N(n)) => sheet += &format!(r#"<c r="{at}"><v>{n}</v></c>"#),
                Some(X::Date(d)) => sheet += &format!(r#"<c r="{at}" s="1"><v>{d}</v></c>"#),
            }
        }
        sheet += "</row>";
    }
    sheet += "</sheetData></worksheet>";
    let parts = [
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/></Types>"#.to_string(),
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_string(),
        ),
        (
            "xl/workbook.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#.to_string(),
        ),
        (
            "xl/_rels/workbook.xml.rels",
            r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#.to_string(),
        ),
        (
            "xl/styles.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="14" applyNumberFormat="1"/></cellXfs></styleSheet>"#.to_string(),
        ),
        ("xl/worksheets/sheet1.xml", sheet),
    ];
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, body) in parts {
        zip.start_file(name, options).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// QuickBooks Desktop for Mac's Excel export: title rows, empty spacer columns between the real
/// ones, numbers stored as numbers, dates as Excel dates (and one as a bare serial number).
#[test]
fn reads_a_desktop_mac_journal_workbook() {
    use X::*;
    let header = vec![None, Some(T("Trans #")), None, Some(T("Type")), None, Some(T("Date")), Some(T("Num")), Some(T("Name")), Some(T("Memo")), Some(T("Account")), Some(T("Debit")), Some(T("Credit"))];
    let rows = vec![
        vec![Some(T("Rock Castle Construction"))],
        vec![Some(T("Journal"))],
        vec![Some(T("January 2024"))],
        header,
        vec![None, Some(N("1")), None, Some(T("Invoice")), None, Some(Date(45296)), Some(N("1001")), Some(T("Kristy Abercrombie")), None, Some(T("Accounts Receivable")), Some(N("1250")), None],
        vec![None, None, None, None, None, None, None, Some(T("Kristy Abercrombie")), Some(T("Framing")), Some(T("Construction Income")), None, Some(N("1250"))],
        vec![None, None, None, None, None, None, None, None, None, None, Some(N("1250")), Some(N("1250"))],
        vec![None, Some(N("2")), None, Some(T("Check")), None, Some(N("45317")), Some(N("241")), Some(T("City Water")), None, Some(T("Checking")), None, Some(N("75.5"))],
        vec![None, None, None, None, None, None, None, Some(T("City Water")), None, Some(T("Utilities")), Some(N("75.5")), None],
        vec![Some(T("TOTAL")), None, None, None, None, None, None, None, None, None, Some(N("1325.5")), Some(N("1325.5"))],
    ];
    let read = read_files(&[("Journal.xlsx".into(), workbook(&rows))]).unwrap();
    assert_eq!(read.sources, vec!["Journal.xlsx: QuickBooks Desktop Journal"]);
    let ledger = read.ledger;
    assert!(ledger.unbalanced.is_empty() && ledger.unreadable.is_empty(), "{:?}", ledger.unreadable);
    let invoice = by_kind(&ledger, "Invoice");
    assert_eq!((invoice.date, invoice.doc_num.as_deref(), invoice.lines[0].cents), (date(2024, 1, 5), Some("1001"), -125_000));
    let check = by_kind(&ledger, "Check");
    assert_eq!((check.date, check.lines[0].cents), (date(2024, 1, 26), 7_550));
}

/// Staged and posted as journal entries, the books' trial balance is the report's, account by
/// account; a full import makes the native documents.
#[tokio::test]
async fn round_trip_posts_the_reports_trial_balance() {
    use crate::qbw_ledger_import::{auto_map_accounts, batch_summary, post_batch, stage, ImportMode, PostLedgerInput};
    let ledger = read_one("Journal.csv", DESKTOP_JOURNAL);
    let expected = totals(&ledger);

    let pool = crate::db::test_pool().await;
    let batch = stage(&pool, Path::new("/exports/Journal.csv"), DESKTOP_JOURNAL.len() as i64, &ledger).await.unwrap();
    let summary = batch_summary(&pool, batch).await.unwrap();
    assert_eq!((summary.source_format.as_str(), summary.transaction_count), ("REPORT", 9));
    auto_map_accounts(&pool, batch).await.unwrap();
    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap();
    assert_eq!(posted.entries_posted, 9);

    let rows: Vec<(i64, i64, i64)> = sqlx::query_as(
        "SELECT l.account_id, COALESCE(SUM(l.debit_cents), 0), COALESCE(SUM(l.credit_cents), 0) FROM journal_lines l
           JOIN journal_entries e ON e.id = l.journal_entry_id WHERE e.status = 'POSTED' GROUP BY l.account_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let (debits, credits) = rows.iter().fold((0, 0), |(d, c), r| (d + r.1, c + r.2));
    assert_eq!((debits - 2_500, credits - 2_500), (918_710, 918_710), "the report's TOTAL row, less the (25.00) debit that posts as a credit");
    // Account by account, through the mapping step 2 shows.
    let mapped = batch_summary(&pool, batch).await.unwrap().accounts;
    for (full_name, cents) in &expected {
        let account_id = mapped.iter().find(|a| &a.full_name == full_name).and_then(|a| a.account_id).unwrap_or_else(|| panic!("{full_name} not mapped"));
        let row = rows.iter().find(|r| r.0 == account_id).unwrap_or_else(|| panic!("{full_name} not posted"));
        assert_eq!(row.2 - row.1, *cents, "{full_name}");
    }

    let pool = crate::db::test_pool().await;
    let batch = stage(&pool, Path::new("/exports/Journal.csv"), DESKTOP_JOURNAL.len() as i64, &ledger).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let full = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap().full.unwrap();
    assert_eq!(
        (full.invoices, full.payments, full.deposits, full.bills, full.bill_payments, full.expenses, full.card_credits),
        (1, 1, 1, 1, 1, 1, 1),
        "{full:?}"
    );
    // The check has a line credited back, so it posts as a journal entry, with the general
    // journal entry.
    assert_eq!((full.checks, full.fell_back, full.journal_entries), (0, 1, 2), "{full:?}");
    let (status, paid): (String, i64) = sqlx::query_as("SELECT status, amount_paid_cents FROM invoices").fetch_one(&pool).await.unwrap();
    assert_eq!((status.as_str(), paid), ("PARTIAL", 100_000));
    assert_eq!(sqlx::query_scalar::<_, String>("SELECT status FROM bills").fetch_one(&pool).await.unwrap(), "PAID");
}
