//! Receive Payments with money left over: overpayments, prepayments and payments from a customer
//! with no open invoice, kept as a credit, refunded or written off (`docs/plans/overpayments.md`).

use super::prelude::*;
use crate::invoices::{ReceivePaymentInput, ReceivedPayment};

handlers![receive_customer_payment, write_off_customer_credit, get_customer_credit];

#[tauri::command]
pub async fn receive_customer_payment(app: AppHandle, db: State<'_, Db>, input: ReceivePaymentInput) -> Result<ReceivedPayment, String> {
    let received = crate::invoices::receive_payment(&db.pool().await, input).await?;
    notify(&app);
    Ok(received)
}

#[tauri::command]
pub async fn write_off_customer_credit(app: AppHandle, db: State<'_, Db>, input: RefundCreditInput) -> Result<CreditMemoSummary, String> {
    let memo = crate::credits::write_off_customer_credit(&db.pool().await, input).await?;
    notify(&app);
    Ok(memo)
}

#[tauri::command]
pub async fn get_customer_credit(db: State<'_, Db>, customer_id: i64) -> Result<crate::credits::CustomerCredit, String> {
    crate::credits::customer_credit(&db.pool().await, customer_id).await
}
