use super::*;
use crate::db::test_pool;
use crate::qbw_company::{LedgerQuality, LineOutcome, SourceAccount, SourceItem, SourceLine, SourceParty, SourceTransaction, UnreadableLine};

fn account(id: i64, name: &str, kind: &'static str, parent: Option<i64>) -> SourceAccount {
    SourceAccount {
        id,
        name: name.into(),
        full_name: name.into(),
        account_type: kind,
        parent_id: parent,
        active: true,
        stored_balance_cents: None,
    }
}

fn txn(id: i64, date: &str, lines: &[(i64, i64)]) -> SourceTransaction {
    SourceTransaction {
        id,
        kind: "Check",
        date: date.parse().unwrap(),
        doc_num: Some(id.to_string()),
        name: Some("Vendor".into()),
        memo: None,
        lines: lines.iter().map(|&(account_id, cents)| SourceLine { account_id, cents, memo: None }).collect(),
        document: None,
    }
}

fn ledger() -> Ledger {
    Ledger {
        accounts: vec![
            account(1, "Riverside Bank", "BANK", None),
            account(2, "Hosting", "EXPENSE", None),
            account(3, "Domain Names", "EXPENSE", Some(2)),
            account(4, "Accounts Receivable", "ACCOUNTS_RECEIVABLE", None),
            account(5, "Unused", "EXPENSE", None),
        ],
        // A check: debit Internet 120.00, credit Checking. Negative = debit, as in the file.
        transactions: vec![txn(10, "2023-03-01", &[(3, -12_000), (1, 12_000)]), txn(11, "2023-03-02", &[(4, -500), (1, 500)])],
        unbalanced: vec![txn(12, "2023-03-03", &[(3, -700)])],
        unreadable: vec![UnreadableLine {
            kind: "Check",
            transaction_id: Some(13),
            date: "2023-03-04".parse().ok(),
            doc_num: Some("13".into()),
            name: None,
            account_id: Some(3),
            cents: Some(-250),
            outcome: LineOutcome::Recovered,
        }],
        customers: vec![],
        vendors: vec![],
        items: vec![],
        quality: LedgerQuality::default(),
    }
}

