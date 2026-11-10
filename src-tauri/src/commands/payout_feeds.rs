//! Payout feeds from payment processors (Stripe, PayPal, Shopify Payments): connect, fetch payouts, record them as deposits.

use super::prelude::*;
use crate::payout_feeds::{self, Apis, FeedInput, FetchResult, PayPalCredentials, Payout, PayoutFeed, STRIPE_API};

handlers![list_payout_feeds, connect_stripe_feed, connect_paypal_feed, connect_shopify_feed, set_payout_auto_fetch, update_payout_feed, disconnect_payout_feed, fetch_payouts, list_payouts, record_payout, set_payout_ignored];

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

/// Connects a PayPal REST app (client id and secret, kept in the credential store).
#[tauri::command]
pub async fn connect_paypal_feed(app: AppHandle, db: State<'_, Db>, credentials: PayPalCredentials, input: FeedInput) -> Result<PayoutFeed, String> {
    let apis = Apis::live();
    let api = if credentials.sandbox { &apis.paypal_sandbox } else { &apis.paypal_live };
    let feed = payout_feeds::connect_paypal(&db.pool().await, &feed_secrets(&app)?, api, &credentials, &input, chrono::Utc::now()).await?;
    notify(&app);
    Ok(feed)
}

/// Connects a Shopify store's Shopify Payments: its address and a custom app's Admin API access
/// token with read-only payout scopes (kept in the credential store).
#[tauri::command]
pub async fn connect_shopify_feed(app: AppHandle, db: State<'_, Db>, shop: String, token: String, input: FeedInput) -> Result<PayoutFeed, String> {
    let token = zeroize::Zeroizing::new(token);
    let feed = payout_feeds::connect_shopify(&db.pool().await, &feed_secrets(&app)?, None, &shop, &token, &input).await?;
    notify(&app);
    Ok(feed)
}

/// Fetches a feed every `hours` (1, 6, 12 or 24) while LocalBooks is open, or only by hand (`None`).
#[tauri::command]
pub async fn set_payout_auto_fetch(app: AppHandle, db: State<'_, Db>, id: i64, hours: Option<i64>) -> Result<PayoutFeed, String> {
    let feed = payout_feeds::set_auto_fetch(&db.pool().await, id, hours).await?;
    notify(&app);
    Ok(feed)
}

/// The feeds due an automatic fetch, fetched (`main.rs` calls this every few minutes). Only while
/// someone who may change the books is signed in (or the file has no users), and never in an
/// archive: the same rule as Get payouts by hand.
pub async fn run_due_payout_fetches(app: &AppHandle) {
    let Some(db) = app.try_state::<Db>() else { return };
    if !payout_feeds::may_auto_fetch(&db).await {
        return;
    }
    let Ok(store) = feed_secrets(app) else { return };
    match payout_feeds::fetch_due(&db.pool().await, &store, &Apis::live(), chrono::Utc::now()).await {
        Ok(done) if !done.is_empty() => notify(app),
        Ok(_) => {}
        // An older company file without payout feeds, or one being switched.
        Err(_) => {}
    }
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
    let result = payout_feeds::fetch(&db.pool().await, &feed_secrets(&app)?, &Apis::live(), id, chrono::Utc::now()).await?;
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
