//! Bank feeds through SimpleFIN (https://www.simplefin.org/protocol.html).
//!
//! The user creates a one-time setup token at a SimpleFIN server (normally SimpleFIN Bridge, which
//! connects to their banks). LocalBooks claims the token once, receiving an Access URL: credentials
//! for read-only access to the user's account data. The Access URL is kept in the operating
//! system's credential store (the macOS Keychain or Windows Credential Manager; an owner-only file
//! elsewhere), never in the company file or its backups.
//!
//! Each SimpleFIN account can be linked to a Bank or Credit Card account. A sync fetches posted
//! transactions and adds them to that account's review queue, the same queue statement files feed,
//! so nothing reaches the ledger until the user approves each line. Transactions are identified by
//! SimpleFIN's account and transaction ids: overlapping or repeated syncs never add a line twice.
//!
//! Server limits (SimpleFIN Bridge developer guide): about 24 requests a day per connection, and
//! at most 90 days between `start-date` and `end-date` (Bridge warns above 45). Each request covers
//! at most 45 days and a sync reaches back at most 90; older history comes from statement files.
//! Syncs overlap the previous one by 5 days, as the guide recommends, and are refused within 10
//! minutes of the last one.
//!
//! Bridge speaks protocol 1.0; responses in the 2.0 draft's shape (connections, `errlist`) are read
//! too. Transaction dates are the UTC calendar date of the `posted` timestamp.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use base64::Engine;
use chrono::{DateTime, Days, NaiveDate, Utc};
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

use crate::bank::{clean_description, insert_line, parse_amount_cents};
use crate::ledger::{db_err, rollback};
use crate::secrets::SecretStore;

/// Longest span one request covers. The protocol allows 90 days; Bridge warns above 45.
const MAX_WINDOW_DAYS: u64 = 45;
/// Each sync re-reads this many days before the last one, so late-posting transactions are caught.
const OVERLAP_DAYS: u64 = 5;
/// Furthest back a sync reaches (and the earliest start date a link may choose).
pub const MAX_HISTORY_DAYS: u64 = 90;
/// Minimum time between syncs of one connection.
const MIN_SYNC_INTERVAL: chrono::Duration = chrono::Duration::minutes(10);

const KEYCHAIN_SERVICE: &str = "LocalBooks SimpleFIN";
// The credential file on Linux.
const SECRETS_FILE: &str = "simplefin-secrets.json";

/// Where Access URLs are kept, by connection `secret_key`.
pub fn secret_store(config_dir: &Path) -> SecretStore {
    SecretStore::for_this_computer(config_dir, KEYCHAIN_SERVICE, SECRETS_FILE, "the bank connection")
}

// ------------------------------------------------------------------------------------------------
// Tokens and Access URLs
// ------------------------------------------------------------------------------------------------

/// Only HTTPS, except a loopback mock server under test.
fn check_secure(url: &Url) -> Result<(), String> {
    match url.scheme() {
        "https" => Ok(()),
        "http" if cfg!(test) && url.host_str() == Some("127.0.0.1") => Ok(()),
        _ => Err("SFIN_INSECURE: SimpleFIN addresses must use https".into()),
    }
}

/// A setup token is a Base64-encoded claim URL. Spaces and line breaks from copying are ignored.
pub fn decode_setup_token(token: &str) -> Result<Url, String> {
    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
    let compact: String = token.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return Err("SFIN_TOKEN_EMPTY: paste the setup token from SimpleFIN Bridge".into());
    }
    let invalid = || "SFIN_TOKEN_INVALID: that is not a SimpleFIN setup token. Copy the whole token from SimpleFIN Bridge (a long string of letters and numbers).".to_string();
    let bytes = [STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD]
        .iter()
        .find_map(|engine| engine.decode(&compact).ok())
        .ok_or_else(invalid)?;
    let text = String::from_utf8(bytes).map_err(|_| invalid())?;
    let url = Url::parse(text.trim()).map_err(|_| invalid())?;
    if url.host_str().is_none() {
        return Err(invalid());
    }
    check_secure(&url)?;
    Ok(url)
}