async fn balance(pool: &SqlitePool, account_id: i64) -> i64 {
    sqlx::query_scalar(
        "SELECT COALESCE(SUM(l.debit_cents - l.credit_cents), 0) FROM journal_lines l
             JOIN journal_entries e ON e.id = l.journal_entry_id WHERE l.account_id = ?1 AND e.status = 'POSTED'",
    )
    .bind(account_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// A full import: an invoice with sales tax, a prepayment dated before it, a second payment,
/// a deposit with a fee taken out, and an invoice whose customer is missing (falls back).
#[tokio::test]
async fn a_full_import_recreates_invoices_payments_and_deposits() {
    use crate::qbw_documents::{Applied, DocumentLine, SourceDocument};
    let pool = test_pool().await;
    let doc = |id: i64, kind: &'static str, date: &str, lines: &[(i64, i64)], document: SourceDocument| SourceTransaction {
        kind,
        document: Some(document),
        ..txn(id, date, lines)
    };
    let invoice = |customer_id| SourceDocument::Invoice {
        customer_id,
        number: Some("501".into()),
        due_date: "2024-02-09".parse().ok(),
        lines: vec![
            DocumentLine { item_id: Some(200), description: None, quantity_e4: 20_000, rate_cents: 5_000, cents: 10_000, account_id: 6 },
            DocumentLine { item_id: None, description: Some("Sales tax".into()), quantity_e4: 10_000, rate_cents: 1_000, cents: 1_000, account_id: 8 },
        ],
    };
    let payment = |cents| SourceDocument::Payment {
        customer_id: 100,
        reference: Some("1234".into()),
        method: "CHECK".into(),
        applied_to: vec![Applied { transaction_id: 20, cents }],
    };
    let ledger = Ledger {
        accounts: vec![
            account(1, "Riverside Bank", "BANK", None),
            account(4, "Accounts Receivable", "ACCOUNTS_RECEIVABLE", None),
            account(6, "Consulting Income", "INCOME", None),
            account(7, "Undeposited Funds", "OTHER_CURRENT_ASSET", None),
            account(8, "Sales Tax Payable", "OTHER_CURRENT_LIABILITY", None),
            account(9, "Merchant Fees", "EXPENSE", None),
        ],
        transactions: vec![
            doc(21, "Payment", "2024-01-05", &[(7, -6_000), (4, 6_000)], payment(6_000)),
            doc(20, "Invoice", "2024-01-10", &[(4, -11_000), (6, 10_000), (8, 1_000)], invoice(100)),
            doc(22, "Payment", "2024-01-20", &[(7, -5_000), (4, 5_000)], payment(5_000)),
            doc(
                23,
                "Deposit",
                "2024-01-21",
                &[(1, -10_900), (9, -100), (7, 6_000), (7, 5_000)],
                SourceDocument::Deposit { payments: vec![Applied { transaction_id: 21, cents: 6_000 }, Applied { transaction_id: 22, cents: 5_000 }] },
            ),
            doc(24, "Invoice", "2024-01-25", &[(4, -2_000), (6, 2_000)], invoice(999)),
        ],
        unbalanced: vec![],
        unreadable: vec![],
        customers: vec![SourceParty {
            id: 100,
            name: "Acme Hosting".into(),
            parent_id: None,
            company_name: None,
            email: None,
            phone: None,
            address: None,
            terms_days: Some(30),
            tax_id: None,
            is_1099: false,
            active: true,
            balance_cents: Some(2_000),
            notes: None,
        }],
        vendors: vec![],
        items: vec![SourceItem {
            id: 200,
            name: "Consulting".into(),
            description: None,
            rate_cents: 5_000,
            income_account_id: Some(6),
            kind: "Service",
            taxable: true,
            active: true,
        }],
        quality: LedgerQuality::default(),
    };
    let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &ledger).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let summary = batch_summary(&pool, batch).await.unwrap();
    assert_eq!(summary.documents.iter().map(|d| d.1).sum::<i64>(), 5);

    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap();
    let full = posted.full.unwrap();
    assert_eq!((full.invoices, full.payments, full.deposits, full.fell_back), (1, 2, 1, 1), "{full:?}");

    let (number, status, total, tax, items): (i64, String, i64, i64, i64) = sqlx::query_as(
        "SELECT invoice_number, status, total_cents, tax_cents, (SELECT COUNT(*) FROM invoice_items WHERE invoice_id = i.id) FROM invoices i",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((number, status.as_str(), total, tax, items), (501, "PAID", 11_000, 1_000, 1), "the prepayment applies too");
    let (deposit_total, linked): (i64, i64) =
        sqlx::query_as("SELECT d.total_cents, (SELECT COUNT(invoice_payment_id) FROM deposit_lines WHERE deposit_id = d.id) FROM deposits d")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((deposit_total, linked), (11_000, 2), "the deposit is gross, with both payments swept out of Undeposited Funds");
    assert!(crate::banking::list_undeposited(&pool).await.unwrap().is_empty());

    // The books match the file exactly: the bank got the net, the fee is an expense.
    let summary = batch_summary(&pool, batch).await.unwrap();
    let mapped = |name: &str| summary.accounts.iter().find(|a| a.full_name == name).and_then(|a| a.account_id).unwrap();
    assert_eq!(balance(&pool, mapped("Riverside Bank")).await, 10_900);
    assert_eq!(balance(&pool, mapped("Merchant Fees")).await, 100);
    assert_eq!(balance(&pool, mapped("Accounts Receivable")).await, 2_000);

    assert_eq!(summary.mode, "FULL");
    assert_eq!(summary.fallbacks.len(), 1);
    assert_eq!(summary.fallbacks[0].reason, "its customer was not imported");
    let err = reverse_batch(&pool, batch).await.unwrap_err();
    assert!(err.starts_with("QBW_FULL_USE_START_OVER"), "{err}");
}

/// The purchase side of a full import: a bill paid by a bill payment dated before it, a check,
/// a card charge, and a card credit (no LocalBooks form, so a journal entry).
#[tokio::test]
async fn a_full_import_recreates_bills_payments_checks_and_card_charges() {
    use crate::qbw_documents::{Applied, DocumentLine, SourceDocument};
    let pool = test_pool().await;
    let doc = |id: i64, kind: &'static str, date: &str, lines: &[(i64, i64)], document: SourceDocument| SourceTransaction {
        kind,
        document: Some(document),
        ..txn(id, date, lines)
    };
    let expense = |number: Option<&str>| SourceDocument::Expense { vendor_id: Some(300), number: number.map(str::to_string), applied_to: vec![] };
    let ledger = Ledger {
        accounts: vec![
            account(1, "Riverside Bank", "BANK", None),
            account(5, "Accounts Payable", "ACCOUNTS_PAYABLE", None),
            account(10, "Office Expense", "EXPENSE", None),
            account(11, "Visa", "CREDIT_CARD", None),
            account(12, "Supplies", "EXPENSE", None),
        ],
        transactions: vec![
            doc(
                31,
                "Bill payment",
                "2024-02-05",
                &[(1, 5_000), (5, -5_000)],
                SourceDocument::BillPayment { vendor_id: Some(300), reference: Some("990".into()), applied_to: vec![Applied { transaction_id: 30, cents: 5_000 }] },
            ),
            doc(
                30,
                "Bill",
                "2024-02-10",
                &[(5, 5_000), (10, -5_000)],
                SourceDocument::Bill {
                    vendor_id: 300,
                    number: Some("B-7".into()),
                    due_date: "2024-03-10".parse().ok(),
                    lines: vec![DocumentLine { item_id: None, description: Some("Paper".into()), quantity_e4: 10_000, rate_cents: 5_000, cents: -5_000, account_id: 10 }],
                },
            ),
            doc(32, "Check", "2024-02-12", &[(1, 2_500), (12, -2_500)], expense(Some("1001"))),
            doc(33, "Credit card charge", "2024-02-13", &[(11, 1_200), (10, -1_200)], expense(None)),
            doc(34, "Credit card charge", "2024-02-14", &[(11, -300), (10, 300)], expense(None)),
        ],
        unbalanced: vec![],
        unreadable: vec![],
        customers: vec![],
        vendors: vec![SourceParty {
            id: 300,
            name: "Paper Supply Co".into(),
            parent_id: None,
            company_name: None,
            email: None,
            phone: None,
            address: None,
            terms_days: None,
            tax_id: None,
            is_1099: false,
            active: true,
            balance_cents: Some(0),
            notes: None,
        }],
        items: vec![],
        quality: LedgerQuality::default(),
    };
    let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &ledger).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap();
    let full = posted.full.unwrap();
    assert_eq!((full.bills, full.bill_payments, full.checks, full.expenses, full.fell_back), (1, 1, 1, 1, 1), "{full:?}");

    let (status, bill_number, lines): (String, Option<String>, i64) =
        sqlx::query_as("SELECT status, bill_number, (SELECT COUNT(*) FROM bill_lines WHERE bill_id = b.id) FROM bills b").fetch_one(&pool).await.unwrap();
    assert_eq!((status.as_str(), bill_number.as_deref(), lines), ("PAID", Some("B-7"), 1), "the payment dated before the bill still applies");
    let checks: Vec<(String, Option<String>, i64)> =
        sqlx::query_as("SELECT kind, check_number, total_cents FROM expenses ORDER BY id").fetch_all(&pool).await.unwrap();
    assert_eq!(checks, vec![("CHECK".into(), Some("1001".into()), 2_500), ("EXPENSE".into(), None, 1_200)]);

    let summary = batch_summary(&pool, batch).await.unwrap();
    assert_eq!(summary.fallbacks.len(), 1);
    assert_eq!(summary.fallbacks[0].reason, "it is a refund or card credit, which has no LocalBooks form");
    let mapped = |name: &str| summary.accounts.iter().find(|a| a.full_name == name).and_then(|a| a.account_id).unwrap();
    assert_eq!(balance(&pool, mapped("Riverside Bank")).await, -7_500);
    assert_eq!(balance(&pool, mapped("Visa")).await, -900);
    assert_eq!(balance(&pool, mapped("Office Expense")).await, 5_900);
    assert_eq!(balance(&pool, mapped("Accounts Payable")).await, 0);
}

