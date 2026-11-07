//! Preferences batch 4: what Receive Payments, Enter Bills and Pay Bills ask the company defaults.

use super::prelude::*;
use crate::sales_prefs::{self, AppliedAmount, DiscountUse, PayBillsPlan, PaymentDiscounts};

handlers![
    get_payment_application,
    get_payment_discounts,
    get_bill_due_date,
    get_pay_bills_plan,
    pay_bills_with_credits,
    set_menu_view_enabled,
];

/// How an amount received from a customer is spread over their open invoices.
#[tauri::command]
pub async fn get_payment_application(
    db: State<'_, Db>,
    customer_name: String,
    currency_code: Option<String>,
    amount_cents: i64,
    mode: Option<String>,
    payment_date: Option<NaiveDate>,
) -> Result<Vec<AppliedAmount>, String> {
    sales_prefs::payment_application(&db.pool().await, &customer_name, currency_code.as_deref(), amount_cents, mode.as_deref(), payment_date).await
}

/// The early-payment discounts the listed invoices still offer on the payment date.
#[tauri::command]
pub async fn get_payment_discounts(db: State<'_, Db>, invoice_ids: Vec<i64>, payment_date: NaiveDate) -> Result<PaymentDiscounts, String> {
    sales_prefs::payment_discounts(&db.pool().await, &invoice_ids, payment_date).await
}

/// A new bill's due date: the vendor's terms, or the company's default days.
#[tauri::command]
pub async fn get_bill_due_date(db: State<'_, Db>, vendor_name: String, bill_date: NaiveDate) -> Result<NaiveDate, String> {
    sales_prefs::bill_due_date(&db.pool().await, &vendor_name, bill_date).await
}

/// The discounts and vendor credits Pay Bills takes off the chosen bills (as far as the company
/// takes them automatically).
#[tauri::command]
pub async fn get_pay_bills_plan(db: State<'_, Db>, bill_ids: Vec<i64>, payment_date: NaiveDate) -> Result<PayBillsPlan, String> {
    sales_prefs::pay_bills_plan(&db.pool().await, &bill_ids, payment_date).await
}

/// Pays bills after taking discounts and using vendor credits on them; all of it posts, or none.
#[tauri::command]
pub async fn pay_bills_with_credits(
    app: AppHandle,
    db: State<'_, Db>,
    inputs: Vec<crate::expenses::BillPaymentInput>,
    credits: Vec<ApplyCreditInput>,
    discounts: Vec<DiscountUse>,
    payment_date: NaiveDate,
) -> Result<usize, String> {
    let count = sales_prefs::pay_bills_with_credits(&db.pool().await, inputs, credits, discounts, payment_date).await?;
    notify(&app);
    Ok(count)
}

/// Greys out (or brings back) the menu items for a feature the company turned off.
#[tauri::command]
pub fn set_menu_view_enabled(app: AppHandle, view: String, enabled: bool) -> Result<(), String> {
    crate::app_menu::set_view_enabled(&app, &view, enabled).map_err(|e| format!("MENU: {e}"))
}
