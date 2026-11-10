use super::*;

const ALL_KINDS: [SheetKind; 14] = [
    SheetKind::Customers,
    SheetKind::Vendors,
    SheetKind::Invoices,
    SheetKind::SalesReceipts,
    SheetKind::CreditMemos,
    SheetKind::Deposits,
    SheetKind::Bills,
    SheetKind::BillCredits,
    SheetKind::Checks,
    SheetKind::CardCharges,
    SheetKind::CardCredits,
    SheetKind::JournalEntries,
    SheetKind::CustomerPayments,
    SheetKind::BillPayments,
];

fn sheet(csv: &str) -> Sheet {
    read("people.csv", csv.as_bytes()).unwrap()
}

#[test]
fn headings_suggest_the_mapping_and_the_template_maps_itself() {
    let s = sheet("Customer Name,E-mail,Phone Number,Payment Terms,Notes\nRivera,jo@r.example,555,Net 30,\n");
    let m = suggest(SheetKind::Customers, &s.headers);
    assert_eq!((m["name"], m["email"], m["phone"], m["terms"], m["notes"]), (0, 1, 2, 3, 4));
    for kind in ALL_KINDS {
        let t = sheet(&template(kind));
        let m = suggest(kind, &t.headers);
        assert_eq!(m.len(), fields(kind).len(), "{kind:?} template maps every field");
        assert!(check(kind, &t, &m, None).problems.is_empty(), "{kind:?} template's example row is clean");
    }
    let inv = sheet("Inv #,Txn Date,Name,Qty,Price,Account\n");
    let m = suggest(SheetKind::Invoices, &inv.headers);
    assert_eq!((m["number"], m["date"], m["customer"], m["quantity"], m["rate"], m["account"]), (0, 1, 2, 3, 4, 5));
}

#[test]
fn the_heading_row_is_found_and_blank_rows_are_skipped_with_file_row_numbers() {
    let s = sheet("My customers\n\nName,Email\nAcme,a@x.test\n,\nBirch,b@x.test\n");
    assert_eq!(s.headers, ["Name", "Email"]);
    assert_eq!(s.rows.iter().map(|r| r.0).collect::<Vec<_>>(), [4, 6]);
}

#[test]
fn customer_rows_that_would_fail_are_listed_and_nothing_is_built() {
    let s = sheet("Name,Email,Terms,Active\nAcme,acme@x.test,Net 30,yes\n,b@x.test,,\nacme,not-an-email,Soon,maybe\n");
    let m = suggest(SheetKind::Customers, &s.headers);
    let c = check(SheetKind::Customers, &s, &m, None);
    let msgs: Vec<(usize, &str)> = c.problems.iter().map(|p| (p.row, p.message.as_str())).collect();
    assert_eq!(msgs[0], (3, "The name is empty"));
    assert!(msgs.iter().any(|(r, m)| *r == 4 && m.contains("also on row 2")));
    assert!(msgs.iter().any(|(r, m)| *r == 4 && m.contains("terms")));
    assert!(msgs.iter().any(|(r, m)| *r == 4 && m.contains("yes or no")));
    assert!(msgs.iter().any(|(r, m)| *r == 4 && m.contains("email")));
    assert!(to_ledger(SheetKind::Customers, &s, &m, None).is_err());
    // A required field left unmapped is a problem too.
    let unmapped = check(SheetKind::Customers, &s, &Mapping::new(), None);
    assert_eq!(unmapped.problems[0].message, "Choose the column that holds Name");
}