/// Transfers, estimates and purchase orders in a full import. Estimates and purchase orders
/// post nothing: one that cannot be brought in is listed, and none counts as postable.
#[tokio::test]
async fn a_full_import_brings_transfers_estimates_and_purchase_orders() {
    use crate::qbw_documents::{DocumentLine, SourceDocument};
    let pool = test_pool().await;
    let line = |item_id, cents, account_id| DocumentLine { item_id, description: None, quantity_e4: 30_000, rate_cents: 1_000, cents, account_id };
    let estimate = |customer_id| SourceDocument::Estimate {
        customer_id,
        date: "2024-04-01".parse().unwrap(),
        number: Some("77".into()),
        expires: "2024-05-01".parse().ok(),
        memo: Some("Spring work".into()),
        lines: vec![line(Some(200), 3_000, 6), DocumentLine { item_id: None, description: Some("Tax".into()), quantity_e4: 10_000, rate_cents: 240, cents: 240, account_id: 8 }],
    };
    let lineless = |id, kind, document: SourceDocument| SourceTransaction { kind, document: Some(document), ..txn(id, "2024-04-01", &[]) };
    let party = |id, name: &str| SourceParty {
        id,
        name: name.into(),
        parent_id: None,
        company_name: None,
        email: None,
        phone: None,
        address: None,
        terms_days: None,
        tax_id: None,
        is_1099: false,
        active: true,
        balance_cents: None,
        notes: None,
    };
    let ledger = Ledger {
        accounts: vec![
            account(1, "Riverside Bank", "BANK", None),
            account(2, "Savings", "BANK", None),
            account(4, "Accounts Receivable", "ACCOUNTS_RECEIVABLE", None),
            account(6, "Consulting Income", "INCOME", None),
            account(8, "Sales Tax Payable", "OTHER_CURRENT_LIABILITY", None),
            account(10, "Office Expense", "EXPENSE", None),
        ],
        transactions: vec![
            SourceTransaction { kind: "Transfer", document: Some(SourceDocument::Transfer), ..txn(40, "2024-04-02", &[(2, -20_000), (1, 20_000)]) },
            SourceTransaction { kind: "Transfer", document: Some(SourceDocument::Transfer), ..txn(41, "2024-04-03", &[(4, -500), (1, 500)]) },
            lineless(42, "Estimate", estimate(100)),
            lineless(43, "Estimate", estimate(999)),
            lineless(
                44,
                "Purchase order",
                SourceDocument::PurchaseOrder {
                    vendor_id: 300,
                    date: "2024-04-01".parse().unwrap(),
                    number: Some("PO-5".into()),
                    expected: None,
                    memo: None,
                    received: true,
                    lines: vec![line(None, 3_000, 10)],
                },
            ),
        ],
        unbalanced: vec![],
        unreadable: vec![],
        customers: vec![party(100, "Acme Hosting")],
        vendors: vec![party(300, "Paper Supply Co")],
        items: vec![SourceItem {
            id: 200,
            name: "Consulting".into(),
            description: None,
            rate_cents: 1_000,
            income_account_id: Some(6),
            kind: "Service",
            taxable: true,
            active: true,
        }],
        quality: LedgerQuality::default(),
    };
    let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &ledger).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    assert_eq!(batch_summary(&pool, batch).await.unwrap().postable_count, 2, "only the transfers post");

    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap();
    let full = posted.full.unwrap();
    assert_eq!((full.transfers, full.estimates, full.purchase_orders, full.not_imported, full.fell_back), (1, 1, 1, 1, 1), "{full:?}");
    assert_eq!(posted.entries_posted, 2);

    let (number, total, tax, status, lines): (i64, i64, i64, String, i64) = sqlx::query_as(
        "SELECT estimate_number, total_cents, tax_cents, status, (SELECT COUNT(*) FROM estimate_lines WHERE estimate_id = e.id) FROM estimates e",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((number, total, tax, status.as_str(), lines), (77, 3_240, 240, "PENDING", 1));
    let (po_number, po_status, po_total): (i64, String, i64) = sqlx::query_as("SELECT po_number, status, total_cents FROM purchase_orders").fetch_one(&pool).await.unwrap();
    assert_eq!((po_number, po_status.as_str(), po_total), (1, "CLOSED", 3_000), "PO-5 is not a number, so it gets the next one");
    let (from, to, amount): (i64, i64, i64) = sqlx::query_as("SELECT from_account_id, to_account_id, amount_cents FROM transfers").fetch_one(&pool).await.unwrap();

    let summary = batch_summary(&pool, batch).await.unwrap();
    let mapped = |name: &str| summary.accounts.iter().find(|a| a.full_name == name).and_then(|a| a.account_id).unwrap();
    assert_eq!((from, to, amount), (mapped("Riverside Bank"), mapped("Savings"), 20_000));
    assert_eq!(summary.postable_count, 0);
    let reasons: Vec<&str> = summary.fallbacks.iter().map(|f| f.reason.as_str()).collect();
    assert_eq!(reasons, ["not brought in: its customer was not imported", "LocalBooks transfers cannot use one of its accounts (accounts receivable)"]);
}

