//! Payout feeds from payment processors (Stripe): connect, fetch payouts, record them as deposits.

use super::prelude::*;
use crate::payout_feeds::{self, FeedInput, FetchResult, Payout, PayoutFeed, STRIPE_API};

handlers![list_payout_feeds, connect_stripe_feed, update_payout_feed, disconnect_payout_feed, fetch_payouts, list_payouts, record_payout, set_payout_ignored];

fn feed_secrets(app: &AppHandle) -> Result<SecretStore, String> {
    Ok(payout_feeds::secret_store(&crate::data_dirs(app).map_err(|e| format!("PAYOUT_CONFIG: {e}"))?.1))
}

#[tauri::command]
pub async fn list_payout_feeds(db: State<'_, Db>) -> Result<Vec<PayoutFeed>, String> {
    payout_feeds::feeds(&db.pool().await).await
}

/// Connects a Stripe account with a restricted, read-only key (kept in the credential store).
#[tauri::command]
pub async fn connect_stripe_feed(app: AppHandle, db: State<'_, Db>, key: String, input: FeedInput) -> Result<PayoutFeed, String> {
    let key = zeroize::Zeroizing::new(key);
    let feed = payout_feeds::connect_stripe(&db.pool().await, &feed_secrets(&app)?, STRIPE_API, &key, &input).await?;
    notify(&app);
    Ok(feed)
}

#[tauri::command]
pub async fn update_payout_feed(app: AppHandle, db: State<'_, Db>, id: i64, input: FeedInput) -> Result<PayoutFeed, String> {
    let feed = payout_feeds::update_feed(&db.pool().await, id, &input).await?;
    notify(&app);
    Ok(feed)
}

#[tauri::command]
pub async fn disconnect_payout_feed(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    payout_feeds::disconnect(&db.pool().await, &feed_secrets(&app)?, id).await?;
    notify(&app);
    Ok(())
}

/// Asks the processor for payouts since the last fetch. The only time LocalBooks calls it.
#[tauri::command]
pub async fn fetch_payouts(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<FetchResult, String> {
    let result = payout_feeds::fetch(&db.pool().await, &feed_secrets(&app)?, STRIPE_API, id, chrono::Utc::now()).await?;
    notify(&app);
    Ok(result)
}

#[tauri::command]
pub async fn list_payouts(db: State<'_, Db>, feed_id: i64) -> Result<Vec<Payout>, String> {
    payout_feeds::list_payouts(&db.pool().await, feed_id).await
}

/// Records a payout as a deposit: its sales gross, less fees, refunds and chargebacks.
#[tauri::command]
pub async fn record_payout(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<i64, String> {
    let deposit = payout_feeds::record(&db.pool().await, id).await?;
    notify(&app);
    Ok(deposit)
}

#[tauri::command]
pub async fn set_payout_ignored(app: AppHandle, db: State<'_, Db>, id: i64, ignored: bool) -> Result<(), String> {
    payout_feeds::set_ignored(&db.pool().await, id, ignored).await?;
    notify(&app);
    Ok(())
}
