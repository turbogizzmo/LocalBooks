//! PayPal payouts (gaps row T68; design in `docs/plans/payout-feeds.md`).
//!
//! PayPal has no payout object like Stripe's: the balance collects payments, fees, refunds and
//! chargebacks, and a **withdrawal** to the bank (event codes T0400–T0403) takes it out. Each
//! withdrawal is a payout here; what it carries is every balance-affecting transaction after the
//! withdrawal before it, read through PayPal's Transaction Search API. Those add up to the
//! withdrawal only when it took the whole balance; a withdrawal of part of the balance doesn't tie,
//! and is refused (`PAYOUT_MISMATCH`) rather than recorded wrong.
//!
//! The credentials are a PayPal REST app's client id and secret, used only to get a short-lived
//! token with the Transaction Search read scope. They live in the credential store
//! ([`crate::secrets`]) as one JSON value, never in the company file or a log.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

use super::{api_url, check_accounts, client, feed, hex, home_currency, keep, reason, seen, text, Breakdown, FeedInput, FetchResult, PayoutFeed, PayoutItem};
use crate::ledger::{db_err, format_cents};
use crate::secrets::SecretStore;

pub const LIVE_API: &str = "https://api-m.paypal.com";
pub const SANDBOX_API: &str = "https://api-m.sandbox.paypal.com";
/// Transaction Search answers at most 31 days per request.
const WINDOW_DAYS: i64 = 31;
/// How far before the fetch window to read, to find the withdrawal before the first new one.
const LOOKBACK_DAYS: i64 = 31;
const PAGE_SIZE: usize = 500;
/// Most pages read for one window (250,000 transactions); past that the fetch is refused.
const MAX_PAGES: i64 = 500;

/// A PayPal REST app's credentials, as the user pastes them. Its `Debug` hides the secret.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayPalCredentials {
    pub client_id: String,
    pub secret: String,
    /// Sandbox app credentials (for trying it out) rather than live ones.
    #[serde(default)]
    pub sandbox: bool,
}

impl std::fmt::Debug for PayPalCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PayPalCredentials").field("client_id", &self.client_id).field("secret", &"…").field("sandbox", &self.sandbox).finish()
    }
}

impl Drop for PayPalCredentials {
    /// The secret doesn't linger in memory after use.
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.secret.zeroize();
    }
}

/// "12.34", "-0.59", "1000" or "7.5" in cents. More than two decimals, or anything else, is refused:
/// PayPal reports two-decimal currencies, and a home currency without cents isn't fetched.
pub fn parse_cents(value: &str) -> Result<i64, String> {
    let bad = || format!("PAYOUT_RESPONSE: PayPal sent an amount Tillhouse can't read ({value})");
    let v = value.trim();
    let (negative, digits) = match v.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, v.strip_prefix('+').unwrap_or(v)),
    };
    let (whole, frac) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) || frac.len() > 2 || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let whole: i64 = whole.parse().map_err(|_| bad())?;
    let frac: i64 = if frac.is_empty() { 0 } else { format!("{frac:0<2}").parse().map_err(|_| bad())? };
    let cents = whole.checked_mul(100).and_then(|c| c.checked_add(frac)).ok_or_else(bad)?;
    Ok(if negative { -cents } else { cents })
}

/// One balance-affecting transaction, as LocalBooks reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayPalTxn {
    pub id: String,
    pub code: String,
    pub at: DateTime<Utc>,
    pub currency: String,
    /// Signed: money in is positive.
    pub amount_cents: i64,
    /// Signed as PayPal reports it: a fee taken is negative, a fee returned positive.
    pub fee_cents: i64,
    pub description: String,
}

/// A withdrawal to the bank: what makes a payout.
fn is_withdrawal(t: &PayPalTxn) -> bool {
    matches!(t.code.as_str(), "T0400" | "T0401" | "T0402" | "T0403") && t.amount_cents < 0
}

/// PayPal's dates come as `2026-11-01T10:00:00+0000` or with a colon in the offset.
fn parse_time(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s).or_else(|_| DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%z")).ok().map(|t| t.with_timezone(&Utc))
}

