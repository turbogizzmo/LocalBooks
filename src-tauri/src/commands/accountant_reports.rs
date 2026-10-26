//! Accountant & Taxes reports: previous reconciliations, reconciliation discrepancy, income tax
//! summary and detail (with each account's tax line), collections and open purchase orders.

use super::prelude::*;
use crate::accountant_reports::{self, AccountTaxLine, AccountantReport, CompletedReconciliation, TaxLine};

handlers![
    list_completed_reconciliations,
    get_previous_reconciliation_report,
    get_reconciliation_discrepancy_report,
    list_tax_lines,
    list_account_tax_lines,
    set_account_tax_line,
    get_income_tax_report,
    get_collections_report,
    get_open_purchase_orders_report,
];

#[tauri::command]
pub async fn list_completed_reconciliations(db: State<'_, Db>) -> Result<Vec<CompletedReconciliation>, String> {
    accountant_reports::list_completed(&db.pool().await).await
}

/// A completed reconciliation reprinted: every transaction (`detail`) or the counts and totals.
#[tauri::command]
pub async fn get_previous_reconciliation_report(db: State<'_, Db>, reconciliation_id: i64, detail: bool) -> Result<AccountantReport, String> {
    accountant_reports::previous_reconciliation(&db.pool().await, reconciliation_id, detail).await
}

#[tauri::command]
pub async fn get_reconciliation_discrepancy_report(db: State<'_, Db>, account_id: Option<i64>) -> Result<AccountantReport, String> {
    accountant_reports::discrepancy(&db.pool().await, account_id, Local::now().date_naive()).await
}

#[tauri::command]
pub async fn list_tax_lines() -> Result<Vec<TaxLine>, String> {
    Ok(accountant_reports::tax_lines())
}

#[tauri::command]
pub async fn list_account_tax_lines(db: State<'_, Db>) -> Result<Vec<AccountTaxLine>, String> {
    accountant_reports::account_tax_lines(&db.pool().await).await
}

#[tauri::command]
pub async fn set_account_tax_line(app: AppHandle, db: State<'_, Db>, account_id: i64, tax_line: Option<String>) -> Result<(), String> {
    accountant_reports::set_account_tax_line(&db.pool().await, account_id, tax_line.as_deref()).await?;
    notify(&app);
    Ok(())
}

/// Income Tax Summary or Detail for the company's tax form (Settings), or `form` when given.
#[tauri::command]
pub async fn get_income_tax_report(
    db: State<'_, Db>,
    start_date: NaiveDate,
    end_date: NaiveDate,
    detail: bool,
    form: Option<String>,
    tax_line: Option<String>,
) -> Result<AccountantReport, String> {
    let pool = db.pool().await;
    let form = match form {
        Some(f) => f,
        None => preferences::get(&pool, "tax_form").await?,
    };
    accountant_reports::income_tax(&pool, &form, start_date, end_date, detail, tax_line.as_deref()).await
}

#[tauri::command]
pub async fn get_collections_report(db: State<'_, Db>, as_of: NaiveDate) -> Result<AccountantReport, String> {
    accountant_reports::collections(&db.pool().await, as_of).await
}

#[tauri::command]
pub async fn get_open_purchase_orders_report(db: State<'_, Db>, as_of: NaiveDate) -> Result<AccountantReport, String> {
    accountant_reports::open_purchase_orders(&db.pool().await, as_of).await
}
