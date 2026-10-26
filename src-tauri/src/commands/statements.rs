//! Dashboard and financial statements.

use super::prelude::*;

handlers![
    get_dashboard,
    get_profit_and_loss,
    get_profit_and_loss_periods,
    get_trial_balance_as_of,
    get_cash_flow,
    get_general_ledger,
    get_aging,
    get_summary_report,
    get_profit_and_loss_by_class,
    get_balance_sheet,
    get_balance_sheet_columns,
    get_inventory_valuation,
    get_inventory_valuation_detail,
    get_stock_status,
    get_missing_checks,
];

#[tauri::command]
pub async fn get_dashboard(db: State<'_, Db>) -> Result<Dashboard, String> {
    reports::dashboard(&db.pool().await, Local::now().date_naive()).await
}

#[tauri::command]
pub async fn get_profit_and_loss(
    db: State<'_, Db>,
    start_date: NaiveDate,
    end_date: NaiveDate,
    class_filter: Option<ClassFilter>,
    basis: Option<Basis>,
) -> Result<ProfitAndLoss, String> {
    statements::profit_and_loss_basis(&db.pool().await, start_date, end_date, class_filter.as_ref(), basis.unwrap_or_default()).await
}

#[tauri::command]
pub async fn get_profit_and_loss_periods(db: State<'_, Db>, input: PeriodReportInput) -> Result<ProfitAndLossByClass, String> {
    statements::profit_and_loss_periods(&db.pool().await, &input).await
}

#[tauri::command]
pub async fn get_trial_balance_as_of(db: State<'_, Db>, as_of: NaiveDate) -> Result<DatedTrialBalance, String> {
    statements::trial_balance_as_of(&db.pool().await, as_of).await
}

#[tauri::command]
pub async fn get_cash_flow(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate) -> Result<CashFlowStatement, String> {
    statements::cash_flow(&db.pool().await, start_date, end_date).await
}

#[tauri::command]
pub async fn get_general_ledger(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate) -> Result<GeneralLedger, String> {
    report_center::general_ledger(&db.pool().await, start_date, end_date).await
}

#[tauri::command]
pub async fn get_aging(db: State<'_, Db>, payable: bool, as_of: NaiveDate) -> Result<AgingReport, String> {
    report_center::aging(&db.pool().await, payable, as_of).await
}

/// `customer` (sales by customer), `vendor` (expenses by vendor), `product` (sales by product/service)
/// or `rep` (sales by rep).
#[tauri::command]
pub async fn get_summary_report(db: State<'_, Db>, by: String, start_date: NaiveDate, end_date: NaiveDate) -> Result<SummaryReport, String> {
    let pool = db.pool().await;
    match by.as_str() {
        "customer" => report_center::by_party(&pool, false, start_date, end_date).await,
        "vendor" => report_center::by_party(&pool, true, start_date, end_date).await,
        "product" => report_center::sales_by_product(&pool, start_date, end_date).await,
        "rep" => report_center::sales_by_rep(&pool, start_date, end_date).await,
        other => Err(format!("RPT_UNKNOWN: '{other}' is not a summary report")),
    }
}

#[tauri::command]
pub async fn get_profit_and_loss_by_class(
    db: State<'_, Db>,
    start_date: NaiveDate,
    end_date: NaiveDate,
) -> Result<ProfitAndLossByClass, String> {
    statements::profit_and_loss_by_class(&db.pool().await, start_date, end_date).await
}

#[tauri::command]
pub async fn get_balance_sheet(db: State<'_, Db>, as_of: NaiveDate) -> Result<BalanceSheet, String> {
    statements::balance_sheet(&db.pool().await, as_of).await
}

/// Balance Sheet with a column per date; `compare` adds $ and % change from the second date.
#[tauri::command]
pub async fn get_balance_sheet_columns(db: State<'_, Db>, columns: Vec<AsOfColumn>, compare: bool) -> Result<ProfitAndLossByClass, String> {
    statements::balance_sheet_columns(&db.pool().await, &columns, compare).await
}

#[tauri::command]
pub async fn get_inventory_valuation(db: State<'_, Db>, as_of: NaiveDate) -> Result<InventoryValuation, String> {
    report_center::inventory_valuation(&db.pool().await, as_of).await
}

#[tauri::command]
pub async fn get_inventory_valuation_detail(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate) -> Result<InventoryValuationDetail, String> {
    report_center::inventory_valuation_detail(&db.pool().await, start_date, end_date).await
}

#[tauri::command]
pub async fn get_stock_status(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate) -> Result<StockStatus, String> {
    report_center::stock_status(&db.pool().await, start_date, end_date).await
}

#[tauri::command]
pub async fn get_missing_checks(db: State<'_, Db>, account_id: i64, start_date: Option<NaiveDate>, end_date: Option<NaiveDate>) -> Result<MissingChecks, String> {
    report_center::missing_checks(&db.pool().await, account_id, start_date, end_date).await
}
