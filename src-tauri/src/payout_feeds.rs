//! Payout feeds from payment processors (gaps row T68; design in `docs/plans/payout-feeds.md`).
//!
//! Stripe, PayPal ([`paypal`]) and Shopify Payments ([`shopify`]). A processor sends the bank one net amount per payout; this module fetches each
//! payout's balance transactions, works out its gross sales, fees, refunds, chargebacks and other
//! adjustments, and records it as a Deposit whose lines are the gross charges and whose deductions
//! are the rest ([`crate::banking::write_deposit`]). The deposit's bank line is the payout's net
//! amount, so the downloaded bank line matches it.
//!
//! Privacy: the feed does nothing until the user sets it up; the restricted, read-only key lives in
//! the OS credential store ([`crate::secrets`]); LocalBooks calls only the processor's API, only
//! when the user asks for payouts.

use std::path::Path;
use std::time::Duration;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

use crate::banking::{DepositDeduction, DepositInput, DepositOtherLine};
use crate::ledger::{db_err, format_cents};
use crate::money::Cents;
use crate::secrets::SecretStore;

const KEYCHAIN_SERVICE: &str = "LocalBooks payout feeds";
const SECRETS_FILE: &str = "payout-feed-keys.json";
pub const STRIPE_API: &str = "https://api.stripe.com";

/// Where each processor's API is: the real ones, or a loopback server under test.
#[derive(Debug, Clone)]
pub struct Apis {
    pub stripe: String,
    pub paypal_live: String,
    pub paypal_sandbox: String,
    /// A test server in place of each Shopify store's own address.
    pub shopify: Option<String>,
}

impl Apis {
    pub fn live() -> Apis {
        Apis { stripe: STRIPE_API.into(), paypal_live: paypal::LIVE_API.into(), paypal_sandbox: paypal::SANDBOX_API.into(), shopify: None }
    }

    /// Every processor at one address (tests).
    #[cfg(test)]
    pub fn all(base: &str) -> Apis {
        Apis { stripe: base.into(), paypal_live: base.into(), paypal_sandbox: base.into(), shopify: Some(base.into()) }
    }

    fn paypal(&self, account_ref: Option<&str>) -> &str {
        if account_ref == Some("SANDBOX") {
            &self.paypal_sandbox
        } else {
            &self.paypal_live
        }
    }
}

mod paypal;
mod shopify;
pub use paypal::{connect_paypal, PayPalCredentials};
pub use shopify::connect_shopify;
/// How far back the first fetch reaches, and how much each later fetch overlaps the last.
const FIRST_DAYS: i64 = 90;
const OVERLAP_DAYS: i64 = 7;

pub fn secret_store(config_dir: &Path) -> SecretStore {
    SecretStore::for_this_computer(config_dir, KEYCHAIN_SERVICE, SECRETS_FILE, "the payout feed key")
}

// ------------------------------------------------------------------------------------------------
// The breakdown of a payout
// ------------------------------------------------------------------------------------------------

/// One charge, refund or dispute in a payout, for its deposit line and the screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayoutItem {
    /// `CHARGE`, `REFUND` or `CHARGEBACK`.
    pub kind: String,
    pub description: String,
    /// Positive: a charge's gross, or what a refund or dispute took (negative when a dispute was won back).
    pub amount_cents: i64,
}

/// A payout's figures: gross − fees − refunds − chargebacks + other = net.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Breakdown {
    pub net_cents: i64,
    pub gross_cents: i64,
    pub fee_cents: i64,
    pub refund_cents: i64,
    pub chargeback_cents: i64,
    /// Adjustments of other kinds, signed (positive adds to the payout).
    pub other_cents: i64,
    pub items: Vec<PayoutItem>,
}

pub(crate) fn int(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}