/// Full pipeline on a real company file into a fresh test database: set LOCALBOOKS_QBW_CHECK
/// and run `cargo test --release -- --ignored posts_a_real --nocapture`.
#[tokio::test]
#[ignore]
async fn posts_a_real_company_file() {
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
    let ledger = tokio::task::spawn_blocking(move || crate::qbw_company::read_ledger(Path::new(&path))).await.unwrap().unwrap();
    let pool = test_pool().await;
    let started = std::time::Instant::now();
    let batch = stage(&pool, Path::new("/books/Books.qbw"), 0, &ledger).await.unwrap();
    let mapped = auto_map_accounts(&pool, batch).await.unwrap();
    let lists = import_lists(&pool, batch).await.unwrap();
    println!("lists: {lists:?}");
    let (customers, active_customers, vendors, items): (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM customers), (SELECT COUNT(*) FROM customers WHERE is_active = 1),
                    (SELECT COUNT(*) FROM vendors), (SELECT COUNT(*) FROM products_services)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    println!("LocalBooks now has {customers} customers ({active_customers} active), {vendors} vendors, {items} items");
    let mode = if std::env::var("LOCALBOOKS_QBW_MODE").as_deref() == Ok("full") { ImportMode::Full } else { ImportMode::Journal };
    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode }).await.unwrap();
    println!("staged, mapped {mapped:?}, posted {posted:?} in {:.1}s", started.elapsed().as_secs_f64());

    // LocalBooks' balance per mapped account against the file's stored balance (negated:
    // the file stores debits as negative, LocalBooks reports debit-normal balances).
    let summary = batch_summary(&pool, batch).await.unwrap();
    let mut per_target: HashMap<i64, (i64, Vec<String>)> = HashMap::new();
    for a in summary.accounts.iter().filter(|a| a.line_count > 0) {
        let e = per_target.entry(a.account_id.unwrap()).or_default();
        e.0 += a.stored_balance_cents.unwrap_or(0);
        e.1.push(a.full_name.clone());
    }
    let (mut same, mut differ) = (0, 0);
    for (account, (stored, names)) in &per_target {
        let books = balance(&pool, *account).await;
        if books == -stored {
            same += 1;
        } else {
            differ += 1;
            println!("  {:<45} books {:>14} file {:>14}", names.join(" + "), crate::ledger::format_cents(books), crate::ledger::format_cents(-stored));
        }
    }
    println!("LocalBooks accounts matching the file: {same}, differing: {differ}");
    if mode == ImportMode::Full {
        let fallbacks: Vec<(String, String, i64)> = sqlx::query_as(
            "SELECT kind, fallback_reason, COUNT(*) FROM ledger_import_transactions
                 WHERE batch_id = ?1 AND fallback_reason IS NOT NULL GROUP BY kind, fallback_reason ORDER BY 3 DESC LIMIT 25",
        )
        .bind(batch)
        .fetch_all(&pool)
        .await
        .unwrap();
        for (kind, reason, n) in fallbacks {
            println!("  fell back: {n:>5} {kind}: {reason}");
        }
        let (open, open_cents): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(total_cents - amount_paid_cents), 0) FROM invoices WHERE status IN ('UNPAID', 'PARTIAL')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let (linked, deposit_lines): (i64, i64) =
            sqlx::query_as("SELECT COUNT(invoice_payment_id) + COUNT(sales_receipt_id), COUNT(*) FROM deposit_lines").fetch_one(&pool).await.unwrap();
        println!("open invoices: {open}, {}; deposit lines linked to a payment or receipt: {linked}/{deposit_lines}", crate::ledger::format_cents(open_cents));
        let (bills, bills_cents): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(total_cents - amount_paid_cents), 0) FROM bills WHERE status IN ('UNPAID', 'PARTIAL')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        println!("open bills: {bills}, {}", crate::ledger::format_cents(bills_cents));
        let waiting = crate::banking::list_undeposited(&pool).await.unwrap();
        let uf: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(l.debit_cents - l.credit_cents), 0) FROM journal_lines l JOIN chart_of_accounts a ON a.id = l.account_id
                 WHERE a.is_system = 1 AND a.account_type = 'OTHER_CURRENT_ASSET'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        println!(
            "waiting to be deposited: {} items, {}; Undeposited Funds balance {}",
            waiting.len(),
            crate::ledger::format_cents(waiting.iter().map(|w| w.amount_cents).sum()),
            crate::ledger::format_cents(uf)
        );
    }
    let total: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM journal_lines").fetch_one(&pool).await.unwrap();
    assert_eq!(total, 0, "the books must balance");
}

