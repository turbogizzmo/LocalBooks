//! Preferences batch 4: what Receive Payments, Enter Bills and Pay Bills ask the company defaults.

use super::prelude::*;
use crate::sales_prefs::{self, AppliedAmount, CreditUse};

handlers![
    get_payment_application,
    get_bill_due_date,
    list_bill_credits,
    pay_bills_with_credits,
];

/// How an amount received from a customer is spread over their open invoices.
#[tauri::command]
pub async fn get_payment_application(db: State<'_, Db>, customer_name: String, currency_code: Option<String>, amount_cents: i64) -> Result<Vec<AppliedAmount>, String> {
    sales_prefs::payment_application(&db.pool().await, &customer_name, currency_code.as_deref(), amount_cents).await
}

/// A new bill's due date: the vendor's terms, or the company's default days.
#[tauri::command]
pub async fn get_bill_due_date(db: State<'_, Db>, vendor_name: String, bill_date: NaiveDate) -> Result<NaiveDate, String> {
    sales_prefs::bill_due_date(&db.pool().await, &vendor_name, bill_date).await
}

/// The vendor credits Pay Bills uses on the chosen bills (none unless the company applies them).
#[tauri::command]
pub async fn list_bill_credits(db: State<'_, Db>, bill_ids: Vec<i64>) -> Result<Vec<CreditUse>, String> {
    sales_prefs::bill_credits(&db.pool().await, &bill_ids).await
}

/// Pays bills after using vendor credits on them; all of it posts, or none.
#[tauri::command]
pub async fn pay_bills_with_credits(app: AppHandle, db: State<'_, Db>, inputs: Vec<crate::expenses::BillPaymentInput>, credits: Vec<ApplyCreditInput>) -> Result<usize, String> {
    let count = sales_prefs::pay_bills_with_credits(&db.pool().await, inputs, credits).await?;
    notify(&app);
    Ok(count)
}
