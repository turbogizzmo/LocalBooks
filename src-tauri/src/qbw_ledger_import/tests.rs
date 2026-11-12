use super::*;
use crate::db::test_pool;
use crate::qbw_company::{LedgerQuality, LineOutcome, SourceAccount, SourceItem, SourceItemGroup, SourceLine, SourceParty, SourceTransaction, UnreadableLine};

fn account(id: i64, name: &str, kind: &'static str, parent: Option<i64>) -> SourceAccount {
    SourceAccount {
        id,
        name: name.into(),
        full_name: name.into(),
        account_type: kind,
        parent_id: parent,
        active: true,
        stored_balance_cents: None,
        tax_line: None,
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
        lines: lines.iter().map(|&(account_id, cents)| SourceLine { account_id, cents, memo: None, class: None }).collect(),
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
        sales_lists: Default::default(),
        classes: Vec::new(),
        stock: Default::default(),
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
            DocumentLine {
                item_id: Some(200),
                description: None,
                quantity_e4: 20_000,
                rate_cents: 5_000,
                cents: 10_000,
                account_id: 6,
                class: None,
                subtotal: false,
            },
            DocumentLine {
                item_id: None,
                description: Some("Sales tax".into()),
                quantity_e4: 10_000,
                rate_cents: 1_000,
                cents: 1_000,
                account_id: 8,
                class: None,
                subtotal: false,
            },
        ],
        sales: Default::default(),
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
            sales_rep: None,
            custom_fields: Vec::new(),
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
            custom_fields: Vec::new(),
            purchase_description: None,
            purchase_cost_cents: None,
            percent_ppm: None,
        }],
        quality: LedgerQuality::default(),
        sales_lists: Default::default(),
        classes: Vec::new(),
        stock: Default::default(),
    };
    let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &ledger).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let summary = batch_summary(&pool, batch).await.unwrap();
    assert_eq!(summary.documents.iter().map(|d| d.1).sum::<i64>(), 5);

    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap();
    let full = posted.full.unwrap();
    assert_eq!((full.invoices, full.payments, full.deposits, full.fell_back), (1, 2, 1, 1), "{full:?}");

    let (number, status, total, tax, items): (i64, String, i64, i64, i64) =
        sqlx::query_as("SELECT invoice_number, status, total_cents, tax_cents, (SELECT COUNT(*) FROM invoice_items WHERE invoice_id = i.id) FROM invoices i")
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

/// A deposit split between two bank accounts is still a deposit: into the one that got the
/// most, with the other's share taken out of it, so its payments leave Undeposited Funds.
#[tokio::test]
async fn a_deposit_split_between_banks_still_banks_its_payments() {
    use crate::qbw_documents::{Applied, DocumentLine, SourceDocument};
    let pool = test_pool().await;
    let doc = |id: i64, kind: &'static str, date: &str, lines: &[(i64, i64)], document: SourceDocument| SourceTransaction {
        kind,
        document: Some(document),
        ..txn(id, date, lines)
    };
    let payment =
        |cents| SourceDocument::Payment { customer_id: 100, reference: None, method: "CASH".into(), applied_to: vec![Applied { transaction_id: 20, cents }] };
    let mut ledger = ledger();
    ledger.accounts = vec![
        account(1, "Riverside Bank", "BANK", None),
        account(2, "Savings", "BANK", None),
        account(4, "Accounts Receivable", "ACCOUNTS_RECEIVABLE", None),
        account(6, "Consulting Income", "INCOME", None),
        account(7, "Undeposited Funds", "OTHER_CURRENT_ASSET", None),
    ];
    ledger.unbalanced = vec![];
    ledger.unreadable = vec![];
    ledger.transactions = vec![
        doc(
            20,
            "Invoice",
            "2024-01-10",
            &[(4, -9_000), (6, 9_000)],
            SourceDocument::Invoice {
                customer_id: 100,
                number: Some("7".into()),
                due_date: None,
                lines: vec![DocumentLine {
                    item_id: None,
                    description: Some("Work".into()),
                    quantity_e4: 10_000,
                    rate_cents: 9_000,
                    cents: 9_000,
                    account_id: 6,
                    class: None,
                    subtotal: false,
                }],
                sales: Default::default(),
            },
        ),
        doc(21, "Payment", "2024-01-12", &[(7, -6_000), (4, 6_000)], payment(6_000)),
        doc(22, "Payment", "2024-01-13", &[(7, -3_000), (4, 3_000)], payment(3_000)),
        doc(
            23,
            "Deposit",
            "2024-01-14",
            &[(1, -2_000), (2, -7_000), (7, 6_000), (7, 3_000)],
            SourceDocument::Deposit { payments: vec![Applied { transaction_id: 21, cents: 6_000 }, Applied { transaction_id: 22, cents: 3_000 }] },
        ),
    ];
    ledger.customers = vec![SourceParty {
        id: 100,
        name: "Acme Hosting".into(),
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
        sales_rep: None,
        custom_fields: Vec::new(),
    }];
    let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &ledger).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let full = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap().full.unwrap();
    assert_eq!((full.deposits, full.fell_back), (1, 0), "{full:?}");
    assert!(crate::banking::list_undeposited(&pool).await.unwrap().is_empty(), "both payments were banked");

    let summary = batch_summary(&pool, batch).await.unwrap();
    let mapped = |name: &str| summary.accounts.iter().find(|a| a.full_name == name).and_then(|a| a.account_id).unwrap();
    let (bank, total): (i64, i64) = sqlx::query_as("SELECT bank_account_id, total_cents FROM deposits").fetch_one(&pool).await.unwrap();
    assert_eq!((bank, total), (mapped("Savings"), 9_000), "the deposit goes to the account that got the most, gross");
    assert_eq!(balance(&pool, mapped("Savings")).await, 7_000);
    assert_eq!(balance(&pool, mapped("Riverside Bank")).await, 2_000);
    assert_eq!(balance(&pool, mapped("Undeposited Funds")).await, 0);
}

