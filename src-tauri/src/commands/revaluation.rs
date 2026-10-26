//! Foreign currency revaluation and exchange-rate lookup.

use super::prelude::*;

handlers![
    preview_fx_revaluation,
    post_fx_revaluation,
    list_fx_revaluations,
    void_fx_revaluation,
    lookup_exchange_rate,
];

#[tauri::command]
pub async fn preview_fx_revaluation(db: State<'_, Db>, as_of: NaiveDate, rates: BTreeMap<String, i64>) -> Result<RevaluationPreview, String> {
    revaluation::preview(&db.pool().await, as_of, &rates).await
}

#[tauri::command]
pub async fn post_fx_revaluation(app: AppHandle, db: State<'_, Db>, as_of: NaiveDate, rates: BTreeMap<String, i64>) -> Result<RevaluationView, String> {
    let posted = revaluation::post(&db.pool().await, as_of, &rates).await?;
    notify(&app);
    Ok(posted)
}

#[tauri::command]
pub async fn list_fx_revaluations(db: State<'_, Db>) -> Result<Vec<RevaluationView>, String> {
    revaluation::list(&db.pool().await).await
}

#[tauri::command]
pub async fn void_fx_revaluation(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    revaluation::void(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

/// Suggests the ECB rate for `currency` in the home currency on `date` (opt-in; see `fx_rates.rs`).
/// Only caches the answer, so no `ledger-changed` event.
#[tauri::command]
pub async fn lookup_exchange_rate(db: State<'_, Db>, http: State<'_, fx_rates::ReqwestHttp>, currency: String, date: NaiveDate) -> Result<fx_rates::RateSuggestion, String> {
    let pool = db.pool().await;
    let home = preferences::get(&pool, "home_currency_code").await?;
    fx_rates::suggest(&pool, http.inner(), &currency, &home, date, Local::now().date_naive()).await
}
