//! Customer credit memos and vendor credits.

use super::prelude::*;

handlers![
    list_credit_memos,
    create_credit_memo,
    apply_customer_credit,
    list_customer_credit_applications,
    remove_customer_credit,
    refund_customer_credit,
    list_vendor_credits,
    create_vendor_credit,
    apply_vendor_credit,
    list_vendor_credit_applications,
    remove_vendor_credit,
    refund_vendor_credit,
];

#[tauri::command]
pub async fn list_credit_memos(db: State<'_, Db>) -> Result<Vec<CreditMemoSummary>, String> {
    credits::list_credit_memos(&db.pool().await).await
}

#[tauri::command]
pub async fn create_credit_memo(app: AppHandle, db: State<'_, Db>, input: CreditMemoInput) -> Result<CreditMemoSummary, String> {
    let memo = credits::create_credit_memo(&db.pool().await, input).await?;
    notify(&app);
    Ok(memo)
}

#[tauri::command]
pub async fn apply_customer_credit(app: AppHandle, db: State<'_, Db>, input: ApplyCreditInput) -> Result<CreditMemoSummary, String> {
    let memo = credits::apply_customer_credit(&db.pool().await, input).await?;
    notify(&app);
    Ok(memo)
}

#[tauri::command]
pub async fn list_customer_credit_applications(db: State<'_, Db>, credit_id: i64) -> Result<Vec<CreditApplicationView>, String> {
    credits::list_customer_credit_applications(&db.pool().await, credit_id).await
}

#[tauri::command]
pub async fn remove_customer_credit(app: AppHandle, db: State<'_, Db>, application_id: i64) -> Result<(), String> {
    credits::remove_customer_credit(&db.pool().await, application_id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn refund_customer_credit(app: AppHandle, db: State<'_, Db>, input: RefundCreditInput) -> Result<CreditMemoSummary, String> {
    let memo = credits::refund_customer_credit(&db.pool().await, input).await?;
    notify(&app);
    Ok(memo)
}

#[tauri::command]
pub async fn list_vendor_credits(db: State<'_, Db>) -> Result<Vec<VendorCreditSummary>, String> {
    credits::list_vendor_credits(&db.pool().await).await
}

#[tauri::command]
pub async fn create_vendor_credit(app: AppHandle, db: State<'_, Db>, input: VendorCreditInput) -> Result<VendorCreditSummary, String> {
    let credit = credits::create_vendor_credit(&db.pool().await, input).await?;
    notify(&app);
    Ok(credit)
}

#[tauri::command]
pub async fn apply_vendor_credit(app: AppHandle, db: State<'_, Db>, input: ApplyCreditInput) -> Result<VendorCreditSummary, String> {
    let credit = credits::apply_vendor_credit(&db.pool().await, input).await?;
    notify(&app);
    Ok(credit)
}

#[tauri::command]
pub async fn list_vendor_credit_applications(db: State<'_, Db>, credit_id: i64) -> Result<Vec<CreditApplicationView>, String> {
    credits::list_vendor_credit_applications(&db.pool().await, credit_id).await
}

#[tauri::command]
pub async fn remove_vendor_credit(app: AppHandle, db: State<'_, Db>, application_id: i64) -> Result<(), String> {
    credits::remove_vendor_credit(&db.pool().await, application_id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn refund_vendor_credit(app: AppHandle, db: State<'_, Db>, input: RefundCreditInput) -> Result<VendorCreditSummary, String> {
    let credit = credits::refund_vendor_credit(&db.pool().await, input).await?;
    notify(&app);
    Ok(credit)
}
