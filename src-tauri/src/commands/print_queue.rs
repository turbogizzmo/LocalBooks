//! Print later: the check and invoice print queues behind File › Print Forms.

use super::prelude::*;
use crate::print_queue::{self, Form, PrintedCheck};

handlers![
    list_checks_to_print,
    list_invoices_to_print,
    set_to_be_printed,
    mark_checks_printed,
    mark_invoices_printed,
    set_print_later_default,
];

#[tauri::command]
pub async fn list_checks_to_print(db: State<'_, Db>, account_id: Option<i64>) -> Result<Vec<ExpenseView>, String> {
    print_queue::queued_checks(&db.pool().await, account_id).await
}

#[tauri::command]
pub async fn list_invoices_to_print(db: State<'_, Db>) -> Result<Vec<InvoiceSummary>, String> {
    print_queue::queued_invoices(&db.pool().await).await
}

/// `kind` is `CHECK` or `INVOICE`.
#[tauri::command]
pub async fn set_to_be_printed(app: AppHandle, db: State<'_, Db>, kind: String, id: i64, queued: bool) -> Result<(), String> {
    print_queue::set_to_be_printed(&db.pool().await, Form::parse(&kind)?, id, queued).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn mark_checks_printed(app: AppHandle, db: State<'_, Db>, printed: Vec<PrintedCheck>) -> Result<usize, String> {
    let count = print_queue::mark_checks_printed(&db.pool().await, printed).await?;
    notify(&app);
    Ok(count)
}

#[tauri::command]
pub async fn mark_invoices_printed(app: AppHandle, db: State<'_, Db>, invoice_ids: Vec<i64>) -> Result<usize, String> {
    let count = print_queue::mark_invoices_printed(&db.pool().await, invoice_ids).await?;
    notify(&app);
    Ok(count)
}

/// Remembers a form's "Print later" choice (`kind` is `CHECK` or `INVOICE`).
#[tauri::command]
pub async fn set_print_later_default(app: AppHandle, db: State<'_, Db>, kind: String, on: bool) -> Result<(), String> {
    print_queue::remember_default(&db.pool().await, Form::parse(&kind)?, on).await?;
    notify(&app);
    Ok(())
}