#[test]
fn clean_customers_and_vendors_become_a_ledger() {
    let s = sheet("Name,Company,Email,Billing address,Terms,Active\nRivera,Rivera LLC,jo@r.example,\"412 Orchard Lane\nSpringfield, OR 97477\",Net 15,yes\nOld Client,,,,,no\n");
    let m = suggest(SheetKind::Customers, &s.headers);
    let ledger = to_ledger(SheetKind::Customers, &s, &m, None).unwrap();
    assert_eq!(ledger.customers.len(), 2);
    let r = ledger.customers.iter().find(|c| c.name == "Rivera").unwrap();
    assert_eq!((r.company_name.as_deref(), r.terms_days, r.active, r.address.as_deref()), (Some("Rivera LLC"), Some(15), true, Some("412 Orchard Lane\nSpringfield, OR 97477")));
    assert!(!ledger.customers.iter().find(|c| c.name == "Old Client").unwrap().active);

    let v = sheet("Name,Tax ID,1099\nPaper Co,12-3456789,yes\n");
    let ledger = to_ledger(SheetKind::Vendors, &v, &suggest(SheetKind::Vendors, &v.headers), None).unwrap();
    assert_eq!((ledger.vendors[0].tax_id.as_deref(), ledger.vendors[0].is_1099), (Some("12-3456789"), true));
}

#[test]
fn invoice_rows_group_by_number_and_build_balanced_invoices() {
    let s = sheet(
        "Invoice no.,Date,Customer,Due date,Item,Description,Quantity,Rate,Amount,Income account\n\
         1001,03/01/2026,Rivera,03/31/2026,Mowing,March mowing,4,60.00,,Sales:Lawn\n\
         1001,03/01/2026,Rivera,,Edging,,1,25.50,25.50,Sales:Lawn\n\
         1002,03/02/2026,Birch Books,,,Consulting,,,\"1,200.00\",Consulting Income\n",
    );
    let m = suggest(SheetKind::Invoices, &s.headers);
    let c = check(SheetKind::Invoices, &s, &m, None);
    assert!(c.problems.is_empty(), "{:?}", c.problems);
    assert_eq!((c.records, c.total_cents), (2, 24_000 + 2_550 + 120_000));
    let ledger = to_ledger(SheetKind::Invoices, &s, &m, None).unwrap();
    assert!(ledger.unbalanced.is_empty());
    assert_eq!(ledger.transactions.len(), 2);
    let first = &ledger.transactions[0];
    assert_eq!((first.kind, first.doc_num.as_deref(), first.date), ("Invoice", Some("1001"), NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()));
    assert_eq!(first.lines.iter().map(|l| l.cents).sum::<i64>(), 0);
    assert!(first.document.is_some(), "becomes an invoice document");
    assert!(ledger.customers.iter().any(|c| c.name == "Birch Books"));
    assert!(ledger.accounts.iter().any(|a| a.full_name == "Sales:Lawn" && a.account_type == "INCOME"));
}

#[test]
fn invoice_rows_that_would_fail_are_listed() {
    let s = sheet(
        "Invoice no.,Date,Customer,Quantity,Rate,Amount,Income account\n\
         1001,03/01/2026,Rivera,2,10.00,20.00,Sales\n\
         1001,03/05/2026,Rivera,,,5.00,Sales\n\
         1001,03/01/2026,Birch,,,5.00,Sales\n\
         ,13/45/2026,,,,abc,\n\
         1003,03/01/2026,Rivera,,,0,Sales\n\
         1004,03/01/2026,Rivera,2,10.00,25.00,Sales\n",
    );
    let m = suggest(SheetKind::Invoices, &s.headers);
    let c = check(SheetKind::Invoices, &s, &m, None);
    let has = |row: usize, text: &str| c.problems.iter().any(|p| p.row == row && p.message.contains(text));
    assert!(has(3, "dated 03/01/2026 on row 2"), "{:?}", c.problems);
    assert!(has(4, "is for Rivera on row 2"));
    assert!(has(5, "invoice number is empty") && has(5, "customer is empty") && has(5, "doesn't read") && has(5, "\"abc\""));
    assert!(has(6, "more than zero"));
    assert!(has(7, "isn't the quantity times the rate"));
    assert!(!c.problems.iter().any(|p| p.row == 2), "row 2 is fine");
}

