//! Budgets.

use super::prelude::*;

handlers![
    list_budgets,
    create_budget,
    delete_budget,
    get_budget_amounts,
    set_budget_amounts,
    get_budget_vs_actual,
    get_balance_sheet_budget_vs_actual,
];

#[tauri::command]
pub async fn list_budgets(db: State<'_, Db>) -> Result<Vec<BudgetView>, String> {
    statements::list_budgets(&db.pool().await).await
}

#[tauri::command]
pub async fn create_budget(app: AppHandle, db: State<'_, Db>, name: String, in_year: NaiveDate) -> Result<BudgetView, String> {
    let budget = statements::create_budget(&db.pool().await, &name, in_year).await?;
    notify(&app);
    Ok(budget)
}

#[tauri::command]
pub async fn delete_budget(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    statements::delete_budget(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn get_budget_amounts(db: State<'_, Db>, id: i64) -> Result<Vec<BudgetAmount>, String> {
    statements::budget_amounts(&db.pool().await, id).await
}

#[tauri::command]
pub async fn set_budget_amounts(app: AppHandle, db: State<'_, Db>, id: i64, amounts: Vec<BudgetAmount>) -> Result<(), String> {
    statements::set_budget_amounts(&db.pool().await, id, &amounts).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn get_budget_vs_actual(db: State<'_, Db>, id: i64, from_period: i64, to_period: i64) -> Result<ProfitAndLossByClass, String> {
    statements::budget_vs_actual(&db.pool().await, id, from_period, to_period).await
}

#[tauri::command]
pub async fn get_balance_sheet_budget_vs_actual(db: State<'_, Db>, id: i64, period: i64) -> Result<ProfitAndLossByClass, String> {
    statements::balance_sheet_budget_vs_actual(&db.pool().await, id, period).await
}
