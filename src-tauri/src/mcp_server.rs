//! Local, read-only Model Context Protocol server.
//!
//! Lets an AI assistant on this computer (Claude Code, Claude Desktop, …) read three accounting
//! summaries through MCP's Streamable HTTP transport (JSON responses, no streaming):
//! `get_trial_balance`, `get_profit_and_loss_summary` and `get_overdue_aging_metrics`.
//!
//! Privacy and safety:
//! - Off until the user turns it on in Settings.
//! - Listens on 127.0.0.1 only and drops any connection that is not from the loopback interface.
//!   It never opens an outbound connection.
//! - Every request needs `Authorization: Bearer <token>`; the token is generated on this machine
//!   and shown only in Settings.
//! - Requests carrying an `Origin` header (web pages) or a `Host` other than this server (DNS
//!   rebinding) are refused, as the MCP specification requires.
//! - The tools only run SELECT queries; nothing here can post, void or edit.

use std::future::Future;
use std::net::SocketAddr;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use chrono::{Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::export_engine::plain_amount;
use crate::ledger::db_err;
use crate::{invoices, preferences, statements};

pub const DEFAULT_PORT: u16 = 8321;
pub const ENDPOINT_PATH: &str = "/mcp";
/// Newest first; the first entry is offered when a client asks for a version we do not know.
pub const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const CONFIG_FILE: &str = "mcp.json";
const MAX_HEAD_BYTES: usize = 16 * 1024;
const MAX_BODY_BYTES: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

// ------------------------------------------------------------------------------------------------
// Settings (stored next to config.json, outside any company file)
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub token: Option<String>,
}

pub fn read_config(config_dir: &Path) -> McpConfig {
    std::fs::read(config_dir.join(CONFIG_FILE)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

pub fn write_config(config_dir: &Path, config: &McpConfig) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| format!("MCP_CONFIG: cannot create {}: {e}", config_dir.display()))?;
    let json = serde_json::to_vec_pretty(config).map_err(|e| format!("MCP_CONFIG: {e}"))?;
    crate::export_engine::write_atomically(&config_dir.join(CONFIG_FILE), &json)
}

/// A 256-bit random token as 64 hex characters. Each 64-bit block is a SipHash output under keys
/// that the standard library seeds from the operating system's random source, so the token cannot
/// be predicted without those keys.
pub fn new_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    (0..4u64)
        .map(|i| {
            let mut h = std::collections::hash_map::RandomState::new().build_hasher();
            h.write_u64(i);
            h.write_u128(nanos);
            h.write_u32(std::process::id());
            format!("{:016x}", h.finish())
        })
        .collect()
}

