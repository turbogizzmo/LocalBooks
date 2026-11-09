use super::*;
use crate::db::test_pool;
use crate::expenses::ExpenseInput;
use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

#[test]
fn periods_cover_the_right_days() {
    let today = d(2026, 5, 14);
    assert_eq!(Period::ThisMonth.dates(today, 1), (d(2026, 5, 1), d(2026, 5, 31)));
    assert_eq!(Period::LastMonth.dates(today, 1), (d(2026, 4, 1), d(2026, 4, 30)));
    assert_eq!(Period::ThisQuarter.dates(today, 1), (d(2026, 4, 1), d(2026, 6, 30)));
    assert_eq!(Period::LastQuarter.dates(today, 1), (d(2026, 1, 1), d(2026, 3, 31)));
    assert_eq!(Period::ThisFiscalYear.dates(today, 7), (d(2025, 7, 1), d(2026, 6, 30)));
    assert_eq!(Period::LastFiscalYear.dates(today, 7), (d(2024, 7, 1), d(2025, 6, 30)));
    assert_eq!(Period::Last12Months.dates(today, 1), (d(2025, 6, 1), d(2026, 5, 31)));
    assert_eq!(Period::Today.dates(today, 1), (d(2026, 1, 1), today));
    assert_eq!(Period::LastQuarter.dates(d(2026, 2, 10), 1), (d(2025, 10, 1), d(2025, 12, 31)));
    for p in Period::ALL {
        assert_eq!(Period::from_code(p.code()), Some(p));
    }
}

