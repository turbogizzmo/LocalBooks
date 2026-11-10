use std::sync::{Arc, Mutex};

use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::db::test_pool;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn new_account(pool: &SqlitePool, number: &str, name: &str, kind: &str) -> i64 {
    sqlx::query_scalar("INSERT INTO chart_of_accounts (account_number, name, account_type) VALUES (?1, ?2, ?3) RETURNING id").bind(number).bind(name).bind(kind).fetch_one(pool).await.unwrap()
}

/// A payout of $243.05: two sales ($200.00 and $100.00) with their fees, a $20.00 refund (its fee
/// partly returned), a $15.00 dispute with its $15.00 fee, and $2.10 of reserve released.
fn transactions() -> Vec<Value> {
    vec![
        json!({ "type": "charge", "amount": 20_000, "fee": 610, "description": "Quillfeather Studio order 1042", "reporting_category": "charge" }),
        json!({ "type": "charge", "amount": 10_000, "fee": 320, "description": "Order 1043", "reporting_category": "charge" }),
        json!({ "type": "refund", "amount": -2_000, "fee": -25, "description": "REFUND FOR CHARGE (Order 1040)", "reporting_category": "refund" }),
        json!({ "type": "adjustment", "amount": -1_500, "fee": 1_500, "description": "Chargeback withdrawal for ch_1", "reporting_category": "dispute" }),
        json!({ "type": "adjustment", "amount": 210, "fee": 0, "description": "Reserve released", "reporting_category": "other_adjustment" }),
        json!({ "type": "payout", "amount": -24_305, "fee": 0, "description": "STRIPE PAYOUT" }),
    ]
}

#[test]
fn a_payout_breaks_down_into_gross_fees_refunds_and_chargebacks() {
    let b = stripe_breakdown(24_305, &transactions()).unwrap();
    assert_eq!((b.gross_cents, b.fee_cents, b.refund_cents, b.chargeback_cents, b.other_cents), (30_000, 2_405, 2_000, 1_500, 210));
    assert_eq!(b.gross_cents - b.fee_cents - b.refund_cents - b.chargeback_cents + b.other_cents, b.net_cents);
    let kinds: Vec<(&str, i64)> = b.items.iter().map(|i| (i.kind.as_str(), i.amount_cents)).collect();
    assert_eq!(kinds, vec![("CHARGE", 20_000), ("CHARGE", 10_000), ("REFUND", 2_000), ("CHARGEBACK", 1_500)]);
    // Transactions that don't add up to the payout are refused, never forced to fit.
    assert!(stripe_breakdown(24_306, &transactions()).unwrap_err().starts_with("PAYOUT_MISMATCH"));
}

async fn setup(pool: &SqlitePool) -> (FeedInput, i64) {
    let bank = account(pool, "1010").await;
    let sales = new_account(pool, "4870", "Online Sales", "INCOME").await;
    let fees = new_account(pool, "6870", "Card Processing Fees", "EXPENSE").await;
    let refunds = new_account(pool, "4871", "Refunds", "INCOME").await;
    let chargebacks = new_account(pool, "6871", "Chargebacks", "EXPENSE").await;
    (FeedInput { name: "Stripe".into(), bank_account_id: bank, sales_account_id: sales, fee_account_id: fees, refund_account_id: refunds, chargeback_account_id: chargebacks }, bank)
}

async fn balance(pool: &SqlitePool, id: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(id).fetch_one(pool).await.unwrap()
}

// ---- a mock Stripe on 127.0.0.1 ----------------------------------------------------------------

async fn mock_stripe(requests: Arc<Mutex<Vec<String>>>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break };
            let requests = requests.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16384];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let line = head.lines().next().unwrap_or("").to_string();
                let authorized = head.lines().any(|l| l.eq_ignore_ascii_case("authorization: Bearer rk_test_good"));
                requests.lock().unwrap().push(line.clone());
                let (code, body) = if !authorized {
                    (401, json!({ "error": { "message": "Invalid API Key provided" } }))
                } else if line.starts_with("GET /v1/balance ") || line.starts_with("GET /v1/balance?") {
                    (200, json!({ "object": "balance" }))
                } else if line.starts_with("GET /v1/payouts") {
                    (200, json!({ "data": [
                        { "id": "po_1", "amount": 24_305, "currency": "usd", "arrival_date": 1_773_014_400, "status": "paid" },
                        { "id": "po_eur", "amount": 5_000, "currency": "eur", "arrival_date": 1_773_014_400, "status": "paid" },
                        { "id": "po_bad", "amount": 999, "currency": "usd", "arrival_date": 1_773_014_400, "status": "paid" }
                    ], "has_more": false }))
                } else if line.contains("payout=po_1") {
                    (200, json!({ "data": transactions(), "has_more": false }))
                } else if line.contains("payout=po_bad") {
                    (200, json!({ "data": [{ "type": "charge", "amount": 500, "fee": 0 }], "has_more": false }))
                } else {
                    (404, json!({ "error": { "message": "no route" } }))
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

fn temp_store(name: &str) -> (SecretStore, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("localbooks-payouts-{name}-{}", crate::mcp_server::new_token()));
    std::fs::create_dir_all(&dir).unwrap();
    (SecretStore::in_file(dir.join(SECRETS_FILE), KEYCHAIN_SERVICE, "the payout feed key"), dir)
}