pub(crate) fn tokens_match(given: &str, expected: &str) -> bool {
    // Constant time in the token length, so response timing does not reveal a matching prefix.
    given.len() == expected.len() && given.bytes().zip(expected.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

// ------------------------------------------------------------------------------------------------
// Tools
// ------------------------------------------------------------------------------------------------

fn tool_definitions() -> Value {
    let date = |description: &str| json!({ "type": "string", "format": "date", "description": description });
    let read_only = json!({ "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false });
    json!([
        {
            "name": "get_trial_balance",
            "title": "Trial balance",
            "description": "Debit and credit balance of every account with activity, from posted journal entries up to and including asOf (default today). Amounts are integer cents in the home currency. Debits always equal credits.",
            "inputSchema": { "type": "object", "properties": { "asOf": date("Balance date, YYYY-MM-DD") }, "additionalProperties": false },
            "annotations": read_only
        },
        {
            "name": "get_profit_and_loss_summary",
            "title": "Profit and loss summary",
            "description": "Revenue, cost of goods sold, expenses and net income for a date range (default: current fiscal year to date), with the amount for each income and expense account. Amounts are integer cents in the home currency.",
            "inputSchema": {
                "type": "object",
                "properties": { "startDate": date("First day, YYYY-MM-DD"), "endDate": date("Last day, YYYY-MM-DD") },
                "additionalProperties": false
            },
            "annotations": read_only
        },
        {
            "name": "get_overdue_aging_metrics",
            "title": "Overdue receivables aging",
            "description": "Open customer invoices grouped by days past due (current, 1-30, 31-60, 61+) as of asOf (default today), the overdue share of receivables, and the most overdue invoices. Amounts are integer cents in the home currency.",
            "inputSchema": { "type": "object", "properties": { "asOf": date("Aging date, YYYY-MM-DD") }, "additionalProperties": false },
            "annotations": read_only
        }
    ])
}

fn date_arg(args: &Value, key: &str) -> Result<Option<NaiveDate>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => s.parse().map(Some).map_err(|_| format!("{key} must be a date in YYYY-MM-DD form")),
        Some(_) => Err(format!("{key} must be a date in YYYY-MM-DD form")),
    }
}

async fn trial_balance(pool: &SqlitePool, args: &Value) -> Result<(Value, String), String> {
    let as_of = date_arg(args, "asOf")?.unwrap_or_else(|| Local::now().date_naive());
    let rows: Vec<(Option<String>, String, String, i64, i64)> = sqlx::query_as(
        "SELECT c.account_number, c.full_name, c.account_type_name,
                SUM(p.debit_cents) AS debits, SUM(p.credit_cents) AS credits
         FROM v_posted_lines p JOIN v_chart_of_accounts c ON c.id = p.account_id
         WHERE p.entry_date <= ?1
         GROUP BY c.id HAVING SUM(p.debit_cents) <> SUM(p.credit_cents)
         ORDER BY c.type_sort_order, COALESCE(c.account_number, ''), c.full_name",
    )
    .bind(as_of.to_string())
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let (mut total_debit, mut total_credit) = (0i64, 0i64);
    let mut lines = Vec::new();
    let mut text = format!("Trial balance as of {as_of}\n");
    for (number, name, kind, debits, credits) in rows {
        let net = debits - credits;
        let (debit, credit) = if net > 0 { (net, 0) } else { (0, -net) };
        total_debit += debit;
        total_credit += credit;
        let label = number.as_deref().map(|n| format!("{n} {name}")).unwrap_or_else(|| name.clone());
        text.push_str(&format!("{label} ({kind}): debit {} credit {}\n", plain_amount(debit), plain_amount(credit)));
        lines.push(json!({ "accountNumber": number, "account": name, "accountType": kind, "debitCents": debit, "creditCents": credit }));
    }
    text.push_str(&format!("Totals: debit {} credit {}", plain_amount(total_debit), plain_amount(total_credit)));
    let data = json!({
        "asOf": as_of,
        "accounts": lines,
        "totalDebitCents": total_debit,
        "totalCreditCents": total_credit,
        "balanced": total_debit == total_credit
    });
    Ok((data, text))
}

async fn profit_and_loss(pool: &SqlitePool, args: &Value) -> Result<(Value, String), String> {
    let today = Local::now().date_naive();
    let ytd = preferences::fiscal_periods(today, preferences::fiscal_start_month(pool).await?).year_to_date;
    let start = date_arg(args, "startDate")?.unwrap_or(ytd.start);
    let end = date_arg(args, "endDate")?.unwrap_or(ytd.end);
    if start > end {
        return Err("startDate must be on or before endDate".into());
    }
    let pl = statements::profit_and_loss(pool, start, end).await?;
    let accounts: Vec<Value> = pl
        .lines
        .iter()
        .filter(|l| matches!(l.kind, statements::LineKind::Account))
        .map(|l| json!({ "account": l.label, "amountCents": l.amount_cents }))
        .collect();
    let text = format!(
        "Profit and loss {start} to {end}: revenue {}, gross profit {}, expenses {}, net income {}",
        plain_amount(pl.total_revenue_cents),
        plain_amount(pl.gross_profit_cents),
        plain_amount(pl.total_expenses_cents),
        plain_amount(pl.net_income_cents)
    );
    let data = json!({
        "startDate": start,
        "endDate": end,
        "totalRevenueCents": pl.total_revenue_cents,
        "grossProfitCents": pl.gross_profit_cents,
        "totalExpensesCents": pl.total_expenses_cents,
        "netIncomeCents": pl.net_income_cents,
        "accounts": accounts
    });
    Ok((data, text))
}

async fn overdue_aging(pool: &SqlitePool, args: &Value) -> Result<(Value, String), String> {
    let as_of = date_arg(args, "asOf")?.unwrap_or_else(|| Local::now().date_naive());
    let aging = invoices::ar_aging(pool, as_of).await?;
    let overdue: Vec<&invoices::AgingBucket> = aging.buckets.iter().filter(|b| b.key != "current").collect();
    let overdue_cents: i64 = overdue.iter().map(|b| b.amount_cents).sum();
    let overdue_count: i64 = overdue.iter().map(|b| b.invoice_count).sum();
    // Integer basis points (1/100 of a percent), rounded half up, so no floating point is involved.
    let share_bp = if aging.total_cents > 0 { (overdue_cents * 10_000 * 2 + aging.total_cents) / (aging.total_cents * 2) } else { 0 };

    let worst: Vec<(i64, String, NaiveDate, i64)> = sqlx::query_as(
        "SELECT i.invoice_number, i.customer_name, i.due_date, b.home_open_cents
         FROM invoices i JOIN v_invoice_balances b ON b.invoice_id = i.id
         WHERE i.status IN ('UNPAID', 'PARTIAL') AND i.is_voided = 0 AND i.due_date < ?1
         ORDER BY i.due_date, i.invoice_number LIMIT 10",
    )
    .bind(as_of.to_string())
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let most_overdue: Vec<Value> = worst
        .iter()
        .map(|(number, customer, due, open)| {
            json!({ "invoiceNumber": number, "customer": customer, "dueDate": due, "daysOverdue": (as_of - *due).num_days(), "openCents": open })
        })
        .collect();

    let buckets: Vec<Value> = aging
        .buckets
        .iter()
        .map(|b| json!({ "bucket": b.key, "label": b.label, "invoiceCount": b.invoice_count, "amountCents": b.amount_cents }))
        .collect();
    let text = format!(
        "Receivables as of {as_of}: {} open, {} overdue across {overdue_count} invoices ({}.{:02}% of receivables)",
        plain_amount(aging.total_cents),
        plain_amount(overdue_cents),
        share_bp / 100,
        share_bp % 100
    );
    let data = json!({
        "asOf": as_of,
        "totalOpenCents": aging.total_cents,
        "overdueCents": overdue_cents,
        "overdueInvoiceCount": overdue_count,
        "overdueShareBasisPoints": share_bp,
        "buckets": buckets,
        "mostOverdue": most_overdue
    });
    Ok((data, text))
}

async fn call_tool(pool: &SqlitePool, name: &str, args: &Value) -> Option<Result<(Value, String), String>> {
    Some(match name {
        "get_trial_balance" => trial_balance(pool, args).await,
        "get_profit_and_loss_summary" => profit_and_loss(pool, args).await,
        "get_overdue_aging_metrics" => overdue_aging(pool, args).await,
        _ => return None,
    })
}

// ------------------------------------------------------------------------------------------------
// JSON-RPC 2.0 / MCP message handling
// ------------------------------------------------------------------------------------------------

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn rpc_result(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

/// Handles one JSON-RPC message. Returns `None` for notifications and client responses, which get
/// an HTTP 202 with no body.
pub async fn handle_message(pool: &SqlitePool, message: Value) -> Option<Value> {
    if message.is_array() {
        return Some(rpc_error(Value::Null, -32600, "Batch requests are not supported"));
    }
    let id = message.get("id").cloned();
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Some(rpc_error(id.unwrap_or(Value::Null), -32600, "Invalid Request: jsonrpc must be \"2.0\""));
    }
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        // A response to a server request (we never send any) or garbage without an id.
        return id.filter(|_| message.get("result").is_none() && message.get("error").is_none()).map(|id| rpc_error(id, -32600, "Invalid Request: method is required"));
    };
    let id = id?; // notifications (no id) are acknowledged without a body
    let params = message.get("params").cloned().unwrap_or(Value::Null);

    Some(match method {
        "initialize" => {
            let requested = params.get("protocolVersion").and_then(Value::as_str).unwrap_or_default();
            let version = PROTOCOL_VERSIONS.iter().find(|v| **v == requested).copied().unwrap_or(PROTOCOL_VERSIONS[0]);
            rpc_result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false } },
                    "serverInfo": { "name": "localbooks", "title": "LocalBooks", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": "Read-only accounting summaries from the LocalBooks company file open on this computer. All amounts are integer cents in the company's home currency; divide by 100 for display. Nothing here can change the books."
                }),
            )
        }
        "ping" => rpc_result(id, json!({})),
        "tools/list" => rpc_result(id, json!({ "tools": tool_definitions() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            match call_tool(pool, name, &args).await {
                None => rpc_error(id, -32602, &format!("Unknown tool: {name}")),
                Some(Ok((data, text))) => {
                    rpc_result(id, json!({ "content": [{ "type": "text", "text": text }], "structuredContent": data, "isError": false }))
                }
                Some(Err(err)) => rpc_result(id, json!({ "content": [{ "type": "text", "text": err }], "isError": true })),
            }
        }
        other => rpc_error(id, -32601, &format!("Method not found: {other}")),
    })
}

