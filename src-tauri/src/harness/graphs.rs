//! The graph reports tie out to their text reports on the fixture company: every bar, slice and
//! total equals the matching report for the same dates, and the monthly bars add up to the report
//! for the whole range.

use super::account;
use super::fixture::{d, harbor_lane};
use crate::graphs::{self, GraphSlice, SalesBy};
use crate::report_center;
use crate::statements::{self, Basis, BudgetAmount, ClassReportLine, LineKind, PeriodColumn, PeriodReportInput};

fn sum(slices: &[GraphSlice]) -> i64 {
    slices.iter().map(|s| s.amount_cents).sum()
}

/// A depth-0 total line of a multi-column statement, column by column.
fn total_row<'l>(lines: &'l [ClassReportLine], label: &str) -> &'l [Option<i64>] {
    &lines.iter().find(|l| l.depth == 0 && l.label == label && l.kind != LineKind::Section).unwrap_or_else(|| panic!("line {label}")).amounts_cents
}

#[test]
fn months_clip_to_the_range_and_cap_at_36() {
    let m = graphs::months(d(2025, 1, 15), d(2025, 3, 10)).unwrap();
    assert_eq!(m.iter().map(|m| m.month.as_str()).collect::<Vec<_>>(), ["2025-01", "2025-02", "2025-03"]);
    assert_eq!((m[0].start_date, m[0].end_date), (d(2025, 1, 15), d(2025, 1, 31)));
    assert_eq!((m[1].start_date, m[1].end_date), (d(2025, 2, 1), d(2025, 2, 28)));
    assert_eq!((m[2].start_date, m[2].end_date), (d(2025, 3, 1), d(2025, 3, 10)));
    assert_eq!(graphs::months(d(2023, 1, 1), d(2025, 12, 31)).unwrap().len(), 36);
    assert!(graphs::months(d(2023, 1, 1), d(2026, 1, 1)).unwrap_err().starts_with("RPT_BAD_COLUMNS"));
    assert!(graphs::months(d(2025, 2, 1), d(2025, 1, 1)).unwrap_err().starts_with("RPT_BAD_RANGE"));
}

#[tokio::test]
async fn income_and_expense_match_profit_and_loss() {
    let pool = harbor_lane().await;
    let (start, end) = (d(2025, 1, 1), d(2025, 12, 31));
    for basis in [Basis::Accrual, Basis::Cash] {
        let g = graphs::income_expense(&pool, start, end, basis).await.unwrap();
        let pl = statements::profit_and_loss_basis(&pool, start, end, None, basis).await.unwrap();
        assert_eq!((g.total_income_cents, g.total_expense_cents, g.net_income_cents), (pl.total_revenue_cents, pl.total_expenses_cents, pl.net_income_cents));
        assert_eq!(g.months.len(), 12);
        assert!(g.total_income_cents > 0 && g.total_expense_cents > 0, "the fixture has a year of activity");
        // The pies split the range's totals exactly ("Other" included).
        assert_eq!(sum(&g.income_slices), pl.total_revenue_cents);
        assert_eq!(sum(&g.expense_slices), pl.total_expenses_cents);
        assert!(g.expense_slices.len() <= graphs::TOP_N + 1);

        // Each month equals that month's column of the multi-period Profit and Loss.
        let columns = g.months.iter().map(|m| PeriodColumn { label: m.month.month.clone(), start_date: m.month.start_date, end_date: m.month.end_date }).collect();
        let periods = statements::profit_and_loss_periods(&pool, &PeriodReportInput { columns, total: false, compare: false, basis, class_filter: None }).await.unwrap();
        let net = &periods.lines.iter().find(|l| l.kind == LineKind::Net).unwrap().amounts_cents;
        let income = total_row(&periods.lines, "Total Income");
        for (i, m) in g.months.iter().enumerate() {
            assert_eq!(Some(m.net_income_cents), net[i], "{} net income", m.month.month);
            assert_eq!(m.income_cents - m.expense_cents, m.net_income_cents);
            // The fixture has no Other Income, so Total Income is the month's whole income.
            assert_eq!(Some(m.income_cents), income[i], "{} income", m.month.month);
        }
        if basis == Basis::Accrual {
            // Accrual months add up to the year; cash basis is checked month by month above.
            assert_eq!(g.months.iter().map(|m| m.income_cents).sum::<i64>(), pl.total_revenue_cents);
            assert_eq!(g.months.iter().map(|m| m.expense_cents).sum::<i64>(), pl.total_expenses_cents);
        }
    }
}