#[tokio::test]
async fn a_fetched_payout_is_recorded_as_a_gross_deposit_that_lands_at_the_net() {
    let pool = test_pool().await;
    let (input, bank) = setup(&pool).await;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let api = mock_stripe(requests.clone()).await;
    let (store, dir) = temp_store("flow");

    // Nothing is called before the feed is set up; a bad key is refused before anything is kept.
    assert!(requests.lock().unwrap().is_empty());
    assert!(connect_stripe(&pool, &store, &api, "rk_test_wrong", &input).await.unwrap_err().starts_with("PAYOUT_KEY"));
    assert!(connect_stripe(&pool, &store, &api, "pk_test_publishable", &input).await.unwrap_err().starts_with("PAYOUT_KEY"));
    assert!(connect_stripe(&pool, &store, &api, "sk_test_full_secret", &input).await.unwrap_err().starts_with("PAYOUT_KEY"), "only a restricted key");
    assert!(feeds(&pool).await.unwrap().is_empty());
    let f = connect_stripe(&pool, &store, &api, "rk_test_good", &input).await.unwrap();
    let kept = std::fs::read_to_string(dir.join(SECRETS_FILE)).unwrap();
    assert!(kept.contains("rk_test_good"), "the key is in the credential store");
    let in_file: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM payout_feeds WHERE secret_key LIKE '%rk_test%'").fetch_one(&pool).await.unwrap();
    assert_eq!(in_file, 0, "and not in the company file");

    let now = DateTime::parse_from_rfc3339("2026-03-10T12:00:00Z").unwrap().with_timezone(&Utc);
    let fetched = fetch(&pool, &store, &Apis::all(&api), f.id, now).await.unwrap();
    assert_eq!(fetched.new_payouts, 1);
    assert_eq!(fetched.refused.len(), 2, "{:?}", fetched.refused);
    assert!(fetched.refused.iter().any(|r| r.starts_with("po_eur: paid in EUR")));
    assert!(fetched.refused.iter().any(|r| r.starts_with("po_bad: the payout's transactions add up")));
    assert!(requests.lock().unwrap().iter().all(|r| r.contains("/v1/")), "only Stripe's API");
    assert_eq!(fetch(&pool, &store, &Apis::all(&api), f.id, now).await.unwrap().new_payouts, 0, "a payout is kept once");

    let listed = list_payouts(&pool, f.id).await.unwrap();
    assert_eq!((listed[0].net_cents, listed[0].gross_cents, listed[0].arrival_date), (24_305, 30_000, NaiveDate::from_ymd_opt(2026, 3, 9).unwrap()));
    let deposit = record(&pool, listed[0].id).await.unwrap();
    assert!(record(&pool, listed[0].id).await.unwrap_err().starts_with("PAYOUT_DONE"));

    // The bank gets the net; sales show gross; each deduction lands in its account.
    assert_eq!(balance(&pool, bank).await, 24_305);
    assert_eq!(balance(&pool, input.sales_account_id).await, -30_000);
    assert_eq!(balance(&pool, input.fee_account_id).await, 2_405 - 210, "fees, less the adjustment back");
    assert_eq!(balance(&pool, input.refund_account_id).await, 2_000);
    assert_eq!(balance(&pool, input.chargeback_account_id).await, 1_500);
    let lines: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM deposit_lines WHERE deposit_id = ?1").bind(deposit).fetch_one(&pool).await.unwrap();
    assert_eq!(lines, 3, "two sales and the adjustment back");
    let kinds: Vec<String> = sqlx::query_scalar("SELECT kind FROM deposit_deductions WHERE deposit_id = ?1 ORDER BY line_number").bind(deposit).fetch_all(&pool).await.unwrap();
    assert_eq!(kinds, vec!["REFUND", "CHARGEBACK", "FEE"]);

    // Without the key on this computer, fetching says so.
    let (other, _) = temp_store("other");
    assert!(fetch(&pool, &other, &Apis::all(&api), f.id, now).await.unwrap_err().starts_with("PAYOUT_KEY_MISSING"));
    disconnect(&pool, &store, f.id).await.unwrap();
    assert!(!std::fs::read_to_string(dir.join(SECRETS_FILE)).unwrap().contains("rk_test_good"), "disconnecting forgets the key");
    assert_eq!(balance(&pool, bank).await, 24_305, "recorded deposits stay");
}

