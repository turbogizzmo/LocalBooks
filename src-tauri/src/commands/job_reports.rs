//! Job costing reports: profitability, estimates against actuals and Profit and Loss by Job.

use super::prelude::*;
use crate::job_reports::{self, JobReport, JobReportKind, JobReportParams};

handlers![
    get_job_report,
    get_profit_and_loss_by_job,
];

#[tauri::command]
pub async fn get_job_report(db: State<'_, Db>, kind: JobReportKind, params: JobReportParams) -> Result<JobReport, String> {
    job_reports::run(&db.pool().await, kind, &params, Local::now().date_naive()).await
}

#[tauri::command]
pub async fn get_profit_and_loss_by_job(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate) -> Result<ProfitAndLossByClass, String> {
    job_reports::profit_and_loss_by_job(&db.pool().await, start_date, end_date).await
}
