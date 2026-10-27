//! Vendors, bills, bill payments and expenses.

use super::prelude::*;

handlers![
    list_vendors,
    create_vendor,
    list_bills,
    post_vendor_bill,
    pay_vendor_bill,
    pay_vendor_bills,
    list_bill_payments,
    next_check_number,
    post_expense,
    list_expenses,
];

#[tauri::command]
pub async fn list_vendors(db: State<'_, Db>) -> Result<Vec<PartyView>, String> {
    expenses::list_vendors(&db.pool().await).await
}

#[tauri::command]
pub async fn create_vendor(
    app: AppHandle,
    db: State<'_, Db>,
    name: String,
    email: Option<String>,
) -> Result<PartyView, String> {
    let vendor = expenses::create_vendor(&db.pool().await, &name, email.as_deref()).await?;
    notify(&app);
    Ok(vendor)
}

#[tauri::command]
pub async fn list_bills(db: State<'_, Db>) -> Result<Vec<BillSummary>, String> {
    expenses::list_bills(&db.pool().await).await
}

#[tauri::command]
pub async fn post_vendor_bill(app: AppHandle, db: State<'_, Db>, input: BillInput) -> Result<BillSummary, String> {
    let bill = expenses::post_vendor_bill(&db.pool().await, input).await?;
    notify(&app);
    Ok(bill)
}

#[tauri::command]
pub async fn pay_vendor_bill(
    app: AppHandle,
    db: State<'_, Db>,
    input: BillPaymentInput,
) -> Result<BillSummary, String> {
    let bill = expenses::pay_vendor_bill(&db.pool().await, input).await?;
    notify(&app);
    Ok(bill)
}

#[tauri::command]
pub async fn pay_vendor_bills(app: AppHandle, db: State<'_, Db>, inputs: Vec<BillPaymentInput>) -> Result<usize, String> {
    let count = expenses::pay_vendor_bills(&db.pool().await, inputs).await?;
    notify(&app);
    Ok(count)
}

#[tauri::command]
pub async fn list_bill_payments(db: State<'_, Db>, bill_id: i64) -> Result<Vec<BillPaymentView>, String> {
    expenses::list_bill_payments(&db.pool().await, bill_id).await
}

#[tauri::command]
pub async fn next_check_number(db: State<'_, Db>, account_id: i64) -> Result<String, String> {
    expenses::next_check_number(&db.pool().await, account_id).await
}

#[tauri::command]
pub async fn post_expense(app: AppHandle, db: State<'_, Db>, input: ExpenseInput) -> Result<ExpenseView, String> {
    let expense = expenses::post_expense(&db.pool().await, input).await?;
    notify(&app);
    Ok(expense)
}

#[tauri::command]
pub async fn list_expenses(db: State<'_, Db>) -> Result<Vec<ExpenseView>, String> {
    expenses::list_expenses(&db.pool().await).await
}
