use std::sync::{Arc, Mutex};

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::db::test_pool;
use crate::payout_feeds::{list_payouts, record, Apis, SECRETS_FILE};

#[test]
fn amounts_are_read_as_cents_without_floats() {
    assert_eq!(parse_cents("12.34").unwrap(), 1_234);
    assert_eq!(parse_cents("-0.59").unwrap(), -59);
    assert_eq!(parse_cents("7.5").unwrap(), 750);
    assert_eq!(parse_cents("1000").unwrap(), 100_000);
    assert_eq!(parse_cents("+3.00").unwrap(), 300);
    for bad in ["", "1.234", "1,000.00", "abc", "-", ".5", "1e3"] {
        assert!(parse_cents(bad).is_err(), "{bad}");
    }
}

fn detail(id: &str, code: &str, at: &str, amount: &str, fee: Option<&str>, subject: &str) -> Value {
    let mut info = json!({
        "transaction_id": id,
        "transaction_event_code": code,
        "transaction_initiation_date": at,
        "transaction_amount": { "currency_code": "USD", "value": amount },
        "transaction_status": "S",
        "transaction_subject": subject,
    });
    if let Some(fee) = fee {
        info["fee_amount"] = json!({ "currency_code": "USD", "value": fee });
    }
    json!({ "transaction_info": info })
}

/// A month of PayPal activity between two withdrawals: two payments with their fees, a refund with
/// its fee partly returned, a chargeback with its fee, a hold and its release; then $181.55 out.
fn month() -> Vec<Value> {
    vec![
        detail("W0", "T0400", "2026-02-01T09:00:00+0000", "-50.00", None, ""),
        detail("P1", "T0006", "2026-02-03T10:00:00+0000", "120.00", Some("-3.78"), "Order 501"),
        detail("P2", "T0007", "2026-02-05T11:00:00+0000", "80.00", Some("-2.62"), "Order 502"),
        detail("R1", "T1107", "2026-02-07T12:00:00+0000", "-10.00", Some("0.30"), "Refund for order 499"),
        detail("C1", "T1201", "2026-02-10T13:00:00+0000", "-20.00", None, "Chargeback, order 480"),
        detail("F1", "T0106", "2026-02-10T13:05:00+0000", "-20.00", None, "Chargeback fee"),
        detail("H1", "T1110", "2026-02-11T08:00:00+0000", "-5.00", None, "Hold"),
        detail("H2", "T1111", "2026-02-12T08:00:00+0000", "5.00", None, "Hold released"),
        detail("PEND", "T0006", "2026-02-13T08:00:00+0000", "999.00", Some("-1.00"), "Pending"),
        detail("W1", "T0403", "2026-02-14T09:00:00+0000", "-143.90", None, ""),
    ]
}

fn txns(details: &[Value]) -> Vec<PayPalTxn> {
    let mut d = details.to_vec();
    d[8]["transaction_info"]["transaction_status"] = json!("P");
    d.iter().filter_map(|v| read_txn(v).unwrap()).collect()
}

#[test]
fn a_withdrawal_breaks_down_into_what_came_in_since_the_last_one() {
    let found = withdrawals(txns(&month()));
    assert_eq!(found.len(), 2);
    assert!(found[0].carried.is_none(), "the first one's start isn't in the history");
    let carried = found[1].carried.clone().unwrap();
    assert_eq!(carried.len(), 7, "the pending payment doesn't count");
    let b = paypal_breakdown(14_390, &carried).unwrap();
    // Sales 200.00; fees 3.78 + 2.62 - 0.30 + 20.00 = 26.10; refund 10.00; chargeback 20.00; hold nets to 0.
    assert_eq!((b.gross_cents, b.fee_cents, b.refund_cents, b.chargeback_cents, b.other_cents), (20_000, 2_610, 1_000, 2_000, 0));
    let kinds: Vec<(&str, i64)> = b.items.iter().map(|i| (i.kind.as_str(), i.amount_cents)).collect();
    assert_eq!(kinds, vec![("CHARGE", 12_000), ("CHARGE", 8_000), ("REFUND", 1_000), ("CHARGEBACK", 2_000)]);
    assert!(paypal_breakdown(10_000, &carried).unwrap_err().starts_with("PAYOUT_MISMATCH"), "part of the balance can't be broken down");
}

#[test]
fn a_transaction_in_the_withdrawals_second_belongs_to_it() {
    let mut d = vec![
        detail("W0", "T0400", "2026-02-01T09:00:00+0000", "-1.00", None, ""),
        // Listed after the withdrawal, in the same second.
        detail("W1", "T0400", "2026-02-02T09:00:00+0000", "-10.00", None, ""),
        detail("P1", "T0006", "2026-02-02T09:00:00+0000", "10.00", None, "Order"),
    ];
    d.push(detail("X", "T0006", "2026-02-02T08:00:00+0000", "1.234", None, "unreadable"));
    let txns: Vec<PayPalTxn> = d.iter().filter_map(|v| read_txn(v).ok().flatten()).collect();
    let found = withdrawals(txns);
    assert_eq!(found[1].carried.as_ref().unwrap().iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["P1"]);
    assert!(format!("{:?}", PayPalCredentials { client_id: "id".into(), secret: "s3cret".into(), sandbox: false }).contains("…"));
    assert!(
        !format!("{:?}", PayPalCredentials { client_id: "id".into(), secret: "s3cret".into(), sandbox: false }).contains("s3cret"),
        "Debug hides the secret"
    );
}

// ---- a mock PayPal on 127.0.0.1 ----------------------------------------------------------------

const BASIC_GOOD: &str = "Basic YXBwLWlkOmFwcC1zZWNyZXQ="; // app-id:app-secret