#[tokio::test]
async fn a_deposit_into_a_non_bank_account_clears_its_payments_as_a_transfer() {
    use crate::qbw_documents::{Applied, DocumentLine, SourceDocument};
    let pool = test_pool().await;
    let doc = |id: i64, kind: &'static str, date: &str, lines: &[(i64, i64)], document: SourceDocument| SourceTransaction {
        kind,
        document: Some(document),
        ..txn(id, date, lines)
    };
    let payment =
        |cents| SourceDocument::Payment { customer_id: 100, reference: None, method: "CASH".into(), applied_to: vec![Applied { transaction_id: 20, cents }] };
    let mut ledger = ledger();
    ledger.accounts = vec![
        account(1, "Riverside Bank", "BANK", None),
        account(2, "Bad Debts", "EXPENSE", None),
        account(4, "Accounts Receivable", "ACCOUNTS_RECEIVABLE", None),
        account(6, "Consulting Income", "INCOME", None),
        account(7, "Undeposited Funds", "OTHER_CURRENT_ASSET", None),
    ];
    ledger.unbalanced = vec![];
    ledger.unreadable = vec![];
    ledger.transactions = vec![
        doc(
            20,
            "Invoice",
            "2024-01-10",
            &[(4, -9_000), (6, 9_000)],
            SourceDocument::Invoice {
                customer_id: 100,
                number: Some("7".into()),
                due_date: None,
                lines: vec![DocumentLine {
                    item_id: None,
                    description: Some("Work".into()),
                    quantity_e4: 10_000,
                    rate_cents: 9_000,
                    cents: 9_000,
                    account_id: 6,
                    class: None,
                    subtotal: false,
                }],
                sales: Default::default(),
            },
        ),
        doc(21, "Payment", "2024-01-12", &[(7, -6_000), (4, 6_000)], payment(6_000)),
        doc(22, "Payment", "2024-01-13", &[(7, -3_000), (4, 3_000)], payment(3_000)),
        doc(
            23,
            "Deposit",
            "2024-01-14",
            &[(2, -9_000), (7, 6_000), (7, 3_000)],
            SourceDocument::Deposit { payments: vec![Applied { transaction_id: 21, cents: 6_000 }, Applied { transaction_id: 22, cents: 3_000 }] },
        ),
    ];
    ledger.customers = vec![SourceParty {
        id: 100,
        name: "Acme Hosting".into(),
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
        sales_rep: None,
        custom_fields: Vec::new(),
    }];
    let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &ledger).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let full = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap().full.unwrap();
    assert_eq!((full.deposits, full.transfers, full.fell_back), (0, 1, 0), "{full:?}");
    assert!(crate::banking::list_undeposited(&pool).await.unwrap().is_empty(), "the transfer cleared both payments");

    let undeposited: i64 =
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'OTHER_CURRENT_ASSET' AND is_system = 1").fetch_one(&pool).await.unwrap();
    assert_eq!(balance(&pool, undeposited).await, 0, "the trial balance matches the file");
    let entry: i64 = sqlx::query_scalar("SELECT DISTINCT journal_entry_id FROM undeposited_transfers").fetch_one(&pool).await.unwrap();
    let debited: i64 = sqlx::query_scalar("SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = ?1").bind(entry).fetch_one(&pool).await.unwrap();
    assert_eq!(debited, 9_000, "the file's own entry: 90.00 written off");

    // A cleared payment can't go into a deposit too; voiding the transfer puts both back.
    let payment: i64 = sqlx::query_scalar("SELECT MIN(invoice_payment_id) FROM undeposited_transfers").fetch_one(&pool).await.unwrap();
    let bank: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'BANK' ORDER BY id LIMIT 1").fetch_one(&pool).await.unwrap();
    let deposit = crate::banking::DepositInput {
        bank_account_id: bank,
        deposit_date: "2024-02-01".parse().unwrap(),
        memo: None,
        items: vec![crate::banking::DepositItemRef { kind: "PAYMENT".into(), id: payment }],
        other_lines: vec![],
        cash_back: None,
        deductions: Vec::new(),
    };
    assert!(crate::banking::post_bank_deposit(&pool, deposit.clone()).await.unwrap_err().contains("ALREADY_DEPOSITED"));
    crate::ledger::void_transaction(&pool, entry).await.unwrap();
    assert_eq!(crate::banking::list_undeposited(&pool).await.unwrap().len(), 2);
    crate::banking::post_bank_deposit(&pool, deposit).await.unwrap();
}

