use super::*;
use crate::db::test_pool;
use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};

async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1")
        .bind(number)
        .fetch_one(pool)
        .await
        .expect("seeded account exists")
}

fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

async fn post(pool: &SqlitePool, date: NaiveDate, debit: i64, credit: i64, cents: i64) {
    post_journal_entry(
        pool,
        JournalEntry { id: None, date, description: String::new(), lines: vec![JournalLine::debit(debit, cents), JournalLine::credit(credit, cents)], currency: None },
    )
    .await
    .expect("posts");
}

fn find<'l>(lines: &'l [ReportLine], label: &str) -> &'l ReportLine {
    lines.iter().find(|l| l.label == label).unwrap_or_else(|| panic!("missing line {label}"))
}

#[tokio::test]
async fn profit_and_loss_by_class_splits_every_row_and_adds_across() {
    use crate::classes::{create_class, ClassInput};
    let pool = test_pool().await;
    let (checking, sales, rent, interest) =
        (account_id(&pool, "1010").await, account_id(&pool, "4000").await, account_id(&pool, "6300").await, account_id(&pool, "7000").await);
    let class = |name: &str| create_class(&pool, ClassInput { name: name.into(), parent_id: None });
    let (east, west, idle) = (class("East").await.unwrap().id, class("West").await.unwrap().id, class("Idle").await.unwrap().id);
    let post_lines = |lines: Vec<JournalLine>| {
        post_journal_entry(&pool, JournalEntry { id: None, date: ymd(2026, 4, 10), description: String::new(), lines, currency: None })
    };

    post_lines(vec![
        JournalLine::debit(checking, 100_000),
        JournalLine::credit(sales, 60_000).with_class(Some(east)),
        JournalLine::credit(sales, 40_000).with_class(Some(west)),
    ])
    .await
    .unwrap();
    post_lines(vec![JournalLine::debit(rent, 30_000).with_class(Some(east)), JournalLine::credit(checking, 30_000)]).await.unwrap();
    post_lines(vec![JournalLine::debit(checking, 500), JournalLine::credit(interest, 500)]).await.unwrap();
    // A voided West expense nets to zero in West.
    let voided = post_lines(vec![JournalLine::debit(rent, 9_999).with_class(Some(west)), JournalLine::credit(checking, 9_999)]).await.unwrap();
    crate::ledger::void_transaction(&pool, voided).await.unwrap();
    // Outside the range.
    post_journal_entry(
        &pool,
        JournalEntry { id: None, date: ymd(2026, 6, 1), description: String::new(), currency: None,
            lines: vec![JournalLine::debit(rent, 1).with_class(Some(idle)), JournalLine::credit(checking, 1)] },
    )
    .await
    .unwrap();

    let report = profit_and_loss_by_class(&pool, ymd(2026, 4, 1), ymd(2026, 4, 30)).await.unwrap();
    let labels: Vec<&str> = report.columns.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["East", "West", "Unclassified", "Total"]);

    let plain = profit_and_loss(&pool, ymd(2026, 4, 1), ymd(2026, 4, 30)).await.unwrap();
    assert_eq!(report.lines.len(), plain.lines.len());
    for (row, line) in report.lines.iter().zip(&plain.lines) {
        assert_eq!(row.label, line.label);
        assert_eq!(*row.amounts_cents.last().unwrap(), line.amount_cents, "Total column of {}", row.label);
        if let Some(total) = line.amount_cents {
            let across: i64 = row.amounts_cents[..3].iter().map(|a| a.unwrap_or(0)).sum();
            assert_eq!(across, total, "{} adds across", row.label);
        }
    }
    let row = |label: &str| report.lines.iter().find(|l| l.label == label).unwrap().amounts_cents.clone();
    assert_eq!(row("Net Income"), [Some(30_000), Some(40_000), Some(500), Some(70_500)]);
    assert_eq!(row("Net Other Income"), [Some(0), Some(0), Some(500), Some(500)]);
    assert_eq!(row("6300 Rent & Lease"), [Some(30_000), Some(0), Some(0), Some(30_000)]);
}

