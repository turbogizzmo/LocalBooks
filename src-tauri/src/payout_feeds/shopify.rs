//! Shopify Payments payouts (gaps row T68; design in `docs/plans/payout-feeds.md`).
//!
//! A store's custom app, given only the read scopes for Shopify Payments payouts, has an Admin API
//! access token; LocalBooks keeps it in the credential store ([`crate::secrets`]) and the store's
//! `….myshopify.com` address in `payout_feeds.account_ref`. Payouts and their balance transactions
//! are read with the GraphQL Admin API (the REST API is legacy). A payout's breakdown comes from the
//! balance transactions that name it, and must add up to its net amount or it is refused.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use super::paypal::parse_cents;
use super::{api_url, check_accounts, client, feed, hex, home_currency, keep, reason, seen, text, Breakdown, FeedInput, FetchResult, PayoutFeed, PayoutItem};
use crate::ledger::{db_err, format_cents};
use crate::secrets::SecretStore;

/// The Admin API version LocalBooks speaks (types it doesn't know come back as ADJUSTMENT).
pub const API_VERSION: &str = "2026-10";
const PAGE: usize = 100;
/// Most pages read in one fetch; past that the fetch is refused, not cut short.
const MAX_PAGES: usize = 200;
/// How long before a payout its transactions may have happened.
const TRANSACTIONS_BEFORE_DAYS: i64 = 60;

/// The store's address as typed ("my-store", "my-store.myshopify.com", or a URL) as
/// `my-store.myshopify.com`; anything else is refused.
pub fn shop_domain(typed: &str) -> Result<String, String> {
    let t = typed.trim().to_lowercase();
    let t = t.trim_start_matches("https://").trim_start_matches("http://");
    let host = t.split('/').next().unwrap_or("");
    let name = host.strip_suffix(".myshopify.com").unwrap_or(host);
    if name.is_empty() || name.len() > 100 || !name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') || name.starts_with('-') {
        return Err("PAYOUT_SHOP: enter the store's address, such as my-store.myshopify.com".into());
    }
    Ok(format!("{name}.myshopify.com"))
}

/// The GraphQL endpoint: the store itself, or a loopback server under test.
fn endpoint(api: Option<&str>, shop: &str) -> String {
    match api {
        Some(base) => base.trim_end_matches('/').to_string(),
        None => format!("https://{shop}"),
    }
}

async fn graphql(http: &reqwest::Client, base: &str, token: &str, query: &str, variables: Value) -> Result<Value, String> {
    let url = api_url(base, &format!("/admin/api/{API_VERSION}/graphql.json"))?;
    let response = http
        .post(url)
        .header("X-Shopify-Access-Token", token)
        .json(&json!({ "query": query, "variables": variables }))
        .send()
        .await
        .map_err(|e| format!("PAYOUT_NETWORK: Shopify couldn't be reached ({})", e.without_url()))?;
    let status = response.status();
    if status.as_u16() == 401 {
        return Err("PAYOUT_KEY: Shopify didn't accept the access token; check it, or install the app again".into());
    }
    if status.as_u16() == 404 {
        return Err("PAYOUT_SHOP: there is no Shopify store at that address".into());
    }
    let body: Value = response.json().await.map_err(|_| "PAYOUT_RESPONSE: Shopify's answer couldn't be read".to_string())?;
    if !status.is_success() {
        return Err(format!("PAYOUT_REFUSED: Shopify answered {status}"));
    }
    if let Some(errors) = body.get("errors").and_then(Value::as_array).filter(|e| !e.is_empty()) {
        let code = errors[0].pointer("/extensions/code").and_then(Value::as_str).unwrap_or("");
        let message = text(&errors[0], "message");
        return Err(match code {
            "ACCESS_DENIED" => format!("PAYOUT_PERMISSION: the app can't read payouts ({message}); give it the read_shopify_payments_payouts and read_shopify_payments_accounts scopes"),
            "THROTTLED" => "PAYOUT_BUSY: Shopify asked LocalBooks to slow down; try again in a minute".into(),
            _ => format!("PAYOUT_REFUSED: Shopify said: {message}"),
        });
    }
    Ok(body.get("data").cloned().unwrap_or(Value::Null))
}

fn money(v: &Value, key: &str) -> Result<i64, String> {
    parse_shopify_amount(v.pointer(&format!("/{key}/amount")).and_then(Value::as_str).unwrap_or("0"))
}

/// Shopify's amounts are decimal strings such as "41.9" or "-12.00".
pub fn parse_shopify_amount(s: &str) -> Result<i64, String> {
    parse_cents(s).map_err(|_| format!("PAYOUT_RESPONSE: Shopify sent an amount LocalBooks can't read ({s})"))
}

/// One balance transaction, as LocalBooks reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopifyTxn {
    pub kind: String,
    pub at: DateTime<Utc>,
    /// Gross, signed: money in is positive.
    pub amount_cents: i64,
    /// What Shopify took, positive (net = amount − fee).
    pub fee_cents: i64,
    pub payout_id: Option<String>,
    pub order: String,
}