/// The purchase side of a full import: a bill paid by a bill payment dated before it, a check,
/// a card charge, a card credit, and a refund into a bank account (no LocalBooks form, so a
/// journal entry).
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
                SourceDocument::BillPayment {
                    vendor_id: Some(300),
                    reference: Some("990".into()),
                    applied_to: vec![Applied { transaction_id: 30, cents: 5_000 }],
                },
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
                    lines: vec![DocumentLine {
                        item_id: None,
                        description: Some("Paper".into()),
                        quantity_e4: 10_000,
                        rate_cents: 5_000,
                        cents: -5_000,
                        account_id: 10,
                        class: None,
                        subtotal: false,
                    }],
                },
            ),
            doc(32, "Check", "2024-02-12", &[(1, 2_500), (12, -2_500)], expense(Some("1001"))),
            doc(33, "Credit card charge", "2024-02-13", &[(11, 1_200), (10, -1_200)], expense(None)),
            doc(34, "Credit card credit", "2024-02-14", &[(11, -300), (10, 200), (12, 100)], expense(None)),
            doc(35, "Check", "2024-02-15", &[(1, -400), (12, 400)], expense(None)),
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
            sales_rep: None,
            custom_fields: Vec::new(),
        }],
        items: vec![],
        quality: LedgerQuality::default(),
        sales_lists: Default::default(),
        classes: Vec::new(),
        stock: Default::default(),
    };
    let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &ledger).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap();
    let full = posted.full.unwrap();
    assert_eq!((full.bills, full.bill_payments, full.checks, full.expenses, full.card_credits, full.fell_back), (1, 1, 1, 1, 1, 1), "{full:?}");

    let (status, bill_number, lines): (String, Option<String>, i64) =
        sqlx::query_as("SELECT status, bill_number, (SELECT COUNT(*) FROM bill_lines WHERE bill_id = b.id) FROM bills b").fetch_one(&pool).await.unwrap();
    assert_eq!((status.as_str(), bill_number.as_deref(), lines), ("PAID", Some("B-7"), 1), "the payment dated before the bill still applies");
    let checks: Vec<(String, Option<String>, i64, bool)> =
        sqlx::query_as("SELECT kind, check_number, total_cents, is_credit FROM expenses ORDER BY id").fetch_all(&pool).await.unwrap();
    assert_eq!(checks, vec![("CHECK".into(), Some("1001".into()), 2_500, false), ("EXPENSE".into(), None, 1_200, false), ("EXPENSE".into(), None, 300, true)]);
    let credit = crate::expenses::list_expenses(&pool).await.unwrap().into_iter().find(|x| x.kind == "CARD_CREDIT").unwrap();
    assert_eq!((credit.payee_name.as_deref(), credit.line_count, credit.category_account_id), (Some("Paper Supply Co"), 2, None));
    let credit_lines: Vec<i64> =
        sqlx::query_scalar("SELECT amount_cents FROM expense_lines WHERE expense_id = ?1 ORDER BY line_number").bind(credit.id).fetch_all(&pool).await.unwrap();
    assert_eq!(credit_lines, vec![200, 100], "a card credit's split lines are positive, like a charge's");

    let summary = batch_summary(&pool, batch).await.unwrap();
    assert_eq!(summary.fallbacks.len(), 1);
    assert_eq!(summary.fallbacks[0].reason, "it is a refund into a bank account, which has no Tillhouse form");
    let mapped = |name: &str| summary.accounts.iter().find(|a| a.full_name == name).and_then(|a| a.account_id).unwrap();
    assert_eq!(balance(&pool, mapped("Riverside Bank")).await, -7_100);
    assert_eq!(balance(&pool, mapped("Visa")).await, -900);
    assert_eq!(balance(&pool, mapped("Office Expense")).await, 6_000);
    assert_eq!(balance(&pool, mapped("Supplies")).await, 2_000);
    assert_eq!(balance(&pool, mapped("Accounts Payable")).await, 0);
    let (debits, credits): (i64, i64) = sqlx::query_as("SELECT SUM(debit_cents), SUM(credit_cents) FROM journal_lines").fetch_one(&pool).await.unwrap();
    assert_eq!(debits, credits, "the books balance");
}

/// Transfers, estimates and purchase orders in a full import. Estimates and purchase orders
/// post nothing: one that cannot be brought in is listed, and none counts as postable.
#[tokio::test]
async fn a_full_import_brings_transfers_estimates_and_purchase_orders() {
    use crate::qbw_documents::{DocumentLine, SourceDocument};
    let pool = test_pool().await;
    let line = |item_id, cents, account_id| DocumentLine {
        item_id,
        description: None,
        quantity_e4: 30_000,
        rate_cents: 1_000,
        cents,
        account_id,
        class: None,
        subtotal: false,
    };
    let estimate = |customer_id| SourceDocument::Estimate {
        customer_id,
        date: "2024-04-01".parse().unwrap(),
        number: Some("77".into()),
        expires: "2024-05-01".parse().ok(),
        memo: Some("Spring work".into()),
        lines: vec![
            line(Some(200), 3_000, 6),
            DocumentLine {
                item_id: None,
                description: Some("Tax".into()),
                quantity_e4: 10_000,
                rate_cents: 240,
                cents: 240,
                account_id: 8,
                class: None,
                subtotal: false,
            },
        ],
        sales: Default::default(),
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
        sales_rep: None,
        custom_fields: Vec::new(),
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
            custom_fields: Vec::new(),
            purchase_description: None,
            purchase_cost_cents: None,
            percent_ppm: None,
        }],
        quality: LedgerQuality::default(),
        sales_lists: Default::default(),
        classes: Vec::new(),
        stock: Default::default(),
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
    let (po_number, po_status, po_total): (i64, String, i64) =
        sqlx::query_as("SELECT po_number, status, total_cents FROM purchase_orders").fetch_one(&pool).await.unwrap();
    assert_eq!((po_number, po_status.as_str(), po_total), (1, "CLOSED", 3_000), "PO-5 is not a number, so it gets the next one");
    let (from, to, amount): (i64, i64, i64) =
        sqlx::query_as("SELECT from_account_id, to_account_id, amount_cents FROM transfers").fetch_one(&pool).await.unwrap();

    let summary = batch_summary(&pool, batch).await.unwrap();
    let mapped = |name: &str| summary.accounts.iter().find(|a| a.full_name == name).and_then(|a| a.account_id).unwrap();
    assert_eq!((from, to, amount), (mapped("Riverside Bank"), mapped("Savings"), 20_000));
    assert_eq!(summary.postable_count, 0);
    let reasons: Vec<&str> = summary.fallbacks.iter().map(|f| f.reason.as_str()).collect();
    assert_eq!(reasons, ["not brought in: its customer was not imported", "Tillhouse transfers cannot use one of its accounts (accounts receivable)"]);
}

