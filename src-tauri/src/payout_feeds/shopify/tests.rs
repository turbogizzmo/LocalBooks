use std::sync::{Arc, Mutex};

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::db::test_pool;
use crate::payout_feeds::{list_payouts, record, Apis, SECRETS_FILE};

#[test]
fn a_store_address_is_read_however_it_is_typed() {
    for typed in ["quill-studio", "Quill-Studio.myshopify.com", "https://quill-studio.myshopify.com/admin", " quill-studio.myshopify.com/ "] {
        assert_eq!(shop_domain(typed).unwrap(), "quill-studio.myshopify.com", "{typed}");
    }
    for bad in ["", "quill studio", "evil.com/quill", "-quill", "https://"] {
        assert!(shop_domain(bad).unwrap_err().starts_with("PAYOUT_SHOP"), "{bad}");
    }
}

/// A balance transaction; `fee` is what Shopify took, sent as `net = amount − fee`.
fn node(kind: &str, at: &str, amount: &str, fee: &str, payout: Option<&str>, order: &str) -> Value {
    let net = parse_shopify_amount(amount).unwrap() - parse_shopify_amount(fee).unwrap();
    json!({
        "type": kind,
        "transactionDate": at,
        "test": false,
        "amount": { "amount": amount },
        "net": { "amount": format!("{}{}.{:02}", if net < 0 { "-" } else { "" }, net.abs() / 100, net.abs() % 100) },
        "associatedPayout": payout.map(|p| json!({ "id": p })),
        "associatedOrder": if order.is_empty() { Value::Null } else { json!({ "name": order }) },
    })
}

/// Payout 501 of $243.05: sales of $200.00 and $100.00 with their fees, a $20.00 refund (fee
/// partly returned), a $15.00 dispute with its $15.00 fee, and $2.10 of reserve released.
fn payout_501() -> Vec<Value> {
    let p = Some("gid://shopify/ShopifyPaymentsPayout/501");
    vec![
        node("TRANSFER", "2026-03-09T00:00:00Z", "-243.05", "0.00", p, ""),
        node("CHARGE", "2026-03-02T10:00:00Z", "200.00", "6.10", p, "#1042"),
        node("CHARGE", "2026-03-03T10:00:00Z", "100.00", "3.20", p, "#1043"),
        node("REFUND", "2026-03-04T10:00:00Z", "-20.00", "-0.25", p, "#1040"),
        node("DISPUTE_WITHDRAWAL", "2026-03-05T10:00:00Z", "-15.00", "0.00", p, "#1031"),
        node("CHARGEBACK_FEE", "2026-03-05T10:00:00Z", "-15.00", "0.00", p, "#1031"),
        node("RESERVED_FUNDS_REVERSAL", "2026-03-06T10:00:00Z", "2.10", "0.00", p, ""),
    ]
}

#[test]
fn a_payout_breaks_down_into_gross_fees_refunds_and_chargebacks() {
    let txns: Vec<ShopifyTxn> = payout_501().iter().map(|n| read_txn(n).unwrap()).collect();
    let b = shopify_breakdown(24_305, &txns).unwrap();
    assert_eq!((b.gross_cents, b.fee_cents, b.refund_cents, b.chargeback_cents, b.other_cents), (30_000, 2_405, 2_000, 1_500, 210));
    let kinds: Vec<(&str, i64)> = b.items.iter().map(|i| (i.kind.as_str(), i.amount_cents)).collect();
    assert_eq!(kinds, vec![("CHARGE", 20_000), ("CHARGE", 10_000), ("REFUND", 2_000), ("CHARGEBACK", 1_500)]);
    assert_eq!(b.items[0].description, "Sale, order #1042");
    assert!(shopify_breakdown(24_306, &txns).unwrap_err().starts_with("PAYOUT_MISMATCH"));
}

// ---- a mock Shopify on 127.0.0.1 ---------------------------------------------------------------

