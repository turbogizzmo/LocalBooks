use super::*;
use crate::db::test_pool;

/// Every preference a migration seeds must be settable to its own value, or its Settings control
/// would fail the first time it is used.
#[tokio::test]
async fn every_seeded_preference_is_settable() {
    let pool = test_pool().await;
    for (key, value) in get_all(&pool).await.unwrap() {
        if key == "company_name" && value.is_empty() {
            continue;
        }
        set(&pool, &key, &value).await.unwrap_or_else(|e| panic!("{key} = {value:?}: {e}"));
    }
}

fn ymd(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[tokio::test]
async fn preferences_validate_normalize_and_persist() {
    let pool = test_pool().await;
    let prefs = get_all(&pool).await.unwrap();
    assert_eq!(prefs.get("fiscal_year_start").map(String::as_str), Some("01-01"));
    assert_eq!(prefs.get("company_name").map(String::as_str), Some("LocalBooks Ledger"));

    assert_eq!(set(&pool, "company_ein", "123456789").await.unwrap(), "12-3456789");
    assert_eq!(set(&pool, "fiscal_year_start", "07-01").await.unwrap(), "07-01");
    assert_eq!(fiscal_start_month(&pool).await.unwrap(), 7);
    for (key, value) in [("fiscal_year_start", "07-15"), ("fiscal_year_start", "13-01"), ("company_name", "  "), ("tax_form", "990-EZ"), ("bogus", "x"), ("company_ein", "12-34")] {
        assert!(set(&pool, key, value).await.is_err(), "{key}={value} should be rejected");
    }
    // The database guards the fiscal start even if the module is bypassed.
    let err = sqlx::query("UPDATE system_preferences SET value = '02-30' WHERE key = 'fiscal_year_start'").execute(&pool).await.unwrap_err();
    assert!(err.to_string().contains("PREF_BAD_FISCAL_START"), "{err}");
}

#[tokio::test]
async fn logo_is_validated_by_content_and_stored_as_a_data_url() {
    let pool = test_pool().await;
    assert_eq!(get_logo(&pool).await.unwrap(), None);
    let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 13];
    let url = set_logo(&pool, &png).await.unwrap();
    assert!(url.starts_with("data:image/png;base64,iVBORw0KGgo"), "{url}");
    assert_eq!(get_logo(&pool).await.unwrap(), Some(url));
    assert!(!get_all(&pool).await.unwrap().contains_key(LOGO_KEY), "kept out of the light preferences map");

    assert!(set_logo(&pool, b"<svg onload=alert(1)>").await.unwrap_err().starts_with("PREF_LOGO_FORMAT"));
    assert!(set_logo(&pool, &vec![0xFF; MAX_LOGO_BYTES + 1]).await.unwrap_err().starts_with("PREF_LOGO_TOO_LARGE"));
    assert!(set(&pool, LOGO_KEY, "data:text/html,boom").await.unwrap_err().starts_with("PREF_LOGO"));
    clear_logo(&pool).await.unwrap();
    assert_eq!(get_logo(&pool).await.unwrap(), None);
}

#[test]
fn fiscal_periods_follow_the_configured_start() {
    let p = fiscal_periods(ymd(2026, 10, 13), 7);
    assert_eq!((p.this_year.start, p.this_year.end), (ymd(2026, 7, 1), ymd(2027, 6, 30)));
    assert_eq!((p.last_year.start, p.last_year.end), (ymd(2025, 7, 1), ymd(2026, 6, 30)));
    assert_eq!((p.this_quarter.start, p.this_quarter.end), (ymd(2026, 10, 1), ymd(2026, 12, 31)));
    assert_eq!(p.year_to_date, DateRange { start: ymd(2026, 7, 1), end: ymd(2026, 10, 13) });

    let jan = fiscal_periods(ymd(2026, 2, 28), 1);
    assert_eq!((jan.this_year.start, jan.this_year.end), (ymd(2026, 1, 1), ymd(2026, 12, 31)));
    assert_eq!((jan.this_quarter.start, jan.this_quarter.end), (ymd(2026, 1, 1), ymd(2026, 3, 31)));
    let early = fiscal_periods(ymd(2026, 3, 5), 4);
    assert_eq!(early.this_year.start, ymd(2025, 4, 1));
}

#[test]
fn tax_calendar_rolls_weekends_and_follows_entity_rules() {
    // Calendar-year S corp: 2026 return due Mar 15, 2027 (Monday); 2025 return due Mar 15, 2026 (a Sunday → Mar 16).
    let s = tax_calendar("1120S", 1, ymd(2026, 10, 13));
    let returns: Vec<NaiveDate> = s.iter().filter(|d| d.form == "Form 1120-S").map(|d| d.due_date).collect();
    assert_eq!(returns, vec![ymd(2026, 3, 16), ymd(2027, 3, 15)]);
    assert!(s.iter().find(|d| d.due_date == ymd(2026, 3, 16)).unwrap().is_past);

    // Sole proprietor: April 15 return and the four 1040-ES dates (the last in January).
    let c = tax_calendar("SCHEDULE_C", 1, ymd(2026, 1, 5));
    let es: Vec<NaiveDate> = c.iter().filter(|d| d.form == "Form 1040-ES").map(|d| d.statutory_date).collect();
    assert_eq!(es, vec![ymd(2026, 4, 15), ymd(2026, 6, 15), ymd(2026, 9, 15), ymd(2027, 1, 15)]);

    // C corp with a September 30 year end: return due the 15th day of the 4th month (Jan 15).
    let corp = tax_calendar("1120", 10, ymd(2026, 11, 1));
    assert!(corp.iter().any(|d| d.form == "Form 1120" && d.statutory_date == ymd(2028, 1, 15)));
    assert_eq!(corp.iter().filter(|d| d.form == "Form 1120-W").count(), 4);

    // Nonprofit: Form 990 is due the 15th day of the 5th month (May 15), with no estimated payments.
    let np = tax_calendar("990", 1, ymd(2026, 6, 1));
    let returns: Vec<NaiveDate> = np.iter().filter(|d| d.form == "Form 990").map(|d| d.statutory_date).collect();
    assert_eq!(returns, vec![ymd(2026, 5, 15), ymd(2027, 5, 15)]);
    assert!(np.iter().all(|d| !d.form.contains("ES") && !d.form.contains("1120-W")));
}