pub(crate) fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Works out a Stripe payout's breakdown from its balance transactions (the payout's own
/// transaction, type `payout`, is skipped). Each transaction's `amount` is gross and `fee` the
/// processor's fee, in the currency's smallest unit; their `amount − fee` add up to the payout.
/// Refused when they don't, so a deposit always ties to the bank.
pub fn stripe_breakdown(payout_amount: i64, transactions: &[Value]) -> Result<Breakdown, String> {
    let overflow = || "PAYOUT_TOO_LARGE: the payout's amounts are out of range".to_string();
    let add = |a: i64, b: i64| a.checked_add(b).ok_or_else(overflow);
    let mut b = Breakdown { net_cents: payout_amount, gross_cents: 0, fee_cents: 0, refund_cents: 0, chargeback_cents: 0, other_cents: 0, items: Vec::new() };
    for t in transactions {
        let kind = text(t, "type");
        if kind == "payout" {
            continue;
        }
        let amount = int(t, "amount");
        let fee = int(t, "fee");
        let description = text(t, "description");
        let category = text(t, "reporting_category");
        b.fee_cents = add(b.fee_cents, fee)?;
        match kind.as_str() {
            "charge" | "payment" => {
                b.gross_cents = add(b.gross_cents, amount)?;
                b.items.push(PayoutItem {
                    kind: "CHARGE".into(),
                    description: if description.is_empty() { "Card sale".into() } else { description },
                    amount_cents: amount,
                });
            }
            "refund" | "payment_refund" | "payment_failure_refund" => {
                b.refund_cents = add(b.refund_cents, -amount)?;
                b.items.push(PayoutItem {
                    kind: "REFUND".into(),
                    description: if description.is_empty() { "Refund".into() } else { description },
                    amount_cents: -amount,
                });
            }
            "adjustment" | "dispute" if category.starts_with("dispute") || kind == "dispute" => {
                b.chargeback_cents = add(b.chargeback_cents, -amount)?;
                b.items.push(PayoutItem {
                    kind: "CHARGEBACK".into(),
                    description: if description.is_empty() { "Dispute".into() } else { description },
                    amount_cents: -amount,
                });
            }
            "stripe_fee" | "application_fee" | "tax" => b.fee_cents = add(b.fee_cents, -amount)?,
            _ => b.other_cents = add(b.other_cents, amount)?,
        }
    }
    let worked_out = b.gross_cents - b.fee_cents - b.refund_cents - b.chargeback_cents + b.other_cents;
    if worked_out != payout_amount {
        return Err(format!(
            "PAYOUT_MISMATCH: the payout's transactions add up to {}, not the {} paid out; it can't be recorded",
            format_cents(worked_out),
            format_cents(payout_amount)
        ));
    }
    Ok(b)
}

/// How a processor is named on deposits and the screen.
pub fn processor_name(processor: &str) -> &'static str {
    match processor {
        "PAYPAL" => "PayPal",
        "SHOPIFY" => "Shopify Payments",
        _ => "Stripe",
    }
}

/// The deposit a payout becomes, with the feed's accounts.
pub fn deposit_for(feed: &PayoutFeed, arrival: NaiveDate, payout_id: &str, b: &Breakdown) -> DepositInput {
    let mut other_lines = Vec::new();
    let mut deductions = Vec::new();
    for item in &b.items {
        let memo = Some(item.description.chars().take(200).collect::<String>());
        match (item.kind.as_str(), item.amount_cents) {
            (_, 0) => {}
            ("CHARGE", a) if a > 0 => other_lines.push(DepositOtherLine {
                account_id: feed.sales_account_id,
                amount_cents: Cents(a),
                memo,
                class_id: None,
                customer_id: None,
                vendor_id: None,
            }),
            ("CHARGE", a) => deductions.push(DepositDeduction { account_id: feed.sales_account_id, amount_cents: Cents(-a), memo, kind: "OTHER".into() }),
            ("REFUND", a) if a > 0 => {
                deductions.push(DepositDeduction { account_id: feed.refund_account_id, amount_cents: Cents(a), memo, kind: "REFUND".into() })
            }
            ("REFUND", a) => other_lines.push(DepositOtherLine {
                account_id: feed.refund_account_id,
                amount_cents: Cents(-a),
                memo,
                class_id: None,
                customer_id: None,
                vendor_id: None,
            }),
            (_, a) if a > 0 => {
                deductions.push(DepositDeduction { account_id: feed.chargeback_account_id, amount_cents: Cents(a), memo, kind: "CHARGEBACK".into() })
            }
            (_, a) => other_lines.push(DepositOtherLine {
                account_id: feed.chargeback_account_id,
                amount_cents: Cents(-a),
                memo,
                class_id: None,
                customer_id: None,
                vendor_id: None,
            }),
        }
    }
    let processor = processor_name(&feed.processor);
    match b.fee_cents {
        f if f > 0 => deductions.push(DepositDeduction {
            account_id: feed.fee_account_id,
            amount_cents: Cents(f),
            memo: Some(format!("{processor} fees")),
            kind: "FEE".into(),
        }),
        f if f < 0 => other_lines.push(DepositOtherLine {
            account_id: feed.fee_account_id,
            amount_cents: Cents(-f),
            memo: Some(format!("{processor} fees returned")),
            class_id: None,
            customer_id: None,
            vendor_id: None,
        }),
        _ => {}
    }
    match b.other_cents {
        o if o > 0 => other_lines.push(DepositOtherLine {
            account_id: feed.fee_account_id,
            amount_cents: Cents(o),
            memo: Some(format!("{processor} adjustments")),
            class_id: None,
            customer_id: None,
            vendor_id: None,
        }),
        o if o < 0 => deductions.push(DepositDeduction {
            account_id: feed.fee_account_id,
            amount_cents: Cents(-o),
            memo: Some(format!("{processor} adjustments")),
            kind: "OTHER".into(),
        }),
        _ => {}
    }
    DepositInput {
        bank_account_id: feed.bank_account_id,
        deposit_date: arrival,
        memo: Some(format!("{processor} payout {payout_id}")),
        items: Vec::new(),
        other_lines,
        cash_back: None,
        deductions,
    }
}

