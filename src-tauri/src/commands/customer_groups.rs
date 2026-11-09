//! Customer groups (`customer_groups.rs`): the Groups view of the Customer Center.

use super::prelude::*;
use crate::customer_groups::{self, CustomerGroup, CustomerGroupInput};

handlers![
    list_customer_groups,
    save_customer_group,
    delete_customer_group,
    mark_group_statements_sent,
];

/// Every group with its members right now (an automatic group's worked out from its rules).
#[tauri::command]
pub async fn list_customer_groups(db: State<'_, Db>) -> Result<Vec<CustomerGroup>, String> {
    customer_groups::list(&db.pool().await).await
}

/// Adds a group (`id` None) or changes one.
#[tauri::command]
pub async fn save_customer_group(app: AppHandle, db: State<'_, Db>, id: Option<i64>, input: CustomerGroupInput) -> Result<CustomerGroup, String> {
    let group = customer_groups::save(&db.pool().await, id, &input).await?;
    notify(&app);
    Ok(group)
}

#[tauri::command]
pub async fn delete_customer_group(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    customer_groups::delete(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

/// The group's statements went out on `date` (today when left out): the reminder waits for the next ones.
#[tauri::command]
pub async fn mark_group_statements_sent(app: AppHandle, db: State<'_, Db>, id: i64, date: Option<NaiveDate>) -> Result<CustomerGroup, String> {
    let group = customer_groups::mark_statements_sent(&db.pool().await, id, date.unwrap_or_else(|| Local::now().date_naive())).await?;
    notify(&app);
    Ok(group)
}