#[tokio::test]
async fn expense_slices_roll_subaccounts_into_their_parent() {
    let pool = harbor_lane().await;
    let rent = account(&pool, "6300").await;
    let sub = crate::accounts::create_account(
        &pool,
        crate::accounts::AccountInput { name: "Storage unit".into(), account_number: Some("6310".into()), account_type: "EXPENSE".into(), parent_id: Some(rent), description: None },
    )
    .await
    .unwrap();
    let checking = account(&pool, "1010").await;
    crate::ledger::post_journal_entry(
        &pool,
        crate::ledger::JournalEntry { id: None, date: d(2025, 6, 2), description: String::new(), lines: vec![crate::ledger::JournalLine::debit(sub.id, 12_345), crate::ledger::JournalLine::credit(checking, 12_345)], currency: None },
    )
    .await
    .unwrap();
    let g = graphs::income_expense(&pool, d(2025, 1, 1), d(2025, 12, 31), Basis::Accrual).await.unwrap();
    let pl = statements::profit_and_loss(&pool, d(2025, 1, 1), d(2025, 12, 31)).await.unwrap();
    let total_rent = pl.lines.iter().find(|l| l.kind == LineKind::Subtotal && l.account_id == Some(rent)).unwrap().amount_cents.unwrap();
    let slice = g.expense_slices.iter().find(|s| s.account_id == Some(rent)).expect("rent is one of the largest expenses");
    assert_eq!(slice.amount_cents, total_rent);
    assert!(g.expense_slices.iter().all(|s| s.account_id != Some(sub.id)), "a subaccount is part of its parent's slice");
    assert_eq!(sum(&g.expense_slices), pl.total_expenses_cents);
}

#[tokio::test]
async fn net_worth_matches_each_month_end_balance_sheet() {
    let pool = harbor_lane().await;
    let g = graphs::net_worth(&pool, d(2025, 1, 1), d(2025, 12, 15)).await.unwrap();
    assert_eq!(g.months.len(), 12);
    assert_eq!(g.months[1].as_of, d(2025, 2, 28));
    assert_eq!(g.months[11].as_of, d(2025, 12, 15), "the last point is the range's end date");
    for p in &g.months {
        let bs = statements::balance_sheet(&pool, p.as_of).await.unwrap();
        assert_eq!((p.assets_cents, p.liabilities_cents), (bs.total_assets_cents, bs.total_liabilities_cents), "{}", p.as_of);
        assert_eq!(p.net_worth_cents, bs.total_equity_cents, "net worth is the balance sheet's equity");
    }
}

#[tokio::test]
async fn aging_graphs_match_the_aging_summaries() {
    let pool = harbor_lane().await;
    for payable in [false, true] {
        for as_of in [d(2025, 3, 31), d(2025, 12, 31)] {
            let g = graphs::aging(&pool, payable, as_of).await.unwrap();
            let report = report_center::aging(&pool, payable, as_of).await.unwrap();
            assert_eq!(g.buckets.iter().map(|b| b.amount_cents).collect::<Vec<_>>(), report.totals_cents.to_vec());
            assert_eq!(g.buckets.iter().map(|b| b.amount_cents).sum::<i64>(), report.total_cents);
            assert_eq!(g.total_cents, report.total_cents);
            let owed: i64 = report.rows.iter().map(|r| r.total_cents).filter(|&c| c > 0).sum();
            assert_eq!(sum(&g.parties), owed, "payable={payable} {as_of}");
            assert!(g.parties.windows(2).all(|w| w[1].other || w[0].amount_cents >= w[1].amount_cents), "largest first");
        }
    }
    let ar = graphs::aging(&pool, false, d(2025, 12, 31)).await.unwrap();
    assert!(ar.total_cents > 0, "the fixture has open invoices at year end");
}