#[tokio::test]
async fn start_over_puts_back_the_books_from_just_before_the_post() {
    let dir = std::env::temp_dir().join(format!("localbooks-start-over-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db = crate::db::Db::open_at(dir.join("live.db"), crate::db::test_config(&dir)).await.unwrap();
    let pool = db.pool().await;
    let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &ledger()).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let err = pre_post_backup(&pool, batch).await.unwrap_err();
    assert!(err.starts_with("QBW_NO_START_OVER"), "{err}");

    // What the post command does: copy the books, post, record the copy.
    let copy = crate::backup::pre_import_snapshot(&db, batch).await.unwrap();
    assert!(copy.file_name().unwrap().to_string_lossy().starts_with(&format!("pre-import-{batch}-")));
    post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap();
    set_pre_post_backup(&pool, batch, &copy).await.unwrap();
    let posted = batch_summary(&pool, batch).await.unwrap();
    assert_eq!((posted.live_entry_count, posted.postable_count), (2, 0));
    assert!(posted.start_over.is_some());
    let kinds: Vec<String> = crate::backup::list_backups(db.backups_dir()).unwrap().into_iter().map(|b| b.kind).collect();
    assert!(kinds.contains(&"PRE_IMPORT".to_string()), "{kinds:?}");

    let result = crate::backup::restore(&db, &pre_post_backup(&pool, batch).await.unwrap()).await.unwrap();
    let pool = db.pool().await;
    let entries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap();
    assert_eq!(entries, 0, "no entries and no reversals remain");
    let again = batch_summary(&pool, batch).await.unwrap();
    assert_eq!((again.live_entry_count, again.postable_count), (0, 2));
    assert!(again.accounts.iter().filter(|a| a.line_count > 0).all(|a| a.account_id.is_some()), "mapping kept");
    assert!(again.start_over.is_none());
    assert!(Path::new(&result.safety_backup.path).is_file());

    // The restored books can post the import again.
    let reposted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap();
    assert_eq!(reposted.entries_posted, 2);

    // A recorded copy that has gone missing is reported, not restored.
    set_pre_post_backup(&pool, batch, &dir.join("gone.db")).await.unwrap();
    assert!(pre_post_backup(&pool, batch).await.unwrap_err().contains("no longer in the backups folder"));
    drop(pool);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn reimporting_after_a_reverse_reuses_the_accounts_it_created() {
    let pool = test_pool().await;
    // A LocalBooks account of another type already holds the name, so the first import
    // creates "Equipment (imported)".
    sqlx::query("INSERT INTO chart_of_accounts (name, account_type) VALUES ('Equipment', 'FIXED_ASSET')").execute(&pool).await.ok();
    let books = || Ledger {
        accounts: vec![account(1, "Riverside Bank", "BANK", None), account(2, "Equipment", "EXPENSE", None)],
        transactions: vec![txn(10, "2023-03-01", &[(2, -12_000), (1, 12_000)])],
        unbalanced: vec![],
        unreadable: vec![],
        customers: vec![],
        vendors: vec![],
        items: vec![],
        quality: LedgerQuality::default(),
    };
    let first = stage(&pool, Path::new("/books/Books.qbw"), 1, &books()).await.unwrap();
    auto_map_accounts(&pool, first).await.unwrap();
    let mapped = |batch: i64| {
        let pool = pool.clone();
        async move {
            batch_summary(&pool, batch).await.unwrap().accounts.into_iter().find(|a| a.full_name == "Equipment").unwrap().account_id.unwrap()
        }
    };
    let equipment = mapped(first).await;
    let name: String = sqlx::query_scalar("SELECT name FROM chart_of_accounts WHERE id = ?1").bind(equipment).fetch_one(&pool).await.unwrap();
    assert_eq!(name, "Equipment (imported)");
    post_batch(&pool, &PostLedgerInput { batch_id: first, mode: ImportMode::Journal }).await.unwrap();
    reverse_batch(&pool, first).await.unwrap();
    discard_batch(&pool, first).await.unwrap();

    // Importing the same books again must not trip the sibling-name rule: the deactivated
    // account is reused and reactivated.
    let second = stage(&pool, Path::new("/books/Books.qbw"), 1, &books()).await.unwrap();
    auto_map_accounts(&pool, second).await.unwrap();
    assert_eq!(mapped(second).await, equipment);
    let active: i64 = sqlx::query_scalar("SELECT is_active FROM chart_of_accounts WHERE id = ?1").bind(equipment).fetch_one(&pool).await.unwrap();
    assert_eq!(active, 1);
    let posted = post_batch(&pool, &PostLedgerInput { batch_id: second, mode: ImportMode::Journal }).await.unwrap();
    assert_eq!(posted.entries_posted, 1);

    // A taken "(imported)" name that is not the importer's own gets the next free suffix.
    sqlx::query("INSERT INTO chart_of_accounts (name, account_type) VALUES ('Tools', 'FIXED_ASSET'), ('Tools (imported)', 'OTHER_ASSET')")
        .execute(&pool)
        .await
        .unwrap();
    let tools = Ledger {
        accounts: vec![account(1, "Riverside Bank", "BANK", None), account(3, "Tools", "EXPENSE", None)],
        transactions: vec![txn(11, "2023-03-02", &[(3, -500), (1, 500)])],
        unbalanced: vec![],
        unreadable: vec![],
        customers: vec![],
        vendors: vec![],
        items: vec![],
        quality: LedgerQuality::default(),
    };
    let third = stage(&pool, Path::new("/books/Other.qbw"), 1, &tools).await.unwrap();
    auto_map_accounts(&pool, third).await.unwrap();
    let id = batch_summary(&pool, third).await.unwrap().accounts.into_iter().find(|a| a.full_name == "Tools").unwrap().account_id.unwrap();
    let name: String = sqlx::query_scalar("SELECT name FROM chart_of_accounts WHERE id = ?1").bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(name, "Tools (imported 2)");
}

fn party(id: i64, name: &str, parent: Option<i64>, email: Option<&str>) -> SourceParty {
    SourceParty {
        id,
        name: name.into(),
        parent_id: parent,
        company_name: None,
        email: email.map(Into::into),
        phone: Some("555-0100".into()),
        address: Some("1 Main St\nSpringfield".into()),
        terms_days: Some(30),
        tax_id: None,
        is_1099: false,
        active: true,
        balance_cents: None,
        notes: None,
    }
}

#[tokio::test]
async fn lists_are_matched_by_name_or_created_with_their_details() {
    let pool = test_pool().await;
    sqlx::query("INSERT INTO customers (name) VALUES ('Acme')").execute(&pool).await.unwrap();
    let mut books = ledger();
    books.customers = vec![
        party(1, "Acme", None, None),
        party(2, "Beta Co", None, Some("ap@beta.com")),
        party(3, "Beta Co:Roof job", Some(2), Some("not an email")),
    ];
    books.vendors = vec![party(9, "Hosting Inc", None, Some("billing@hosting.example"))];
    let item = |id, name: &str, account| SourceItem {
        id,
        name: name.into(),
        description: Some("desc".into()),
        rate_cents: 8_500,
        income_account_id: account,
        kind: "Service",
        taxable: false,
        active: true,
    };
    books.accounts.push(account(6, "Consulting Income", "INCOME", None));
    books.transactions.push(txn(14, "2023-03-05", &[(4, -9_000), (6, 9_000)]));
    books.items = vec![item(20, "Business Labor", Some(6)), item(21, "Mystery", Some(999)), item(22, "Parts kit", Some(2))];
    let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &books).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();

    let r = import_lists(&pool, batch).await.unwrap();
    assert_eq!((r.customers_matched, r.customers_created, r.vendors_created, r.items_created), (1, 2, 1, 3));
    assert_eq!(r.items_on_fallback_account, 2, "the unmapped item and the one posting to an expense account");
    let kit: String = sqlx::query_scalar("SELECT description FROM products_services WHERE name = 'Parts kit'").fetch_one(&pool).await.unwrap();
    assert_eq!(kit, "desc (posted to Hosting in the original file)");
    let (parent, notes, email): (Option<i64>, Option<String>, Option<String>) =
        sqlx::query_as("SELECT parent_id, notes, email FROM customers WHERE name = 'Beta Co:Roof job'").fetch_one(&pool).await.unwrap();
    let beta: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Beta Co'").fetch_one(&pool).await.unwrap();
    assert_eq!(parent, Some(beta));
    assert_eq!((email, notes.as_deref()), (None, Some("Email: not an email")));
    let (terms, address): (Option<i64>, Option<String>) =
        sqlx::query_as("SELECT terms_days, billing_address FROM customers WHERE id = ?1").bind(beta).fetch_one(&pool).await.unwrap();
    assert_eq!((terms, address.as_deref()), (Some(30), Some("1 Main St\nSpringfield")));
    let summary = batch_summary(&pool, batch).await.unwrap();
    let consulting = summary.accounts.iter().find(|a| a.full_name == "Consulting Income").unwrap().account_id.unwrap();
    let labor_account: i64 = sqlx::query_scalar("SELECT income_account_id FROM products_services WHERE name = 'Business Labor'").fetch_one(&pool).await.unwrap();
    assert_eq!(labor_account, consulting, "posts to the account its source account maps to");
    assert_eq!((summary.lists.customers_done, summary.lists.vendors_done, summary.lists.items_done), (3, 1, 3));

    // Again: nothing new.
    let again = import_lists(&pool, batch).await.unwrap();
    assert_eq!(again, ImportListsResult::default());

    post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap();
    let reversed = reverse_batch(&pool, batch).await.unwrap();
    assert_eq!(reversed.list_records_deactivated, 6, "2 customers, 1 vendor, 3 items created by the import");
    let acme: i64 = sqlx::query_scalar("SELECT is_active FROM customers WHERE name = 'Acme'").fetch_one(&pool).await.unwrap();
    assert_eq!(acme, 1, "a matched customer is not the import's to deactivate");
}

#[tokio::test]
async fn a_ledger_maps_posts_once_and_reverses() {
    let pool = test_pool().await;
    let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &ledger()).await.unwrap();
    let summary = batch_summary(&pool, batch).await.unwrap();
    assert_eq!((summary.transaction_count, summary.postable_count, summary.unbalanced.len()), (3, 2, 1));
    let u = &summary.unreadable[0];
    assert_eq!((u.outcome.as_str(), u.account_name.as_deref(), u.cents), ("RECOVERED", Some("Domain Names"), Some(-250)));

    let err = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap_err();
    assert!(err.starts_with("QBW_UNMAPPED"), "{err}");

    let mapped = auto_map_accounts(&pool, batch).await.unwrap();
    // Receivables fold into the system account; the bank, the expense and its subaccount are
    // created; the unused account is left alone.
    assert_eq!(mapped, AutoMapResult { matched: 1, created: 3, reactivated: 0 });
    let accounts = batch_summary(&pool, batch).await.unwrap().accounts;
    let id = |name: &str| accounts.iter().find(|a| a.full_name == name).unwrap().account_id;
    assert!(id("Unused").is_none());
    let domains = id("Domain Names").unwrap();
    let parent: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM chart_of_accounts WHERE id = ?1").bind(domains).fetch_one(&pool).await.unwrap();
    assert_eq!(parent, id("Hosting"));

    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap();
    assert_eq!(posted, PostLedgerResult { entries_posted: 2, lines_posted: 4, skipped_unbalanced: 1, skipped_already_posted: 0, ..Default::default() });
    assert_eq!(balance(&pool, domains).await, 12_000);
    assert_eq!(balance(&pool, id("Riverside Bank").unwrap()).await, -12_500);
    let memo: String = sqlx::query_scalar("SELECT memo FROM journal_entries WHERE id = (SELECT journal_entry_id FROM ledger_import_transactions WHERE source_id = 10)")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(memo, "Check #10 · Vendor");

    // Posting again posts nothing new; a second import of the same books is refused.
    let again = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap();
    assert_eq!(again.entries_posted, 0);
    let second = stage(&pool, Path::new("/books/Books copy.qbw"), 1, &ledger()).await.unwrap();
    auto_map_accounts(&pool, second).await.unwrap();
    let err = post_batch(&pool, &PostLedgerInput { batch_id: second, mode: ImportMode::Journal }).await.unwrap_err();
    assert!(err.starts_with("QBW_ALREADY_IMPORTED"), "{err}");
    discard_batch(&pool, second).await.unwrap();

    assert!(discard_batch(&pool, batch).await.unwrap_err().starts_with("QBW_POSTED"));
    let reversed = reverse_batch(&pool, batch).await.unwrap();
    assert_eq!(reversed.entries_voided, 2);
    assert_eq!(reversed.accounts_deactivated, 3);
    assert_eq!(balance(&pool, domains).await, 0);
    discard_batch(&pool, batch).await.unwrap();
}

