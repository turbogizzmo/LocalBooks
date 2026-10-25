use super::*;
use crate::db::test_pool;
use std::path::PathBuf;
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
    SimpleFin::new(SecretStore::in_file(dir.join(SECRETS_FILE), KEYCHAIN_SERVICE, "the bank connection")).unwrap()
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
