//! Preferences batch 5 (tax and time): sales tax filing periods, timesheet weeks, billable
//! expenses, recalling a vendor's last transaction, 1099 boxes and vendor account numbers.

use super::prelude::*;
use crate::billable_costs::{self, BillCostsInput, UnbilledCost};
use crate::edits::RecalledTransaction;
use crate::invoices::InvoiceSummary;
use crate::sales_tax::FilingPeriods;
use crate::timesheet::TimesheetWeek;
use crate::workflows::Box1099;

handlers![
    get_sales_tax_periods,
    get_timesheet_week,
    list_unbilled_costs,
    list_unbilled_time,
    set_reimbursement_account,
    get_billing_defaults,
    bill_costs,
    get_recalled_vendor_transaction,
    list_1099_boxes,
    get_vendor_account_number,
    set_vendor_account_number,
];

/// The filing period `as_of` falls in, the last one that ended, and its return's reminder date.
#[tauri::command]
pub async fn get_sales_tax_periods(db: State<'_, Db>, as_of: NaiveDate) -> Result<FilingPeriods, String> {
    sales_tax::company_filing_periods(&db.pool().await, as_of).await
}

#[tauri::command]
pub async fn get_timesheet_week(db: State<'_, Db>, date: NaiveDate) -> Result<TimesheetWeek, String> {
    crate::timesheet::timesheet_week(&db.pool().await, date).await
}

#[tauri::command]
pub async fn list_unbilled_costs(db: State<'_, Db>, customer_id: Option<i64>) -> Result<Vec<UnbilledCost>, String> {
    billable_costs::list_unbilled(&db.pool().await, customer_id).await
}

#[tauri::command]
pub async fn get_billing_defaults(db: State<'_, Db>) -> Result<billable_costs::Defaults, String> {
    billable_costs::defaults(&db.pool().await).await
}

#[tauri::command]
pub async fn bill_costs(app: AppHandle, db: State<'_, Db>, input: BillCostsInput, confirmed: Option<Vec<String>>) -> Result<InvoiceSummary, String> {
    let pool = db.pool().await;
    guards::check(&pool, guards::dated(input.issue_date), confirmed.as_deref()).await?;
    let invoice = billable_costs::bill_costs(&pool, input).await?;
    notify(&app);
    Ok(invoice)
}

/// The vendor's latest bill (`kind` = `bill`), check or expense (`expense`), or either.
#[tauri::command]
pub async fn get_recalled_vendor_transaction(db: State<'_, Db>, vendor_name: String, kind: Option<String>) -> Result<Option<RecalledTransaction>, String> {
    edits::last_for_vendor(&db.pool().await, &vendor_name, kind.as_deref()).await
}

#[tauri::command]
pub async fn list_1099_boxes() -> Result<Vec<Box1099>, String> {
    Ok(workflows::BOXES_1099.to_vec())
}

#[tauri::command]
pub async fn get_vendor_account_number(db: State<'_, Db>, vendor_id: i64) -> Result<Option<String>, String> {
    crate::expenses::vendor_account_number(&db.pool().await, vendor_id).await
}

#[tauri::command]
pub async fn set_vendor_account_number(app: AppHandle, db: State<'_, Db>, vendor_id: i64, account_number: Option<String>) -> Result<Option<String>, String> {
    let stored = crate::expenses::set_vendor_account_number(&db.pool().await, vendor_id, account_number.as_deref()).await?;
    notify(&app);
    Ok(stored)
}

/// Billable time not yet invoiced, for Sales › Bill Costs.
#[tauri::command]
pub async fn list_unbilled_time(db: State<'_, Db>, customer_id: Option<i64>) -> Result<Vec<billable_costs::UnbilledTime>, String> {
    billable_costs::list_unbilled_time(&db.pool().await, customer_id).await
}

/// The income account an expense account's reimbursements go to (None: the one in Preferences).
#[tauri::command]
pub async fn set_reimbursement_account(app: AppHandle, db: State<'_, Db>, expense_account_id: i64, income_account_id: Option<i64>) -> Result<(), String> {
    billable_costs::set_reimbursement_account(&db.pool().await, expense_account_id, income_account_id).await?;
    notify(&app);
    Ok(())
}