/// A clean invoice sheet goes through the same staging, review and posting as any other import,
/// and becomes LocalBooks invoices owed by the customers it names.
#[tokio::test]
async fn an_invoice_sheet_stages_and_posts_as_invoices() {
    use crate::qbw_ledger_import::{auto_map_accounts, batch_summary, import_lists, post_batch, stage_as, ImportMode, PostLedgerInput};
    let pool = crate::db::test_pool().await;
    let s = sheet(
        "Invoice no.,Date,Customer,Description,Quantity,Rate,Amount,Income account\n\
         2001,03/02/2026,Rivera Bakery,Pastry boxes,3,4.50,13.50,Sales\n\
         2001,03/02/2026,Rivera Bakery,Delivery,,,5.00,Delivery Income\n\
         2002,03/09/2026,Birch & Co,Consulting,2,100.00,200.00,Sales\n",
    );
    let m = suggest(SheetKind::Invoices, &s.headers);
    let ledger = to_ledger(SheetKind::Invoices, &s, &m, None).unwrap();
    let batch = stage_as(&pool, std::path::Path::new("/sheets/March invoices.csv"), 1, &ledger, Some("SHEET")).await.unwrap();
    let summary = batch_summary(&pool, batch).await.unwrap();
    assert_eq!((summary.source_format.as_str(), summary.transaction_count, summary.unbalanced.len()), ("SHEET", 2, 0));

    auto_map_accounts(&pool, batch).await.unwrap();
    import_lists(&pool, batch).await.unwrap();
    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap();
    assert_eq!(posted.full.unwrap().invoices, 2);
    let rows: Vec<(i64, String, i64, i64)> = sqlx::query_as(
        "SELECT i.invoice_number, c.name, i.total_cents, (SELECT COUNT(*) FROM invoice_items WHERE invoice_id = i.id)
         FROM invoices i JOIN customers c ON c.id = i.customer_id ORDER BY i.invoice_number",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows, [(2001, "Rivera Bakery".into(), 1_850, 2), (2002, "Birch & Co".into(), 20_000, 1)]);
}

/// Every transaction kind builds a balanced transaction of its type, its total on the account the
/// kind posts to: a debit for receipts, deposits and credits from vendors, a credit for the rest.
#[test]
fn each_transaction_kind_builds_its_document() {
    let cases: [(SheetKind, &str, &str, &str, i64); 8] = [
        (SheetKind::SalesReceipts, "Receipt no.,Date,Customer,Deposit to,Amount,Income account\n7,03/02/2026,Rivera,,40.00,Sales\n", "Sales receipt", "Undeposited Funds", -4_000),
        (SheetKind::CreditMemos, "Credit no.,Date,Customer,Amount,Income account\nCM-1,03/02/2026,Rivera,40.00,Sales\n", "Credit memo", "Accounts Receivable", 4_000),
        (SheetKind::Deposits, "Date,Bank account,Description,Amount,From account\n03/02/2026,Checking,Interest,1.25,Interest Income\n", "Deposit", "Checking", -125),
        (SheetKind::Bills, "Bill no.,Date,Vendor,Due date,Amount,Expense account\nB-9,03/02/2026,Paper Co,04/01/2026,50.00,Office Supplies\n", "Bill", "Accounts Payable", 5_000),
        (SheetKind::BillCredits, "Credit no.,Date,Vendor,Amount,Expense account\nVC-1,03/02/2026,Paper Co,5.00,Office Supplies\n", "Vendor credit", "Accounts Payable", -500),
        (SheetKind::Checks, "Check no.,Date,Payee,Bank account,Amount,Expense account\n1201,03/02/2026,City Water,Checking,80.00,Utilities\n", "Check", "Checking", 8_000),
        (SheetKind::CardCharges, "Date,Payee,Card account,Amount,Expense account\n03/02/2026,Fuel Stop,Visa,30.00,Fuel\n", "Credit card charge", "Visa", 3_000),
        (SheetKind::CardCredits, "Date,Payee,Card account,Amount,Expense account\n03/02/2026,Fuel Stop,Visa,30.00,Fuel\n", "Credit card credit", "Visa", -3_000),
    ];
    for (kind, csv, label, header, cents) in cases {
        let s = sheet(csv);
        let m = suggest(kind, &s.headers);
        let c = check(kind, &s, &m, None);
        assert!(c.problems.is_empty(), "{kind:?}: {:?}", c.problems);
        let ledger = to_ledger(kind, &s, &m, None).unwrap();
        assert!(ledger.unbalanced.is_empty(), "{kind:?}");
        let t = &ledger.transactions[0];
        assert_eq!(t.kind, label, "{kind:?}");
        assert!(t.document.is_some(), "{kind:?} becomes a document");
        assert_eq!(t.lines.iter().map(|l| l.cents).sum::<i64>(), 0, "{kind:?} balances");
        let account = ledger.accounts.iter().find(|a| a.full_name == header).unwrap_or_else(|| panic!("{kind:?}: {header}"));
        let on_header: i64 = t.lines.iter().filter(|l| l.account_id == account.id).map(|l| l.cents).sum();
        assert_eq!(on_header, cents, "{kind:?}: the total on {header}");
    }
    // A payee becomes a vendor the check is from.
    let s = sheet("Date,Payee,Bank account,Amount,Expense account\n03/02/2026,City Water,Checking,80.00,Utilities\n");
    let ledger = to_ledger(SheetKind::Checks, &s, &suggest(SheetKind::Checks, &s.headers), None).unwrap();
    assert!(ledger.vendors.iter().any(|v| v.name == "City Water"));
}

#[test]
fn rows_of_one_transaction_must_agree_and_unnumbered_rows_stand_alone() {
    let s = sheet(
        "Check no.,Date,Payee,Bank account,Amount,Expense account\n\
         1201,03/02/2026,City Water,Checking,80.00,Utilities\n\
         1201,03/02/2026,City Water,Savings,5.00,Utilities\n\
         ,03/03/2026,Fuel Stop,Checking,30.00,Fuel\n\
         ,03/04/2026,Fuel Stop,Checking,31.00,Fuel\n\
         1202,03/05/2026,,,9.00,Utilities\n",
    );
    let m = suggest(SheetKind::Checks, &s.headers);
    let c = check(SheetKind::Checks, &s, &m, None);
    let has = |row: usize, text: &str| c.problems.iter().any(|p| p.row == row && p.message.contains(text));
    assert!(has(3, "Check 1201 uses Checking on row 2, not Savings"), "{:?}", c.problems);
    assert!(has(6, "The bank account is empty"), "{:?}", c.problems);
    assert_eq!(c.problems.len(), 2, "{:?}", c.problems);
    assert_eq!(c.records, 3, "1201 and the two unnumbered checks");
}

#[test]
fn journal_entries_balance_or_are_listed() {
    let s = sheet(
        "Entry no.,Date,Account,Debit,Credit,Memo\n\
         JE-1,03/31/2026,Depreciation Expense,125.00,,March\n\
         JE-1,03/31/2026,Accumulated Depreciation,,125.00,March\n\
         JE-2,03/31/2026,Rent Expense,500.00,,\n\
         JE-2,03/31/2026,Checking,,450.00,\n\
         JE-3,03/31/2026,Checking,10.00,10.00,\n\
         JE-3,04/01/2026,Checking,,,\n",
    );
    let m = suggest(SheetKind::JournalEntries, &s.headers);
    let c = check(SheetKind::JournalEntries, &s, &m, None);
    let has = |row: usize, text: &str| c.problems.iter().any(|p| p.row == row && p.message.contains(text));
    assert!(has(4, "Entry JE-2 doesn't balance: debits $500.00, credits $450.00"), "{:?}", c.problems);
    assert!(has(6, "not both"));
    assert!(has(7, "debit or credit"));
    assert!(!c.problems.iter().any(|p| p.row <= 3));

    let ok = sheet("Entry no.,Date,Account,Debit,Credit\nJE-1,03/31/2026,Depreciation Expense,125.00,\nJE-1,03/31/2026,Accumulated Depreciation,,125.00\n");
    let m = suggest(SheetKind::JournalEntries, &ok.headers);
    let c = check(SheetKind::JournalEntries, &ok, &m, None);
    assert_eq!((c.records, c.total_cents, c.problems.len()), (1, 12_500, 0));
    let ledger = to_ledger(SheetKind::JournalEntries, &ok, &m, None).unwrap();
    let t = &ledger.transactions[0];
    assert_eq!((t.kind, t.doc_num.as_deref(), t.lines.len()), ("Journal entry", Some("JE-1"), 2));
    assert_eq!(t.lines.iter().map(|l| l.cents).sum::<i64>(), 0);
    // Without a debit or a credit column there is nothing to read.
    let none = check(SheetKind::JournalEntries, &ok, &[("number".to_string(), 0), ("date".to_string(), 1), ("account".to_string(), 2)].into_iter().collect(), None);
    assert!(none.problems.iter().any(|p| p.message.contains("Debit and Credit")));
}

/// Purchase sheets post as LocalBooks bills, credits, checks and card charges, and a journal entry
/// sheet as journal entries, through the same review as any import.
#[tokio::test]
async fn purchase_and_journal_sheets_post() {
    use crate::qbw_ledger_import::{auto_map_accounts, import_lists, post_batch, stage_as, ImportMode, PostLedgerInput};
    let pool = crate::db::test_pool().await;
    let mut n = 0;
    let mut post = |kind: SheetKind, csv: &'static str| {
        n += 1;
        let pool = pool.clone();
        let path = format!("/sheets/{n}.csv");
        async move {
            let s = sheet(csv);
            let ledger = to_ledger(kind, &s, &suggest(kind, &s.headers), None).unwrap();
            let batch = stage_as(&pool, std::path::Path::new(&path), 1, &ledger, Some("SHEET")).await.unwrap();
            auto_map_accounts(&pool, batch).await.unwrap();
            import_lists(&pool, batch).await.unwrap();
            post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap()
        }
    };
    let bills = post(SheetKind::Bills, "Bill no.,Date,Vendor,Amount,Expense account\nB-1,03/02/2026,Paper Co,50.00,Office Supplies\nB-1,03/02/2026,Paper Co,20.00,Postage\n").await;
    assert_eq!(bills.full.unwrap().bills, 1);
    let checks = post(SheetKind::Checks, "Check no.,Date,Payee,Bank account,Amount,Expense account\n1201,03/03/2026,City Water,Checking,80.00,Utilities\n").await;
    assert_eq!(checks.full.unwrap().checks, 1);
    let card = post(SheetKind::CardCharges, "Date,Payee,Card account,Amount,Expense account\n03/04/2026,Fuel Stop,Visa,30.00,Fuel\n").await;
    assert_eq!(card.full.unwrap().expenses, 1);
    let journal = post(SheetKind::JournalEntries, "Entry no.,Date,Account,Debit,Credit\nJE-1,03/31/2026,Rent Expense,500.00,\nJE-1,03/31/2026,Checking,,500.00\n").await;
    assert_eq!(journal.entries_posted, 1);

    let (number, vendor, total): (Option<String>, String, i64) =
        sqlx::query_as("SELECT b.bill_number, v.name, b.total_cents FROM bills b JOIN vendors v ON v.id = b.vendor_id").fetch_one(&pool).await.unwrap();
    assert_eq!((number.as_deref(), vendor.as_str(), total), (Some("B-1"), "Paper Co", 7_000));
    // Checking: the check and the journal entry both took money out.
    let checking: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(l.debit_cents - l.credit_cents), 0) FROM journal_lines l JOIN chart_of_accounts a ON a.id = l.account_id WHERE a.name = 'Checking'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(checking, -58_000);
}