async fn mock_paypal(requests: Arc<Mutex<Vec<String>>>, details: Vec<Value>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break };
            let requests = requests.clone();
            let details = details.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16384];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = head.lines().next().unwrap_or("").to_string();
                requests.lock().unwrap().push(line.clone());
                let lower = head.to_lowercase();
                let (code, body) = if line.starts_with("POST /v1/oauth2/token") {
                    if head.lines().any(|l| l.split_once(':').is_some_and(|(k, v)| k.eq_ignore_ascii_case("authorization") && v.trim() == BASIC_GOOD)) {
                        (200, json!({ "access_token": "A21-token", "token_type": "Bearer", "expires_in": 32_400 }))
                    } else {
                        (401, json!({ "error": "invalid_client", "error_description": "Client Authentication failed" }))
                    }
                } else if !lower.contains("authorization: bearer a21-token") {
                    (401, json!({ "name": "AUTHENTICATION_FAILURE", "message": "Authentication failed" }))
                } else if line.starts_with("GET /v1/reporting/transactions") {
                    // Every window gets the whole month, over two pages; LocalBooks drops the repeats.
                    let second = line.contains("&page=2");
                    let (a, b) = details.split_at(details.len() / 2);
                    (200, json!({ "transaction_details": if second { b } else { a }, "total_pages": 2, "page": if second { 2 } else { 1 } }))
                } else {
                    (404, json!({ "name": "NOT_FOUND" }))
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
async fn a_paypal_withdrawal_is_recorded_as_a_gross_deposit_that_lands_at_the_net() {
    let pool = test_pool().await;
    let bank = account(&pool, "1010").await;
    let input = FeedInput {
        name: "PayPal".into(),
        bank_account_id: bank,
        sales_account_id: new_account(&pool, "4880", "PayPal Sales", "INCOME").await,
        fee_account_id: new_account(&pool, "6880", "PayPal Fees", "EXPENSE").await,
        refund_account_id: new_account(&pool, "4881", "PayPal Refunds", "INCOME").await,
        chargeback_account_id: new_account(&pool, "6881", "PayPal Chargebacks", "EXPENSE").await,
    };
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut details = month();
    details[8]["transaction_info"]["transaction_status"] = json!("P");
    let api = mock_paypal(requests.clone(), details).await;
    let dir = std::env::temp_dir().join(format!("localbooks-paypal-{}", crate::mcp_server::new_token()));
    std::fs::create_dir_all(&dir).unwrap();
    let store = SecretStore::in_file(dir.join(SECRETS_FILE), "LocalBooks payout feeds", "the payout feed key");
    let now = DateTime::parse_from_rfc3339("2026-02-20T12:00:00Z").unwrap().with_timezone(&Utc);

    let creds = |id: &str, secret: &str| PayPalCredentials { client_id: id.into(), secret: secret.into(), sandbox: true };
    assert!(requests.lock().unwrap().is_empty(), "nothing is called before the feed is set up");
    assert!(connect_paypal(&pool, &store, &api, &creds("app-id", "wrong"), &input, now).await.unwrap_err().starts_with("PAYOUT_KEY"));
    assert!(connect_paypal(&pool, &store, &api, &creds("", ""), &input, now).await.unwrap_err().starts_with("PAYOUT_KEY"));
    let f = connect_paypal(&pool, &store, &api, &creds(" app-id ", "app-secret"), &input, now).await.unwrap();
    assert_eq!((f.processor.as_str(), f.account_ref.as_deref()), ("PAYPAL", Some("SANDBOX")));
    let kept = std::fs::read_to_string(dir.join(SECRETS_FILE)).unwrap();
    assert!(kept.contains("app-secret"), "the credentials are in the credential store");
    let in_file: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM payout_feeds WHERE secret_key LIKE '%app-secret%' OR account_ref LIKE '%app-%'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(in_file, 0, "and not in the company file");

    let fetched = crate::payout_feeds::fetch(&pool, &store, &Apis::all(&api), f.id, now).await.unwrap();
    assert_eq!(fetched.new_payouts, 1, "{:?}", fetched.refused);
    assert!(fetched.refused.iter().any(|r| r.starts_with("W0: the withdrawal before it")), "{:?}", fetched.refused);
    assert!(requests.lock().unwrap().iter().all(|r| r.contains("/v1/oauth2/token") || r.contains("/v1/reporting/transactions")), "only PayPal's API");
    assert_eq!(crate::payout_feeds::fetch(&pool, &store, &Apis::all(&api), f.id, now).await.unwrap().new_payouts, 0, "a payout is kept once");

    let listed = list_payouts(&pool, f.id).await.unwrap();
    let w1 = listed.iter().find(|p| p.processor_payout_id == "W1").unwrap();
    assert_eq!((w1.net_cents, w1.gross_cents, w1.arrival_date), (14_390, 20_000, NaiveDate::from_ymd_opt(2026, 2, 14).unwrap()));
    record(&pool, w1.id).await.unwrap();
    assert_eq!(balance(&pool, bank).await, 14_390);
    assert_eq!(balance(&pool, input.sales_account_id).await, -20_000);
    assert_eq!(balance(&pool, input.fee_account_id).await, 2_610);
    assert_eq!(balance(&pool, input.refund_account_id).await, 1_000);
    assert_eq!(balance(&pool, input.chargeback_account_id).await, 2_000);
    let memo: String = sqlx::query_scalar("SELECT memo FROM deposits ORDER BY id DESC LIMIT 1").fetch_one(&pool).await.unwrap();
    assert_eq!(memo, "PayPal payout W1");
}