pub fn read_txn(node: &Value) -> Result<ShopifyTxn, String> {
    let at = DateTime::parse_from_rfc3339(&text(node, "transactionDate")).map(|t| t.with_timezone(&Utc)).map_err(|_| "PAYOUT_RESPONSE: Shopify sent a transaction without a date LocalBooks can read".to_string())?;
    Ok(ShopifyTxn {
        kind: text(node, "type"),
        at,
        amount_cents: money(node, "amount")?,
        fee_cents: money(node, "fee")?,
        payout_id: node.pointer("/associatedPayout/id").and_then(Value::as_str).map(str::to_string),
        order: node.pointer("/associatedOrder/name").and_then(Value::as_str).unwrap_or_default().to_string(),
    })
}

/// A payout's breakdown from its balance transactions (the transfer itself is skipped). Each one's
/// `amount − fee` is what it added to the payout; they must add up to `net_cents`.
pub fn shopify_breakdown(net_cents: i64, txns: &[ShopifyTxn]) -> Result<Breakdown, String> {
    let overflow = || "PAYOUT_TOO_LARGE: the payout's amounts are out of range".to_string();
    let add = |a: i64, b: i64| a.checked_add(b).ok_or_else(overflow);
    let mut b = Breakdown { net_cents, gross_cents: 0, fee_cents: 0, refund_cents: 0, chargeback_cents: 0, other_cents: 0, items: Vec::new() };
    for t in txns {
        if t.kind.starts_with("TRANSFER") {
            continue;
        }
        b.fee_cents = add(b.fee_cents, t.fee_cents)?;
        let described = |fallback: &str| if t.order.is_empty() { fallback.to_string() } else { format!("{fallback}, order {}", t.order) };
        match t.kind.as_str() {
            "CHARGE" => {
                b.gross_cents = add(b.gross_cents, t.amount_cents)?;
                b.items.push(PayoutItem { kind: "CHARGE".into(), description: described("Sale"), amount_cents: t.amount_cents });
            }
            "REFUND" | "REFUND_FAILURE" | "REFUND_ADJUSTMENT" => {
                b.refund_cents = add(b.refund_cents, -t.amount_cents)?;
                b.items.push(PayoutItem { kind: "REFUND".into(), description: described("Refund"), amount_cents: -t.amount_cents });
            }
            "DISPUTE_WITHDRAWAL" | "DISPUTE_REVERSAL" => {
                b.chargeback_cents = add(b.chargeback_cents, -t.amount_cents)?;
                b.items.push(PayoutItem { kind: "CHARGEBACK".into(), description: described("Chargeback"), amount_cents: -t.amount_cents });
            }
            // Fees of their own: a chargeback fee (or its refund), and processing fees recorded apart.
            "CHARGEBACK_FEE" | "CHARGEBACK_FEE_REFUND" | "STRIPE_FEE" => b.fee_cents = add(b.fee_cents, -t.amount_cents)?,
            // Reserves, holds and their releases, adjustments, protection credits, and anything newer.
            _ => b.other_cents = add(b.other_cents, t.amount_cents)?,
        }
    }
    let worked_out = b.gross_cents - b.fee_cents - b.refund_cents - b.chargeback_cents + b.other_cents;
    if worked_out != net_cents {
        return Err(format!(
            "PAYOUT_MISMATCH: the payout's transactions add up to {}, not the {} paid out; it can't be recorded",
            format_cents(worked_out),
            format_cents(net_cents)
        ));
    }
    Ok(b)
}

const CHECK_QUERY: &str = "{ shop { currencyCode } shopifyPaymentsAccount { id } }";

const PAYOUTS_QUERY: &str = "query($after: String) { shopifyPaymentsAccount { payouts(first: 100, after: $after, sortKey: ISSUED_AT, reverse: true) {
  pageInfo { hasNextPage endCursor }
  nodes { id legacyResourceId issuedAt status transactionType net { amount currencyCode } } } } }";

const TRANSACTIONS_QUERY: &str = "query($after: String) { shopifyPaymentsAccount { balanceTransactions(first: 100, after: $after, sortKey: PROCESSED_AT, reverse: true) {
  pageInfo { hasNextPage endCursor }
  nodes { type transactionDate test amount { amount } fee { amount } associatedPayout { id } associatedOrder { name } } } } }";

/// Walks a newest-first connection under `shopifyPaymentsAccount.<field>` until `stop` says a node
/// is too old (or the pages end).
async fn walk(http: &reqwest::Client, base: &str, token: &str, query: &str, field: &str, stop: impl Fn(&Value) -> bool) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let data = graphql(http, base, token, query, json!({ "after": after })).await?;
        let conn = data.pointer(&format!("/shopifyPaymentsAccount/{field}")).ok_or_else(|| "PAYOUT_PERMISSION: this store has no Shopify Payments account LocalBooks can read".to_string())?;
        let nodes = conn.get("nodes").and_then(Value::as_array).cloned().unwrap_or_default();
        let mut done = false;
        for n in nodes {
            if stop(&n) {
                done = true;
                break;
            }
            out.push(n);
        }
        let more = conn.pointer("/pageInfo/hasNextPage").and_then(Value::as_bool).unwrap_or(false);
        after = conn.pointer("/pageInfo/endCursor").and_then(Value::as_str).map(str::to_string);
        if done || !more || after.is_none() {
            return Ok(out);
        }
    }
    Err("PAYOUT_TOO_MANY: Shopify has more than LocalBooks reads in one go; get payouts more often so each fetch is smaller".into())
}