// ------------------------------------------------------------------------------------------------
// Feeds
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PayoutFeed {
    pub id: i64,
    pub processor: String,
    pub name: String,
    #[serde(skip)]
    pub secret_key: String,
    /// PayPal: `LIVE` or `SANDBOX`; Shopify: the store's `….myshopify.com` address.
    pub account_ref: Option<String>,
    pub bank_account_id: i64,
    pub sales_account_id: i64,
    pub fee_account_id: i64,
    pub refund_account_id: i64,
    pub chargeback_account_id: i64,
    pub last_fetched_at: Option<String>,
    /// Fetched automatically every this many hours while LocalBooks is open; `None`: only by hand.
    pub auto_fetch_hours: Option<i64>,
    /// What the last automatic fetch found, or why it failed.
    pub last_auto_result: Option<String>,
    /// When the last automatic try was.
    #[serde(skip)]
    pub last_auto_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedInput {
    pub name: String,
    pub bank_account_id: i64,
    pub sales_account_id: i64,
    pub fee_account_id: i64,
    pub refund_account_id: i64,
    pub chargeback_account_id: i64,
}

const FEED_SELECT: &str =
    "SELECT id, processor, name, secret_key, account_ref, bank_account_id, sales_account_id, fee_account_id, refund_account_id, chargeback_account_id, last_fetched_at, auto_fetch_hours, last_auto_result, last_auto_at FROM payout_feeds";

pub async fn feeds(pool: &SqlitePool) -> Result<Vec<PayoutFeed>, String> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("{FEED_SELECT} ORDER BY name"))).fetch_all(pool).await.map_err(db_err)
}

pub(crate) async fn feed(pool: &SqlitePool, id: i64) -> Result<PayoutFeed, String> {
    sqlx::query_as(sqlx::AssertSqlSafe(format!("{FEED_SELECT} WHERE id = ?1")))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| "PAYOUT_NO_FEED: that payout feed was removed".into())
}

