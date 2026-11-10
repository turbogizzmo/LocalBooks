//! Discount and charge lines on the purchase forms (T21).

use super::prelude::*;
use crate::purchase_lines::{self, PurchaseDoc, PurchaseFormLineView};

handlers![
    get_purchase_form_lines,
];

/// A bill's, expense's, vendor credit's or purchase order's discount and charge lines.
#[tauri::command]
pub async fn get_purchase_form_lines(db: State<'_, Db>, doc: PurchaseDoc, id: i64) -> Result<Vec<PurchaseFormLineView>, String> {
    purchase_lines::lines_of(&db.pool().await, doc, id).await
}
