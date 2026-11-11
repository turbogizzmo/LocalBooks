//! A customer credit's refunds and write-offs (T73).

use super::prelude::*;
use crate::credits::refund_list::{self, CreditRefundView};

handlers![
    list_customer_credit_refunds,
];

#[tauri::command]
pub async fn list_customer_credit_refunds(db: State<'_, Db>, credit_id: i64) -> Result<Vec<CreditRefundView>, String> {
    refund_list::list_customer_credit_refunds(&db.pool().await, credit_id).await
}
