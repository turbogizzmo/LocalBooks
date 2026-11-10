//! Item receipts: Receive Items and Enter Bill for Received Items (T12).

use super::prelude::*;
use crate::item_receipts::{self as receipts, OrderLineReceiving, ReceiptBillInput, ReceiptInput, ReceiptLineView, ReceiptView};

handlers![
    receive_items,
    list_item_receipts,
    list_item_receipt_lines,
    list_order_receiving,
    bill_received_items,
    get_receipt_bill_form_lines,
];

/// The purchase order's discounts and charges the receipt's bill will take (T21).
#[tauri::command]
pub async fn get_receipt_bill_form_lines(db: State<'_, Db>, item_receipt_id: i64) -> Result<Vec<crate::purchase_lines::PurchaseFormLineInput>, String> {
    receipts::receipt_bill_form_lines(&db.pool().await, item_receipt_id).await
}

/// Records goods received from a vendor before the bill; posts to Items Received Not Billed.
#[tauri::command]
pub async fn receive_items(app: AppHandle, db: State<'_, Db>, input: ReceiptInput) -> Result<ReceiptView, String> {
    let receipt = receipts::receive_items(&db.pool().await, input).await?;
    notify(&app);
    Ok(receipt)
}

/// Receipts, newest first; `open_only` keeps those still waiting for their bill.
#[tauri::command]
pub async fn list_item_receipts(db: State<'_, Db>, open_only: bool) -> Result<Vec<ReceiptView>, String> {
    receipts::list_receipts(&db.pool().await, open_only).await
}

#[tauri::command]
pub async fn list_item_receipt_lines(db: State<'_, Db>, item_receipt_id: i64) -> Result<Vec<ReceiptLineView>, String> {
    receipts::receipt_lines(&db.pool().await, item_receipt_id).await
}

/// A purchase order's lines with what has been received of each.
#[tauri::command]
pub async fn list_order_receiving(db: State<'_, Db>, purchase_order_id: i64) -> Result<Vec<OrderLineReceiving>, String> {
    receipts::order_receiving(&db.pool().await, purchase_order_id).await
}

/// Enter Bill for Received Items: posts the vendor's bill for a receipt; returns the bill id.
#[tauri::command]
pub async fn bill_received_items(app: AppHandle, db: State<'_, Db>, input: ReceiptBillInput) -> Result<i64, String> {
    let bill = receipts::bill_received_items(&db.pool().await, input).await?;
    notify(&app);
    Ok(bill)
}
