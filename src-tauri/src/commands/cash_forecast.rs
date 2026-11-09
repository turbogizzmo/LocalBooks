//! Cash Flow Forecast: cash on hand plus money due in and out, week by week.

use super::prelude::*;
use crate::cash_forecast::{self, CashFlowForecast};

handlers![get_cash_flow_forecast];

#[tauri::command]
pub async fn get_cash_flow_forecast(db: State<'_, Db>, as_of: NaiveDate, weeks: u32) -> Result<CashFlowForecast, String> {
    cash_forecast::forecast(&db.pool().await, as_of, weeks).await
}