/// Reads one `transaction_details` entry; `None` for one that doesn't move the balance (pending,
/// denied or reversed status).
pub fn read_txn(detail: &Value) -> Result<Option<PayPalTxn>, String> {
    let info = detail.get("transaction_info").unwrap_or(&Value::Null);
    if matches!(text(info, "transaction_status").as_str(), "P" | "D" | "V") {
        return Ok(None);
    }
    let money = |key: &str| -> Result<(i64, String), String> {
        match info.get(key) {
            Some(m) => Ok((parse_cents(m.get("value").and_then(Value::as_str).unwrap_or("0"))?, text(m, "currency_code").to_uppercase())),
            None => Ok((0, String::new())),
        }
    };
    let (amount_cents, currency) = money("transaction_amount")?;
    let (fee_cents, _) = money("fee_amount")?;
    let at = parse_time(&text(info, "transaction_initiation_date")).ok_or("PAYOUT_RESPONSE: PayPal sent a transaction without a date Tillhouse can read")?;
    let subject = text(info, "transaction_subject");
    let note = text(info, "transaction_note");
    let description = [subject, note].into_iter().find(|s| !s.trim().is_empty()).unwrap_or_default();
    Ok(Some(PayPalTxn { id: text(info, "transaction_id"), code: text(info, "transaction_event_code"), at, currency, amount_cents, fee_cents, description }))
}

/// The breakdown of a withdrawal of `net_cents` (positive) from the transactions since the one
/// before it. Each transaction's amount and fee are both signed, so their sum is what it did to the
/// balance; they must add up to the withdrawal, or it is refused.
pub fn paypal_breakdown(net_cents: i64, txns: &[PayPalTxn]) -> Result<Breakdown, String> {
    let overflow = || "PAYOUT_TOO_LARGE: the payout's amounts are out of range".to_string();
    let add = |a: i64, b: i64| a.checked_add(b).ok_or_else(overflow);
    let mut b = Breakdown { net_cents, gross_cents: 0, fee_cents: 0, refund_cents: 0, chargeback_cents: 0, other_cents: 0, items: Vec::new() };
    for t in txns {
        // A fee taken is negative in PayPal's figures; a cost is positive here.
        b.fee_cents = add(b.fee_cents, -t.fee_cents)?;
        let described = |fallback: &str| if t.description.trim().is_empty() { format!("{fallback} {}", t.id) } else { t.description.clone() };
        let code = t.code.as_str();
        match code {
            // Payments received.
            _ if code.starts_with("T00") && t.amount_cents > 0 => {
                b.gross_cents = add(b.gross_cents, t.amount_cents)?;
                b.items.push(PayoutItem { kind: "CHARGE".into(), description: described("PayPal payment"), amount_cents: t.amount_cents });
            }
            // Fees that aren't part of a payment, and fees given back.
            _ if code.starts_with("T01") => b.fee_cents = add(b.fee_cents, -t.amount_cents)?,
            "T1108" | "T1109" => b.fee_cents = add(b.fee_cents, -t.amount_cents)?,
            // Refunds.
            "T1107" | "T1115" | "T1120" => {
                b.refund_cents = add(b.refund_cents, -t.amount_cents)?;
                b.items.push(PayoutItem { kind: "REFUND".into(), description: described("Refund"), amount_cents: -t.amount_cents });
            }
            // Reversals and chargebacks, and chargebacks won back.
            "T1106" | "T1201" | "T1202" | "T1205" => {
                b.chargeback_cents = add(b.chargeback_cents, -t.amount_cents)?;
                b.items.push(PayoutItem { kind: "CHARGEBACK".into(), description: described("Chargeback"), amount_cents: -t.amount_cents });
            }
            // Holds and their releases, payments sent, conversions and anything else.
            _ => b.other_cents = add(b.other_cents, t.amount_cents)?,
        }
    }
    let worked_out = b.gross_cents - b.fee_cents - b.refund_cents - b.chargeback_cents + b.other_cents;
    if worked_out != net_cents {
        return Err(format!(
            "PAYOUT_MISMATCH: the PayPal activity since the last withdrawal adds up to {}, not the {} withdrawn (a withdrawal of part of the balance can't be broken down); it can't be recorded",
            format_cents(worked_out),
            format_cents(net_cents)
        ));
    }
    Ok(b)
}

/// One payout: a withdrawal and what it carried.
#[derive(Debug, Clone)]
pub struct Withdrawal {
    pub txn: PayPalTxn,
    /// `None` when the withdrawal before it isn't in the history read, so what it carried is unknown.
    pub carried: Option<Vec<PayPalTxn>>,
}