#[tokio::test]
async fn sales_graph_matches_the_sales_summaries() {
    let pool = harbor_lane().await;
    let (start, end) = (d(2025, 1, 1), d(2025, 12, 31));
    for by in [SalesBy::Customer, SalesBy::Item, SalesBy::Rep] {
        let g = graphs::sales(&pool, by, start, end).await.unwrap();
        let report = match by {
            SalesBy::Customer => report_center::by_party(&pool, false, start, end).await,
            SalesBy::Item => report_center::sales_by_product(&pool, start, end).await,
            SalesBy::Rep => report_center::sales_by_rep(&pool, start, end).await,
        }
        .unwrap();
        assert_eq!(g.total_cents, report.total_cents, "{by:?}");
        assert_eq!(g.months.iter().map(|m| m.amount_cents).sum::<i64>(), report.total_cents, "{by:?} months add up to the year");
        let positive: i64 = report.rows.iter().map(|r| r.amount_cents).filter(|&c| c > 0).sum();
        assert_eq!(sum(&g.top), positive, "{by:?}");
        let march = report_center::by_party(&pool, false, d(2025, 3, 1), d(2025, 3, 31)).await.unwrap();
        if by == SalesBy::Customer {
            assert_eq!(g.months[2].amount_cents, march.total_cents);
        }
    }
    let by_customer = graphs::sales(&pool, SalesBy::Customer, start, end).await.unwrap();
    assert!(by_customer.total_cents > 0);
}

#[tokio::test]
async fn budget_graph_matches_budget_vs_actual() {
    let pool = harbor_lane().await;
    let (services, rent, office) = (account(&pool, "4100").await, account(&pool, "6300").await, account(&pool, "6250").await);
    let budget = statements::create_budget(&pool, "2025 plan", d(2025, 1, 1)).await.unwrap();
    let mut amounts = Vec::new();
    for period in 1..=12 {
        let a = |account_id, amount_cents| BudgetAmount { account_id, period, amount_cents };
        amounts.extend([a(services, 1_500_000 + period * 10_000), a(rent, 240_000), a(office, 30_000)]);
    }
    statements::set_budget_amounts(&pool, budget.id, &amounts).await.unwrap();

    let g = graphs::budget_vs_actual(&pool, budget.id, 1, 12).await.unwrap();
    assert_eq!(g.months.len(), 12);
    let report = statements::budget_vs_actual(&pool, budget.id, 1, 12).await.unwrap();
    let net = &report.lines.iter().find(|l| l.kind == LineKind::Net).unwrap().amounts_cents;
    assert_eq!((Some(g.actual.net_income_cents), Some(g.budget.net_income_cents)), (net[0], net[1]));
    assert_eq!((Some(g.actual.income_cents), Some(g.budget.income_cents)), (total_row(&report.lines, "Total Income")[0], total_row(&report.lines, "Total Income")[1]));
    assert_eq!(g.budget.income_cents, (1..=12).map(|p| 1_500_000 + p * 10_000).sum::<i64>());
    assert_eq!(g.budget.expense_cents, 12 * 270_000);
    // The months add up to the year, and the actuals are the year's Profit and Loss.
    for pick in [|f: &graphs::BudgetFigures| f.income_cents, |f: &graphs::BudgetFigures| f.expense_cents, |f: &graphs::BudgetFigures| f.net_income_cents] {
        assert_eq!(g.months.iter().map(|m| pick(&m.actual)).sum::<i64>(), pick(&g.actual));
        assert_eq!(g.months.iter().map(|m| pick(&m.budget)).sum::<i64>(), pick(&g.budget));
    }
    let pl = statements::profit_and_loss(&pool, d(2025, 1, 1), d(2025, 12, 31)).await.unwrap();
    assert_eq!((g.actual.income_cents, g.actual.expense_cents, g.actual.net_income_cents), (pl.total_revenue_cents, pl.total_expenses_cents, pl.net_income_cents));
    assert_eq!(g.months[3].budget.income_cents, 1_540_000);
    assert_eq!((g.months[3].month.start_date, g.months[3].month.end_date), (d(2025, 4, 1), d(2025, 4, 30)));

    let q2 = graphs::budget_vs_actual(&pool, budget.id, 4, 6).await.unwrap();
    assert_eq!(q2.months.iter().map(|m| m.period).collect::<Vec<_>>(), [4, 5, 6]);
    assert!(graphs::budget_vs_actual(&pool, budget.id, 5, 4).await.unwrap_err().starts_with("RPT_BAD_RANGE"));
}