/// Full pipeline on a real company file (or backup) into a fresh test database: set LOCALBOOKS_QBW_CHECK
/// and run `cargo test --release -- --ignored posts_a_real --nocapture`.
#[tokio::test]
#[ignore]
async fn posts_a_real_company_file() {
    let path = std::path::PathBuf::from(std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK"));
    // A backup (.qbb) is unpacked to a temporary company file first, as the app does.
    let unpacked = crate::qbw_backup::is_backup(&path).then(|| crate::qbw_backup::unpack(&path, &std::env::temp_dir()).unwrap());
    let file = unpacked.as_ref().map_or(path, |u| u.path.clone());
    let ledger = tokio::task::spawn_blocking(move || crate::qbw_company::read_ledger(&file)).await.unwrap().unwrap();
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
    println!("Tillhouse now has {customers} customers ({active_customers} active), {vendors} vendors, {items} items");
    let mode = if std::env::var("LOCALBOOKS_QBW_MODE").as_deref() == Ok("full") { ImportMode::Full } else { ImportMode::Journal };
    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode }).await.unwrap();
    println!("staged, mapped {mapped:?}, posted {posted:?} in {:.1}s", started.elapsed().as_secs_f64());

    // LocalBooks' balance per mapped account against the file's stored balance (negated:
    // the file stores debits as negative, LocalBooks reports debit-normal balances).
    let summary = batch_summary(&pool, batch).await.unwrap();
    // An account the file keeps no balance for (some equity and system accounts) can't be compared;
    // nor can a LocalBooks account that any such file account maps to.
    let mut per_target: HashMap<i64, (Option<i64>, Vec<String>)> = HashMap::new();
    for a in summary.accounts.iter().filter(|a| a.line_count > 0) {
        let e = per_target.entry(a.account_id.unwrap()).or_insert((Some(0), Vec::new()));
        e.0 = e.0.zip(a.stored_balance_cents).map(|(sum, b)| sum + b);
        e.1.push(a.full_name.clone());
    }
    let unstored: Vec<&str> = per_target.values().filter(|(s, _)| s.is_none()).flat_map(|(_, n)| n.iter().map(String::as_str)).collect();
    if !unstored.is_empty() {
        println!("not compared (the file keeps no balance): {}", unstored.join(", "));
    }
    let (mut same, mut differ) = (0, 0);
    for (account, stored, names) in per_target.iter().filter_map(|(a, (s, n))| Some((a, (*s)?, n))) {
        let books = balance(&pool, *account).await;
        if books == -stored {
            same += 1;
        } else {
            differ += 1;
            println!("  {:<45} books {:>14} file {:>14}", names.join(" + "), crate::ledger::format_cents(books), crate::ledger::format_cents(-stored));
        }
    }
    println!("Tillhouse accounts matching the file: {same}, differing: {differ}");
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
        let (open, open_cents): (i64, i64) =
            sqlx::query_as("SELECT COUNT(*), COALESCE(SUM(total_cents - amount_paid_cents), 0) FROM invoices WHERE status IN ('UNPAID', 'PARTIAL')")
                .fetch_one(&pool)
                .await
                .unwrap();
        let (linked, deposit_lines): (i64, i64) =
            sqlx::query_as("SELECT COUNT(invoice_payment_id) + COUNT(sales_receipt_id), COUNT(*) FROM deposit_lines").fetch_one(&pool).await.unwrap();
        println!("open invoices: {open}, {}; deposit lines linked to a payment or receipt: {linked}/{deposit_lines}", crate::ledger::format_cents(open_cents));
        let (bills, bills_cents): (i64, i64) =
            sqlx::query_as("SELECT COUNT(*), COALESCE(SUM(total_cents - amount_paid_cents), 0) FROM bills WHERE status IN ('UNPAID', 'PARTIAL')")
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
        // Item quantities: each tracked item against the file's own quantity and value.
        for d in &summary.stock.differences {
            println!(
                "  item {:<40} file {:?} / {:?}, imported {} / {}: {}",
                d.name,
                d.expected_qty_e4,
                d.expected_value_cents,
                d.imported_qty_e4,
                d.imported_value_cents,
                d.note.as_deref().unwrap_or("-")
            );
        }
        println!("inventory items matching the file: {} of {}", summary.stock.matching, summary.stock.items);
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
        sales_lists: Default::default(),
        classes: Vec::new(),
        stock: Default::default(),
    };
    let first = stage(&pool, Path::new("/books/Books.qbw"), 1, &books()).await.unwrap();
    auto_map_accounts(&pool, first).await.unwrap();
    let mapped = |batch: i64| {
        let pool = pool.clone();
        async move { batch_summary(&pool, batch).await.unwrap().accounts.into_iter().find(|a| a.full_name == "Equipment").unwrap().account_id.unwrap() }
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
        sales_lists: Default::default(),
        classes: Vec::new(),
        stock: Default::default(),
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
        sales_rep: None,
        custom_fields: Vec::new(),
    }
}

