//! The sales form header: a customer's Bill To / Ship To defaults and the credit-limit check.

use super::prelude::*;
use crate::sales_header::{self, CreditLimitWarning, CustomerSalesDefaults};

handlers![
    customer_sales_defaults,
    check_credit_limit,
];

#[tauri::command]
pub async fn customer_sales_defaults(db: State<'_, Db>, customer_id: i64) -> Result<CustomerSalesDefaults, String> {
    sales_header::customer_sales_defaults(&db.pool().await, customer_id).await
}

/// `amount_cents` in the home currency; `replacing_invoice_id` when an edit replaces an invoice.
#[tauri::command]
pub async fn check_credit_limit(
    db: State<'_, Db>,
    customer_name: String,
    amount_cents: i64,
    replacing_invoice_id: Option<i64>,
) -> Result<Option<CreditLimitWarning>, String> {
    sales_header::credit_check(&db.pool().await, &customer_name, amount_cents, replacing_invoice_id).await
}
