//! Customer credit memos and vendor credits.

use super::prelude::*;

handlers![
    list_credit_memos,
    create_credit_memo,
    apply_customer_credit,
    apply_open_credits_to_invoice,
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
pub async fn create_credit_memo(app: AppHandle, db: State<'_, Db>, input: CreditMemoInput, confirmed: Option<Vec<String>>) -> Result<CreditMemoSummary, String> {
    let pool = db.pool().await;
    // The date and retained-earnings warnings every other form gets.
    let mut accounts: Vec<i64> = input.lines.iter().map(|l| l.income_account_id).collect();
    accounts.extend(input.refund_account_id);
    guards::check(&pool, guards::Posting { date: Some(input.credit_date), account_ids: accounts, ..Default::default() }, confirmed.as_deref()).await?;
    let memo = credits::create_credit_memo(&pool, input).await?;
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
pub async fn apply_open_credits_to_invoice(app: AppHandle, db: State<'_, Db>, invoice_id: i64, applied_date: NaiveDate) -> Result<AppliedCredits, String> {
    let used = credits::apply_open_credits_to_invoice(&db.pool().await, invoice_id, applied_date).await?;
    notify(&app);
    Ok(used)
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
pub async fn refund_customer_credit(app: AppHandle, db: State<'_, Db>, input: RefundCreditInput, confirmed: Option<Vec<String>>) -> Result<CreditMemoSummary, String> {
    let pool = db.pool().await;
    guards::check(&pool, guards::Posting { date: Some(input.refund_date), account_ids: vec![input.account_id], ..Default::default() }, confirmed.as_deref()).await?;
    let memo = credits::refund_customer_credit(&pool, input).await?;
    notify(&app);
    Ok(memo)
}

#[tauri::command]
pub async fn list_vendor_credits(db: State<'_, Db>) -> Result<Vec<VendorCreditSummary>, String> {
    credits::list_vendor_credits(&db.pool().await).await
}

#[tauri::command]
pub async fn create_vendor_credit(app: AppHandle, db: State<'_, Db>, input: VendorCreditInput, confirmed: Option<Vec<String>>) -> Result<VendorCreditSummary, String> {
    let pool = db.pool().await;
    let accounts = input.lines.iter().map(|l| l.account_id).collect();
    guards::check(&pool, guards::Posting { date: Some(input.credit_date), account_ids: accounts, ..Default::default() }, confirmed.as_deref()).await?;
    let credit = credits::create_vendor_credit(&pool, input).await?;
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
pub async fn refund_vendor_credit(app: AppHandle, db: State<'_, Db>, input: RefundCreditInput, confirmed: Option<Vec<String>>) -> Result<VendorCreditSummary, String> {
    let pool = db.pool().await;
    guards::check(&pool, guards::Posting { date: Some(input.refund_date), account_ids: vec![input.account_id], ..Default::default() }, confirmed.as_deref()).await?;
    let credit = credits::refund_vendor_credit(&pool, input).await?;
    notify(&app);
    Ok(credit)
}
