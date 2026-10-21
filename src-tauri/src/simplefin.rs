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

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use chrono::{DateTime, Days, NaiveDate, Utc};
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::SqlitePool;

use crate::bank::{clean_description, insert_line, parse_amount_cents};
use crate::ledger::{db_err, rollback};

/// Longest span one request covers. The protocol allows 90 days; Bridge warns above 45.
const MAX_WINDOW_DAYS: u64 = 45;
/// Each sync re-reads this many days before the last one, so late-posting transactions are caught.
const OVERLAP_DAYS: u64 = 5;
/// Furthest back a sync reaches (and the earliest start date a link may choose).
pub const MAX_HISTORY_DAYS: u64 = 90;
/// Minimum time between syncs of one connection.
const MIN_SYNC_INTERVAL: chrono::Duration = chrono::Duration::minutes(10);

const KEYCHAIN_SERVICE: &str = "LocalBooks SimpleFIN";
// Used on Linux, and by tests everywhere.
#[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
const SECRETS_FILE: &str = "simplefin-secrets.json";

// ------------------------------------------------------------------------------------------------
// Credential storage
// ------------------------------------------------------------------------------------------------

/// Where Access URLs are kept, by connection `secret_key`.
#[derive(Debug, Clone)]
pub enum SecretStore {
    /// The macOS Keychain or Windows Credential Manager.
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    System,
    /// A JSON file readable by its owner only (Linux, and tests).
    #[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
    File(PathBuf),
}

