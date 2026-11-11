//! Approvals for bills and purchase orders.

use super::prelude::*;
use crate::approvals::{self, ApprovalRequest, ApprovalRule, Kind, RuleInput};

handlers![
    list_approval_rules,
    add_approval_rule,
    delete_approval_rule,
    submit_for_approval,
    submit_purchase_order_bill_for_approval,
    list_approval_requests,
    approve_request,
    reject_request,
];

/// Who is acting: the signed-in user's name, or "this computer" in a file without users.
async fn who(db: &Db) -> Result<String, String> {
    let (_, session) = signed_in(db).await?;
    Ok(session.as_ref().map_or_else(|| "this computer".to_string(), |s| s.user.display_name.clone()))
}

#[tauri::command]
pub async fn list_approval_rules(db: State<'_, Db>) -> Result<Vec<ApprovalRule>, String> {
    approvals::list_rules(&db.pool().await).await
}

#[tauri::command]
pub async fn add_approval_rule(app: AppHandle, db: State<'_, Db>, input: RuleInput) -> Result<ApprovalRule, String> {
    let rule = approvals::add_rule(&db.pool().await, &input).await?;
    notify(&app);
    Ok(rule)
}

#[tauri::command]
pub async fn delete_approval_rule(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    approvals::delete_rule(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

/// Holds a bill or purchase order (the same input its form posts) for approval.
#[tauri::command]
pub async fn submit_for_approval(app: AppHandle, db: State<'_, Db>, kind: Kind, input: serde_json::Value) -> Result<ApprovalRequest, String> {
    let by = who(&db).await?;
    let request = approvals::submit(&db.pool().await, kind, &input, &by).await?;
    notify(&app);
    Ok(request)
}

/// Create Bill on a purchase order, when an approval rule holds the bill: builds the bill as
/// Create Bill would and holds it. Approving it posts the bill and marks the order billed.
#[tauri::command]
pub async fn submit_purchase_order_bill_for_approval(
    app: AppHandle,
    db: State<'_, Db>,
    id: i64,
    bill_date: NaiveDate,
    due_date: NaiveDate,
    bill_number: Option<String>,
) -> Result<ApprovalRequest, String> {
    let by = who(&db).await?;
    let pool = db.pool().await;
    let bill = workflows::purchase_order_bill(&pool, id, bill_date, due_date, bill_number).await?;
    let request = approvals::submit_purchase_order_bill(&pool, id, &bill, &by).await?;
    notify(&app);
    Ok(request)
}

#[tauri::command]
pub async fn list_approval_requests(db: State<'_, Db>, status: Option<String>) -> Result<Vec<ApprovalRequest>, String> {
    approvals::list(&db.pool().await, status.as_deref()).await
}

#[tauri::command]
pub async fn approve_request(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<ApprovalRequest, String> {
    let by = who(&db).await?;
    let request = approvals::approve(&db.pool().await, id, &by).await?;
    notify(&app);
    Ok(request)
}

#[tauri::command]
pub async fn reject_request(app: AppHandle, db: State<'_, Db>, id: i64, note: String) -> Result<ApprovalRequest, String> {
    let by = who(&db).await?;
    let request = approvals::reject(&db.pool().await, id, &by, &note).await?;
    notify(&app);
    Ok(request)
}
