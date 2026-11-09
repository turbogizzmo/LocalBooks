//! Customers › Create Batch Invoices and payment reminders by customer group.

use super::prelude::*;
use crate::batch_invoices::{self, BatchInput, BatchResult};
use crate::payment_reminders::{self, OverdueInvoice};

handlers![
    create_batch_invoices,
    group_overdue_invoices,
    send_payment_reminders,
];

/// One invoice per customer (a group's, or picked), all posted in one write.
#[tauri::command]
pub async fn create_batch_invoices(app: AppHandle, db: State<'_, Db>, input: BatchInput) -> Result<BatchResult, String> {
    let result = batch_invoices::create(&db.pool().await, input).await?;
    notify(&app);
    Ok(result)
}

/// A group's invoices overdue enough for a payment reminder, as of today.
#[tauri::command]
pub async fn group_overdue_invoices(db: State<'_, Db>, group_id: i64) -> Result<Vec<OverdueInvoice>, String> {
    let pool = db.pool().await;
    let group = crate::customer_groups::get(&pool, group_id).await?;
    payment_reminders::overdue(&pool, &group, Local::now().date_naive()).await
}

/// Queues the chosen overdue invoices for Email later and marks the group's reminder sent today.
#[tauri::command]
pub async fn send_payment_reminders(app: AppHandle, db: State<'_, Db>, group_id: i64, invoice_ids: Vec<i64>) -> Result<usize, String> {
    let sent = payment_reminders::send(&db.pool().await, group_id, &invoice_ids, Local::now().date_naive()).await?;
    notify(&app);
    Ok(sent)
}
