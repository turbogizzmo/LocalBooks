//! Sales tax adjustments (T70).

use super::prelude::*;
use crate::sales_tax::adjustments::{self, SalesTaxAdjustmentInput, SalesTaxAdjustmentView};

handlers![
    adjust_sales_tax,
    list_sales_tax_adjustments,
];

#[tauri::command]
pub async fn adjust_sales_tax(app: AppHandle, db: State<'_, Db>, input: SalesTaxAdjustmentInput, confirmed: Option<Vec<String>>) -> Result<i64, String> {
    let pool = db.pool().await;
    // The date and retained-earnings warnings other forms get.
    guards::check(&pool, guards::Posting { date: Some(input.adjustment_date), account_ids: vec![input.account_id], ..Default::default() }, confirmed.as_deref()).await?;
    let id = adjustments::adjust_sales_tax(&pool, &input).await?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub async fn list_sales_tax_adjustments(db: State<'_, Db>) -> Result<Vec<SalesTaxAdjustmentView>, String> {
    adjustments::list_sales_tax_adjustments(&db.pool().await).await
}
