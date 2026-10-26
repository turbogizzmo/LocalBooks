//! Customer statements.

use super::prelude::*;

handlers![
    get_customer_statement,
    list_statement_customers,
];

#[tauri::command]
pub async fn get_customer_statement(db: State<'_, Db>, customer_id: i64, start_date: NaiveDate, end_date: NaiveDate) -> Result<CustomerStatement, String> {
    workflows::customer_statement(&db.pool().await, customer_id, start_date, end_date).await
}

#[tauri::command]
pub async fn list_statement_customers(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate) -> Result<Vec<(i64, String, i64)>, String> {
    workflows::statement_customers(&db.pool().await, start_date, end_date).await
}