pub(crate) async fn check_accounts(pool: &SqlitePool, input: &FeedInput) -> Result<(), String> {
    let kind = |id: i64| async move {
        sqlx::query_scalar::<_, String>("SELECT account_type FROM chart_of_accounts WHERE id = ?1 AND is_active = 1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)
    };
    if kind(input.bank_account_id).await?.as_deref() != Some("BANK") {
        return Err("PAYOUT_BANK: choose the bank account payouts arrive in".into());
    }
    for (id, what) in
        [(input.sales_account_id, "sales"), (input.fee_account_id, "fees"), (input.refund_account_id, "refunds"), (input.chargeback_account_id, "chargebacks")]
    {
        match kind(id).await? {
            None => return Err(format!("PAYOUT_ACCOUNT: choose an active account for {what}")),
            Some(_) if id == input.bank_account_id => return Err(format!("PAYOUT_ACCOUNT: the {what} account can't be the bank account")),
            Some(_) => {}
        }
    }
    if input.name.trim().is_empty() {
        return Err("PAYOUT_NAME: name the feed".into());
    }
    Ok(())
}

/// Connects a Stripe account: checks the key with one call, keeps it in the credential store, and
/// saves the feed with its accounts.
pub async fn connect_stripe(pool: &SqlitePool, store: &SecretStore, api: &str, key: &str, input: &FeedInput) -> Result<PayoutFeed, String> {
    let key = key.trim();
    // Only a restricted key: a full secret key (sk_) could move money, and LocalBooks needs to read.
    if !key.starts_with("rk_") {
        return Err("PAYOUT_KEY: paste a Stripe restricted key with read access to Balance and Payouts (it starts rk_live_ or rk_test_)".into());
    }
    check_accounts(pool, input).await?;
    stripe_get(&client()?, api, key, "/v1/balance", &[]).await?;
    let secret_key = format!("stripe-{}", hex(&crate::crypto::random::<8>()?));
    store.put(&secret_key, key).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO payout_feeds (processor, name, secret_key, bank_account_id, sales_account_id, fee_account_id, refund_account_id, chargeback_account_id)
         VALUES ('STRIPE', ?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
    )
    .bind(input.name.trim())
    .bind(&secret_key)
    .bind(input.bank_account_id)
    .bind(input.sales_account_id)
    .bind(input.fee_account_id)
    .bind(input.refund_account_id)
    .bind(input.chargeback_account_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    feed(pool, id).await
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Changes a feed's accounts or name.
pub async fn update_feed(pool: &SqlitePool, id: i64, input: &FeedInput) -> Result<PayoutFeed, String> {
    check_accounts(pool, input).await?;
    sqlx::query("UPDATE payout_feeds SET name = ?1, bank_account_id = ?2, sales_account_id = ?3, fee_account_id = ?4, refund_account_id = ?5, chargeback_account_id = ?6 WHERE id = ?7")
        .bind(input.name.trim())
        .bind(input.bank_account_id)
        .bind(input.sales_account_id)
        .bind(input.fee_account_id)
        .bind(input.refund_account_id)
        .bind(input.chargeback_account_id)
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    feed(pool, id).await
}

/// Whether an automatic fetch may run now: a company file is open, it isn't an archive, and someone
/// who may change the books is signed in (or the file has no users). The same rule as Get payouts;
/// it doesn't count as the signed-in user's activity, so it never keeps an idle session open.
pub async fn may_auto_fetch(db: &crate::db::Db) -> bool {
    db.is_open().await && db.check(crate::permissions::Need::Write, false).is_ok()
}

/// The intervals an Admin can choose for fetching automatically.
pub const AUTO_FETCH_HOURS: [i64; 4] = [1, 6, 12, 24];

/// Turns automatic fetching on (every `hours`) or off (`None`) for a feed.
pub async fn set_auto_fetch(pool: &SqlitePool, id: i64, hours: Option<i64>) -> Result<PayoutFeed, String> {
    if hours.is_some_and(|h| !AUTO_FETCH_HOURS.contains(&h)) {
        return Err("PAYOUT_AUTO: fetch every 1, 6, 12 or 24 hours, or turn it off".into());
    }
    let done = sqlx::query("UPDATE payout_feeds SET auto_fetch_hours = ?1 WHERE id = ?2").bind(hours).bind(id).execute(pool).await.map_err(db_err)?;
    if done.rows_affected() != 1 {
        return Err("PAYOUT_NO_FEED: that payout feed was removed".into());
    }
    feed(pool, id).await
}

/// What one automatic fetch did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoFetch {
    pub feed_id: i64,
    pub new_payouts: usize,
    /// Why it failed (the network, the key), kept for the screen.
    pub error: Option<String>,
}

/// Fetches every feed whose automatic interval has passed since its last fetch (by hand or not),
/// and keeps a line saying what each found. Never posts anything: recording stays a choice.
pub async fn fetch_due(pool: &SqlitePool, store: &SecretStore, apis: &Apis, now: DateTime<Utc>) -> Result<Vec<AutoFetch>, String> {
    let at = |t: &Option<String>| t.as_deref().and_then(|t| DateTime::parse_from_rfc3339(t).ok()).map(|t| t.with_timezone(&Utc));
    let mut done = Vec::new();
    for f in feeds(pool).await? {
        let Some(hours) = f.auto_fetch_hours else { continue };
        // The later of the last fetch (by hand or automatic) and the last automatic try: a failed
        // try waits for the next interval too, rather than calling the processor again soon.
        let last = at(&f.last_fetched_at).max(at(&f.last_auto_at));
        if last.is_some_and(|t| now - t < chrono::Duration::hours(hours)) {
            continue;
        }
        let stamp = now.format("%Y-%m-%d %H:%M UTC");
        let (line, result) = match fetch(pool, store, apis, f.id, now).await {
            Ok(r) => {
                let refused = if r.refused.is_empty() { String::new() } else { format!(" Not kept: {}.", r.refused.join("; ")) };
                let line = format!("{stamp}: {} new payout{}.{refused}", r.new_payouts, if r.new_payouts == 1 { "" } else { "s" });
                (line, AutoFetch { feed_id: f.id, new_payouts: r.new_payouts, error: None })
            }
            Err(e) => (format!("{stamp}: not fetched: {}", reason(&e)), AutoFetch { feed_id: f.id, new_payouts: 0, error: Some(e) }),
        };
        let line: String = line.chars().take(2000).collect();
        sqlx::query("UPDATE payout_feeds SET last_auto_result = ?1, last_auto_at = ?2 WHERE id = ?3")
            .bind(&line)
            .bind(now.to_rfc3339())
            .bind(f.id)
            .execute(pool)
            .await
            .map_err(db_err)?;
        done.push(result);
    }
    Ok(done)
}

/// Disconnects a feed: forgets its key and its payouts not yet recorded. Recorded deposits stay.
pub async fn disconnect(pool: &SqlitePool, store: &SecretStore, id: i64) -> Result<(), String> {
    let f = feed(pool, id).await?;
    store.delete(&f.secret_key).await?;
    sqlx::query("DELETE FROM payout_feeds WHERE id = ?1").bind(id).execute(pool).await.map_err(db_err)?;
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Fetching from Stripe
// ------------------------------------------------------------------------------------------------

/// Only HTTPS, except a loopback server under test.
pub(crate) fn api_url(api: &str, path: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(&format!("{}{path}", api.trim_end_matches('/'))).map_err(|_| "PAYOUT_URL: bad processor address".to_string())?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost"));
    if url.scheme() != "https" && !loopback {
        return Err("PAYOUT_URL: the processor must be reached over HTTPS".into());
    }
    Ok(url)
}

pub(crate) fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder().timeout(Duration::from_secs(30)).build().map_err(|e| format!("PAYOUT_NETWORK: {e}"))
}

async fn stripe_get(client: &reqwest::Client, api: &str, key: &str, path: &str, query: &[(&str, String)]) -> Result<Value, String> {
    let mut url = api_url(api, path)?;
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
    }
    let response = client.get(url).bearer_auth(key).send().await.map_err(|e| format!("PAYOUT_NETWORK: Stripe couldn't be reached ({})", e.without_url()))?;
    let status = response.status();
    let body: Value = response.json().await.map_err(|_| "PAYOUT_RESPONSE: Stripe's answer couldn't be read".to_string())?;
    if !status.is_success() {
        // Stripe's message names the problem (a bad key, a missing permission); it never echoes the key.
        let message = body.pointer("/error/message").and_then(Value::as_str).unwrap_or("request refused");
        return Err(match status.as_u16() {
            401 => "PAYOUT_KEY: Stripe didn't accept the key; check it, or create a new restricted key".into(),
            403 => format!("PAYOUT_PERMISSION: the key can't read this ({message}); give it read access to Balance and Payouts"),
            _ => format!("PAYOUT_REFUSED: Stripe said: {message}"),
        });
    }
    Ok(body)
}

/// Most pages a list is read to (10,000 objects); past that the fetch is refused, not cut short.
const MAX_PAGES: usize = 100;

/// Every object of a list endpoint, page by page.
async fn stripe_list(client: &reqwest::Client, api: &str, key: &str, path: &str, query: &[(&str, String)]) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut q = query.to_vec();
        q.push(("limit", "100".into()));
        if let Some(a) = &after {
            q.push(("starting_after", a.clone()));
        }
        let page = stripe_get(client, api, key, path, &q).await?;
        let data = page.get("data").and_then(Value::as_array).cloned().unwrap_or_default();
        after = data.last().map(|d| text(d, "id"));
        out.extend(data);
        if !page.get("has_more").and_then(Value::as_bool).unwrap_or(false) || after.is_none() {
            return Ok(out);
        }
    }
    Err("PAYOUT_TOO_MANY: Stripe has more than Tillhouse reads in one go; get payouts more often so each fetch is smaller".into())
}

