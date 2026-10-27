//! Graph reports (Reports › Graphs), each built from the matching text report.

use super::prelude::*;
use crate::graphs::{self, AgingGraph, BudgetGraph, IncomeExpenseGraph, NetWorthGraph, SalesBy, SalesGraph};

handlers![
    get_income_expense_graph,
    get_net_worth_graph,
    get_aging_graph,
    get_sales_graph,
    get_budget_graph,
];

#[tauri::command]
pub async fn get_income_expense_graph(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate, basis: Option<Basis>) -> Result<IncomeExpenseGraph, String> {
    graphs::income_expense(&db.pool().await, start_date, end_date, basis.unwrap_or_default()).await
}

#[tauri::command]
pub async fn get_net_worth_graph(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate) -> Result<NetWorthGraph, String> {
    graphs::net_worth(&db.pool().await, start_date, end_date).await
}

#[tauri::command]
pub async fn get_aging_graph(db: State<'_, Db>, payable: bool, as_of: NaiveDate) -> Result<AgingGraph, String> {
    graphs::aging(&db.pool().await, payable, as_of).await
}

#[tauri::command]
pub async fn get_sales_graph(db: State<'_, Db>, by: SalesBy, start_date: NaiveDate, end_date: NaiveDate) -> Result<SalesGraph, String> {
    graphs::sales(&db.pool().await, by, start_date, end_date).await
}

#[tauri::command]
pub async fn get_budget_graph(db: State<'_, Db>, id: i64, from_period: i64, to_period: i64) -> Result<BudgetGraph, String> {
    graphs::budget_vs_actual(&db.pool().await, id, from_period, to_period).await
}
