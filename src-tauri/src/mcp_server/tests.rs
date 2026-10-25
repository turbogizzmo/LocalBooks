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