fn issued(n: &Value) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&text(n, "issuedAt")).ok().map(|t| t.with_timezone(&Utc))
}

/// Connects a Shopify store: checks the token can read its Shopify Payments account, keeps it in
/// the credential store, and saves the feed with the store's address and its accounts.
pub async fn connect_shopify(pool: &SqlitePool, store: &SecretStore, api: Option<&str>, shop: &str, token: &str, input: &FeedInput) -> Result<PayoutFeed, String> {
    let shop = shop_domain(shop)?;
    let token = token.trim();
    if token.is_empty() {
        return Err("PAYOUT_KEY: paste the app's Admin API access token".into());
    }
    check_accounts(pool, input).await?;
    let http = client()?;
    let data = graphql(&http, &endpoint(api, &shop), token, CHECK_QUERY, json!({})).await?;
    if data.pointer("/shopifyPaymentsAccount/id").and_then(Value::as_str).is_none() {
        return Err("PAYOUT_PERMISSION: this store has no Shopify Payments account LocalBooks can read".into());
    }
    let secret_key = format!("shopify-{}", hex(&crate::crypto::random::<8>()?));
    store.put(&secret_key, token).await?;
    let id: Result<i64, String> = sqlx::query_scalar(
        "INSERT INTO payout_feeds (processor, name, secret_key, account_ref, bank_account_id, sales_account_id, fee_account_id, refund_account_id, chargeback_account_id)
         VALUES ('SHOPIFY', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING id",
    )
    .bind(input.name.trim())
    .bind(&secret_key)
    .bind(&shop)
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
            let _ = store.delete(&secret_key).await;
            Err(e)
        }
    }
}

/// Fetches paid payouts to the bank issued since `since`, with the balance transactions that make
/// them up, and keeps the ones not seen before.
pub(super) async fn fetch(pool: &SqlitePool, f: &PayoutFeed, api: Option<&str>, token: &str, since: DateTime<Utc>) -> Result<FetchResult, String> {
    let shop = f.account_ref.as_deref().ok_or("PAYOUT_SHOP: the feed has no store address; connect it again")?;
    let base = endpoint(api, shop);
    let http = client()?;
    let payouts = walk(&http, &base, token, PAYOUTS_QUERY, "payouts", |n| issued(n).is_some_and(|t| t < since)).await?;
    let home = home_currency(pool).await?;
    let mut result = FetchResult { new_payouts: 0, refused: Vec::new() };
    let mut wanted = Vec::new();
    for p in payouts {
        let id = text(&p, "legacyResourceId");
        let id = if id.is_empty() { p.get("legacyResourceId").map(Value::to_string).unwrap_or_default() } else { id };
        if text(&p, "status") != "PAID" || text(&p, "transactionType") == "WITHDRAWAL" || seen(pool, f.id, &id).await? {
            continue;
        }
        let currency = p.pointer("/net/currencyCode").and_then(Value::as_str).unwrap_or_default().to_uppercase();
        if currency != home {
            result.refused.push(format!("{id}: paid in {currency}, not {home}"));
            continue;
        }
        wanted.push(p);
    }
    let Some(oldest) = wanted.iter().filter_map(issued).min() else {
        return Ok(result);
    };
    // Every balance transaction back to well before the oldest payout wanted, grouped by payout.
    let cutoff = oldest - Duration::days(TRANSACTIONS_BEFORE_DAYS);
    let nodes = walk(&http, &base, token, TRANSACTIONS_QUERY, "balanceTransactions", |n| {
        DateTime::parse_from_rfc3339(&text(n, "transactionDate")).is_ok_and(|t| t.with_timezone(&Utc) < cutoff)
    })
    .await?;
    let mut by_payout: HashMap<String, Vec<ShopifyTxn>> = HashMap::new();
    for n in nodes.iter().filter(|n| !n.get("test").and_then(Value::as_bool).unwrap_or(false)) {
        let t = read_txn(n)?;
        if let Some(p) = t.payout_id.clone() {
            by_payout.entry(p).or_default().push(t);
        }
    }
    for p in wanted {
        let gid = text(&p, "id");
        let id = text(&p, "legacyResourceId");
        let id = if id.is_empty() { p.get("legacyResourceId").map(Value::to_string).unwrap_or_default() } else { id };
        let net = money(&p, "net")?;
        let mut txns = by_payout.remove(&gid).unwrap_or_default();
        txns.sort_by_key(|t| t.at);
        let arrival = issued(&p).map_or_else(|| Utc::now().date_naive(), |t| t.date_naive());
        match shopify_breakdown(net, &txns) {
            Ok(b) => {
                keep(pool, f.id, &id, arrival, &home, &b).await?;
                result.new_payouts += 1;
            }
            Err(e) => result.refused.push(format!("{id}: {}", reason(&e))),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
