//! Sales tax.

use super::prelude::*;

handlers![
    list_tax_rates,
    create_tax_rate,
    update_tax_rate,
    set_tax_rate_active,
    pay_sales_tax,
    list_sales_tax_payments,
    get_tax_liability,
    save_combined_rate,
    get_sales_tax_return,
];

#[tauri::command]
pub async fn list_tax_rates(db: State<'_, Db>) -> Result<Vec<TaxRateView>, String> {
    sales_tax::list_tax_rates(&db.pool().await).await
}

#[tauri::command]
pub async fn create_tax_rate(app: AppHandle, db: State<'_, Db>, input: TaxRateInput) -> Result<TaxRateView, String> {
    let rate = sales_tax::create_tax_rate(&db.pool().await, input).await?;
    notify(&app);
    Ok(rate)
}

#[tauri::command]
pub async fn update_tax_rate(app: AppHandle, db: State<'_, Db>, id: i64, input: TaxRateInput) -> Result<TaxRateView, String> {
    let rate = sales_tax::update_tax_rate(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(rate)
}

#[tauri::command]
pub async fn set_tax_rate_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<TaxRateView, String> {
    let rate = sales_tax::set_tax_rate_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(rate)
}

#[tauri::command]
pub async fn pay_sales_tax(app: AppHandle, db: State<'_, Db>, input: SalesTaxPaymentInput) -> Result<i64, String> {
    let id = sales_tax::pay_sales_tax(&db.pool().await, input).await?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub async fn list_sales_tax_payments(db: State<'_, Db>) -> Result<Vec<SalesTaxPaymentView>, String> {
    sales_tax::list_sales_tax_payments(&db.pool().await).await
}

#[tauri::command]
pub async fn get_tax_liability(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate) -> Result<TaxLiabilityReport, String> {
    sales_tax::tax_liability(&db.pool().await, start_date, end_date).await
}

/// Saves a combined rate (new when `id` is null): its component rates, each with its own agency.
#[tauri::command]
pub async fn save_combined_rate(app: AppHandle, db: State<'_, Db>, id: Option<i64>, input: sales_tax::combined::CombinedRateInput) -> Result<TaxRateView, String> {
    let rate = sales_tax::combined::save_combined_rate(&db.pool().await, id, &input).await?;
    notify(&app);
    Ok(rate)
}

/// The sales tax return per agency for a period, on `basis` (the company's when not given).
#[tauri::command]
pub async fn get_sales_tax_return(db: State<'_, Db>, start_date: NaiveDate, end_date: NaiveDate, basis: Option<String>) -> Result<sales_tax::combined::SalesTaxReturn, String> {
    let pool = db.pool().await;
    let basis = match basis {
        Some(b) => sales_tax::Basis::parse(&b),
        None => sales_tax::basis(&pool).await?,
    };
    sales_tax::combined::sales_tax_return(&pool, start_date, end_date, basis).await
}
