//! Applying payments and vendor credits automatically (Preferences › Payments and Bills).

use super::prelude::*;
use crate::auto_apply::{self, Allocation, PayBillsResult};
use crate::expenses::BillPaymentInput;

handlers![
    auto_apply_customer_payment,
    pay_bills_applying_credits,
];

/// How an amount received applies to the customer's open invoices; empty when the preference is off.
#[tauri::command]
pub async fn auto_apply_customer_payment(db: State<'_, Db>, customer_name: String, currency_code: Option<String>, amount_cents: i64, on_request: Option<bool>) -> Result<Vec<Allocation>, String> {
    auto_apply::customer_payment(&db.pool().await, &customer_name, currency_code.as_deref(), amount_cents, on_request.unwrap_or(false)).await
}

/// Pay Bills: uses the vendors' open credits first when the preference is on.
#[tauri::command]
pub async fn pay_bills_applying_credits(app: AppHandle, db: State<'_, Db>, inputs: Vec<BillPaymentInput>, confirmed: Option<Vec<String>>) -> Result<PayBillsResult, String> {
    let pool = db.pool().await;
    if let Some(first) = inputs.first() {
        guards::check(&pool, guards::dated(first.payment_date), confirmed.as_deref()).await?;
    }
    let result = auto_apply::pay_bills(&pool, inputs).await?;
    notify(&app);
    Ok(result)
}