fn percent_decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// An Access URL split into its address and Basic Auth credentials.
struct Access {
    base: Url,
    username: String,
    password: String,
}

// Debug output never shows the password.
impl std::fmt::Debug for Access {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Access").field("base", &self.base.as_str()).field("username", &self.username).finish_non_exhaustive()
    }
}

impl Access {
    fn parse(text: &str) -> Result<Access, String> {
        let bad = || "SFIN_ACCESS_INVALID: SimpleFIN returned an access address LocalBooks cannot use".to_string();
        let mut base = Url::parse(text.trim()).map_err(|_| bad())?;
        check_secure(&base)?;
        let username = percent_decode(base.username());
        let password = base.password().map(percent_decode).unwrap_or_default();
        if username.is_empty() || password.is_empty() || base.host_str().is_none() {
            return Err(bad());
        }
        base.set_username("").map_err(|_| bad())?;
        base.set_password(None).map_err(|_| bad())?;
        base.set_query(None);
        base.set_fragment(None);
        Ok(Access { base, username, password })
    }

    fn host(&self) -> String {
        self.base.host_str().unwrap_or_default().to_string()
    }

    fn endpoint(&self, name: &str) -> Url {
        let mut url = self.base.clone();
        let path = format!("{}/{name}", url.path().trim_end_matches('/'));
        url.set_path(&path);
        url
    }
}

// ------------------------------------------------------------------------------------------------
// Protocol messages
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Default, Deserialize)]
struct AccountSet {
    /// 1.0: messages as strings (2.0 keeps it, deprecated).
    #[serde(default)]
    errors: Vec<Value>,
    /// 2.0: structured messages.
    #[serde(default)]
    errlist: Vec<ErrorItem>,
    /// 2.0: the institutions; accounts refer to them by `conn_id`.
    #[serde(default)]
    connections: Vec<RemoteConnection>,
    #[serde(default)]
    accounts: Vec<RemoteAccount>,
}

#[derive(Debug, Deserialize)]
struct ErrorItem {
    #[serde(default)]
    msg: String,
    #[serde(default)]
    code: String,
}