#[tokio::test]
async fn cash_basis_counts_invoices_as_paid_and_periods_line_up() {
    let pool = test_pool().await;
    let (checking, services, rent) = (account_id(&pool, "1010").await, account_id(&pool, "4100").await, account_id(&pool, "6300").await);
    let invoice = |day: &str, cents: i64| {
        let pool = pool.clone();
        let input: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
            "customerName": "Acme", "issueDate": day, "dueDate": "2026-12-31",
            "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }]
        }))
        .unwrap();
        async move { crate::invoices::create_and_post_invoice(&pool, input).await.unwrap().id }
    };
    let pay = |invoice_id, day, cents| {
        crate::invoices::record_payment(
            &pool,
            crate::invoices::PaymentInput { invoice_id, deposit_account_id: checking, payment_date: day, amount_cents: cents, payment_method: "CHECK".into(), reference: None, exchange_rate_e6: None },
        )
    };
    let first = invoice("2026-01-10", 100_000).await;
    post(&pool, ymd(2026, 1, 15), rent, checking, 10_000).await;
    pay(first, ymd(2026, 2, 5), 40_000).await.unwrap();
    pay(first, ymd(2026, 3, 3), 60_000).await.unwrap();
    let second = invoice("2026-03-01", 50_000).await;
    let memo = crate::credits::create_credit_memo(
        &pool,
        crate::credits::CreditMemoInput {
            customer_name: "Acme".into(),
            credit_date: ymd(2026, 3, 10),
            memo: None,
            refund_account_id: None,
            lines: vec![crate::sales::SalesLineInput { product_service_id: None, description: "Credit".into(), quantity_e4: 10_000, rate_cents: 20_000, income_account_id: services, class_id: None, is_taxable: false }],
            tax_rate_id: None,
        },
    )
    .await
    .unwrap();
    crate::credits::apply_customer_credit(&pool, crate::credits::ApplyCreditInput { credit_id: memo.id, document_id: second, amount_cents: 20_000, applied_date: ymd(2026, 3, 12) }).await.unwrap();
    pay(second, ymd(2026, 3, 20), 30_000).await.unwrap();

    let net = |basis, m: u32, last: u32| profit_and_loss_basis(&pool, ymd(2026, m, 1), ymd(2026, m, last), None, basis);
    assert_eq!(net(Basis::Accrual, 1, 31).await.unwrap().net_income_cents, 90_000);
    assert_eq!(net(Basis::Cash, 1, 31).await.unwrap().net_income_cents, -10_000, "January: only the rent was paid");
    assert_eq!(net(Basis::Cash, 2, 28).await.unwrap().net_income_cents, 40_000);
    assert_eq!(net(Basis::Accrual, 3, 31).await.unwrap().net_income_cents, 30_000, "invoice 50,000 less the 20,000 credit");
    assert_eq!(net(Basis::Cash, 3, 31).await.unwrap().net_income_cents, 90_000, "60,000 + 30,000 received; the credit applied nets to zero");
    let year = |basis| profit_and_loss_basis(&pool, ymd(2026, 1, 1), ymd(2026, 12, 31), None, basis);
    assert_eq!(year(Basis::Cash).await.unwrap().net_income_cents, year(Basis::Accrual).await.unwrap().net_income_cents, "everything is paid by year end");

    let report = profit_and_loss_periods(
        &pool,
        &PeriodReportInput {
            columns: vec![
                PeriodColumn { label: "Mar 2026".into(), start_date: ymd(2026, 3, 1), end_date: ymd(2026, 3, 31) },
                PeriodColumn { label: "Feb 2026".into(), start_date: ymd(2026, 2, 1), end_date: ymd(2026, 2, 28) },
            ],
            total: false,
            compare: true,
            basis: Basis::Cash,
            class_filter: None,
        },
    )
    .await
    .unwrap();
    let labels: Vec<&str> = report.columns.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["Mar 2026", "Feb 2026", "$ Change", "% Change"]);
    let net_row = report.lines.iter().find(|l| l.label == "Net Income").unwrap();
    assert_eq!(net_row.amounts_cents, [Some(90_000), Some(40_000), Some(50_000), Some(12_500)], "up 125%");
    assert!(report.lines.iter().all(|l| l.label != "6300 Rent & Lease"), "rent was paid in January, outside both columns");
    let services_row = report.lines.iter().find(|l| l.label == "4100 Services").unwrap();
    assert_eq!(services_row.amounts_cents, [Some(90_000), Some(40_000), Some(50_000), Some(12_500)]);
}