#[tokio::test]
async fn lists_are_matched_by_name_or_created_with_their_details() {
    let pool = test_pool().await;
    sqlx::query("INSERT INTO customers (name) VALUES ('Acme')").execute(&pool).await.unwrap();
    let mut books = ledger();
    books.customers =
        vec![party(1, "Acme", None, None), party(2, "Beta Co", None, Some("ap@beta.com")), party(3, "Beta Co:Roof job", Some(2), Some("not an email"))];
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
        custom_fields: Vec::new(),
        purchase_description: None,
        purchase_cost_cents: None,
        percent_ppm: None,
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
    let labor_account: i64 =
        sqlx::query_scalar("SELECT income_account_id FROM products_services WHERE name = 'Business Labor'").fetch_one(&pool).await.unwrap();
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
    let memo: String =
        sqlx::query_scalar("SELECT memo FROM journal_entries WHERE id = (SELECT journal_entry_id FROM ledger_import_transactions WHERE source_id = 10)")
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
    let labor: i64 = sqlx::query_scalar(
        "SELECT a.id FROM chart_of_accounts a JOIN chart_of_accounts p ON p.id = a.parent_id WHERE a.name = 'Labor' AND p.name = 'Construction'",
    )
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
    assert_eq!(
        after,
        (
            count("SELECT COUNT(*) FROM chart_of_accounts").await,
            count("SELECT COUNT(*) FROM customers").await,
            count("SELECT COUNT(*) FROM vendors").await,
            count("SELECT COUNT(*) FROM products_services").await,
        )
    );
    reverse_batch(&pool, batch).await.unwrap();
    discard_batch(&pool, batch).await.unwrap();
}

/// A file's discount item comes in as a LocalBooks discount item (with its percent), and an
/// invoice that used it after a subtotal comes in with that subtotal and a discount line, not a
/// document discount. Subtotal items are form lines, not items.
#[tokio::test]
async fn discount_and_subtotal_items_become_form_lines() {
    use crate::qbw_documents::{DocumentLine, SourceDocument};
    let pool = test_pool().await;
    let line = |item_id: i64, cents: i64, account_id: i64| DocumentLine {
        item_id: Some(item_id),
        description: None,
        quantity_e4: 10_000,
        rate_cents: cents.abs(),
        cents,
        account_id,
        class: None,
        subtotal: false,
    };
    let item = |id: i64, name: &str, kind: &'static str, account: i64, percent_ppm: Option<i64>| SourceItem {
        id,
        name: name.into(),
        description: None,
        rate_cents: if percent_ppm.is_some() { 0 } else { 10_000 },
        income_account_id: Some(account),
        kind,
        taxable: false,
        active: true,
        custom_fields: Vec::new(),
        purchase_description: None,
        purchase_cost_cents: None,
        percent_ppm,
    };
    let mut l = ledger();
    l.accounts.extend([account(6, "Design Income", "INCOME", None), account(11, "Discounts Given", "INCOME", None)]);
    l.unbalanced.clear();
    l.unreadable.clear();
    l.items = vec![item(200, "Design", "Service", 6, None), item(301, "Loyal Customer", "Discount", 11, Some(100_000))];
    l.customers = vec![SourceParty {
        id: 100,
        name: "Hillside Builders".into(),
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
        sales_rep: None,
        custom_fields: Vec::new(),
    }];
    // Design 100.00, a subtotal, then 10% off it: receivable 90.00.
    let subtotal = DocumentLine { subtotal: true, quantity_e4: 0, rate_cents: 0, account_id: 0, ..line(300, 0, 0) };
    l.transactions = vec![
        SourceTransaction {
            kind: "Invoice",
            document: Some(SourceDocument::Invoice {
                customer_id: 100,
                number: Some("12".into()),
                due_date: None,
                lines: vec![line(200, 10_000, 6), subtotal.clone(), line(301, -1_000, 11)],
                sales: Default::default(),
            }),
            ..txn(50, "2024-04-01", &[(4, -9_000), (6, 10_000), (11, -1_000)])
        },
        SourceTransaction {
            kind: "Sales receipt",
            document: Some(SourceDocument::SalesReceipt {
                customer_id: Some(100),
                number: Some("13".into()),
                method: "CASH".into(),
                lines: vec![line(200, 10_000, 6), subtotal, line(301, -1_000, 11)],
                sales: Default::default(),
            }),
            ..txn(51, "2024-04-02", &[(1, -9_000), (6, 10_000), (11, -1_000)])
        },
    ];
    let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &l).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let full = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap().full.unwrap();
    assert_eq!((full.invoices, full.sales_receipts, full.fell_back), (1, 1, 0), "{full:?}");

    let (item_type, percent): (String, Option<i64>) =
        sqlx::query_as("SELECT item_type, percent_ppm FROM products_services WHERE name = 'Loyal Customer'").fetch_one(&pool).await.unwrap();
    assert_eq!((item_type.as_str(), percent), ("DISCOUNT", Some(100_000)));
    let (total, discount): (i64, i64) = sqlx::query_as("SELECT total_cents, discount_cents FROM invoices").fetch_one(&pool).await.unwrap();
    assert_eq!((total, discount), (9_000, 0), "the discount is a line, not the document discount");
    let lines: Vec<(i64, String, i64)> =
        sqlx::query_as("SELECT line_number, kind, amount_cents FROM invoice_form_lines ORDER BY line_number").fetch_all(&pool).await.unwrap();
    assert_eq!(lines, [(2, "SUBTOTAL".to_string(), 10_000), (3, "DISCOUNT".to_string(), -1_000)]);
    let item_line: i64 = sqlx::query_scalar("SELECT line_number FROM invoice_items").fetch_one(&pool).await.unwrap();
    assert_eq!(item_line, 1);
    let receipt: Vec<(i64, String, i64)> =
        sqlx::query_as("SELECT line_number, kind, amount_cents FROM sales_receipt_form_lines ORDER BY line_number").fetch_all(&pool).await.unwrap();
    assert_eq!(receipt, lines);
    let (total, discount): (i64, i64) = sqlx::query_as("SELECT total_cents, discount_cents FROM sales_receipts").fetch_one(&pool).await.unwrap();
    assert_eq!((total, discount), (9_000, 0));
}