/// Intuit's "Import First - List Declarations" kind of file: lists, no transactions.
const LISTS_IIF: &str = "!ACCNT\tNAME\tACCNTTYPE\tDESC\tACCNUM\tEXTRA\r
ACCNT\tChecking\tBANK\t\t1000\t\r
ACCNT\tAccounts Receivable\tAR\t\t1200\t\r
ACCNT\tConstruction:Labor\tINC\t\t4100\t\r
ACCNT\tRepairs\tEXP\t\t6100\t\r
!CUST\tNAME\tBADDR1\tBADDR2\tPHONE1\tTERMS\r
CUST\tCustomer\tJoe Customer\t444 Road Rd\t5554443333\tNet 30\r
!VEND\tNAME\tADDR1\tTAXID\t1099\r
VEND\tVendor\tJon Vendor\t12-3456789\tY\r
!INVITEM\tNAME\tINVITEMTYPE\tDESC\tACCNT\tPRICE\tTAXABLE\r
INVITEM\tFraming\tSERV\tFraming labor\tConstruction:Labor\t55\tN\r
";

#[tokio::test]
async fn a_lists_only_file_adds_its_lists_once_and_reverses() {
    let pool = test_pool().await;
    let books = crate::iif_import::read(LISTS_IIF.as_bytes()).unwrap();
    assert!(books.transactions.is_empty());
    let batch = stage(&pool, Path::new("/books/Lists.iif"), 1, &books).await.unwrap();
    let summary = batch_summary(&pool, batch).await.unwrap();
    assert!(summary.lists_only && !summary.lists_added);
    assert_eq!((summary.transaction_count, summary.postable_count, summary.accounts.len()), (0, 0, 5), "Construction is added as Labor's parent");

    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap();
    assert_eq!(posted.entries_posted, 0);
    let accounts = posted.accounts.unwrap();
    assert_eq!(accounts.matched + accounts.created, 5, "every account is mapped, used or not");
    let lists = posted.lists.unwrap();
    assert_eq!((lists.customers_created, lists.vendors_created, lists.items_created), (1, 1, 1));
    let labor: i64 = sqlx::query_scalar("SELECT a.id FROM chart_of_accounts a JOIN chart_of_accounts p ON p.id = a.parent_id WHERE a.name = 'Labor' AND p.name = 'Construction'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let framing: i64 = sqlx::query_scalar("SELECT income_account_id FROM products_services WHERE name = 'Framing'").fetch_one(&pool).await.unwrap();
    assert_eq!(framing, labor);
    let summary = batch_summary(&pool, batch).await.unwrap();
    assert!(summary.lists_added && summary.posted_at.is_some());
    assert_eq!(summary.live_entry_count, 0);
    assert!(discard_batch(&pool, batch).await.is_err(), "reverse first");

    // Again: nothing new, nothing duplicated.
    let count = |sql: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>(sql).fetch_one(&pool).await.unwrap() }
    };
    let before = (
        count("SELECT COUNT(*) FROM chart_of_accounts").await,
        count("SELECT COUNT(*) FROM customers").await,
        count("SELECT COUNT(*) FROM vendors").await,
        count("SELECT COUNT(*) FROM products_services").await,
    );
    let again = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap();
    assert_eq!(again, PostLedgerResult::default());
    let after = (
        count("SELECT COUNT(*) FROM chart_of_accounts").await,
        count("SELECT COUNT(*) FROM customers").await,
        count("SELECT COUNT(*) FROM vendors").await,
        count("SELECT COUNT(*) FROM products_services").await,
    );
    assert_eq!(before, after);

    let reversed = reverse_batch(&pool, batch).await.unwrap();
    assert_eq!((reversed.entries_voided, reversed.list_records_deactivated), (0, 3));
    assert_eq!(reversed.accounts_deactivated, accounts.created);
    assert_eq!(count("SELECT is_active FROM products_services WHERE name = 'Framing'").await, 0);
    assert!(!batch_summary(&pool, batch).await.unwrap().lists_added);
    assert!(reverse_batch(&pool, batch).await.is_err(), "nothing left to reverse");

    // Adding them again puts back what the reversal deactivated, without duplicates.
    let readded = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap();
    assert_eq!(readded.accounts.unwrap().reactivated, accounts.created);
    assert_eq!(readded.lists.unwrap(), ImportListsResult::default());
    assert_eq!(count("SELECT is_active FROM products_services WHERE name = 'Framing'").await, 1);
    assert_eq!(count("SELECT is_active FROM vendors WHERE name = 'Vendor'").await, 1);
    assert_eq!(after, (
        count("SELECT COUNT(*) FROM chart_of_accounts").await,
        count("SELECT COUNT(*) FROM customers").await,
        count("SELECT COUNT(*) FROM vendors").await,
        count("SELECT COUNT(*) FROM products_services").await,
    ));
    reverse_batch(&pool, batch).await.unwrap();
    discard_batch(&pool, batch).await.unwrap();
}