/// A payout as listed on the screen.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Payout {
    pub id: i64,
    pub feed_id: i64,
    pub processor_payout_id: String,
    pub arrival_date: NaiveDate,
    pub currency: String,
    pub net_cents: i64,
    pub gross_cents: i64,
    pub fee_cents: i64,
    pub refund_cents: i64,
    pub chargeback_cents: i64,
    pub other_cents: i64,
    pub details_json: String,
    pub status: String,
    pub deposit_id: Option<i64>,
}

/// What a fetch found.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FetchResult {
    pub new_payouts: usize,
    /// Payouts that couldn't be broken down (with why), not kept.
    pub refused: Vec<String>,
}

/// Fetches the payouts that arrived since the last fetch (overlapping a week; 90 days the first
/// time), with what each is made of, and keeps the ones not seen before.
pub async fn fetch(pool: &SqlitePool, store: &SecretStore, apis: &Apis, feed_id: i64, now: DateTime<Utc>) -> Result<FetchResult, String> {
    let f = feed(pool, feed_id).await?;
    let key = store.get(&f.secret_key).await?.ok_or_else(|| "PAYOUT_KEY_MISSING: this computer doesn't have the feed's key; connect it again".to_string())?;
    let since = f
        .last_fetched_at
        .as_deref()
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map_or(now - chrono::Duration::days(FIRST_DAYS), |t| t.with_timezone(&Utc) - chrono::Duration::days(OVERLAP_DAYS));
    let result = match f.processor.as_str() {
        "PAYPAL" => paypal::fetch(pool, &f, apis.paypal(f.account_ref.as_deref()), &key, since, now).await?,
        "SHOPIFY" => shopify::fetch(pool, &f, apis.shopify.as_deref(), &key, since).await?,
        _ => fetch_stripe(pool, &f, &apis.stripe, &key, since, now).await?,
    };
    sqlx::query("UPDATE payout_feeds SET last_fetched_at = ?1 WHERE id = ?2").bind(now.to_rfc3339()).bind(feed_id).execute(pool).await.map_err(db_err)?;
    Ok(result)
}

