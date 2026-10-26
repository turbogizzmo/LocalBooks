//! Suggested exchange rates: the European Central Bank's daily reference rates, looked up through
//! the Frankfurter API (<https://github.com/lineofflight/frankfurter>) and cached in the company file.
//!
//! Off unless the `exchange_rate_lookup` preference is on. A lookup sends only the two currency
//! codes and a date. Each answer is stored in `exchange_rate_cache` (migration 0052), so a date is
//! fetched at most once and later lookups work offline. The ECB publishes on business days only;
//! Frankfurter answers a weekend or holiday with the most recent rate before it, and the cache
//! remembers which requested dates that rate covers.
//!
//! A suggestion only pre-fills the rate field. Transactions store the rate they were saved with,
//! and nothing here reads or changes saved transactions.

use std::future::Future;
use std::time::Duration;

use chrono::{Days, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use sqlx::SqlitePool;

use crate::ledger::db_err;

pub const PREF_KEY: &str = "exchange_rate_lookup";
pub const SOURCE: &str = "ECB via Frankfurter";
pub const API_BASE: &str = "https://api.frankfurter.dev/v1";

/// Currencies the ECB publishes reference rates for (Frankfurter's `/currencies`). Checking here
/// answers an unsupported currency at once, without a network request.
pub const SUPPORTED: &[&str] = &[
    "AUD", "BRL", "CAD", "CHF", "CNY", "CZK", "DKK", "EUR", "GBP", "HKD", "HUF", "IDR", "ILS", "INR", "ISK", "JPY", "KRW", "MXN", "MYR", "NOK", "NZD", "PHP",
    "PLN", "RON", "SEK", "SGD", "THB", "TRY", "USD", "ZAR",
];

/// The first day of ECB reference rates.
fn first_rate_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(1999, 1, 4).expect("valid date")
}

/// An offline lookup may fall back to a cached rate at most this many days older than the date.
const STALE_DAYS: u64 = 7;

// ------------------------------------------------------------------------------------------------
// Decimal parsing
// ------------------------------------------------------------------------------------------------

/// Parses a non-negative JSON number (`1.0942`, `0.0068744`, `6.4e-05`) to an integer scaled by
/// 1,000,000, rounding half up, using digit arithmetic only (no floating point).
pub fn parse_decimal_e6(text: &str) -> Option<i64> {
    let t = text.trim().trim_matches('"');
    let (mantissa, exp) = match t.find(['e', 'E']) {
        Some(i) => {
            let e = &t[i + 1..];
            let digits = e.strip_prefix(['+', '-']).unwrap_or(e);
            if digits.is_empty() || digits.len() > 3 || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            (&t[..i], e.parse::<i64>().ok()?)
        }
        None => (t, 0),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if int.is_empty() || (mantissa.contains('.') && frac.is_empty()) || !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits: Vec<u8> = int.bytes().chain(frac.bytes()).map(|b| b - b'0').collect();
    // How many leading digits form the integer part of the scaled value.
    let whole_len = int.len() as i64 + exp + 6;
    let mut value: i64 = 0;
    for i in 0..whole_len.max(0) {
        let digit = digits.get(i as usize).copied().unwrap_or(0);
        value = value.checked_mul(10)?.checked_add(i64::from(digit))?;
    }
    let next = if whole_len < 0 { 0 } else { digits.get(whole_len as usize).copied().unwrap_or(0) };
    if next >= 5 {
        value = value.checked_add(1)?;
    }
    Some(value)
}

// ------------------------------------------------------------------------------------------------
// HTTP
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    Timeout,
    Network(String),
}

/// The one HTTP GET the lookup needs; tests substitute a canned implementation.
pub trait RateHttp: Sync {
    fn get(&self, url: &str) -> impl Future<Output = Result<HttpResponse, HttpError>> + Send;
}

/// The real client, managed as app state.
pub struct ReqwestHttp {
    client: reqwest::Client,
}

impl ReqwestHttp {
    pub fn new() -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("LocalBooks/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::limited(3))
            .build()
            .map_err(|e| format!("FX_CLIENT: {e}"))?;
        Ok(ReqwestHttp { client })
    }
}

impl RateHttp for ReqwestHttp {
    async fn get(&self, url: &str) -> Result<HttpResponse, HttpError> {
        let classify = |e: reqwest::Error| if e.is_timeout() { HttpError::Timeout } else { HttpError::Network(e.without_url().to_string()) };
        let response = self.client.get(url).send().await.map_err(classify)?;
        let status = response.status().as_u16();
        let body = response.text().await.map_err(classify)?;
        Ok(HttpResponse { status, body })
    }
}

// ------------------------------------------------------------------------------------------------
// Lookup
// ------------------------------------------------------------------------------------------------

/// A suggested rate: one `currency` unit is worth `rate_e6 / 1,000,000` units of `home_currency`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RateSuggestion {
    pub currency: String,
    pub home_currency: String,
    pub rate_e6: i64,
    /// The business day the rate was published (on or before `requested_date`).
    pub rate_date: NaiveDate,
    pub requested_date: NaiveDate,
    pub source: String,
    /// Served from the cache, without a network request.
    pub from_cache: bool,
    /// The service could not be reached, so this is the most recent cached rate before the date.
    pub stale: bool,
}

#[derive(Deserialize)]
struct ApiResponse<'a> {
    base: String,
    date: NaiveDate,
    #[serde(borrow)]
    rates: std::collections::BTreeMap<String, &'a RawValue>,
}

#[derive(Deserialize)]
struct ApiMessage {
    message: Option<String>,
}