#[tokio::test]
async fn parent_classes_get_subclass_columns_other_and_a_total() {
    use crate::classes::{create_class, ClassInput};
    let pool = test_pool().await;
    let (checking, sales, rent) = (account_id(&pool, "1010").await, account_id(&pool, "4000").await, account_id(&pool, "6300").await);
    let class = |name: &str, parent| create_class(&pool, ClassInput { name: name.into(), parent_id: parent });
    let east = class("East", None).await.unwrap().id;
    let retail = class("Retail", Some(east)).await.unwrap().id;
    let wholesale = class("Wholesale", Some(east)).await.unwrap().id;
    let _quiet = class("Quiet", Some(east)).await.unwrap().id;
    let west = class("West", None).await.unwrap().id;
    let post = |lines: Vec<JournalLine>| post_journal_entry(&pool, JournalEntry { id: None, date: ymd(2026, 5, 1), description: String::new(), lines, currency: None });
    post(vec![
        JournalLine::debit(checking, 1_000),
        JournalLine::credit(sales, 600).with_class(Some(retail)),
        JournalLine::credit(sales, 300).with_class(Some(wholesale)),
        JournalLine::credit(sales, 100).with_class(Some(west)),
    ])
    .await
    .unwrap();
    post(vec![JournalLine::debit(rent, 200).with_class(Some(east)), JournalLine::credit(checking, 200)]).await.unwrap();

    let report = profit_and_loss_by_class(&pool, ymd(2026, 5, 1), ymd(2026, 5, 31)).await.unwrap();
    let labels: Vec<(&str, &str)> = report.columns.iter().map(|c| (c.label.as_str(), c.kind)).collect();
    assert_eq!(
        labels,
        [("East:Retail", "class"), ("East:Wholesale", "class"), ("East (other)", "other"), ("Total East", "subtotal"), ("West", "class"), ("Total", "total")]
    );
    let net = report.lines.iter().find(|l| l.label == "Net Income").unwrap();
    assert_eq!(net.amounts_cents, [Some(600), Some(300), Some(-200), Some(700), Some(100), Some(800)]);

    // Filtering the plain Profit & Loss by East includes its subclasses; West alone excludes them.
    let east_only = profit_and_loss_filtered(&pool, ymd(2026, 5, 1), ymd(2026, 5, 31), Some(&ClassFilter { class_ids: vec![east], unclassified: false })).await.unwrap();
    assert_eq!(east_only.net_income_cents, 700);
    let west_only = profit_and_loss_filtered(&pool, ymd(2026, 5, 1), ymd(2026, 5, 31), Some(&ClassFilter { class_ids: vec![west], unclassified: false })).await.unwrap();
    assert_eq!(west_only.net_income_cents, 100);
    let none = profit_and_loss_filtered(&pool, ymd(2026, 5, 1), ymd(2026, 5, 31), Some(&ClassFilter { class_ids: vec![], unclassified: true })).await.unwrap();
    assert_eq!(none.net_income_cents, 0);
}