/// Payments sheets pay invoices and bills already in the books; every row is checked against the
/// books first, and they post all together or not at all.
#[tokio::test]
async fn payments_sheets_pay_what_is_in_the_books() {
    let pool = crate::db::test_pool().await;
    let income: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(&pool).await.unwrap();
    let invoice = |customer: &str, cents: i64| {
        let pool = pool.clone();
        let input: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
            "customerName": customer, "issueDate": "2026-03-01", "dueDate": "2026-03-31",
            "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": income }]
        }))
        .unwrap();
        async move { crate::invoices::create_and_post_invoice(&pool, input).await.unwrap().invoice_number }
    };
    let (a, b) = (invoice("Rivera", 30_000).await, invoice("Chen", 10_000).await);
    let check = |kind: SheetKind, csv: String| {
        let pool = pool.clone();
        async move {
            let s = sheet(&csv);
            payments::check_against_books(&pool, kind, &s, &suggest(kind, &s.headers), None).await.unwrap()
        }
    };
    let bad = format!(
        "Date,Customer,Invoice no.,Amount\n03/10/2026,Rivera,{a},200.00\n03/11/2026,Rivera,{a},150.00\n03/12/2026,Rivera,{b},10.00\n03/12/2026,Rivera,9999,10.00\n"
    );
    let found = check(SheetKind::CustomerPayments, bad).await;
    let messages: Vec<(usize, String)> = found.problems.iter().map(|p| (p.row, p.message.clone())).collect();
    assert_eq!(
        messages,
        [
            (3, format!("Invoice {a} has $100.00 left to pay, less than $150.00")),
            (4, format!("Invoice {b} is Chen's, not Rivera's")),
            (5, "Invoice 9999 isn't in the books".into()),
        ]
    );

    // A clean sheet posts: Rivera's invoice paid in two parts into Undeposited Funds, Chen's into Checking.
    let good = format!("Date,Customer,Invoice no.,Amount,Deposit to,Payment method,Reference no.\n03/10/2026,Rivera,{a},200.00,,Check,5521\n03/11/2026,Rivera,{a},100.00,,Check,5530\n03/12/2026,chen,{b},100.00,Checking,ACH,\n");
    assert!(check(SheetKind::CustomerPayments, good.clone()).await.problems.is_empty());
    let s = sheet(&good);
    let posted = payments::post(&pool, SheetKind::CustomerPayments, &s, &suggest(SheetKind::CustomerPayments, &s.headers), None).await.unwrap();
    assert_eq!((posted.payments, posted.total_cents), (3, 40_000));
    let statuses: Vec<String> = sqlx::query_scalar("SELECT status FROM invoices ORDER BY invoice_number").fetch_all(&pool).await.unwrap();
    assert_eq!(statuses, ["PAID", "PAID"]);
    assert_eq!(crate::banking::list_undeposited(&pool).await.unwrap().len(), 2);
    // Posting the same sheet again finds nothing left to pay, and posts nothing.
    let err = payments::post(&pool, SheetKind::CustomerPayments, &s, &suggest(SheetKind::CustomerPayments, &s.headers), None).await.unwrap_err();
    assert!(err.starts_with("SHEET_PROBLEMS"), "{err}");

    // Bill payments name the vendor's bill and the account they're paid from.
    let expense: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'EXPENSE' ORDER BY id LIMIT 1").fetch_one(&pool).await.unwrap();
    let bill: crate::expenses::BillInput = serde_json::from_value(serde_json::json!({
        "vendorName": "Paper Co", "billNumber": "B-7", "billDate": "2026-03-02", "dueDate": "2026-03-20", "memo": null, "totalCents": 7_000,
        "lines": [{ "accountId": expense, "amountCents": 7_000, "memo": null }]
    }))
    .unwrap();
    crate::expenses::post_vendor_bill(&pool, bill).await.unwrap();
    let bills = "Date,Vendor,Bill no.,Amount,Pay from,Check no.\n03/15/2026,Paper Co,B-7,70.00,Checking,1042\n03/15/2026,Paper Co,B-8,5.00,Checking,\n03/15/2026,Paper Co,B-7,1.00,Sales,\n";
    let found = check(SheetKind::BillPayments, bills.into()).await;
    let messages: Vec<String> = found.problems.iter().map(|p| p.message.clone()).collect();
    assert_eq!(messages, ["Bill B-8 from Paper Co isn't in the books", "Bill B-7 from Paper Co has $0.00 left to pay, less than $1.00"]);
    let s = sheet("Date,Vendor,Bill no.,Amount,Pay from,Check no.\n03/15/2026,Paper Co,B-7,70.00,Checking,1042\n");
    payments::post(&pool, SheetKind::BillPayments, &s, &suggest(SheetKind::BillPayments, &s.headers), None).await.unwrap();
    let status: String = sqlx::query_scalar("SELECT status FROM bills").fetch_one(&pool).await.unwrap();
    assert_eq!(status, "PAID");
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);

    // Payments sheets aren't staged.
    assert!(to_ledger(SheetKind::BillPayments, &s, &suggest(SheetKind::BillPayments, &s.headers), None).is_err());
}