/// Amounts the file stores with a fraction of a cent are left out, and the summary lists each one.
#[tokio::test]
async fn the_summary_lists_amounts_that_are_not_whole_cents() {
    let pool = crate::db::test_pool().await;
    let mut odd = ledger();
    odd.quality.fractional_amounts = vec!["Invoice line, transaction 812: 10.005".into(), "Stored balance of account 3: 0.125".into()];
    let batch = stage(&pool, std::path::Path::new("/books/Odd.qbw"), 1, &odd).await.unwrap();
    let summary = batch_summary(&pool, batch).await.unwrap();
    assert_eq!(summary.fractional_amounts, odd.quality.fractional_amounts);

    let clean = stage(&pool, std::path::Path::new("/books/Clean.qbw"), 1, &ledger()).await.unwrap();
    assert!(batch_summary(&pool, clean).await.unwrap().fractional_amounts.is_empty());
}

/// Forms that sold inventory carry its cost (cost of goods sold debit, inventory credit) in the
/// same transaction. A full import still makes them an invoice and a sales receipt, and their
/// entries move the cost.
#[tokio::test]
async fn sold_inventory_cost_stays_with_its_invoice_and_sales_receipt() {
    use crate::qbw_documents::{DocumentLine, SourceDocument};
    let pool = test_pool().await;
    let doc = |id: i64, kind: &'static str, lines: &[(i64, i64)], document: SourceDocument| SourceTransaction {
        kind,
        document: Some(document),
        ..txn(id, "2024-03-01", lines)
    };
    let sold = vec![DocumentLine {
        item_id: None,
        description: Some("Tile".into()),
        quantity_e4: 10_000,
        rate_cents: 9_000,
        cents: 9_000,
        account_id: 6,
        class: None,
        subtotal: false,
    }];
    let mut l = ledger();
    l.accounts.extend([
        account(6, "Materials Income", "INCOME", None),
        account(30, "Cost of Goods Sold", "COST_OF_GOODS_SOLD", None),
        account(31, "Inventory Asset", "OTHER_CURRENT_ASSET", None),
    ]);
    l.unbalanced.clear();
    l.unreadable.clear();
    l.transactions = vec![
        doc(
            40,
            "Invoice",
            &[(4, -9_000), (6, 9_000), (30, -5_500), (31, 5_500)],
            SourceDocument::Invoice { customer_id: 100, number: Some("7".into()), due_date: None, lines: sold.clone(), sales: Default::default() },
        ),
        doc(
            41,
            "Sales receipt",
            &[(1, -9_000), (6, 9_000), (30, -5_500), (31, 5_500)],
            SourceDocument::SalesReceipt { customer_id: Some(100), number: Some("8".into()), method: "CASH".into(), lines: sold, sales: Default::default() },
        ),
    ];
    l.customers = vec![SourceParty {
        id: 100,
        name: "Hillside Builders".into(),
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
        sales_rep: None,
        custom_fields: Vec::new(),
    }];
    let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &l).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let full = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap().full.unwrap();
    assert_eq!((full.invoices, full.sales_receipts, full.fell_back), (1, 1, 0), "{full:?}");
    let summary = batch_summary(&pool, batch).await.unwrap();
    let mapped = |name: &str| summary.accounts.iter().find(|a| a.full_name == name).and_then(|a| a.account_id).unwrap();
    assert_eq!(balance(&pool, mapped("Cost of Goods Sold")).await, 11_000);
    assert_eq!(balance(&pool, mapped("Inventory Asset")).await, -11_000);
    assert_eq!(balance(&pool, mapped("Riverside Bank")).await, 9_000, "the receipt's money went to the bank");
}

/// An account the file says holds more than its transactions add up to (here a transaction that
/// could not be read completely) is listed with its expected and imported balance, not hidden.
#[tokio::test]
async fn balances_that_differ_from_the_file_are_listed() {
    let pool = test_pool().await;
    let mut l = ledger();
    // Stored balances are credit-positive, as in the file. Riverside Bank and Hosting match;
    // Domain Names also has the unbalanced 7.00 the import leaves out; Unused has a balance
    // with no transactions behind it.
    for (name, stored) in [("Riverside Bank", 12_500), ("Domain Names", -12_700), ("Unused", -4_000), ("Hosting", 0)] {
        l.accounts.iter_mut().find(|a| a.full_name == name).unwrap().stored_balance_cents = Some(stored);
    }
    let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &l).await.unwrap();
    let summary = batch_summary(&pool, batch).await.unwrap();
    let rows: Vec<(&str, i64, i64, i64)> =
        summary.balance_differences.iter().map(|d| (d.full_name.as_str(), d.expected_cents, d.imported_cents, d.difference_cents)).collect();
    assert_eq!(rows, [("Unused", 4_000, 0, -4_000), ("Domain Names", 12_700, 12_000, -700)]);

    l.accounts.iter_mut().for_each(|a| a.stored_balance_cents = None);
    let batch = stage(&pool, Path::new("/books/Other.qbw"), 1, &l).await.unwrap();
    assert!(batch_summary(&pool, batch).await.unwrap().balance_differences.is_empty(), "no stored balance, nothing to compare");

    // A file of lists only has balances but nothing to post: nothing is listed as differing.
    let mut lists = ledger();
    lists.transactions.clear();
    lists.unbalanced.clear();
    lists.unreadable.clear();
    lists.accounts[0].stored_balance_cents = Some(-9_900);
    let batch = stage(&pool, Path::new("/books/Lists.qbw"), 1, &lists).await.unwrap();
    let summary = batch_summary(&pool, batch).await.unwrap();
    assert!(summary.lists_only && summary.balance_differences.is_empty());
}