#[derive(Debug, Deserialize)]
struct RemoteConnection {
    conn_id: String,
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct RemoteOrg {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    domain: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RemoteAccount {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    currency: String,
    #[serde(default)]
    balance: Value,
    #[serde(default, rename = "balance-date")]
    balance_date: Option<i64>,
    /// 1.0: the institution.
    #[serde(default)]
    org: Option<RemoteOrg>,
    /// 2.0: the institution, by id.
    #[serde(default)]
    conn_id: Option<String>,
    #[serde(default)]
    transactions: Vec<RemoteTransaction>,
}

#[derive(Debug, Deserialize)]
struct RemoteTransaction {
    id: String,
    #[serde(default)]
    posted: i64,
    #[serde(default)]
    amount: Value,
    #[serde(default)]
    description: String,
    /// Not in the specification, but sent by Bridge.
    #[serde(default)]
    payee: Option<String>,
    #[serde(default)]
    pending: bool,
}

impl AccountSet {
    fn messages(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .errors
            .iter()
            .filter_map(|v| match v {
                Value::String(s) => Some(s.clone()),
                Value::Null => None,
                other => Some(other.to_string()),
            })
            .collect();
        for e in &self.errlist {
            let text = if e.msg.is_empty() { e.code.clone() } else { e.msg.clone() };
            if !text.is_empty() && !out.contains(&text) {
                out.push(text);
            }
        }
        out
    }

    fn institution(&self, account: &RemoteAccount) -> Option<String> {
        let from_org = account.org.as_ref().and_then(|o| o.name.clone().filter(|n| !n.is_empty()).or_else(|| o.domain.clone()));
        let from_connection = account
            .conn_id
            .as_ref()
            .and_then(|id| self.connections.iter().find(|c| &c.conn_id == id))
            .map(|c| c.name.clone())
            .filter(|n| !n.is_empty());
        from_org.or(from_connection)
    }
}

/// Amounts are numeric strings; a bare JSON number is accepted too.
fn amount_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn utc_date(epoch: i64) -> Option<NaiveDate> {
    DateTime::<Utc>::from_timestamp(epoch, 0).map(|d| d.date_naive())
}

fn epoch(date: NaiveDate) -> i64 {
    date.and_hms_opt(0, 0, 0).expect("midnight exists").and_utc().timestamp()
}

/// Request windows `[start, end)` from `from` through `today`, each at most 45 days. The last
/// window ends two days after `today`, so transactions dated "tomorrow" in UTC are included.
fn windows(from: NaiveDate, today: NaiveDate) -> Vec<(NaiveDate, NaiveDate)> {
    let end = today + Days::new(2);
    let mut out = Vec::new();
    let mut start = from.min(today);
    while start < end {
        let next = (start + Days::new(MAX_WINDOW_DAYS)).min(end);
        out.push((start, next));
        start = next;
    }
    out
}

// ------------------------------------------------------------------------------------------------
// Service
// ------------------------------------------------------------------------------------------------

/// The HTTP client and credential store, managed as app state.
pub struct SimpleFin {
    client: reqwest::Client,
    secrets: SecretStore,
    /// One connect or sync at a time, so two clicks cannot spend two requests.
    busy: tokio::sync::Mutex<()>,
}

fn network_error(err: reqwest::Error) -> String {
    // `without_url` keeps the Access URL's credentials out of every message.
    let err = err.without_url();
    let mut text = err.to_string();
    let mut source = std::error::Error::source(&err);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    format!("SFIN_NETWORK: could not reach SimpleFIN ({text}). Check the internet connection and try again.")
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedAccountView {
    pub id: i64,
    pub name: String,
    pub institution: Option<String>,
    pub currency: String,
    pub balance_cents: Option<i64>,
    pub balance_date: Option<String>,
    pub bank_account_id: Option<i64>,
    pub sync_from: Option<String>,
    pub last_synced_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    pub id: i64,
    pub server_host: String,
    pub created_at: String,
    pub last_synced_at: Option<String>,
    /// From the server's last answer, or why the last contact failed; shown to the user.
    pub messages: Vec<String>,
    /// False when this computer has no saved access for the connection (the company file was
    /// set up elsewhere, or the saved access was removed): it must be reconnected.
    pub has_access: bool,
    pub accounts: Vec<LinkedAccountView>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncSummary {
    pub imported: usize,
    pub duplicates: usize,
    /// Pending (not yet posted) transactions, which are left for a later sync.
    pub pending: usize,
    /// Transactions dated before an account's chosen start date.
    pub before_start: usize,
    pub skipped_zero: usize,
    pub accounts: usize,
    pub requests: usize,
    /// Transactions that could not be read (bad amount or date), each with the reason.
    pub problems: Vec<String>,
    pub messages: Vec<String>,
}

struct ConnectionRow {
    id: i64,
    secret_key: String,
    protocol: i64,
    last_synced_at: Option<String>,
}

async fn connection_row(pool: &SqlitePool, connection_id: i64) -> Result<ConnectionRow, String> {
    let row: Option<(i64, String, i64, Option<String>)> =
        sqlx::query_as("SELECT id, secret_key, protocol, last_synced_at FROM simplefin_connections WHERE id = ?1")
            .bind(connection_id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    let (id, secret_key, protocol, last_synced_at) = row.ok_or("SFIN_NOT_FOUND: that bank connection no longer exists")?;
    Ok(ConnectionRow { id, secret_key, protocol, last_synced_at })
}

async fn save_messages(pool: &SqlitePool, connection_id: i64, messages: &[String]) -> Result<(), String> {
    let text = if messages.is_empty() { None } else { Some(serde_json::to_string(messages).map_err(|e| e.to_string())?) };
    sqlx::query("UPDATE simplefin_connections SET last_messages = ?2 WHERE id = ?1")
        .bind(connection_id)
        .bind(text)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

fn parse_timestamp(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text).ok().map(|d| d.with_timezone(&Utc))
}

fn now_text(now: DateTime<Utc>) -> String {
    now.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
}

impl SimpleFin {
    pub fn new(secrets: SecretStore) -> Result<Self, String> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("LocalBooks/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(20))
            .timeout(Duration::from_secs(90))
            // A redirect could carry the credentials elsewhere; SimpleFIN servers do not redirect.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| format!("SFIN_CLIENT: {e}"))?;
        Ok(SimpleFin { client, secrets, busy: tokio::sync::Mutex::new(()) })
    }

    /// Claims a setup token, returning the Access URL. A token can be claimed only once.
    async fn claim(&self, token: &str) -> Result<String, String> {
        let claim_url = decode_setup_token(token)?;
        let response = self
            .client
            .post(claim_url)
            .header(reqwest::header::CONTENT_LENGTH, "0")
            .send()
            .await
            .map_err(network_error)?;
        match response.status() {
            s if s.is_success() => {
                let text = response.text().await.map_err(network_error)?;
                Access::parse(&text)?; // refuse an unusable answer before it is saved
                Ok(text.trim().to_string())
            }
            StatusCode::FORBIDDEN => Err("SFIN_TOKEN_USED: this setup token was already used or does not exist. \
                 If you did not use it yourself, someone else may have: disable it in SimpleFIN Bridge and create a new one."
                .into()),
            s => Err(format!("SFIN_CLAIM_FAILED: SimpleFIN answered {s} to the setup token")),
        }
    }

    /// The highest protocol major version (1 or 2) the server reports; 1 if it does not say.
    async fn protocol(&self, access: &Access) -> i64 {
        let Ok(response) = self.client.get(access.endpoint("info")).send().await else { return 1 };
        let Ok(bytes) = response.bytes().await else { return 1 };
        let Ok(info) = serde_json::from_slice::<Value>(&bytes) else { return 1 };
        let reports_2 = info["versions"].as_array().is_some_and(|v| v.iter().any(|v| v.as_str().is_some_and(|s| s.starts_with("2."))));
        if reports_2 { 2 } else { 1 }
    }

    async fn fetch(&self, access: &Access, protocol: i64, query: &[(&str, String)]) -> Result<AccountSet, String> {
        let mut url = access.endpoint("accounts");
        {
            let mut pairs = url.query_pairs_mut();
            if protocol == 2 {
                pairs.append_pair("version", "2");
            }
            for (k, v) in query {
                pairs.append_pair(k, v);
            }
        }
        let response = self
            .client
            .get(url)
            .basic_auth(&access.username, Some(&access.password))
            .send()
            .await
            .map_err(network_error)?;
        match response.status() {
            StatusCode::OK => {
                let bytes = response.bytes().await.map_err(network_error)?;
                serde_json::from_slice(&bytes).map_err(|e| format!("SFIN_BAD_RESPONSE: SimpleFIN sent data LocalBooks could not read ({e})"))
            }
            StatusCode::PAYMENT_REQUIRED => Err("SFIN_PAYMENT: SimpleFIN says this connection's subscription needs payment. Renew it at SimpleFIN Bridge, then sync again.".into()),
            StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED => Err("SFIN_REVOKED: SimpleFIN no longer accepts this connection (its access was disabled or revoked). \
                 Create a new setup token in SimpleFIN Bridge and choose Reconnect."
                .into()),
            s => Err(format!("SFIN_SERVER: SimpleFIN answered {s}; try again later")),
        }
    }

    async fn access_for(&self, connection: &ConnectionRow) -> Result<Access, String> {
        let text = self.secrets.get(&connection.secret_key).await?.ok_or(
            "SFIN_NO_ACCESS: this computer has no saved access for this bank connection (the company file may have been set up on another computer). \
             Create a new setup token in SimpleFIN Bridge and choose Reconnect.",
        )?;
        Access::parse(&text)
    }

    /// Creates a connection from a setup token and lists its accounts.
    pub async fn connect(&self, pool: &SqlitePool, token: &str) -> Result<ConnectionView, String> {
        let _guard = self.busy.lock().await;
        let access_text = self.claim(token).await?;
        let access = Access::parse(&access_text)?;
        // The token is spent: save the access before anything else can fail.
        let secret_key = crate::mcp_server::new_token();
        self.secrets.put(&secret_key, &access_text).await?;
        let protocol = self.protocol(&access).await;
        let inserted = sqlx::query_scalar("INSERT INTO simplefin_connections (secret_key, server_host, protocol) VALUES (?1, ?2, ?3) RETURNING id")
            .bind(&secret_key)
            .bind(access.host())
            .bind(protocol)
            .fetch_one(pool)
            .await
            .map_err(db_err);
        let id: i64 = match inserted {
            Ok(id) => id,
            Err(err) => {
                let _ = self.secrets.delete(&secret_key).await;
                return Err(err);
            }
        };
        if let Err(err) = self.refresh_locked(pool, id, &access, protocol).await {
            save_messages(pool, id, &[err]).await?;
        }
        self.view(pool, id).await
    }

    /// Replaces a connection's access with a new setup token, keeping its account links.
    pub async fn reconnect(&self, pool: &SqlitePool, connection_id: i64, token: &str) -> Result<ConnectionView, String> {
        let _guard = self.busy.lock().await;
        let connection = connection_row(pool, connection_id).await?;
        let access_text = self.claim(token).await?;
        let access = Access::parse(&access_text)?;
        self.secrets.put(&connection.secret_key, &access_text).await?;
        let protocol = self.protocol(&access).await;
        sqlx::query("UPDATE simplefin_connections SET server_host = ?2, protocol = ?3, last_messages = NULL WHERE id = ?1")
            .bind(connection_id)
            .bind(access.host())
            .bind(protocol)
            .execute(pool)
            .await
            .map_err(db_err)?;
        if let Err(err) = self.refresh_locked(pool, connection_id, &access, protocol).await {
            save_messages(pool, connection_id, &[err]).await?;
        }
        self.view(pool, connection_id).await
    }

    /// Re-reads the connection's account list and balances (no transactions).
    pub async fn refresh_accounts(&self, pool: &SqlitePool, connection_id: i64) -> Result<ConnectionView, String> {
        let _guard = self.busy.lock().await;
        let connection = connection_row(pool, connection_id).await?;
        let access = self.access_for(&connection).await?;
        if let Err(err) = self.refresh_locked(pool, connection_id, &access, connection.protocol).await {
            save_messages(pool, connection_id, std::slice::from_ref(&err)).await?;
            return Err(err);
        }
        self.view(pool, connection_id).await
    }

    async fn refresh_locked(&self, pool: &SqlitePool, connection_id: i64, access: &Access, protocol: i64) -> Result<(), String> {
        let set = self.fetch(access, protocol, &[("balances-only", "1".into())]).await?;
        let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
        for account in &set.accounts {
            if let Err(err) = upsert_account(&mut tx, connection_id, &set, account).await {
                return Err(rollback(tx, err).await);
            }
        }
        crate::ledger::commit_write(tx).await.map_err(db_err)?;
        save_messages(pool, connection_id, &set.messages()).await
    }

    /// Fetches posted transactions for every linked account of the connection into the review queue.
    pub async fn sync(&self, pool: &SqlitePool, connection_id: i64, now: DateTime<Utc>) -> Result<SyncSummary, String> {
        let _guard = self.busy.lock().await;
        let connection = connection_row(pool, connection_id).await?;
        if let Some(last) = connection.last_synced_at.as_deref().and_then(parse_timestamp) {
            let wait = last + MIN_SYNC_INTERVAL - now;
            if wait > chrono::Duration::zero() {
                let minutes = (wait.num_seconds() + 59) / 60;
                return Err(format!(
                    "SFIN_TOO_SOON: this connection was synced a few minutes ago. SimpleFIN allows about 24 updates a day; try again in {minutes} minute{}.",
                    if minutes == 1 { "" } else { "s" }
                ));
            }
        }

        let linked: Vec<(i64, String, i64, String, Option<String>)> = sqlx::query_as(
            "SELECT id, remote_id, bank_account_id, sync_from, last_synced_at FROM simplefin_accounts
             WHERE connection_id = ?1 AND bank_account_id IS NOT NULL ORDER BY id",
        )
        .bind(connection_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
        if linked.is_empty() {
            return Err("SFIN_NOTHING_LINKED: link at least one SimpleFIN account to a LocalBooks bank account first".into());
        }

        let access = self.access_for(&connection).await?;
        let today = now.date_naive();
        let floor = today - Days::new(MAX_HISTORY_DAYS);
        let mut from_dates = HashMap::new();
        let mut earliest = today;
        for (id, _, _, sync_from, last) in &linked {
            let start = NaiveDate::parse_from_str(sync_from, "%Y-%m-%d").map_err(|e| format!("SFIN_BAD_START: {e}"))?;
            let resume = last.as_deref().and_then(parse_timestamp).map(|t| t.date_naive() - Days::new(OVERLAP_DAYS));
            let need = resume.map_or(start, |r| r.max(start)).max(floor);
            earliest = earliest.min(need);
            from_dates.insert(*id, start);
        }

        let mut summary = SyncSummary::default();
        let mut sets = Vec::new();
        for (start, end) in windows(earliest, today) {
            let mut query = vec![("start-date", epoch(start).to_string()), ("end-date", epoch(end).to_string())];
            query.extend(linked.iter().map(|(_, remote, ..)| ("account", remote.clone())));
            summary.requests += 1;
            match self.fetch(&access, connection.protocol, &query).await {
                Ok(set) => sets.push(set),
                Err(err) => {
                    save_messages(pool, connection_id, std::slice::from_ref(&err)).await?;
                    return Err(err);
                }
            }
        }

        let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
        match write_sync(&mut tx, connection_id, &linked, &from_dates, &sets, &mut summary, now).await {
            Ok(()) => crate::ledger::commit_write(tx).await.map_err(db_err)?,
            Err(err) => return Err(rollback(tx, err).await),
        }
        for set in &sets {
            for m in set.messages() {
                if !summary.messages.contains(&m) {
                    summary.messages.push(m);
                }
            }
        }
        save_messages(pool, connection_id, &summary.messages).await?;
        Ok(summary)
    }

    /// Removes the connection and its saved access. Lines already imported stay in Bank Feeds.
    pub async fn disconnect(&self, pool: &SqlitePool, connection_id: i64) -> Result<(), String> {
        let _guard = self.busy.lock().await;
        let connection = connection_row(pool, connection_id).await?;
        self.secrets.delete(&connection.secret_key).await?;
        sqlx::query("DELETE FROM simplefin_connections WHERE id = ?1").bind(connection.id).execute(pool).await.map_err(db_err)?;
        Ok(())
    }

    pub async fn list(&self, pool: &SqlitePool) -> Result<Vec<ConnectionView>, String> {
        let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM simplefin_connections ORDER BY id").fetch_all(pool).await.map_err(db_err)?;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            out.push(self.view(pool, id).await?);
        }
        Ok(out)
    }

    async fn view(&self, pool: &SqlitePool, connection_id: i64) -> Result<ConnectionView, String> {
        let (id, secret_key, server_host, created_at, last_synced_at, messages): (i64, String, String, String, Option<String>, Option<String>) =
            sqlx::query_as("SELECT id, secret_key, server_host, created_at, last_synced_at, last_messages FROM simplefin_connections WHERE id = ?1")
                .bind(connection_id)
                .fetch_one(pool)
                .await
                .map_err(db_err)?;
        let accounts: Vec<(i64, String, Option<String>, String, Option<i64>, Option<String>, Option<i64>, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT id, name, institution, currency, balance_cents, balance_date, bank_account_id, sync_from, last_synced_at
             FROM simplefin_accounts WHERE connection_id = ?1 ORDER BY institution, name, id",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
        let has_access = matches!(self.secrets.get(&secret_key).await, Ok(Some(_)));
        Ok(ConnectionView {
            id,
            server_host,
            created_at,
            last_synced_at,
            messages: messages.and_then(|m| serde_json::from_str(&m).ok()).unwrap_or_default(),
            has_access,
            accounts: accounts
                .into_iter()
                .map(|(id, name, institution, currency, balance_cents, balance_date, bank_account_id, sync_from, last_synced_at)| LinkedAccountView {
                    id,
                    name,
                    institution,
                    currency,
                    balance_cents,
                    balance_date,
                    bank_account_id,
                    sync_from,
                    last_synced_at,
                })
                .collect(),
        })
    }
}

async fn upsert_account(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, connection_id: i64, set: &AccountSet, account: &RemoteAccount) -> Result<(), String> {
    if account.id.is_empty() {
        return Ok(());
    }
    let balance_cents = amount_text(&account.balance).and_then(|t| parse_amount_cents(&t).ok());
    let balance_date = account.balance_date.and_then(utc_date);
    let name = if account.name.trim().is_empty() { account.id.clone() } else { account.name.trim().to_string() };
    sqlx::query(
        "INSERT INTO simplefin_accounts (connection_id, remote_id, name, institution, currency, balance_cents, balance_date)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (connection_id, remote_id) DO UPDATE SET
             name = excluded.name, institution = excluded.institution, currency = excluded.currency,
             balance_cents = excluded.balance_cents, balance_date = excluded.balance_date",
    )
    .bind(connection_id)
    .bind(&account.id)
    .bind(name)
    .bind(set.institution(account))
    .bind(account.currency.trim().to_uppercase())
    .bind(balance_cents)
    .bind(balance_date)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

async fn write_sync(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    connection_id: i64,
    linked: &[(i64, String, i64, String, Option<String>)],
    from_dates: &HashMap<i64, NaiveDate>,
    sets: &[AccountSet],
    summary: &mut SyncSummary,
    now: DateTime<Utc>,
) -> Result<(), String> {
    let stamp = now_text(now);
    let mut synced = std::collections::HashSet::new();
    for set in sets {
        for account in &set.accounts {
            let Some((link_id, remote, bank_account_id, ..)) = linked.iter().find(|l| l.1 == account.id) else { continue };
            upsert_account(tx, connection_id, set, account).await?;
            synced.insert(*link_id);
            let start = from_dates[link_id];
            for t in &account.transactions {
                if t.pending || t.posted <= 0 {
                    summary.pending += 1;
                    continue;
                }
                let Some(date) = utc_date(t.posted) else {
                    summary.problems.push(format!("{}: transaction {} has an unreadable date", account.name, t.id));
                    continue;
                };
                if date < start {
                    summary.before_start += 1;
                    continue;
                }
                let amount = match amount_text(&t.amount).ok_or_else(|| "no amount".to_string()).and_then(|a| parse_amount_cents(&a)) {
                    Ok(a) => a,
                    Err(e) => {
                        summary.problems.push(format!("{}: transaction on {date} could not be read ({e})", account.name));
                        continue;
                    }
                };
                if amount == 0 {
                    summary.skipped_zero += 1;
                    continue;
                }
                let text = if t.description.trim().is_empty() { t.payee.as_deref().unwrap_or("") } else { &t.description };
                let fingerprint = format!("SFIN|{remote}|{}", t.id);
                if insert_line(tx, *bank_account_id, date, &clean_description(text), amount, &fingerprint).await? {
                    summary.imported += 1;
                } else {
                    summary.duplicates += 1;
                }
            }
        }
    }
    for id in &synced {
        sqlx::query("UPDATE simplefin_accounts SET last_synced_at = ?2 WHERE id = ?1")
            .bind(id)
            .bind(&stamp)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
    }
    summary.accounts = synced.len();
    sqlx::query("UPDATE simplefin_connections SET last_synced_at = ?2 WHERE id = ?1")
        .bind(connection_id)
        .bind(&stamp)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Linking
// ------------------------------------------------------------------------------------------------

/// Links a SimpleFIN account to a Bank or Credit Card account, importing from `sync_from` onward.
pub async fn link_account(pool: &SqlitePool, account_id: i64, bank_account_id: i64, sync_from: NaiveDate, today: NaiveDate) -> Result<(), String> {
    let currency: Option<String> = sqlx::query_scalar("SELECT currency FROM simplefin_accounts WHERE id = ?1")
        .bind(account_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    let currency = currency.ok_or("SFIN_NOT_FOUND: that SimpleFIN account no longer exists")?;
    let home: String = sqlx::query_scalar("SELECT value FROM system_preferences WHERE key = 'home_currency_code'")
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    if !currency.eq_ignore_ascii_case(&home) {
        return Err(format!(
            "SFIN_CURRENCY: this account is in {currency}, but LocalBooks bank accounts are kept in {home}. Import its statements as files instead."
        ));
    }
    let target: Option<(String, i64)> = sqlx::query_as("SELECT account_type, is_active FROM chart_of_accounts WHERE id = ?1")
        .bind(bank_account_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    match target {
        Some((kind, 1)) if kind == "BANK" || kind == "CREDIT_CARD" => {}
        Some((_, 0)) => return Err("SFIN_ACCOUNT_INACTIVE: that account is inactive".into()),
        _ => return Err("SFIN_ACCOUNT_TYPE: a bank connection can only feed a Bank or Credit Card account".into()),
    }
    let taken: Option<String> = sqlx::query_scalar("SELECT name FROM simplefin_accounts WHERE bank_account_id = ?1 AND id <> ?2")
        .bind(bank_account_id)
        .bind(account_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    if let Some(other) = taken {
        return Err(format!("SFIN_ALREADY_LINKED: that account already receives transactions from \"{other}\". Unlink it first."));
    }
    if sync_from > today {
        return Err("SFIN_BAD_START: the start date cannot be in the future".into());
    }
    if sync_from < today - Days::new(MAX_HISTORY_DAYS) {
        return Err(format!("SFIN_BAD_START: the start date can be at most {MAX_HISTORY_DAYS} days ago; import older transactions from statement files"));
    }
    sqlx::query("UPDATE simplefin_accounts SET bank_account_id = ?2, sync_from = ?3, last_synced_at = NULL WHERE id = ?1")
        .bind(account_id)
        .bind(bank_account_id)
        .bind(sync_from)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub async fn unlink_account(pool: &SqlitePool, account_id: i64) -> Result<(), String> {
    sqlx::query("UPDATE simplefin_accounts SET bank_account_id = NULL, sync_from = NULL, last_synced_at = NULL WHERE id = ?1")
        .bind(account_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// A start date that does not repeat what is already in Bank Feeds: the day after the account's
/// latest imported line, or 30 days ago when it has none (never more than a year ago).
pub async fn suggested_start(pool: &SqlitePool, bank_account_id: i64, today: NaiveDate) -> Result<NaiveDate, String> {
    let latest: Option<NaiveDate> = sqlx::query_scalar("SELECT MAX(txn_date) FROM bank_transactions WHERE bank_account_id = ?1")
        .bind(bank_account_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    let suggestion = latest.map_or(today - Days::new(30), |d| d + Days::new(1));
    Ok(suggestion.clamp(today - Days::new(MAX_HISTORY_DAYS), today))
}

#[cfg(test)]
mod tests;