impl SecretStore {
    pub fn for_this_computer(config_dir: &Path) -> Self {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            let _ = config_dir;
            SecretStore::System
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            SecretStore::File(config_dir.join(SECRETS_FILE))
        }
    }

    fn read_file(path: &Path) -> Result<BTreeMap<String, String>, String> {
        match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("SFIN_SECRETS: {} is damaged: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(format!("SFIN_SECRETS: cannot read {}: {e}", path.display())),
        }
    }

    fn write_file(path: &Path, map: &BTreeMap<String, String>) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("SFIN_SECRETS: cannot create {}: {e}", dir.display()))?;
        }
        let json = serde_json::to_vec_pretty(map).map_err(|e| format!("SFIN_SECRETS: {e}"))?;
        crate::instance::write_private(path, &json).map_err(|e| format!("SFIN_SECRETS: cannot write {}: {e}", path.display()))
    }

    fn get_blocking(&self, key: &str) -> Result<Option<String>, String> {
        match self {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            SecretStore::System => match keyring::Entry::new(KEYCHAIN_SERVICE, key).and_then(|e| e.get_password()) {
                Ok(secret) => Ok(Some(secret)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(e) => Err(format!("SFIN_KEYCHAIN: cannot read the saved bank connection: {e}")),
            },
            SecretStore::File(path) => Ok(Self::read_file(path)?.remove(key)),
        }
    }

    fn put_blocking(&self, key: &str, secret: &str) -> Result<(), String> {
        match self {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            SecretStore::System => keyring::Entry::new(KEYCHAIN_SERVICE, key)
                .and_then(|e| e.set_password(secret))
                .map_err(|e| format!("SFIN_KEYCHAIN: cannot save the bank connection: {e}")),
            SecretStore::File(path) => {
                let mut map = Self::read_file(path)?;
                map.insert(key.to_string(), secret.to_string());
                Self::write_file(path, &map)
            }
        }
    }

    fn delete_blocking(&self, key: &str) -> Result<(), String> {
        match self {
            #[cfg(any(target_os = "macos", target_os = "windows"))]
            SecretStore::System => match keyring::Entry::new(KEYCHAIN_SERVICE, key).and_then(|e| e.delete_credential()) {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(format!("SFIN_KEYCHAIN: cannot remove the saved bank connection: {e}")),
            },
            SecretStore::File(path) => {
                let mut map = Self::read_file(path)?;
                if map.remove(key).is_some() {
                    Self::write_file(path, &map)?;
                }
                Ok(())
            }
        }
    }

    // The system store may wait on the user (a Keychain permission prompt), so it runs off the
    // async workers.
    async fn get(&self, key: &str) -> Result<Option<String>, String> {
        let (store, key) = (self.clone(), key.to_string());
        tokio::task::spawn_blocking(move || store.get_blocking(&key)).await.map_err(|e| format!("SFIN_KEYCHAIN: {e}"))?
    }

    async fn put(&self, key: &str, secret: &str) -> Result<(), String> {
        let (store, key, secret) = (self.clone(), key.to_string(), secret.to_string());
        tokio::task::spawn_blocking(move || store.put_blocking(&key, &secret)).await.map_err(|e| format!("SFIN_KEYCHAIN: {e}"))?
    }

    async fn delete(&self, key: &str) -> Result<(), String> {
        let (store, key) = (self.clone(), key.to_string());
        tokio::task::spawn_blocking(move || store.delete_blocking(&key)).await.map_err(|e| format!("SFIN_KEYCHAIN: {e}"))?
    }
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
        let mut tx = pool.begin().await.map_err(db_err)?;
        for account in &set.accounts {
            if let Err(err) = upsert_account(&mut tx, connection_id, &set, account).await {
                return Err(rollback(tx, err).await);
            }
        }
        tx.commit().await.map_err(db_err)?;
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

        let mut tx = pool.begin().await.map_err(db_err)?;
        match write_sync(&mut tx, connection_id, &linked, &from_dates, &sets, &mut summary, now).await {
            Ok(()) => tx.commit().await.map_err(db_err)?,
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
mod tests {
    use super::*;
    use crate::db::test_pool;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn setup_tokens_decode_to_https_claim_urls() {
        let b64 = base64::engine::general_purpose::STANDARD;
        let token = b64.encode("https://bridge.example.org/simplefin/claim/ABC123");
        assert_eq!(decode_setup_token(&token).unwrap().as_str(), "https://bridge.example.org/simplefin/claim/ABC123");
        // Line breaks and spaces from copying, and missing padding, are tolerated.
        let wrapped = format!("  {}\n{}  ", &token[..10], token[10..].trim_end_matches('='));
        assert!(decode_setup_token(&wrapped).is_ok());

        assert!(decode_setup_token("").unwrap_err().starts_with("SFIN_TOKEN_EMPTY"));
        assert!(decode_setup_token("not a token!").unwrap_err().starts_with("SFIN_TOKEN_INVALID"));
        assert!(decode_setup_token(&b64.encode("hello world")).unwrap_err().starts_with("SFIN_TOKEN_INVALID"));
        assert!(decode_setup_token(&b64.encode("http://bridge.example.org/claim/x")).unwrap_err().starts_with("SFIN_INSECURE"));
    }

    #[test]
    fn access_urls_split_into_address_and_credentials() {
        let a = Access::parse("https://user:p%40ss@bridge.example.org/simplefin\n").unwrap();
        assert_eq!((a.username.as_str(), a.password.as_str()), ("user", "p@ss"));
        assert_eq!(a.endpoint("accounts").as_str(), "https://bridge.example.org/simplefin/accounts");
        assert_eq!(a.host(), "bridge.example.org");
        assert!(!a.base.as_str().contains("user"), "credentials are not part of the address");
        assert!(Access::parse("https://bridge.example.org/simplefin").is_err(), "credentials are required");
        assert!(Access::parse("http://u:p@bridge.example.org/simplefin").unwrap_err().starts_with("SFIN_INSECURE"));
    }

    #[test]
    fn windows_cover_the_range_in_spans_of_at_most_45_days() {
        let d = |s: &str| NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap();
        assert_eq!(windows(d("2026-10-01"), d("2026-10-20")), vec![(d("2026-10-01"), d("2026-10-22"))]);
        let long = windows(d("2026-07-22"), d("2026-10-20"));
        assert_eq!(long.first().unwrap().0, d("2026-07-22"));
        assert_eq!(long.last().unwrap().1, d("2026-10-22"));
        assert!(long.iter().all(|(s, e)| s < e && (*e - *s).num_days() <= 45));
        assert!(long.windows(2).all(|w| w[0].1 == w[1].0), "no gaps between windows");
        assert_eq!(long.len(), 3);
    }

    #[test]
    fn version_1_and_2_responses_are_both_read() {
        let v1: AccountSet = serde_json::from_str(
            r#"{"errors":["Connection to Example Bank may need attention"],"accounts":[{"org":{"domain":"example.com","name":"Example Bank","sfin-url":"https://x"},
                "id":"ACT-1","name":"Checking","currency":"USD","balance":"1234.56","balance-date":1760000000,"transactions":[]}]}"#,
        )
        .unwrap();
        assert_eq!(v1.messages(), vec!["Connection to Example Bank may need attention"]);
        assert_eq!(v1.institution(&v1.accounts[0]).as_deref(), Some("Example Bank"));

        let v2: AccountSet = serde_json::from_str(
            r#"{"errlist":[{"code":"con.auth","msg":"Reauthenticate with Example Bank","conn_id":"C1"}],"connections":[{"conn_id":"C1","name":"Example Bank","org_id":"o","sfin_url":"https://x"}],
                "accounts":[{"id":"ACT-1","name":"Checking","conn_id":"C1","currency":"USD","balance":1234.5,"balance-date":1760000000}]}"#,
        )
        .unwrap();
        assert_eq!(v2.messages(), vec!["Reauthenticate with Example Bank"]);
        assert_eq!(v2.institution(&v2.accounts[0]).as_deref(), Some("Example Bank"));
        assert_eq!(amount_text(&v2.accounts[0].balance).as_deref(), Some("1234.5"));
    }

    // ---- a mock SimpleFIN server on 127.0.0.1 -------------------------------------------------

    struct Mock {
        base: String,
        token: String,
        requests: Arc<Mutex<Vec<String>>>,
        /// The body `GET /accounts` returns (balances-only or not); replaced between steps.
        accounts: Arc<Mutex<String>>,
        /// Status returned for `GET /accounts` (200 normally).
        status: Arc<Mutex<u16>>,
    }

    async fn mock_server() -> Mock {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let accounts = Arc::new(Mutex::new(String::from(r#"{"errors":[],"accounts":[]}"#)));
        let status = Arc::new(Mutex::new(200u16));
        let claimed = Arc::new(Mutex::new(false));
        let (req, acc, st, base2) = (requests.clone(), accounts.clone(), status.clone(), base.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                let (req, acc, st, claimed, base) = (req.clone(), acc.clone(), st.clone(), claimed.clone(), base2.clone());
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]).to_string();
                    let line = head.lines().next().unwrap_or("").to_string();
                    let authorized = head.lines().any(|l| l.eq_ignore_ascii_case(&format!("authorization: Basic {}", base64::engine::general_purpose::STANDARD.encode("demo:s3cret"))));
                    req.lock().unwrap().push(line.clone());
                    let (code, body) = if line.starts_with("POST /simplefin/claim/GOOD ") {
                        let mut c = claimed.lock().unwrap();
                        if *c {
                            (403, String::new())
                        } else {
                            *c = true;
                            (200, format!("{}/simplefin", base.replace("http://", "http://demo:s3cret@")))
                        }
                    } else if line.starts_with("POST ") {
                        (403, String::new())
                    } else if line.starts_with("GET /simplefin/info ") {
                        (200, r#"{"versions":["1.0"]}"#.to_string())
                    } else if line.starts_with("GET /simplefin/accounts") {
                        if !authorized {
                            (403, String::new())
                        } else {
                            (*st.lock().unwrap(), acc.lock().unwrap().clone())
                        }
                    } else {
                        (404, String::new())
                    };
                    let reply = format!("HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = socket.write_all(reply.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        let token = base64::engine::general_purpose::STANDARD.encode(format!("{base}/simplefin/claim/GOOD"));
        Mock { base, token, requests, accounts, status }
    }

    fn service(dir: &Path) -> SimpleFin {
        SimpleFin::new(SecretStore::File(dir.join(SECRETS_FILE))).unwrap()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("localbooks-sfin-{name}-{}", crate::mcp_server::new_token()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    async fn bank_id(pool: &SqlitePool, number: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
    }

    const BALANCES: &str = r#"{"errors":[],"accounts":[
        {"org":{"name":"Example Bank","sfin-url":"x"},"id":"CHK","name":"Everyday Checking","currency":"USD","balance":"1500.00","balance-date":1792800000,"transactions":[]},
        {"org":{"name":"Example Bank","sfin-url":"x"},"id":"EUR","name":"Euro Account","currency":"EUR","balance":"20.00","balance-date":1792800000,"transactions":[]}]}"#;

    fn ts(date: &str) -> i64 {
        epoch(NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap()) + 12 * 3600
    }

    fn transactions(list: &[(&str, &str, &str, &str, bool)]) -> String {
        let txns: Vec<String> = list
            .iter()
            .map(|(id, date, amount, desc, pending)| {
                let posted = if *pending { 0 } else { ts(date) };
                format!(r#"{{"id":"{id}","posted":{posted},"amount":"{amount}","description":"{desc}","pending":{pending}}}"#)
            })
            .collect();
        format!(
            r#"{{"errors":["Example Bank: last updated 2 hours ago"],"accounts":[{{"org":{{"name":"Example Bank","sfin-url":"x"}},"id":"CHK","name":"Everyday Checking","currency":"USD","balance":"1400.00","balance-date":1792800000,"transactions":[{}]}}]}}"#,
            txns.join(",")
        )
    }

    #[tokio::test]
    async fn connect_link_and_sync_without_duplicates() {
        let pool = test_pool().await;
        let dir = temp_dir("flow");
        let sf = service(&dir);
        let mock = mock_server().await;
        *mock.accounts.lock().unwrap() = BALANCES.to_string();

        // Claiming stores the access on this computer, not in the company file.
        let view = sf.connect(&pool, &mock.token).await.unwrap();
        assert!(view.has_access);
        assert_eq!(view.server_host, "127.0.0.1");
        assert_eq!(view.accounts.len(), 2);
        let chk = view.accounts.iter().find(|a| a.name == "Everyday Checking").unwrap();
        assert_eq!((chk.balance_cents, chk.institution.as_deref()), (Some(150_000), Some("Example Bank")));
        let in_db: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM simplefin_connections WHERE secret_key LIKE '%s3cret%' OR server_host LIKE '%s3cret%'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(in_db, 0, "the credentials are not stored in the company file");
        assert!(std::fs::read_to_string(dir.join(SECRETS_FILE)).unwrap().contains("s3cret"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(dir.join(SECRETS_FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        }

        // The token is single-use.
        assert!(sf.connect(&pool, &mock.token).await.unwrap_err().starts_with("SFIN_TOKEN_USED"));

        // A foreign-currency account cannot feed a home-currency bank account.
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let checking = bank_id(&pool, "1010").await;
        let eur = view.accounts.iter().find(|a| a.currency == "EUR").unwrap();
        assert!(link_account(&pool, eur.id, checking, today, today).await.unwrap_err().starts_with("SFIN_CURRENCY"));
        // Only bank and card accounts, and not too far back.
        let income = bank_id(&pool, "4000").await;
        assert!(link_account(&pool, chk.id, income, today, today).await.unwrap_err().starts_with("SFIN_ACCOUNT_TYPE"));
        assert!(link_account(&pool, chk.id, checking, today - Days::new(100), today).await.unwrap_err().starts_with("SFIN_BAD_START"));

        let start = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        assert_eq!(suggested_start(&pool, checking, today).await.unwrap(), today - Days::new(30));
        link_account(&pool, chk.id, checking, start, today).await.unwrap();

        *mock.accounts.lock().unwrap() = transactions(&[
            ("T1", "2026-10-02", "-42.17", "COFFEE SHOP  #12", false),
            ("T2", "2026-10-03", "1200.00", "CLIENT PAYMENT", false),
            ("T3", "2026-10-03", "-5.00", "COFFEE SHOP  #12", false),
            ("T4", "2026-10-04", "-5.00", "COFFEE SHOP  #12", false),
            ("T0", "2026-09-28", "-9.99", "BEFORE THE START DATE", false),
            ("P1", "2026-10-19", "-15.00", "PENDING CHARGE", true),
            ("Z1", "2026-10-05", "0.00", "AUTH CHECK", false),
            ("B1", "2026-10-05", "1.005", "FRACTIONAL", false),
        ]);
        let now = DateTime::parse_from_rfc3339("2026-10-20T15:00:00Z").unwrap().with_timezone(&Utc);
        let summary = sf.sync(&pool, view.id, now).await.unwrap();
        assert_eq!((summary.imported, summary.duplicates, summary.pending, summary.before_start, summary.skipped_zero), (4, 0, 1, 1, 1));
        assert_eq!(summary.problems.len(), 1, "{:?}", summary.problems);
        assert_eq!(summary.messages, vec!["Example Bank: last updated 2 hours ago"]);
        assert_eq!(summary.requests, 1);

        let lines: Vec<(String, String, i64)> =
            sqlx::query_as("SELECT txn_date, description, amount_cents FROM bank_transactions WHERE bank_account_id = ?1 ORDER BY txn_date, id")
                .bind(checking)
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(lines[0], ("2026-10-02".into(), "COFFEE SHOP #12".into(), -4_217));
        assert_eq!(lines.len(), 4, "two identical $5 coffees on different days both import");

        // The request asked only for the linked account, from the start date, without credentials in the URL.
        let get = mock.requests.lock().unwrap().iter().rev().find(|l| l.contains("/accounts")).unwrap().clone();
        assert!(get.contains("account=CHK") && !get.contains("EUR"), "{get}");
        assert!(get.contains(&format!("start-date={}", epoch(start))), "{get}");
        assert!(!get.contains("s3cret"));

        // Too soon: refused without contacting the server.
        let count = mock.requests.lock().unwrap().len();
        assert!(sf.sync(&pool, view.id, now + chrono::Duration::minutes(3)).await.unwrap_err().starts_with("SFIN_TOO_SOON"));
        assert_eq!(mock.requests.lock().unwrap().len(), count);

        // A later sync overlaps by five days and adds only what is new.
        *mock.accounts.lock().unwrap() = transactions(&[
            ("T3", "2026-10-03", "-5.00", "COFFEE SHOP  #12", false),
            ("T4", "2026-10-04", "-5.00", "COFFEE SHOP  #12", false),
            ("P1", "2026-10-19", "-15.00", "PENDING CHARGE", false),
        ]);
        let later = now + chrono::Duration::days(1);
        let second = sf.sync(&pool, view.id, later).await.unwrap();
        assert_eq!((second.imported, second.duplicates), (1, 2));
        let get = mock.requests.lock().unwrap().iter().rev().find(|l| l.contains("/accounts")).unwrap().clone();
        assert!(get.contains(&format!("start-date={}", epoch(NaiveDate::from_ymd_opt(2026, 10, 15).unwrap()))), "{get}");

        // A revoked connection says what to do and keeps the message for the connection list.
        *mock.status.lock().unwrap() = 403;
        let err = sf.sync(&pool, view.id, later + chrono::Duration::hours(1)).await.unwrap_err();
        assert!(err.starts_with("SFIN_REVOKED") && !err.contains("s3cret"), "{err}");
        assert!(sf.list(&pool).await.unwrap()[0].messages[0].starts_with("SFIN_REVOKED"));

        // Disconnecting removes the saved access; imported lines stay for review.
        sf.disconnect(&pool, view.id).await.unwrap();
        assert!(!std::fs::read_to_string(dir.join(SECRETS_FILE)).unwrap().contains("s3cret"));
        let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM bank_transactions").fetch_one(&pool).await.unwrap();
        assert_eq!(left, 5);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_company_file_without_saved_access_asks_to_reconnect() {
        let pool = test_pool().await;
        let dir = temp_dir("reconnect");
        let mock = mock_server().await;
        *mock.accounts.lock().unwrap() = BALANCES.to_string();
        let view = service(&dir).connect(&pool, &mock.token).await.unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 10, 20).unwrap();
        let checking = bank_id(&pool, "1010").await;
        let chk = view.accounts.iter().find(|a| a.currency == "USD").unwrap();
        link_account(&pool, chk.id, checking, today, today).await.unwrap();

        // The same company file on a computer that never connected.
        let elsewhere = service(&temp_dir("elsewhere"));
        let listed = elsewhere.list(&pool).await.unwrap();
        assert!(!listed[0].has_access);
        assert_eq!(listed[0].accounts.iter().filter(|a| a.bank_account_id.is_some()).count(), 1, "links are kept");
        let now = DateTime::parse_from_rfc3339("2026-10-20T15:00:00Z").unwrap().with_timezone(&Utc);
        assert!(elsewhere.sync(&pool, view.id, now).await.unwrap_err().starts_with("SFIN_NO_ACCESS"));

        // A new token from a second mock server reconnects it, keeping the links.
        let fresh = mock_server().await;
        *fresh.accounts.lock().unwrap() = BALANCES.to_string();
        let again = elsewhere.reconnect(&pool, view.id, &fresh.token).await.unwrap();
        assert!(again.has_access);
        assert_eq!(again.accounts.iter().find(|a| a.id == chk.id).unwrap().bank_account_id, Some(checking));
        let _ = mock.base;
        let _ = std::fs::remove_dir_all(dir);
    }
    /// Opt-in check against a real SimpleFIN server, normally with Bridge's public demo token
    /// (https://beta-bridge.simplefin.org/info/developers):
    /// `LOCALBOOKS_SIMPLEFIN_TOKEN=<token> cargo test simplefin_live -- --ignored --nocapture`
    #[tokio::test]
    #[ignore]
    async fn simplefin_live() {
        let Ok(token) = std::env::var("LOCALBOOKS_SIMPLEFIN_TOKEN") else { panic!("set LOCALBOOKS_SIMPLEFIN_TOKEN") };
        let pool = test_pool().await;
        let dir = temp_dir("live");
        let sf = service(&dir);
        let view = sf.connect(&pool, &token).await.unwrap();
        println!("server {} · messages {:?}", view.server_host, view.messages);
        for a in &view.accounts {
            println!("  {} · {:?} · {} · balance {:?} on {:?}", a.name, a.institution, a.currency, a.balance_cents, a.balance_date);
        }
        let today = Utc::now().date_naive();
        let checking = bank_id(&pool, "1010").await;
        let account = view.accounts.iter().find(|a| a.currency == "USD").expect("a USD account");
        link_account(&pool, account.id, checking, today - Days::new(60), today).await.unwrap();
        let summary = sf.sync(&pool, view.id, Utc::now()).await.unwrap();
        println!("{summary:#?}");
        let sample: Vec<(String, String, i64)> = sqlx::query_as("SELECT txn_date, description, amount_cents FROM bank_transactions ORDER BY txn_date DESC LIMIT 8")
            .fetch_all(&pool)
            .await
            .unwrap();
        for line in sample {
            println!("  {line:?}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}