// ------------------------------------------------------------------------------------------------
// HTTP transport
// ------------------------------------------------------------------------------------------------

#[derive(Debug)]
struct Request {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

struct Response {
    status: u16,
    body: Option<Value>,
    extra_headers: &'static [(&'static str, &'static str)],
}

impl Response {
    fn json(status: u16, body: Value) -> Self {
        Self { status, body: Some(body), extra_headers: &[] }
    }

    fn refuse(status: u16, message: &str) -> Self {
        Self::json(status, json!({ "error": message }))
    }

    fn to_bytes(&self) -> Vec<u8> {
        let reason = match self.status {
            200 => "OK",
            202 => "Accepted",
            400 => "Bad Request",
            401 => "Unauthorized",
            403 => "Forbidden",
            404 => "Not Found",
            405 => "Method Not Allowed",
            408 => "Request Timeout",
            413 => "Payload Too Large",
            415 => "Unsupported Media Type",
            _ => "Error",
        };
        let body = self.body.as_ref().map(|b| b.to_string()).unwrap_or_default();
        let mut head = format!("HTTP/1.1 {} {reason}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n", self.status, body.len());
        if self.body.is_some() {
            head.push_str("Content-Type: application/json\r\n");
        }
        for (k, v) in self.extra_headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        let mut bytes = head.into_bytes();
        bytes.extend_from_slice(body.as_bytes());
        bytes
    }
}

async fn read_request(stream: &mut TcpStream) -> Result<Request, Response> {
    let mut buf = Vec::with_capacity(4096);
    let head_end = loop {
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        if buf.len() > MAX_HEAD_BYTES {
            return Err(Response::refuse(413, "Request headers too large"));
        }
        let mut chunk = [0u8; 4096];
        let n = stream.read(&mut chunk).await.map_err(|_| Response::refuse(400, "Unreadable request"))?;
        if n == 0 {
            return Err(Response::refuse(400, "Incomplete request"));
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| Response::refuse(400, "Request headers must be UTF-8"))?;
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next().unwrap_or_default().split(' ');
    let (method, path) = match (request_line.next(), request_line.next()) {
        (Some(m), Some(p)) if !m.is_empty() => (m.to_string(), p.to_string()),
        _ => return Err(Response::refuse(400, "Malformed request line")),
    };
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    if headers.iter().any(|(k, _)| k == "transfer-encoding") {
        return Err(Response::refuse(400, "Chunked request bodies are not supported; send Content-Length"));
    }
    let length = match headers.iter().find(|(k, _)| k == "content-length") {
        None => 0,
        Some((_, v)) => v.parse::<usize>().map_err(|_| Response::refuse(400, "Invalid Content-Length"))?,
    };
    if length > MAX_BODY_BYTES {
        return Err(Response::refuse(413, "Request body too large"));
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let mut chunk = vec![0u8; (length - body.len()).min(64 * 1024)];
        let n = stream.read(&mut chunk).await.map_err(|_| Response::refuse(400, "Unreadable request body"))?;
        if n == 0 {
            return Err(Response::refuse(400, "Incomplete request body"));
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Ok(Request { method, path, headers, body })
}

/// Transport-level checks, in the order a client is most likely to need to fix them.
fn check_request(req: &Request, token: &str, port: u16) -> Result<(), Response> {
    if req.header("origin").is_some() {
        return Err(Response::refuse(403, "Requests from web pages are not accepted"));
    }
    let host_ok = req.header("host").is_some_and(|h| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}"));
    if !host_ok {
        return Err(Response::refuse(403, "Invalid Host header"));
    }
    if req.path.split('?').next() != Some(ENDPOINT_PATH) {
        return Err(Response::refuse(404, "The MCP endpoint is /mcp"));
    }
    if req.method != "POST" {
        return Err(Response { status: 405, body: Some(json!({ "error": "Use POST" })), extra_headers: &[("Allow", "POST")] });
    }
    let bearer = req.header("authorization").and_then(|v| v.strip_prefix("Bearer ")).map(str::trim);
    if !bearer.is_some_and(|given| tokens_match(given, token)) {
        return Err(Response {
            status: 401,
            body: Some(json!({ "error": "Missing or wrong access token" })),
            extra_headers: &[("WWW-Authenticate", "Bearer realm=\"LocalBooks\"")],
        });
    }
    if !req.header("content-type").is_some_and(|v| v.to_ascii_lowercase().starts_with("application/json")) {
        return Err(Response::refuse(415, "Content-Type must be application/json"));
    }
    if let Some(version) = req.header("mcp-protocol-version") {
        if !PROTOCOL_VERSIONS.contains(&version) {
            return Err(Response::refuse(400, "Unsupported MCP-Protocol-Version"));
        }
    }
    Ok(())
}

/// Yields the live company file's pool for each request, so a restore or File › Open is followed.
pub type PoolSource = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = SqlitePool> + Send>> + Send + Sync>;

async fn serve_connection(mut stream: TcpStream, token: &str, port: u16, pools: &PoolSource) {
    let response = match tokio::time::timeout(REQUEST_TIMEOUT, read_request(&mut stream)).await {
        Err(_) => Response::refuse(408, "Request timed out"),
        Ok(Err(response)) => response,
        Ok(Ok(req)) => match check_request(&req, token, port) {
            Err(response) => response,
            Ok(()) => match serde_json::from_slice::<Value>(&req.body) {
                Err(_) => Response::json(400, rpc_error(Value::Null, -32700, "Parse error")),
                Ok(message) => match handle_message(&pools().await, message).await {
                    Some(reply) => Response::json(200, reply),
                    None => Response { status: 202, body: None, extra_headers: &[] },
                },
            },
        },
    };
    let _ = stream.write_all(&response.to_bytes()).await;
    let _ = stream.shutdown().await;
}

/// A running server. Dropping it (or calling [`ServerHandle::stop`]) closes the listening socket.
pub struct ServerHandle {
    pub addr: SocketAddr,
    shutdown: tokio::sync::watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl ServerHandle {
    pub async fn stop(self) {
        let _ = self.shutdown.send(true);
        let _ = self.task.await;
    }
}

/// Starts listening on 127.0.0.1:`port` (0 picks a free port).
pub async fn start(port: u16, token: String, pools: PoolSource) -> Result<ServerHandle, String> {
    if token.len() < 32 {
        return Err("MCP_TOKEN: the access token is missing".into());
    }
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            format!("MCP_PORT_IN_USE: another program is already using port {port} on this computer")
        } else {
            format!("MCP_BIND: cannot listen on 127.0.0.1:{port}: {e}")
        }
    })?;
    let addr = listener.local_addr().map_err(|e| format!("MCP_BIND: {e}"))?;
    let (shutdown, mut stopped) = tokio::sync::watch::channel(false);
    let token: Arc<str> = token.into();
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = stopped.changed() => break,
                accepted = listener.accept() => {
                    let Ok((stream, peer)) = accepted else { continue };
                    if !peer.ip().is_loopback() {
                        continue; // unreachable while bound to 127.0.0.1; kept as a second guard
                    }
                    let (token, pools) = (token.clone(), pools.clone());
                    tokio::spawn(async move { serve_connection(stream, &token, addr.port(), &pools).await });
                }
            }
        }
    });
    Ok(ServerHandle { addr, shutdown, task })
}