/// The cases a full import keeps as they were, pinned: a receipt whose cost pair runs the other
/// way (a debit to Inventory Asset beside Undeposited Funds) is still a receipt; a receipt split
/// between Undeposited Funds and a bank falls back to a journal entry; a discount item that posts
/// to an expense account comes off the whole invoice, while the invoice keeps its subtotal.
#[tokio::test]
async fn receipts_and_discounts_the_form_cannot_show_as_they_were() {
    use crate::qbw_documents::{DocumentLine, SourceDocument};
    let pool = test_pool().await;
    let line = |item_id: i64, cents: i64, account_id: i64| DocumentLine {
        item_id: Some(item_id),
        description: None,
        quantity_e4: 10_000,
        rate_cents: cents.abs(),
        cents,
        account_id,
        class: None,
        subtotal: false,
    };
    let subtotal = DocumentLine { subtotal: true, quantity_e4: 0, rate_cents: 0, account_id: 0, ..line(300, 0, 0) };
    let item = |id: i64, name: &str, kind: &'static str, account: i64| SourceItem {
        id,
        name: name.into(),
        description: None,
        rate_cents: 0,
        income_account_id: Some(account),
        kind,
        taxable: false,
        active: true,
        custom_fields: Vec::new(),
        percent_ppm: None,
        purchase_description: None,
        purchase_cost_cents: None,
    };
    let mut l = ledger();
    l.accounts.extend([
        account(6, "Design Income", "INCOME", None),
        account(7, "Undeposited Funds", "OTHER_CURRENT_ASSET", None),
        account(11, "Promotions", "EXPENSE", None),
        account(12, "Discounts Given", "INCOME", None),
        account(30, "Cost of Goods Sold", "COST_OF_GOODS_SOLD", None),
        account(31, "Inventory Asset", "OTHER_CURRENT_ASSET", None),
    ]);
    l.unbalanced.clear();
    l.unreadable.clear();
    l.items = vec![item(200, "Design", "Service", 6), item(301, "Loyal Customer", "Discount", 12), item(302, "Promo", "Discount", 11)];
    l.customers = vec![SourceParty {
        id: 100,
        name: "Hillside Builders".into(),
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
        sales_rep: None,
        custom_fields: Vec::new(),
    }];
    let receipt = |lines: Vec<DocumentLine>| SourceDocument::SalesReceipt {
        customer_id: Some(100),
        number: None,
        method: "CASH".into(),
        lines,
        sales: Default::default(),
    };
    let doc = |id: i64, kind: &'static str, lines: &[(i64, i64)], document: SourceDocument| SourceTransaction {
        kind,
        document: Some(document),
        ..txn(id, "2024-05-01", lines)
    };
    l.transactions = vec![
        doc(60, "Sales receipt", &[(7, -9_000), (6, 9_000), (31, -500), (30, 500)], receipt(vec![line(200, 9_000, 6)])),
        doc(
            61,
            "Sales receipt",
            &[(7, -5_000), (1, -3_000), (6, 9_000), (12, -1_000)],
            receipt(vec![line(200, 9_000, 6), subtotal.clone(), line(301, -1_000, 12)]),
        ),
        doc(
            62,
            "Invoice",
            &[(4, -9_000), (6, 10_000), (11, -1_000)],
            SourceDocument::Invoice {
                customer_id: 100,
                number: Some("20".into()),
                due_date: None,
                lines: vec![line(200, 10_000, 6), subtotal, line(302, -1_000, 11)],
                sales: Default::default(),
            },
        ),
    ];
    let batch = stage(&pool, Path::new("/books/Books.qbw"), 1, &l).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let full = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap().full.unwrap();
    assert_eq!((full.invoices, full.sales_receipts, full.fell_back), (1, 1, 1), "{full:?}");
    let fallbacks = batch_summary(&pool, batch).await.unwrap().fallbacks;
    assert_eq!(fallbacks.iter().map(|f| f.reason.as_str()).collect::<Vec<_>>(), ["the money went into more than one account"]);

    let (total, discount): (i64, i64) = sqlx::query_as("SELECT total_cents, discount_cents FROM invoices").fetch_one(&pool).await.unwrap();
    assert_eq!((total, discount), (9_000, 1_000), "an expense-account discount comes off the whole invoice");
    let kinds: Vec<String> = sqlx::query_scalar("SELECT kind FROM invoice_form_lines ORDER BY line_number").fetch_all(&pool).await.unwrap();
    assert_eq!(kinds, ["SUBTOTAL"]);
}