#[tokio::test]
async fn deposits_refuse_deductions_that_leave_nothing_in_the_bank() {
    let pool = test_pool().await;
    let (input, bank) = setup(&pool).await;
    let deposit = |deductions: Vec<DepositDeduction>| DepositInput {
        bank_account_id: bank,
        deposit_date: NaiveDate::from_ymd_opt(2026, 3, 9).unwrap(),
        memo: None,
        items: Vec::new(),
        other_lines: vec![DepositOtherLine { account_id: input.sales_account_id, amount_cents: 1_000, memo: None, class_id: None, customer_id: None, vendor_id: None }],
        cash_back: None,
        deductions,
    };
    let fee = |a: i64, account: i64, kind: &str| DepositDeduction { account_id: account, amount_cents: a, memo: None, kind: kind.into() };
    assert!(crate::banking::post_bank_deposit(&pool, deposit(vec![fee(1_000, input.fee_account_id, "FEE")])).await.unwrap_err().starts_with("DEP_NOTHING_BANKED"));
    assert!(crate::banking::post_bank_deposit(&pool, deposit(vec![fee(0, input.fee_account_id, "FEE")])).await.unwrap_err().starts_with("DEP_BAD_DEDUCTION"));
    assert!(crate::banking::post_bank_deposit(&pool, deposit(vec![fee(10, bank, "FEE")])).await.unwrap_err().starts_with("DEP_DEDUCTION_ACCOUNT"));
    assert!(crate::banking::post_bank_deposit(&pool, deposit(vec![fee(10, input.fee_account_id, "TIP")])).await.unwrap_err().starts_with("DEP_BAD_DEDUCTION"));
    crate::banking::post_bank_deposit(&pool, deposit(vec![fee(35, input.fee_account_id, "FEE")])).await.unwrap();
    assert_eq!(balance(&pool, bank).await, 965);
}

#[tokio::test]
async fn a_feed_set_to_fetch_automatically_is_fetched_when_its_interval_has_passed() {
    let pool = test_pool().await;
    let (input, _) = setup(&pool).await;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let api = mock_stripe(requests.clone()).await;
    let apis = Apis::all(&api);
    let (store, _dir) = temp_store("auto");
    let f = connect_stripe(&pool, &store, &api, "rk_test_good", &input).await.unwrap();
    let at = |t: &str| DateTime::parse_from_rfc3339(t).unwrap().with_timezone(&Utc);

    // Off until an Admin picks an interval: nothing is fetched.
    assert!(fetch_due(&pool, &store, &apis, at("2026-03-10T12:00:00Z")).await.unwrap().is_empty());
    assert!(set_auto_fetch(&pool, f.id, Some(5)).await.unwrap_err().starts_with("PAYOUT_AUTO"));
    assert_eq!(set_auto_fetch(&pool, f.id, Some(6)).await.unwrap().auto_fetch_hours, Some(6));
    let calls = requests.lock().unwrap().len();

    let done = fetch_due(&pool, &store, &apis, at("2026-03-10T12:00:00Z")).await.unwrap();
    assert_eq!(done, vec![AutoFetch { feed_id: f.id, new_payouts: 1, error: None }]);
    let line = feed(&pool, f.id).await.unwrap().last_auto_result.unwrap();
    assert!(line.starts_with("2026-03-10 12:00 UTC: 1 new payout. Not kept:"), "{line}");
    assert!(requests.lock().unwrap().len() > calls);

    // Not again until six hours have passed; then again, finding nothing new.
    assert!(fetch_due(&pool, &store, &apis, at("2026-03-10T17:59:00Z")).await.unwrap().is_empty());
    assert_eq!(fetch_due(&pool, &store, &apis, at("2026-03-10T18:00:00Z")).await.unwrap()[0].new_payouts, 0);

    // A failing try is kept on the screen and waits for the next interval too.
    let (empty, _other) = temp_store("auto-empty");
    let failed = fetch_due(&pool, &empty, &apis, at("2026-03-11T01:00:00Z")).await.unwrap();
    assert!(failed[0].error.as_deref().is_some_and(|e| e.starts_with("PAYOUT_KEY_MISSING")));
    assert!(feed(&pool, f.id).await.unwrap().last_auto_result.unwrap().contains("not fetched: this computer doesn't have the feed's key"));
    assert!(fetch_due(&pool, &empty, &apis, at("2026-03-11T02:00:00Z")).await.unwrap().is_empty(), "no retry before the interval");

    set_auto_fetch(&pool, f.id, None).await.unwrap();
    assert!(fetch_due(&pool, &store, &apis, at("2026-03-12T12:00:00Z")).await.unwrap().is_empty());
}