fn check_code(code: &str) -> Result<String, String> {
    let code = code.trim().to_ascii_uppercase();
    if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(format!("FX_BAD_CURRENCY: '{code}' is not a three-letter currency code"));
    }
    if !SUPPORTED.contains(&code.as_str()) {
        return Err(format!("FX_UNSUPPORTED: the European Central Bank publishes no rate for {code}; enter the rate yourself"));
    }
    Ok(code)
}

pub async fn lookup_enabled(pool: &SqlitePool) -> Result<bool, String> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM system_preferences WHERE key = ?1").bind(PREF_KEY).fetch_optional(pool).await.map_err(db_err)?;
    Ok(value.as_deref() == Some("true"))
}

/// A cached rate answering `date`: published on or before it, and known to be the latest as of it.
async fn cached(pool: &SqlitePool, base: &str, quote: &str, date: NaiveDate, earliest: Option<NaiveDate>) -> Result<Option<(i64, NaiveDate, String)>, String> {
    let covering = earliest.is_none();
    sqlx::query_as(
        "SELECT rate_e6, rate_date, source FROM exchange_rate_cache
         WHERE base = ?1 AND quote = ?2 AND rate_date <= ?3 AND (?4 OR rate_date >= ?5) AND (NOT ?4 OR covers_through >= ?3)
         ORDER BY rate_date DESC LIMIT 1",
    )
    .bind(base)
    .bind(quote)
    .bind(date)
    .bind(covering)
    .bind(earliest.unwrap_or(date))
    .fetch_optional(pool)
    .await
    .map_err(db_err)
}

/// Suggests the rate for one `currency` in `home_currency` on `date` (a future date uses today's).
/// `today` is the local date, passed in so tests are deterministic.
pub async fn suggest<H: RateHttp>(pool: &SqlitePool, http: &H, currency: &str, home_currency: &str, date: NaiveDate, today: NaiveDate) -> Result<RateSuggestion, String> {
    if !lookup_enabled(pool).await? {
        return Err("FX_LOOKUP_OFF: turn on “Look up daily exchange rates” in Settings › Preferences Console to suggest rates".into());
    }
    let base = check_code(currency)?;
    let quote = check_code(home_currency)?;
    if base == quote {
        return Err(format!("FX_SAME_CURRENCY: {base} is the home currency"));
    }
    let requested = date.min(today);
    if requested < first_rate_date() {
        return Err("FX_TOO_EARLY: ECB reference rates start on January 4, 1999; enter the rate yourself".into());
    }
    let suggestion = |rate_e6, rate_date, source, from_cache, stale| RateSuggestion {
        currency: base.clone(),
        home_currency: quote.clone(),
        rate_e6,
        rate_date,
        requested_date: requested,
        source,
        from_cache,
        stale,
    };

    if let Some((rate_e6, rate_date, source)) = cached(pool, &base, &quote, requested, None).await? {
        return Ok(suggestion(rate_e6, rate_date, source, true, false));
    }

    let url = format!("{API_BASE}/{requested}?from={base}&to={quote}");
    let response = match http.get(&url).await {
        Ok(response) => response,
        Err(err) => {
            let earliest = requested.checked_sub_days(Days::new(STALE_DAYS)).unwrap_or(requested);
            if let Some((rate_e6, rate_date, source)) = cached(pool, &base, &quote, requested, Some(earliest)).await? {
                return Ok(suggestion(rate_e6, rate_date, source, true, true));
            }
            let why = match err {
                HttpError::Timeout => "it did not answer in time".to_string(),
                HttpError::Network(text) => text,
            };
            return Err(format!("FX_OFFLINE: could not reach the exchange-rate service ({why}); enter the rate yourself"));
        }
    };
    match response.status {
        200 => {}
        404 | 422 => {
            let message = serde_json::from_str::<ApiMessage>(&response.body).ok().and_then(|m| m.message).unwrap_or_default();
            return Err(format!("FX_UNAVAILABLE: no ECB rate for {base} in {quote} on {requested} ({message}); enter the rate yourself"));
        }
        status => return Err(format!("FX_SERVICE: the exchange-rate service answered with HTTP {status}; enter the rate yourself")),
    }
    let bad = || "FX_BAD_RESPONSE: the exchange-rate service sent an answer LocalBooks could not read; enter the rate yourself".to_string();
    let parsed: ApiResponse = serde_json::from_str(&response.body).map_err(|_| bad())?;
    let raw = parsed.rates.get(&quote).ok_or_else(bad)?;
    if parsed.base != base || parsed.date > requested || parsed.date < first_rate_date() {
        return Err(bad());
    }
    let rate_e6 = parse_decimal_e6(raw.get()).ok_or_else(bad)?;
    if rate_e6 <= 0 {
        return Err(format!("FX_RATE_TOO_SMALL: one {base} is worth less than 0.000001 {quote}; enter the rate yourself"));
    }
    // A past date's rate is final. Today's may not be published yet, so an earlier answer for today
    // is not cached as covering it; the next lookup asks again.
    let covers_through = if requested < today || parsed.date == requested { requested } else { parsed.date };
    sqlx::query(
        "INSERT INTO exchange_rate_cache (base, quote, rate_date, covers_through, rate_e6, source) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (base, quote, rate_date) DO UPDATE SET
             covers_through = max(covers_through, excluded.covers_through), rate_e6 = excluded.rate_e6, source = excluded.source,
             fetched_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(&base)
    .bind(&quote)
    .bind(parsed.date)
    .bind(covers_through)
    .bind(rate_e6)
    .bind(SOURCE)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(suggestion(rate_e6, parsed.date, SOURCE.to_string(), false, false))
}

#[cfg(test)]
mod tests;