async fn mock_shopify(requests: Arc<Mutex<Vec<String>>>) -> String {
    // The first transactions query is throttled once; the transactions come over two pages.
    let throttled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break };
            let requests = requests.clone();
            let throttled = throttled.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 32768];
                let mut n = 0;
                // Read the whole request: headers, then the JSON body.
                loop {
                    let got = socket.read(&mut buf[n..]).await.unwrap_or(0);
                    n += got;
                    let text = String::from_utf8_lossy(&buf[..n]);
                    if got == 0 || text.contains("\r\n\r\n") && text.trim_end().ends_with('}') {
                        break;
                    }
                }
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = request.lines().next().unwrap_or("").to_string();
                requests.lock().unwrap().push(line.clone());
                let authorized = request
                    .lines()
                    .any(|l| l.split_once(':').is_some_and(|(k, v)| k.eq_ignore_ascii_case("x-shopify-access-token") && v.trim() == "shpat_good"));
                let (code, body) = if !line.starts_with(&format!("POST /admin/api/{API_VERSION}/graphql.json")) {
                    (404, json!({ "errors": "Not Found" }))
                } else if !authorized {
                    (401, json!({ "errors": "[API] Invalid API key or access token (unrecognized login or wrong password)" }))
                } else if request.contains("currencyCode } shopifyPaymentsAccount") {
                    (
                        200,
                        json!({ "data": { "shop": { "currencyCode": "USD" }, "shopifyPaymentsAccount": { "id": "gid://shopify/ShopifyPaymentsAccount/1" } } }),
                    )
                } else if request.contains("payouts(first") {
                    (
                        200,
                        json!({ "data": { "shopifyPaymentsAccount": { "payouts": {
                        "pageInfo": { "hasNextPage": false, "endCursor": null },
                        "nodes": [
                            { "id": "gid://shopify/ShopifyPaymentsPayout/503", "legacyResourceId": "503", "issuedAt": "2026-03-11T00:00:00Z", "status": "SCHEDULED", "transactionType": "DEPOSIT", "net": { "amount": "10.00", "currencyCode": "USD" } },
                            { "id": "gid://shopify/ShopifyPaymentsPayout/502", "legacyResourceId": "502", "issuedAt": "2026-03-10T00:00:00Z", "status": "PAID", "transactionType": "DEPOSIT", "net": { "amount": "9.99", "currencyCode": "USD" } },
                            { "id": "gid://shopify/ShopifyPaymentsPayout/501", "legacyResourceId": "501", "issuedAt": "2026-03-09T00:00:00Z", "status": "PAID", "transactionType": "DEPOSIT", "net": { "amount": "243.05", "currencyCode": "USD" } },
                            { "id": "gid://shopify/ShopifyPaymentsPayout/400", "legacyResourceId": "400", "issuedAt": "2025-01-01T00:00:00Z", "status": "PAID", "transactionType": "DEPOSIT", "net": { "amount": "1.00", "currencyCode": "USD" } }
                        ] } } } }),
                    )
                } else if request.contains("balanceTransactions(first") && !throttled.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    (200, json!({ "errors": [{ "message": "Throttled", "extensions": { "code": "THROTTLED" } }] }))
                } else if request.contains("balanceTransactions(first") {
                    let mut nodes = payout_501();
                    nodes.push(node("CHARGE", "2026-03-08T10:00:00Z", "5.00", "0.15", Some("gid://shopify/ShopifyPaymentsPayout/502"), "#1050"));
                    nodes.sort_by(|a, b| b["transactionDate"].as_str().cmp(&a["transactionDate"].as_str()));
                    let second = request.contains("\"after\":\"page2\"");
                    let (a, b) = nodes.split_at(nodes.len() / 2);
                    let page_info =
                        if second { json!({ "hasNextPage": false, "endCursor": null }) } else { json!({ "hasNextPage": true, "endCursor": "page2" }) };
                    (
                        200,
                        json!({ "data": { "shopifyPaymentsAccount": { "balanceTransactions": { "pageInfo": page_info, "nodes": if second { b } else { a } } } } }),
                    )
                } else {
                    (200, json!({ "errors": [{ "message": "unexpected query", "extensions": { "code": "BAD_REQUEST" } }] }))
                };
                let body = body.to_string();
                let reply = format!("HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = socket.write_all(reply.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    base
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn new_account(pool: &SqlitePool, number: &str, name: &str, kind: &str) -> i64 {
    sqlx::query_scalar("INSERT INTO chart_of_accounts (account_number, name, account_type) VALUES (?1, ?2, ?3) RETURNING id")
        .bind(number)
        .bind(name)
        .bind(kind)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn balance(pool: &SqlitePool, id: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(id).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn a_shopify_payout_is_recorded_as_a_gross_deposit_that_lands_at_the_net() {
    let pool = test_pool().await;
    let bank = account(&pool, "1010").await;
    let input = FeedInput {
        name: "Shopify".into(),
        bank_account_id: bank,
        sales_account_id: new_account(&pool, "4890", "Shop Sales", "INCOME").await,
        fee_account_id: new_account(&pool, "6890", "Shopify Fees", "EXPENSE").await,
        refund_account_id: new_account(&pool, "4891", "Shop Refunds", "INCOME").await,
        chargeback_account_id: new_account(&pool, "6891", "Shop Chargebacks", "EXPENSE").await,
    };
    let requests = Arc::new(Mutex::new(Vec::new()));
    let api = mock_shopify(requests.clone()).await;
    let dir = std::env::temp_dir().join(format!("localbooks-shopify-{}", crate::mcp_server::new_token()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = SecretStore::in_file(dir.join(SECRETS_FILE), "LocalBooks payout feeds", "the payout feed key");

    assert!(requests.lock().unwrap().is_empty(), "nothing is called before the feed is set up");
    assert!(connect_shopify(&pool, &store, Some(&api), "quill-studio", "shpat_wrong", &input).await.unwrap_err().starts_with("PAYOUT_KEY"));
    assert!(connect_shopify(&pool, &store, Some(&api), "not a shop", "shpat_good", &input).await.unwrap_err().starts_with("PAYOUT_SHOP"));
    let f = connect_shopify(&pool, &store, Some(&api), "https://Quill-Studio.myshopify.com/admin", " shpat_good ", &input).await.unwrap();
    assert_eq!((f.processor.as_str(), f.account_ref.as_deref()), ("SHOPIFY", Some("quill-studio.myshopify.com")));
    assert!(std::fs::read_to_string(dir.join(SECRETS_FILE)).unwrap().contains("shpat_good"), "the token is in the credential store");
    let in_file: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM payout_feeds WHERE secret_key LIKE '%shpat%' OR account_ref LIKE '%shpat%'").fetch_one(&pool).await.unwrap();
    assert_eq!(in_file, 0, "and not in the company file");

    let now = DateTime::parse_from_rfc3339("2026-03-12T12:00:00Z").unwrap().with_timezone(&Utc);
    let apis = Apis { shopify: Some(api.clone()), ..Apis::all(&api) };
    let fetched = crate::payout_feeds::fetch(&pool, &store, &apis, f.id, now).await.unwrap();
    assert_eq!(fetched.new_payouts, 1, "{:?}", fetched.refused);
    assert_eq!(fetched.refused.len(), 1, "{:?}", fetched.refused);
    assert!(fetched.refused[0].starts_with("502: the payout's transactions add up to $4.85"), "{:?}", fetched.refused);
    assert!(requests.lock().unwrap().iter().all(|r| r.contains("/graphql.json")), "only the store's Admin API");
    assert_eq!(crate::payout_feeds::fetch(&pool, &store, &apis, f.id, now).await.unwrap().new_payouts, 0, "a payout is kept once");

    let listed = list_payouts(&pool, f.id).await.unwrap();
    assert_eq!(listed.len(), 1, "the scheduled payout and the one before the 90 days aren't kept");
    assert_eq!((listed[0].processor_payout_id.as_str(), listed[0].net_cents, listed[0].gross_cents), ("501", 24_305, 30_000));
    record(&pool, listed[0].id).await.unwrap();
    assert_eq!(balance(&pool, bank).await, 24_305);
    assert_eq!(balance(&pool, input.sales_account_id).await, -30_000);
    assert_eq!(balance(&pool, input.fee_account_id).await, 2_405 - 210);
    assert_eq!(balance(&pool, input.refund_account_id).await, 2_000);
    assert_eq!(balance(&pool, input.chargeback_account_id).await, 1_500);
    let memo: String = sqlx::query_scalar("SELECT memo FROM deposits ORDER BY id DESC LIMIT 1").fetch_one(&pool).await.unwrap();
    assert_eq!(memo, "Shopify Payments payout 501");
}