#[tokio::test]
async fn every_widget_ties_out_to_its_report() {
    let pool = test_pool().await;
    let (checking, services, rent, contractors) = (account(&pool, "1010").await, account(&pool, "4100").await, account(&pool, "6300").await, account(&pool, "6400").await);
    // Sales to seven customers (so the top five fold the rest into Others), and some expenses.
    for (i, name) in ["Alder", "Birch", "Cedar", "Dogwood", "Elm", "Fir", "Gum"].iter().enumerate() {
        let cents = 10_000 * (i as i64 + 1);
        let input: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
            "customerName": name, "issueDate": "2026-03-02", "dueDate": "2026-03-16",
            "lines": [{ "description": "Design", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }] })).unwrap();
        crate::invoices::create_and_post_invoice(&pool, input).await.unwrap();
    }
    for (vendor, account_id, cents) in [("Harbor Leasing", rent, 150_000), ("Ridgeline Carpentry", contractors, 80_000)] {
        let check: ExpenseInput = serde_json::from_value(serde_json::json!({ "kind": "CHECK", "payeeName": vendor, "paymentAccountId": checking, "expenseDate": "2026-04-01",
            "checkNumber": null, "paymentMethod": null, "memo": null, "lines": [{ "accountId": account_id, "amountCents": cents, "memo": null }] })).unwrap();
        crate::expenses::post_expense(&pool, check).await.unwrap();
    }
    post_journal_entry(&pool, JournalEntry { id: None, date: d(2026, 1, 5), description: "Owner money".into(), lines: vec![JournalLine::debit(checking, 1_000_000), JournalLine::credit(account(&pool, "3000").await, 1_000_000)], currency: None })
        .await
        .unwrap();

    for kind in WidgetKind::ALL {
        if !matches!(kind, WidgetKind::AccountBalances | WidgetKind::WhoOwes | WidgetKind::VendorsToPay | WidgetKind::IncomeExpenseTrend | WidgetKind::TopCustomers) {
            add(&pool, kind).await.unwrap();
        }
    }
    let today = d(2026, 5, 14);
    let page = snapshot(&pool, today).await.unwrap();
    assert_eq!(page.len(), 7, "the starting page and the two added");
    let widget = |k: WidgetKind| page.iter().find(|w| w.kind == k).unwrap();

    // Account balances: the Balance Sheet's bank lines as of today.
    let bs = crate::statements::balance_sheet(&pool, today).await.unwrap();
    let bank = widget(WidgetKind::AccountBalances);
    let checking_line = bank.lines.iter().find(|l| l.account_id == Some(checking)).unwrap();
    assert_eq!(Some(checking_line.amount_cents), bs.lines.iter().find(|l| l.account_id == Some(checking)).and_then(|l| l.amount_cents));
    assert_eq!(bank.total_cents, 1_000_000 - 230_000);
    assert_eq!(bank.report, "bs");

    // Who owes: the A/R aging total, top five, the rest folded.
    let ar = crate::report_center::aging(&pool, false, today).await.unwrap();
    let owes = widget(WidgetKind::WhoOwes);
    assert_eq!((owes.total_cents, owes.lines.len()), (ar.total_cents, 5));
    assert_eq!(owes.lines[0].label, "Gum");
    assert_eq!(owes.lines.iter().map(|l| l.amount_cents).sum::<i64>() + owes.others_cents, ar.total_cents);
    assert_eq!(owes.lines[0].overdue_cents, Some(70_000), "due in March, so all overdue by May");
    assert_eq!(widget(WidgetKind::VendorsToPay).total_cents, crate::report_center::aging(&pool, true, today).await.unwrap().total_cents);

    // Trend: the Profit and Loss by month for the last 12 months.
    let trend = widget(WidgetKind::IncomeExpenseTrend);
    assert_eq!(trend.trend.len(), 12);
    let pl = crate::statements::profit_and_loss_basis(&pool, trend.start_date, trend.end_date, None, Basis::Accrual).await.unwrap();
    assert_eq!(trend.total_cents, pl.net_income_cents);
    assert_eq!(trend.trend.iter().map(|p| p.net_cents).sum::<i64>(), pl.net_income_cents);
    assert_eq!(trend.trend.iter().find(|p| p.month == "2026-03").unwrap().income_cents, 280_000);

    // Top customers, vendors and items: the summaries for the fiscal year.
    let (start, end) = (d(2026, 1, 1), d(2026, 12, 31));
    let customers = widget(WidgetKind::TopCustomers);
    assert_eq!((customers.start_date, customers.end_date), (start, end));
    assert_eq!(customers.total_cents, crate::report_center::by_party(&pool, false, start, end).await.unwrap().total_cents);
    assert_eq!(customers.others_cents, 10_000 + 20_000, "Alder and Birch fold into Others");
    assert_eq!(widget(WidgetKind::TopVendors).total_cents, crate::report_center::by_party(&pool, true, start, end).await.unwrap().total_cents);
    assert_eq!(widget(WidgetKind::TopVendors).lines[0].label, "Harbor Leasing");
    assert_eq!(widget(WidgetKind::TopItems).total_cents, crate::report_center::sales_by_product(&pool, start, end).await.unwrap().total_cents);
}

#[tokio::test]
async fn widgets_are_added_removed_moved_and_given_a_period() {
    let pool = test_pool().await;
    let today = d(2026, 5, 14);
    let start: Vec<i64> = snapshot(&pool, today).await.unwrap().iter().map(|w| w.id).collect();
    assert_eq!(start.len(), 5);
    let items = add(&pool, WidgetKind::TopItems).await.unwrap();
    remove(&pool, start[1]).await.unwrap();
    let mut order: Vec<i64> = snapshot(&pool, today).await.unwrap().iter().map(|w| w.id).collect();
    order.reverse();
    arrange(&pool, &order).await.unwrap();
    let page = snapshot(&pool, today).await.unwrap();
    assert_eq!(page.iter().map(|w| w.id).collect::<Vec<_>>(), order);
    assert_eq!(page[0].id, items);
    assert!(arrange(&pool, &order[1..]).await.unwrap_err().starts_with("SNAPSHOT_ORDER"), "every widget, once");

    set_period(&pool, items, Period::LastQuarter).await.unwrap();
    let w = snapshot(&pool, today).await.unwrap().into_iter().find(|w| w.id == items).unwrap();
    assert_eq!((w.period, w.start_date, w.end_date), (Period::LastQuarter, d(2026, 1, 1), d(2026, 3, 31)));
    assert!(set_period(&pool, start[1], Period::ThisMonth).await.unwrap_err().starts_with("SNAPSHOT_NO_WIDGET"));
}