#[tokio::test]
async fn statements_tie_out_with_subaccounts_and_retained_earnings() {
    let pool = test_pool().await;
    let n = |s| account_id(&pool, s);
    let (checking, card, owner, services, cogs, rent, utilities, interest) =
        (n("1010").await, n("2100").await, n("3100").await, n("4100").await, n("5000").await, n("6300").await, n("6500").await, n("7000").await);
    let electric: i64 = sqlx::query_scalar(
        "INSERT INTO chart_of_accounts (account_number, name, account_type, parent_id) VALUES ('6510', 'Electric', 'EXPENSE', ?1) RETURNING id",
    )
    .bind(utilities)
    .fetch_one(&pool)
    .await
    .unwrap();

    // Prior year: invest 10,000 and earn 2,000 of service income.
    post(&pool, ymd(2025, 3, 1), checking, owner, 1_000_000).await;
    post(&pool, ymd(2025, 6, 1), checking, services, 200_000).await;
    // Current year activity.
    post(&pool, ymd(2026, 2, 1), checking, services, 500_000).await;
    post(&pool, ymd(2026, 2, 5), cogs, checking, 50_000).await;
    post(&pool, ymd(2026, 2, 10), rent, checking, 120_000).await;
    post(&pool, ymd(2026, 2, 12), utilities, card, 10_000).await;
    post(&pool, ymd(2026, 2, 13), electric, card, 7_500).await;
    post(&pool, ymd(2026, 2, 20), checking, interest, 1_234).await;

    let pl = profit_and_loss(&pool, ymd(2026, 1, 1), ymd(2026, 12, 31)).await.unwrap();
    assert_eq!(pl.total_revenue_cents, 501_234);
    assert_eq!(pl.total_expenses_cents, 187_500);
    assert_eq!(pl.gross_profit_cents, 450_000);
    assert_eq!(pl.net_income_cents, 313_734);
    assert_eq!(pl.net_income_display, "$3,137.34");
    let net = pl.lines.last().unwrap();
    assert_eq!((net.kind, net.label.as_str(), net.amount_cents), (LineKind::Net, "Net Income", Some(313_734)));
    // Subaccount nests under its parent with a parent total.
    assert_eq!(find(&pl.lines, "6510 Electric").depth, 2);
    assert_eq!(find(&pl.lines, "6500 Utilities").amount_cents, Some(10_000));
    assert_eq!(find(&pl.lines, "Total 6500 Utilities").amount_cents, Some(17_500));
    assert_eq!(find(&pl.lines, "Total Other Income").amount_cents, Some(1_234));

    let bs = balance_sheet(&pool, ymd(2026, 6, 30)).await.unwrap();
    assert!(bs.is_balanced, "difference {}", bs.difference_cents);
    assert_eq!(bs.total_assets_cents, 1_531_234);
    assert_eq!(bs.total_liabilities_cents, 17_500);
    assert_eq!(bs.net_income_cents, 313_734);
    assert_eq!(bs.total_equity_cents, 1_000_000 + 200_000 + 313_734);
    assert_eq!(find(&bs.lines, "3900 Retained Earnings").amount_cents, Some(200_000));
    assert_eq!(find(&bs.lines, "TOTAL ASSETS").kind, LineKind::Net);
    assert_eq!(find(&bs.lines, "TOTAL LIABILITIES AND EQUITY").amount_cents, Some(1_531_234));

    // As of the prior year end, everything is still current-year income.
    let prior = balance_sheet(&pool, ymd(2025, 12, 31)).await.unwrap();
    assert!(prior.is_balanced);
    assert_eq!(prior.net_income_cents, 200_000);
    assert!(prior.lines.iter().all(|l| l.label != "3900 Retained Earnings"));
}

#[tokio::test]
async fn balance_sheet_closes_net_income_at_the_configured_fiscal_year() {
    let pool = test_pool().await;
    let (checking, services) = (account_id(&pool, "1010").await, account_id(&pool, "4100").await);
    post(&pool, ymd(2026, 3, 1), checking, services, 100_000).await; // fiscal 2025-26 when FY starts in July
    post(&pool, ymd(2026, 8, 1), checking, services, 40_000).await; // fiscal 2026-27

    let calendar = balance_sheet(&pool, ymd(2026, 10, 13)).await.unwrap();
    assert_eq!((calendar.fiscal_year_start, calendar.net_income_cents), (ymd(2026, 1, 1), 140_000));

    crate::preferences::set(&pool, "fiscal_year_start", "07-01").await.unwrap();
    crate::preferences::set(&pool, "currency_symbol", "€").await.unwrap();
    let july = balance_sheet(&pool, ymd(2026, 10, 13)).await.unwrap();
    assert!(july.is_balanced);
    assert_eq!((july.fiscal_year_start, july.net_income_cents), (ymd(2026, 7, 1), 40_000));
    assert_eq!(find(&july.lines, "3900 Retained Earnings").amount_cents, Some(100_000));
    assert_eq!(july.currency_symbol, "€");
    let pl = profit_and_loss(&pool, ymd(2026, 7, 1), ymd(2026, 10, 13)).await.unwrap();
    assert_eq!(pl.net_income_display, "€400.00");
}

#[tokio::test]
async fn empty_books_still_render_and_balance() {
    let pool = test_pool().await;
    let pl = profit_and_loss(&pool, ymd(2026, 1, 1), ymd(2026, 1, 31)).await.unwrap();
    assert_eq!(pl.net_income_display, "$0.00");
    assert!(pl.lines.iter().any(|l| l.label == "Total Income"));
    let bs = balance_sheet(&pool, ymd(2026, 1, 31)).await.unwrap();
    assert!(bs.is_balanced);
    assert!(profit_and_loss(&pool, ymd(2026, 2, 1), ymd(2026, 1, 1)).await.is_err());
}
