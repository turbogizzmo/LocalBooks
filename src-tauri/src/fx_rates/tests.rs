use std::collections::VecDeque;
use std::sync::Mutex;

use super::*;
use crate::db::test_pool;

/// Canned responses in order, recording every URL requested. No test touches the network.
#[derive(Default)]
struct MockHttp {
    responses: Mutex<VecDeque<Result<HttpResponse, HttpError>>>,
    calls: Mutex<Vec<String>>,
}

impl MockHttp {
    fn with(responses: Vec<Result<HttpResponse, HttpError>>) -> Self {
        MockHttp { responses: Mutex::new(responses.into()), calls: Mutex::default() }
    }
    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl RateHttp for MockHttp {
    async fn get(&self, url: &str) -> Result<HttpResponse, HttpError> {
        self.calls.lock().unwrap().push(url.to_string());
        self.responses.lock().unwrap().pop_front().expect("an unexpected HTTP request")
    }
}

fn ok(body: &str) -> Result<HttpResponse, HttpError> {
    Ok(HttpResponse { status: 200, body: body.to_string() })
}

fn status(status: u16, body: &str) -> Result<HttpResponse, HttpError> {
    Ok(HttpResponse { status, body: body.to_string() })
}

fn d(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

async fn enabled_pool() -> SqlitePool {
    let pool = test_pool().await;
    crate::preferences::set(&pool, PREF_KEY, "true").await.unwrap();
    pool
}

const TODAY: &str = "2024-03-01";

#[test]
fn parses_decimal_strings_to_scaled_integers_without_floats() {
    assert_eq!(parse_decimal_e6("1.0942"), Some(1_094_200));
    assert_eq!(parse_decimal_e6("1"), Some(1_000_000));
    assert_eq!(parse_decimal_e6("1.0"), Some(1_000_000));
    assert_eq!(parse_decimal_e6("130.25"), Some(130_250_000));
    assert_eq!(parse_decimal_e6("0.74771"), Some(747_710));
    // Rounded half up at the sixth decimal.
    assert_eq!(parse_decimal_e6("0.0068744"), Some(6_874));
    assert_eq!(parse_decimal_e6("0.0068745"), Some(6_875));
    assert_eq!(parse_decimal_e6("1.2345675"), Some(1_234_568));
    assert_eq!(parse_decimal_e6("0.0000004"), Some(0));
    // Exponent notation.
    assert_eq!(parse_decimal_e6("6.4e-05"), Some(64));
    assert_eq!(parse_decimal_e6("6.45E-5"), Some(65));
    assert_eq!(parse_decimal_e6("1e2"), Some(100_000_000));
    assert_eq!(parse_decimal_e6("1.5e+1"), Some(15_000_000));
    assert_eq!(parse_decimal_e6("5e-7"), Some(1));
    assert_eq!(parse_decimal_e6("4e-7"), Some(0));
    assert_eq!(parse_decimal_e6("1e-30"), Some(0));
    // A quoted string, as some APIs send rates.
    assert_eq!(parse_decimal_e6("\"0.9123\""), Some(912_300));
    // A long fraction is not limited by its length.
    assert_eq!(parse_decimal_e6("0.12345649999999999999999"), Some(123_456));
    for bad in ["", "-1", "1.", ".5", "abc", "1e", "1e-", "1.2.3", "NaN", "1e9999", "+1", "1,5", "1e30"] {
        assert_eq!(parse_decimal_e6(bad), None, "{bad}");
    }
}

#[tokio::test]
async fn lookup_is_off_until_the_preference_is_turned_on() {
    let pool = test_pool().await;
    assert!(!lookup_enabled(&pool).await.unwrap());
    let http = MockHttp::default();
    let err = suggest(&pool, &http, "EUR", "USD", d("2024-01-12"), d(TODAY)).await.unwrap_err();
    assert!(err.starts_with("FX_LOOKUP_OFF"), "{err}");
    assert!(http.calls().is_empty());
    assert!(crate::preferences::set(&pool, PREF_KEY, "maybe").await.is_err());
}

#[tokio::test]
async fn fetches_the_foreign_to_home_rate_and_caches_it() {
    let pool = enabled_pool().await;
    let http = MockHttp::with(vec![ok(r#"{"amount":1.0,"base":"EUR","date":"2024-01-12","rates":{"USD":1.0942}}"#)]);
    let s = suggest(&pool, &http, "eur", "USD", d("2024-01-12"), d(TODAY)).await.unwrap();
    assert_eq!(http.calls(), vec![format!("{API_BASE}/2024-01-12?from=EUR&to=USD")]);
    assert_eq!((s.rate_e6, s.rate_date, s.from_cache, s.stale), (1_094_200, d("2024-01-12"), false, false));
    assert_eq!(s.source, SOURCE);
    assert_eq!((s.currency.as_str(), s.home_currency.as_str()), ("EUR", "USD"));

    // The same date never needs fetching twice.
    let again = suggest(&pool, &http, "EUR", "USD", d("2024-01-12"), d(TODAY)).await.unwrap();
    assert_eq!((again.rate_e6, again.from_cache), (1_094_200, true));
    assert_eq!(http.calls().len(), 1);

    let row: (String, String, String, String, i64, String) =
        sqlx::query_as("SELECT base, quote, rate_date, covers_through, rate_e6, source FROM exchange_rate_cache").fetch_one(&pool).await.unwrap();
    assert_eq!(row, ("EUR".into(), "USD".into(), "2024-01-12".into(), "2024-01-12".into(), 1_094_200, SOURCE.into()));
}

#[tokio::test]
async fn a_weekend_uses_the_prior_business_day_and_is_cached_as_covered() {
    let pool = enabled_pool().await;
    // Saturday the 13th: the service answers with Friday's rate.
    let http = MockHttp::with(vec![ok(r#"{"amount":1.0,"base":"CAD","date":"2024-01-12","rates":{"USD":0.74771}}"#)]);
    let s = suggest(&pool, &http, "CAD", "USD", d("2024-01-13"), d(TODAY)).await.unwrap();
    assert_eq!((s.rate_e6, s.rate_date, s.requested_date), (747_710, d("2024-01-12"), d("2024-01-13")));

    // Saturday again, and Friday itself, come from the cache.
    for date in ["2024-01-13", "2024-01-12"] {
        let cached = suggest(&pool, &http, "CAD", "USD", d(date), d(TODAY)).await.unwrap();
        assert_eq!((cached.rate_e6, cached.rate_date, cached.from_cache), (747_710, d("2024-01-12"), true));
    }
    assert_eq!(http.calls().len(), 1);

    // Sunday has not been asked about yet, so it is fetched; the answer extends the same row.
    let http = MockHttp::with(vec![ok(r#"{"amount":1.0,"base":"CAD","date":"2024-01-12","rates":{"USD":0.74771}}"#)]);
    suggest(&pool, &http, "CAD", "USD", d("2024-01-14"), d(TODAY)).await.unwrap();
    assert_eq!(http.calls().len(), 1);
    let covers: Vec<(String, String)> = sqlx::query_as("SELECT rate_date, covers_through FROM exchange_rate_cache").fetch_all(&pool).await.unwrap();
    assert_eq!(covers, vec![("2024-01-12".into(), "2024-01-14".into())]);
    // An earlier Friday rate does not answer the following Monday.
    let http = MockHttp::with(vec![ok(r#"{"amount":1.0,"base":"CAD","date":"2024-01-15","rates":{"USD":0.745}}"#)]);
    let monday = suggest(&pool, &http, "CAD", "USD", d("2024-01-15"), d(TODAY)).await.unwrap();
    assert_eq!((monday.rate_e6, monday.from_cache), (745_000, false));
}

#[tokio::test]
async fn todays_rate_is_asked_for_again_until_it_is_published() {
    let pool = enabled_pool().await;
    let today = d("2024-01-16");
    let http = MockHttp::with(vec![
        ok(r#"{"amount":1.0,"base":"GBP","date":"2024-01-15","rates":{"USD":1.27}}"#),
        ok(r#"{"amount":1.0,"base":"GBP","date":"2024-01-16","rates":{"USD":1.268}}"#),
    ]);
    let early = suggest(&pool, &http, "GBP", "USD", today, today).await.unwrap();
    assert_eq!(early.rate_date, d("2024-01-15"));
    let later = suggest(&pool, &http, "GBP", "USD", today, today).await.unwrap();
    assert_eq!((later.rate_e6, later.rate_date, later.from_cache), (1_268_000, today, false));
    // Once published, today's rate is cached like any other.
    let cached = suggest(&pool, &http, "GBP", "USD", today, today).await.unwrap();
    assert!(cached.from_cache);
    assert_eq!(http.calls().len(), 2);
}

#[tokio::test]
async fn a_future_date_suggests_the_latest_rate() {
    let pool = enabled_pool().await;
    let http = MockHttp::with(vec![ok(r#"{"amount":1.0,"base":"EUR","date":"2024-03-01","rates":{"USD":1.08}}"#)]);
    let s = suggest(&pool, &http, "EUR", "USD", d("2024-04-30"), d(TODAY)).await.unwrap();
    assert_eq!(http.calls(), vec![format!("{API_BASE}/{TODAY}?from=EUR&to=USD")]);
    assert_eq!((s.requested_date, s.rate_e6), (d(TODAY), 1_080_000));
}

#[tokio::test]
async fn unsupported_currencies_and_dates_are_refused_without_a_request() {
    let pool = enabled_pool().await;
    let http = MockHttp::default();
    for (currency, home, date, code) in [
        ("KWD", "USD", "2024-01-12", "FX_UNSUPPORTED"),
        ("EUR", "VND", "2024-01-12", "FX_UNSUPPORTED"),
        ("US", "EUR", "2024-01-12", "FX_BAD_CURRENCY"),
        ("USD", "usd", "2024-01-12", "FX_SAME_CURRENCY"),
        ("EUR", "USD", "1998-12-31", "FX_TOO_EARLY"),
    ] {
        let err = suggest(&pool, &http, currency, home, d(date), d(TODAY)).await.unwrap_err();
        assert!(err.starts_with(code), "{currency}/{home}: {err}");
    }
    assert!(http.calls().is_empty());
}

#[tokio::test]
async fn service_refusals_ask_for_a_manual_rate() {
    let pool = enabled_pool().await;
    let http = MockHttp::with(vec![
        status(404, r#"{"message":"not found"}"#),
        status(422, r#"{"message":"bad currency pair"}"#),
        status(503, "<html>down</html>"),
        ok("<html>not json</html>"),
        ok(r#"{"amount":1.0,"base":"EUR","date":"2024-01-12","rates":{"GBP":0.86}}"#),
        ok(r#"{"amount":1.0,"base":"GBP","date":"2024-01-12","rates":{"USD":1.27}}"#),
        ok(r#"{"amount":1.0,"base":"EUR","date":"2024-02-01","rates":{"USD":1.08}}"#),
        ok(r#"{"amount":1.0,"base":"EUR","date":"2024-01-12","rates":{"USD":-1.08}}"#),
        ok(r#"{"amount":1.0,"base":"EUR","date":"2024-01-12","rates":{"USD":"1.08"}}"#),
    ]);
    for code in ["FX_UNAVAILABLE", "FX_UNAVAILABLE", "FX_SERVICE", "FX_BAD_RESPONSE", "FX_BAD_RESPONSE", "FX_BAD_RESPONSE", "FX_BAD_RESPONSE", "FX_BAD_RESPONSE"] {
        let err = suggest(&pool, &http, "EUR", "USD", d("2024-01-12"), d(TODAY)).await.unwrap_err();
        assert!(err.starts_with(code), "{code}: {err}");
        assert!(err.contains("enter the rate yourself"), "{err}");
    }
    // A rate sent as a decimal string is read the same way.
    let s = suggest(&pool, &http, "EUR", "USD", d("2024-01-12"), d(TODAY)).await.unwrap();
    assert_eq!(s.rate_e6, 1_080_000);
    let cached: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM exchange_rate_cache").fetch_one(&pool).await.unwrap();
    assert_eq!(cached, 1);
}

#[tokio::test]
async fn a_rate_too_small_to_store_is_refused() {
    let pool = enabled_pool().await;
    let http = MockHttp::with(vec![ok(r#"{"amount":1.0,"base":"IDR","date":"2024-01-12","rates":{"EUR":4e-7}}"#)]);
    let err = suggest(&pool, &http, "IDR", "EUR", d("2024-01-12"), d(TODAY)).await.unwrap_err();
    assert!(err.starts_with("FX_RATE_TOO_SMALL"), "{err}");
}

#[tokio::test]
async fn offline_falls_back_to_a_recent_cached_rate_or_manual_entry() {
    let pool = enabled_pool().await;
    let http = MockHttp::with(vec![
        HttpError::Timeout.into_err(),
        ok(r#"{"amount":1.0,"base":"EUR","date":"2024-01-12","rates":{"USD":1.0942}}"#),
        HttpError::Network("dns error".into()).into_err(),
        HttpError::Timeout.into_err(),
    ]);
    let err = suggest(&pool, &http, "EUR", "USD", d("2024-01-12"), d(TODAY)).await.unwrap_err();
    assert!(err.starts_with("FX_OFFLINE") && err.contains("did not answer in time") && err.contains("enter the rate yourself"), "{err}");

    suggest(&pool, &http, "EUR", "USD", d("2024-01-12"), d(TODAY)).await.unwrap();
    // Offline the next week: Friday's cached rate is offered, marked stale.
    let s = suggest(&pool, &http, "EUR", "USD", d("2024-01-17"), d(TODAY)).await.unwrap();
    assert_eq!((s.rate_e6, s.rate_date, s.from_cache, s.stale), (1_094_200, d("2024-01-12"), true, true));
    // More than a week later, the cached rate is too old to suggest.
    let err = suggest(&pool, &http, "EUR", "USD", d("2024-01-22"), d(TODAY)).await.unwrap_err();
    assert!(err.starts_with("FX_OFFLINE"), "{err}");
    // Offline never extends what a cached rate covers.
    let covers: String = sqlx::query_scalar("SELECT covers_through FROM exchange_rate_cache").fetch_one(&pool).await.unwrap();
    assert_eq!(covers, "2024-01-12");
}

#[tokio::test]
async fn cached_rates_work_offline_and_never_touch_saved_transactions() {
    let pool = enabled_pool().await;
    let http = MockHttp::with(vec![ok(r#"{"amount":1.0,"base":"EUR","date":"2024-01-12","rates":{"USD":1.0942}}"#)]);
    let entries_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap();
    suggest(&pool, &http, "EUR", "USD", d("2024-01-13"), d(TODAY)).await.unwrap();
    // No responses are left, so any request would panic: this is answered from the cache.
    let s = suggest(&pool, &http, "EUR", "USD", d("2024-01-13"), d(TODAY)).await.unwrap();
    assert!(s.from_cache && !s.stale);
    let entries_after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap();
    assert_eq!(entries_before, entries_after);
}

impl HttpError {
    fn into_err(self) -> Result<HttpResponse, HttpError> {
        Err(self)
    }
}