/// The company's home currency code.
pub(crate) async fn home_currency(pool: &SqlitePool) -> Result<String, String> {
    let home: Option<String> =
        sqlx::query_scalar("SELECT value FROM system_preferences WHERE key = 'home_currency_code'").fetch_optional(pool).await.map_err(db_err)?;
    Ok(home.unwrap_or_else(|| "USD".into()).to_uppercase())
}

/// Whether a payout was already fetched for this feed.
pub(crate) async fn seen(pool: &SqlitePool, feed_id: i64, payout_id: &str) -> Result<bool, String> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM payouts WHERE feed_id = ?1 AND processor_payout_id = ?2)")
        .bind(feed_id)
        .bind(payout_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

/// Keeps a payout's breakdown, NEW; false when it was already kept (by a fetch running at the same
/// time: the automatic one and Get payouts can overlap).
pub(crate) async fn keep(pool: &SqlitePool, feed_id: i64, payout_id: &str, arrival: NaiveDate, currency: &str, b: &Breakdown) -> Result<bool, String> {
    sqlx::query(
        "INSERT INTO payouts (feed_id, processor_payout_id, arrival_date, currency, net_cents, gross_cents, fee_cents, refund_cents, chargeback_cents, other_cents, details_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT (feed_id, processor_payout_id) DO NOTHING",
    )
    .bind(feed_id)
    .bind(payout_id)
    .bind(arrival)
    .bind(currency)
    .bind(b.net_cents)
    .bind(b.gross_cents)
    .bind(b.fee_cents)
    .bind(b.refund_cents)
    .bind(b.chargeback_cents)
    .bind(b.other_cents)
    .bind(serde_json::to_string(&b.items).map_err(|e| e.to_string())?)
    .execute(pool)
    .await
    .map_err(db_err)
    .map(|done| done.rows_affected() == 1)
}

/// The reason part of an error ("PAYOUT_MISMATCH: the payout's…" → "the payout's…").
pub(crate) fn reason(e: &str) -> &str {
    e.split_once(": ").map_or(e, |(_, m)| m)
}

async fn fetch_stripe(pool: &SqlitePool, f: &PayoutFeed, api: &str, key: &str, since: DateTime<Utc>, now: DateTime<Utc>) -> Result<FetchResult, String> {
    let feed_id = f.id;
    let client = client()?;
    let payouts = stripe_list(&client, api, key, "/v1/payouts", &[("status", "paid".into()), ("arrival_date[gte]", since.timestamp().to_string())]).await?;
    let home = home_currency(pool).await?;
    let mut result = FetchResult { new_payouts: 0, refused: Vec::new() };
    for p in payouts {
        let id = text(&p, "id");
        if seen(pool, feed_id, &id).await? {
            continue;
        }
        let currency = text(&p, "currency").to_uppercase();
        let arrival = DateTime::from_timestamp(int(&p, "arrival_date"), 0).map_or_else(|| now.date_naive(), |t| t.date_naive());
        if currency != home {
            result.refused.push(format!("{id}: paid in {currency}, not {home}"));
            continue;
        }
        let transactions = stripe_list(&client, api, key, "/v1/balance_transactions", &[("payout", id.clone())]).await?;
        match stripe_breakdown(int(&p, "amount"), &transactions) {
            Ok(b) => {
                if keep(pool, feed_id, &id, arrival, &currency, &b).await? {
                    result.new_payouts += 1;
                }
            }
            Err(e) => result.refused.push(format!("{id}: {}", reason(&e))),
        }
    }
    Ok(result)
}

pub async fn list_payouts(pool: &SqlitePool, feed_id: i64) -> Result<Vec<Payout>, String> {
    sqlx::query_as(
        "SELECT id, feed_id, processor_payout_id, arrival_date, currency, net_cents, gross_cents, fee_cents, refund_cents, chargeback_cents, other_cents, details_json, status, deposit_id
         FROM payouts WHERE feed_id = ?1 ORDER BY (status = 'NEW') DESC, arrival_date DESC, id DESC",
    )
    .bind(feed_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Records a new payout as a deposit, in one write transaction with marking it recorded.
pub async fn record(pool: &SqlitePool, payout_id: i64) -> Result<i64, String> {
    let p: Payout = sqlx::query_as(
        "SELECT id, feed_id, processor_payout_id, arrival_date, currency, net_cents, gross_cents, fee_cents, refund_cents, chargeback_cents, other_cents, details_json, status, deposit_id
         FROM payouts WHERE id = ?1",
    )
    .bind(payout_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or_else(|| "PAYOUT_NOT_FOUND: that payout is gone".to_string())?;
    if p.status != "NEW" {
        return Err(format!("PAYOUT_DONE: this payout was already {}", p.status.to_lowercase()));
    }
    let f = feed(pool, p.feed_id).await?;
    let items: Vec<PayoutItem> = serde_json::from_str(&p.details_json).map_err(|_| "PAYOUT_DAMAGED: the payout's details can't be read".to_string())?;
    let b = Breakdown {
        net_cents: p.net_cents,
        gross_cents: p.gross_cents,
        fee_cents: p.fee_cents,
        refund_cents: p.refund_cents,
        chargeback_cents: p.chargeback_cents,
        other_cents: p.other_cents,
        items,
    };
    let input = deposit_for(&f, p.arrival_date, &p.processor_payout_id, &b);
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result: Result<i64, String> = async {
        let (deposit_id, _) = crate::banking::write_deposit(&mut tx, &input).await?;
        let done = sqlx::query("UPDATE payouts SET status = 'RECORDED', deposit_id = ?1 WHERE id = ?2 AND status = 'NEW'")
            .bind(deposit_id)
            .bind(payout_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        if done.rows_affected() != 1 {
            return Err("PAYOUT_DONE: this payout was recorded meanwhile".into());
        }
        Ok(deposit_id)
    }
    .await;
    match result {
        Ok(id) => crate::ledger::commit_write(tx).await.map(|()| id).map_err(db_err),
        Err(e) => Err(crate::ledger::rollback(tx, e).await),
    }
}

/// Leaves a new payout out (it was entered by hand), or brings an ignored one back.
pub async fn set_ignored(pool: &SqlitePool, payout_id: i64, ignored: bool) -> Result<(), String> {
    let (from, to) = if ignored { ("NEW", "IGNORED") } else { ("IGNORED", "NEW") };
    let done =
        sqlx::query("UPDATE payouts SET status = ?1 WHERE id = ?2 AND status = ?3").bind(to).bind(payout_id).bind(from).execute(pool).await.map_err(db_err)?;
    if done.rows_affected() != 1 {
        return Err("PAYOUT_DONE: that payout can't be changed now".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