/// An invoice's sales tax and discount come from their own columns, for the whole invoice, and
/// the full import keeps them on the invoice.
#[tokio::test]
async fn invoices_take_sales_tax_and_a_discount() {
    use crate::qbw_ledger_import::{auto_map_accounts, import_lists, post_batch, stage_as, ImportMode, PostLedgerInput};
    let csv = "Invoice no.,Date,Customer,Amount,Income account,Sales tax,Discount\n\
               2001,03/02/2026,Rivera,100.00,Sales,8.00,10.00\n\
               2001,03/02/2026,Rivera,50.00,Sales,8.00,\n\
               2002,03/03/2026,Chen,40.00,Sales,,\n";
    let s = sheet(csv);
    let m = suggest(SheetKind::Invoices, &s.headers);
    let c = check(SheetKind::Invoices, &s, &m, None);
    assert!(c.problems.is_empty(), "{:?}", c.problems);
    assert_eq!((c.records, c.total_cents), (2, 15_000 + 800 - 1_000 + 4_000));

    // Rows that disagree on the document's tax, or a discount bigger than the lines, are listed.
    let bad = sheet("Invoice no.,Date,Customer,Amount,Income account,Sales tax,Discount\n1,03/02/2026,A,10.00,Sales,1.00,\n1,03/02/2026,A,10.00,Sales,2.00,\n2,03/02/2026,B,5.00,Sales,,6.00\n3,03/02/2026,C,5.00,Sales,-1,\n");
    let messages: Vec<String> = check(SheetKind::Invoices, &bad, &m, None).problems.into_iter().map(|p| p.message).collect();
    assert_eq!(
        messages,
        ["Invoice 1's sales tax is $1.00 on row 2, not $2.00", "Invoice 2's discount of $6.00 is more than its lines, $5.00", "Write the sales tax as zero or more"]
    );

    let pool = crate::db::test_pool().await;
    let ledger = to_ledger(SheetKind::Invoices, &s, &m, None).unwrap();
    let batch = stage_as(&pool, std::path::Path::new("/sheets/tax.csv"), 1, &ledger, Some("SHEET")).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    import_lists(&pool, batch).await.unwrap();
    let full = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap().full.unwrap();
    assert_eq!((full.invoices, full.fell_back), (2, 0), "{full:?}");
    let rows: Vec<(i64, i64, i64, i64)> =
        sqlx::query_as("SELECT invoice_number, total_cents, tax_cents, discount_cents FROM invoices ORDER BY invoice_number").fetch_all(&pool).await.unwrap();
    assert_eq!(rows, [(2001, 14_800, 800, 1_000), (2002, 4_000, 0, 0)]);
}