/// Migration 0151 relabels the fixed asset items that desktop company file batches staged by the
/// older reader (before migration 0150) labelled "Other charge". Batches the corrected reader
/// staged, where "Other charge" is a real other charge, and other formats are left alone.
#[tokio::test]
async fn older_desktop_batches_get_their_fixed_assets_back() {
    let pool = test_pool().await;
    let mut l = ledger();
    l.items = vec![SourceItem {
        id: 700,
        name: "Delivery van".into(),
        description: None,
        rate_cents: 0,
        income_account_id: None,
        kind: "Other charge",
        taxable: false,
        active: true,
        custom_fields: Vec::new(),
        percent_ppm: None,
        purchase_description: None,
        purchase_cost_cents: None,
    }];
    let desktop = stage(&pool, Path::new("/books/Old.QBB"), 1, &l).await.unwrap();
    let iif = stage(&pool, Path::new("/books/Lists.iif"), 1, &l).await.unwrap();
    let newer = stage(&pool, Path::new("/books/New.qbw"), 1, &l).await.unwrap();
    for batch in [desktop, iif] {
        sqlx::query("UPDATE ledger_import_batches SET imported_at = '2020-01-01T00:00:00.000Z' WHERE id = ?1").bind(batch).execute(&pool).await.unwrap();
    }
    sqlx::raw_sql(include_str!("../../migrations/0151_staged_item_kinds.sql")).execute(&pool).await.unwrap();
    let kind = |batch: i64| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, String>("SELECT item_kind FROM ledger_import_items WHERE batch_id = ?1").bind(batch).fetch_one(&pool).await.unwrap() }
    };
    assert_eq!((kind(desktop).await.as_str(), kind(iif).await.as_str(), kind(newer).await.as_str()), ("Fixed asset", "Other charge", "Other charge"));
}

/// A company file's item groups come in as LocalBooks groups, with their items in order and their
/// quantities. Items a LocalBooks group can't hold (a discount) are left out of the group; a group
/// none of whose items came in, or whose name an item has, is left out; one already in LocalBooks
/// is matched by name. Reversing the import deactivates the groups it created, and adding the
/// lists again puts them back.
#[tokio::test]
async fn item_groups_come_in_as_groups() {
    let pool = test_pool().await;
    let product: i64 = sqlx::query_scalar(
        "INSERT INTO products_services (name, rate_cents, income_account_id)
         SELECT 'Paint', 1000, a.id FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE t.classification = 'INCOME' LIMIT 1
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO item_groups (name) VALUES ('Starter set')").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO item_group_members (group_id, position, product_id, quantity_e4) VALUES (1, 1, ?1, 10000)")
        .bind(product)
        .execute(&pool)
        .await
        .unwrap();
    // A file with lists only, so adding its lists again after a reversal puts them back.
    let mut books = ledger();
    (books.transactions, books.unbalanced, books.unreadable) = (Vec::new(), Vec::new(), Vec::new());
    books.accounts.push(account(6, "Sales", "INCOME", None));
    let item = |id: i64, name: &str, kind: &'static str| SourceItem {
        id,
        name: name.into(),
        description: None,
        rate_cents: 2_500,
        income_account_id: Some(6),
        kind,
        taxable: false,
        active: true,
        custom_fields: Vec::new(),
        purchase_description: None,
        purchase_cost_cents: None,
        percent_ppm: None,
    };
    books.items =
        vec![item(20, "Door frame", "Inventory part"), item(21, "Hinge", "Inventory part"), item(22, "Install", "Service"), item(23, "Promo", "Discount")];
    let group = |id: i64, name: &str, print_items: bool, members: Vec<(i64, i64)>| SourceItemGroup {
        id,
        name: name.into(),
        description: Some(format!("{name} description")),
        print_items,
        active: true,
        members,
    };
    books.sales_lists.item_groups = vec![
        // Install first, then a frame and three hinges; the discount can't be in a LocalBooks group.
        group(60, "Door kit", false, vec![(22, 10_000), (20, 10_000), (21, 30_000), (23, 10_000)]),
        group(61, "Only promos", true, vec![(23, 10_000), (99, 10_000)]),
        group(62, "Hinge", true, vec![(21, 10_000)]),
        group(63, "Starter set", true, vec![(20, 20_000)]),
    ];
    let batch = stage(&pool, Path::new("/books/Kits.qbw"), 1, &books).await.unwrap();
    let r = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap().lists.unwrap();
    assert_eq!((r.groups_created, r.groups_matched, r.groups_left_out, r.group_items_left_out), (1, 1, 2, 1), "the discount is left out of Door kit");
    let (id, description, print_items, active): (i64, Option<String>, i64, i64) =
        sqlx::query_as("SELECT id, description, print_items, is_active FROM item_groups WHERE name = 'Door kit'").fetch_one(&pool).await.unwrap();
    assert_eq!((description.as_deref(), print_items, active), (Some("Door kit description"), 0, 1));
    let members: Vec<(String, i64)> = sqlx::query_as(
        "SELECT p.name, m.quantity_e4 FROM item_group_members m JOIN products_services p ON p.id = m.product_id WHERE m.group_id = ?1 ORDER BY m.position",
    )
    .bind(id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(members, [("Install".to_string(), 10_000), ("Door frame".to_string(), 10_000), ("Hinge".to_string(), 30_000)]);
    let starter: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM item_group_members WHERE group_id = 1").fetch_one(&pool).await.unwrap();
    assert_eq!(starter, 1, "a group already in Tillhouse is left as it is");
    // Again: nothing new. The groups left out are tried again (an item may have been renamed since).
    let left_out = ImportListsResult { groups_left_out: 2, ..Default::default() };
    assert_eq!(import_lists(&pool, batch).await.unwrap(), left_out);

    reverse_batch(&pool, batch).await.unwrap();
    let active = |name: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT is_active FROM item_groups WHERE name = ?1").bind(name).fetch_one(&pool).await.unwrap() }
    };
    assert_eq!((active("Door kit").await, active("Starter set").await), (0, 1), "only the group the import created goes inactive");
    let readded = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Journal }).await.unwrap();
    assert_eq!(readded.lists.unwrap(), left_out, "nothing duplicated");
    assert_eq!(active("Door kit").await, 1);
    reverse_batch(&pool, batch).await.unwrap();
    discard_batch(&pool, batch).await.unwrap();
    let staged: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ledger_import_item_groups").fetch_one(&pool).await.unwrap();
    assert_eq!(staged, 0);
}
