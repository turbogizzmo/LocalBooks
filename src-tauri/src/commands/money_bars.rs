//! The money bars' extra segments (T61): unbilled and estimates on sales, purchase orders and
//! card balances on bills, each with the transactions behind it.

use super::prelude::*;
use crate::money_bars::{self, BarSegment};

handlers![
    get_money_bar_segments,
];

/// `side` is `SALES` or `BILLS`.
#[tauri::command]
pub async fn get_money_bar_segments(db: State<'_, Db>, side: String) -> Result<Vec<BarSegment>, String> {
    let pool = db.pool().await;
    match side.as_str() {
        "SALES" => money_bars::sales_segments(&pool).await,
        "BILLS" => money_bars::bill_segments(&pool, Local::now().date_naive()).await,
        other => Err(format!("BAR_SIDE: unknown money bar {other}")),
    }
}