/// Splits time-ordered transactions at each withdrawal.
pub fn withdrawals(mut txns: Vec<PayPalTxn>) -> Vec<Withdrawal> {
    // In the same second as a withdrawal, the other transactions come first: they are what it took.
    txns.sort_by_key(|t| (t.at, is_withdrawal(t)));
    let mut out = Vec::new();
    let mut since_last: Vec<PayPalTxn> = Vec::new();
    let mut seen_one = false;
    for t in txns {
        if is_withdrawal(&t) {
            let carried = std::mem::take(&mut since_last);
            out.push(Withdrawal { txn: t, carried: seen_one.then_some(carried) });
            seen_one = true;
        } else {
            since_last.push(t);
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------
// The API
// ------------------------------------------------------------------------------------------------

/// A short-lived access token for the app's credentials.
async fn token(http: &reqwest::Client, api: &str, creds: &PayPalCredentials) -> Result<String, String> {
    let url = api_url(api, "/v1/oauth2/token")?;
    let response = http
        .post(url)
        .basic_auth(creds.client_id.trim(), Some(creds.secret.trim()))
        .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body("grant_type=client_credentials")
        .send()
        .await
        .map_err(|e| format!("PAYOUT_NETWORK: PayPal couldn't be reached ({})", e.without_url()))?;
    let status = response.status();
    let body: Value = response.json().await.map_err(|_| "PAYOUT_RESPONSE: PayPal's answer couldn't be read".to_string())?;
    if !status.is_success() {
        return Err(match status.as_u16() {
            400 | 401 => {
                "PAYOUT_KEY: PayPal didn't accept the client id and secret; check them, and that they are for the right environment (live or sandbox)".into()
            }
            _ => format!("PAYOUT_REFUSED: PayPal said: {}", text(&body, "error_description")),
        });
    }
    body.get("access_token").and_then(Value::as_str).map(str::to_string).ok_or_else(|| "PAYOUT_RESPONSE: PayPal sent no access token".to_string())
}

/// One page of Transaction Search.
async fn search(http: &reqwest::Client, api: &str, token: &str, start: DateTime<Utc>, end: DateTime<Utc>, page: i64) -> Result<Value, String> {
    let mut url = api_url(api, "/v1/reporting/transactions")?;
    url.query_pairs_mut()
        .append_pair("start_date", &start.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .append_pair("end_date", &end.format("%Y-%m-%dT%H:%M:%SZ").to_string())
        .append_pair("fields", "transaction_info")
        .append_pair("page_size", &PAGE_SIZE.to_string())
        .append_pair("page", &page.to_string());
    let response = http.get(url).bearer_auth(token).send().await.map_err(|e| format!("PAYOUT_NETWORK: PayPal couldn't be reached ({})", e.without_url()))?;
    let status = response.status();
    let body: Value = response.json().await.map_err(|_| "PAYOUT_RESPONSE: PayPal's answer couldn't be read".to_string())?;
    if !status.is_success() {
        let message = [text(&body, "message"), text(&body, "error_description"), text(&body, "name")]
            .into_iter()
            .find(|m| !m.is_empty())
            .unwrap_or_else(|| "request refused".into());
        return Err(match status.as_u16() {
            401 => "PAYOUT_KEY: PayPal didn't accept the app's credentials".into(),
            403 => format!("PAYOUT_PERMISSION: the app can't search transactions ({message}); turn on Transaction search for it at developer.paypal.com"),
            _ => format!("PAYOUT_REFUSED: PayPal said: {message}"),
        });
    }
    Ok(body)
}

/// Every balance-affecting transaction from `start` to `end`, 31 days per request, page by page.
async fn transactions(http: &reqwest::Client, api: &str, token: &str, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<PayPalTxn>, String> {
    let mut out = Vec::new();
    let mut from = start;
    while from < end {
        let to = (from + Duration::days(WINDOW_DAYS)).min(end);
        let mut page = 1;
        loop {
            let body = search(http, api, token, from, to, page).await?;
            for d in body.get("transaction_details").and_then(Value::as_array).cloned().unwrap_or_default() {
                // One LocalBooks can't read is left out: the withdrawal it belongs to then doesn't
                // add up and is refused, while the others still come in.
                if let Ok(Some(t)) = read_txn(&d) {
                    out.push(t);
                }
            }
            let pages = body.get("total_pages").and_then(Value::as_i64).unwrap_or(1);
            if page >= pages {
                break;
            }
            page += 1;
            if page > MAX_PAGES {
                return Err("PAYOUT_TOO_MANY: PayPal has more than Tillhouse reads in one go; get payouts more often so each fetch is smaller".into());
            }
        }
        from = to;
    }
    // A transaction on a window's edge can come back twice.
    out.sort_by(|a, b| (a.at, &a.id, &a.code).cmp(&(b.at, &b.id, &b.code)));
    out.dedup_by(|a, b| a.id == b.id && a.code == b.code && !a.id.is_empty());
    Ok(out)
}

/// Connects a PayPal app: checks the credentials with a token and one search, keeps them in the
/// credential store, and saves the feed with its accounts.
pub async fn connect_paypal(
    pool: &SqlitePool,
    store: &SecretStore,
    api: &str,
    creds: &PayPalCredentials,
    input: &FeedInput,
    now: DateTime<Utc>,
) -> Result<PayoutFeed, String> {
    if creds.client_id.trim().is_empty() || creds.secret.trim().is_empty() {
        return Err("PAYOUT_KEY: paste the PayPal app's client id and secret".into());
    }
    check_accounts(pool, input).await?;
    let http = client()?;
    let token = token(&http, api, creds).await?;
    search(&http, api, &token, now - Duration::days(1), now, 1).await?;
    let secret_key = format!("paypal-{}", hex(&crate::crypto::random::<8>()?));
    let stored =
        serde_json::to_string(&PayPalCredentials { client_id: creds.client_id.trim().into(), secret: creds.secret.trim().into(), sandbox: creds.sandbox })
            .map_err(|e| e.to_string())?;
    store.put(&secret_key, &stored).await?;
    let id: Result<i64, String> = sqlx::query_scalar(
        "INSERT INTO payout_feeds (processor, name, secret_key, account_ref, bank_account_id, sales_account_id, fee_account_id, refund_account_id, chargeback_account_id)
         VALUES ('PAYPAL', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING id",
    )
    .bind(input.name.trim())
    .bind(&secret_key)
    .bind(if creds.sandbox { "SANDBOX" } else { "LIVE" })
    .bind(input.bank_account_id)
    .bind(input.sales_account_id)
    .bind(input.fee_account_id)
    .bind(input.refund_account_id)
    .bind(input.chargeback_account_id)
    .fetch_one(pool)
    .await
    .map_err(db_err);
    match id {
        Ok(id) => feed(pool, id).await,
        Err(e) => {
            // Don't leave the credentials behind for a feed that wasn't saved.
            let _ = store.delete(&secret_key).await;
            Err(e)
        }
    }
}

/// Fetches the withdrawals since `since` (and a month before, to find what the first one carried)
/// and keeps the ones not seen before.
pub(super) async fn fetch(pool: &SqlitePool, f: &PayoutFeed, api: &str, stored: &str, since: DateTime<Utc>, now: DateTime<Utc>) -> Result<FetchResult, String> {
    let creds: PayPalCredentials =
        serde_json::from_str(stored).map_err(|_| "PAYOUT_KEY_MISSING: the feed's PayPal credentials can't be read; connect it again".to_string())?;
    let http = client()?;
    let token = token(&http, api, &creds).await?;
    let txns = transactions(&http, api, &token, since - Duration::days(LOOKBACK_DAYS), now).await?;
    let home = home_currency(pool).await?;
    let mut result = FetchResult { new_payouts: 0, refused: Vec::new() };
    for w in withdrawals(txns) {
        if w.txn.at < since || seen(pool, f.id, &w.txn.id).await? {
            continue;
        }
        let id = w.txn.id.clone();
        if w.txn.currency != home {
            result.refused.push(format!("{id}: paid in {}, not {home}", w.txn.currency));
            continue;
        }
        let Some(mut carried) = w.carried else {
            result.refused.push(format!("{id}: the withdrawal before it is more than a month earlier, so what it carried isn't known; enter it by hand"));
            continue;
        };
        // Only what moved the withdrawal's own currency: a payment in another currency counts as
        // what its conversion brought in (an adjustment), never as cents of a different currency.
        carried.retain(|t| t.currency == w.txn.currency);
        // The bank gets what was withdrawn; a fee PayPal took for the withdrawal is one more fee.
        let net = -w.txn.amount_cents;
        if w.txn.fee_cents != 0 {
            carried.push(PayPalTxn { code: "WITHDRAWAL_FEE".into(), amount_cents: 0, ..w.txn.clone() });
        }
        match paypal_breakdown(net, &carried) {
            Ok(b) => {
                let arrival: NaiveDate = w.txn.at.date_naive();
                if keep(pool, f.id, &id, arrival, &home, &b).await? {
                    result.new_payouts += 1;
                }
            }
            Err(e) => result.refused.push(format!("{id}: {}", reason(&e))),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