// ------------------------------------------------------------------------------------------------
// App integration: on/off state shown in Settings
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub enabled: bool,
    pub running: bool,
    pub url: String,
    pub token: Option<String>,
    pub error: Option<String>,
}

/// Owns the running server (if any) and its settings file.
pub struct McpService {
    config_dir: std::path::PathBuf,
    pools: PoolSource,
    inner: tokio::sync::Mutex<(Option<ServerHandle>, Option<String>)>,
}

impl McpService {
    pub fn new(config_dir: std::path::PathBuf, pools: PoolSource) -> Self {
        Self { config_dir, pools, inner: tokio::sync::Mutex::new((None, None)) }
    }

    pub async fn status(&self) -> McpStatus {
        let config = read_config(&self.config_dir);
        let inner = self.inner.lock().await;
        let addr = inner.0.as_ref().map(|h| h.addr).unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT)));
        McpStatus {
            enabled: config.enabled,
            running: inner.0.is_some(),
            url: format!("http://{addr}{ENDPOINT_PATH}"),
            token: config.token,
            error: inner.1.clone(),
        }
    }

    /// Starts the server if the saved settings say it should run (called at launch).
    pub async fn start_if_enabled(&self) {
        let config = read_config(&self.config_dir);
        if config.enabled {
            if let Some(token) = config.token {
                self.restart(token).await;
            }
        }
    }

    async fn restart(&self, token: String) {
        let mut inner = self.inner.lock().await;
        if let Some(handle) = inner.0.take() {
            handle.stop().await;
        }
        match start(DEFAULT_PORT, token, self.pools.clone()).await {
            Ok(handle) => *inner = (Some(handle), None),
            Err(err) => *inner = (None, Some(err)),
        }
    }

    pub async fn set_enabled(&self, enabled: bool) -> Result<McpStatus, String> {
        let mut config = read_config(&self.config_dir);
        config.enabled = enabled;
        if enabled && config.token.is_none() {
            config.token = Some(new_token());
        }
        write_config(&self.config_dir, &config)?;
        if enabled {
            self.restart(config.token.clone().expect("set above")).await;
        } else {
            let mut inner = self.inner.lock().await;
            if let Some(handle) = inner.0.take() {
                handle.stop().await;
            }
            inner.1 = None;
        }
        Ok(self.status().await)
    }

    /// Issues a new token; the old one stops working immediately.
    pub async fn regenerate_token(&self) -> Result<McpStatus, String> {
        let mut config = read_config(&self.config_dir);
        let token = new_token();
        config.token = Some(token.clone());
        write_config(&self.config_dir, &config)?;
        if config.enabled {
            self.restart(token).await;
        }
        Ok(self.status().await)
    }

    pub async fn shutdown(&self) {
        if let Some(handle) = self.inner.lock().await.0.take() {
            handle.stop().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};

    async fn account(pool: &SqlitePool, number: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
    }

    async fn post(pool: &SqlitePool, date: &str, debit: i64, credit: i64, cents: i64) {
        post_journal_entry(
            pool,
            JournalEntry {
                id: None,
                date: date.parse().unwrap(),
                description: String::new(),
                lines: vec![JournalLine::debit(debit, cents), JournalLine::credit(credit, cents)],
                currency: None,
            },
        )
        .await
        .unwrap();
    }

    async fn call(pool: &SqlitePool, id: i64, method: &str, params: Value) -> Value {
        handle_message(pool, json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await.unwrap()
    }

    #[tokio::test]
    async fn speaks_mcp_lifecycle_and_lists_read_only_tools() {
        let pool = test_pool().await;
        let init = call(&pool, 1, "initialize", json!({ "protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } })).await;
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26", "a supported version is echoed");
        assert_eq!(init["result"]["serverInfo"]["name"], "localbooks");
        let init = call(&pool, 2, "initialize", json!({ "protocolVersion": "1999-01-01" })).await;
        assert_eq!(init["result"]["protocolVersion"], PROTOCOL_VERSIONS[0], "an unknown version gets our latest");

        assert!(handle_message(&pool, json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await.is_none());
        assert!(handle_message(&pool, json!({ "jsonrpc": "2.0", "id": 9, "result": {} })).await.is_none(), "client responses get no body");
        assert_eq!(call(&pool, 3, "ping", json!({})).await["result"], json!({}));

        let tools = call(&pool, 4, "tools/list", json!({})).await;
        let tools = tools["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["get_trial_balance", "get_profit_and_loss_summary", "get_overdue_aging_metrics"]);
        assert!(tools.iter().all(|t| t["annotations"]["readOnlyHint"] == true && t["inputSchema"]["type"] == "object"));

        assert_eq!(call(&pool, 5, "resources/list", json!({})).await["error"]["code"], -32601);
        assert_eq!(call(&pool, 6, "tools/call", json!({ "name": "post_journal_entry", "arguments": {} })).await["error"]["code"], -32602);
        assert_eq!(handle_message(&pool, json!([{ "jsonrpc": "2.0", "id": 1, "method": "ping" }])).await.unwrap()["error"]["code"], -32600);
        assert_eq!(handle_message(&pool, json!({ "id": 1, "method": "ping" })).await.unwrap()["error"]["code"], -32600);

        let bad = call(&pool, 7, "tools/call", json!({ "name": "get_trial_balance", "arguments": { "asOf": "yesterday" } })).await;
        assert_eq!(bad["result"]["isError"], true, "bad arguments are a tool error the model can read");
    }

    #[tokio::test]
    async fn tools_report_the_books_without_changing_them() {
        let pool = test_pool().await;
        let (checking, services, rent, owner) = (account(&pool, "1010").await, account(&pool, "4100").await, account(&pool, "6100").await, account(&pool, "3000").await);
        post(&pool, "2026-01-02", checking, owner, 1_000_000).await;
        post(&pool, "2026-03-10", checking, services, 250_000).await;
        post(&pool, "2026-03-15", rent, checking, 120_000).await;
        post(&pool, "2026-12-01", checking, services, 99_999).await; // after the as-of date below
        let before: (i64, i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM journal_entries), (SELECT COUNT(*) FROM ledger_audit_log)").fetch_one(&pool).await.unwrap();

        let tb = call(&pool, 1, "tools/call", json!({ "name": "get_trial_balance", "arguments": { "asOf": "2026-06-30" } })).await;
        let tb = &tb["result"]["structuredContent"];
        assert_eq!(tb["balanced"], true);
        assert_eq!(tb["totalDebitCents"], 1_000_000 + 250_000 - 120_000 + 120_000);
        let checking_row = tb["accounts"].as_array().unwrap().iter().find(|a| a["accountNumber"] == "1010").unwrap();
        assert_eq!(checking_row["debitCents"], 1_130_000, "December activity is excluded");

        let pl = call(&pool, 2, "tools/call", json!({ "name": "get_profit_and_loss_summary", "arguments": { "startDate": "2026-01-01", "endDate": "2026-06-30" } })).await;
        let pl = &pl["result"]["structuredContent"];
        assert_eq!(pl["totalRevenueCents"], 250_000);
        assert_eq!(pl["netIncomeCents"], 130_000);
        let reversed = call(&pool, 3, "tools/call", json!({ "name": "get_profit_and_loss_summary", "arguments": { "startDate": "2026-06-30", "endDate": "2026-01-01" } })).await;
        assert_eq!(reversed["result"]["isError"], true);

        // Two open invoices: one 45 days overdue, one not yet due.
        let mk = |customer: &str, due: &str, cents: i64| {
            let pool = pool.clone();
            let input: invoices::InvoiceInput = serde_json::from_value(json!({
                "customerName": customer, "issueDate": "2026-04-01", "dueDate": due,
                "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }]
            }))
            .unwrap();
            async move { invoices::create_and_post_invoice(&pool, input).await.unwrap().invoice_number }
        };
        let late = mk("Slow Payer", "2026-05-16", 40_000).await;
        mk("Prompt Payer", "2026-07-31", 60_000).await;
        let aging = call(&pool, 4, "tools/call", json!({ "name": "get_overdue_aging_metrics", "arguments": { "asOf": "2026-06-30" } })).await;
        let text = aging["result"]["content"][0]["text"].as_str().unwrap().to_string();
        let aging = &aging["result"]["structuredContent"];
        assert_eq!(aging["totalOpenCents"], 100_000);
        assert_eq!(aging["overdueCents"], 40_000);
        assert_eq!(aging["overdueShareBasisPoints"], 4_000);
        assert_eq!(aging["mostOverdue"][0]["invoiceNumber"], late);
        assert_eq!(aging["mostOverdue"][0]["customer"], "Slow Payer");
        assert_eq!(aging["mostOverdue"].as_array().unwrap().len(), 1, "invoices not yet due are not overdue");
        assert_eq!(aging["mostOverdue"][0]["daysOverdue"], 45);
        assert!(text.contains("40.00%"), "{text}");

        let after: (i64, i64) = sqlx::query_as("SELECT (SELECT COUNT(*) FROM journal_entries), (SELECT COUNT(*) FROM ledger_audit_log)").fetch_one(&pool).await.unwrap();
        assert_eq!(after.0, before.0 + 2, "only the two invoices this test created were posted");
        assert_eq!(after.1, before.1 + 2);
    }

    async fn send(addr: SocketAddr, raw: String) -> (u16, String, String) {
        let mut stream = TcpStream::connect(addr).await.unwrap();
        stream.write_all(raw.as_bytes()).await.unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).await.unwrap();
        let status: u16 = out[9..12].parse().unwrap();
        let (head, body) = out.split_once("\r\n\r\n").unwrap();
        (status, head.to_string(), body.to_string())
    }

    fn post_raw(port: u16, token: Option<&str>, extra: &str, body: &str) -> String {
        let auth = token.map(|t| format!("Authorization: Bearer {t}\r\n")).unwrap_or_default();
        format!(
            "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{auth}Content-Type: application/json\r\nAccept: application/json, text/event-stream\r\n{extra}Content-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn http_transport_admits_only_authorized_local_clients() {
        let pool = test_pool().await;
        let source_pool = pool.clone();
        let pools: PoolSource = Arc::new(move || {
            let pool = source_pool.clone();
            Box::pin(async move { pool })
        });
        let token = new_token();
        assert_eq!(token.len(), 64);
        assert_ne!(token, new_token());
        assert!(start(0, "short".into(), pools.clone()).await.is_err());

        let server = start(0, token.clone(), pools).await.unwrap();
        let addr = server.addr;
        assert!(addr.ip().is_loopback());
        let port = addr.port();
        let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#;

        let (status, _, body) = send(addr, post_raw(port, Some(&token), "", init)).await;
        assert_eq!(status, 200);
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["result"]["protocolVersion"], "2025-06-18");

        let (status, head, _) = send(addr, post_raw(port, None, "", init)).await;
        assert_eq!(status, 401);
        assert!(head.contains("WWW-Authenticate: Bearer"));
        assert_eq!(send(addr, post_raw(port, Some("0".repeat(64).as_str()), "", init)).await.0, 401);
        assert_eq!(send(addr, post_raw(port, Some(&token), "Origin: https://evil.example\r\n", init)).await.0, 403, "web pages are refused");
        let rebound = post_raw(port, Some(&token), "", init).replace(&format!("Host: 127.0.0.1:{port}"), "Host: attacker.example");
        assert_eq!(send(addr, rebound).await.0, 403, "DNS-rebinding hosts are refused");
        assert_eq!(send(addr, post_raw(port, Some(&token), "", init).replace("POST /mcp", "POST /other")).await.0, 404);
        assert_eq!(send(addr, format!("GET /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n")).await.0, 405);
        assert_eq!(send(addr, post_raw(port, Some(&token), "", init).replace("Content-Type: application/json", "Content-Type: text/plain")).await.0, 415);
        assert_eq!(send(addr, post_raw(port, Some(&token), "MCP-Protocol-Version: 1999-01-01\r\n", init)).await.0, 400);
        let (status, _, body) = send(addr, post_raw(port, Some(&token), "", "{not json")).await;
        assert_eq!((status, serde_json::from_str::<Value>(&body).unwrap()["error"]["code"].as_i64()), (400, Some(-32700)));
        let (status, _, body) = send(addr, post_raw(port, Some(&token), "MCP-Protocol-Version: 2025-06-18\r\n", r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)).await;
        assert_eq!((status, body.as_str()), (202, ""));

        let call = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_trial_balance","arguments":{}}}"#;
        let (status, _, body) = send(addr, post_raw(port, Some(&token), "", call)).await;
        assert_eq!(status, 200);
        assert_eq!(serde_json::from_str::<Value>(&body).unwrap()["result"]["structuredContent"]["balanced"], true);

        server.stop().await;
        assert!(TcpStream::connect(addr).await.is_err(), "stopping closes the port");
    }

    #[test]
    fn settings_round_trip_through_mcp_json() {
        let dir = std::env::temp_dir().join(format!("localbooks-mcp-{}", std::process::id()));
        assert_eq!(read_config(&dir), McpConfig::default(), "missing settings mean off");
        let config = McpConfig { enabled: true, token: Some(new_token()) };
        write_config(&dir, &config).unwrap();
        assert_eq!(read_config(&dir), config);
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(tokens_match("abc", "abc") && !tokens_match("abc", "abd") && !tokens_match("abc", "abcd"));
    }
}
