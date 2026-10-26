use super::*;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

#[test]
fn periods_have_labels_and_bounds() {
    assert_eq!(period_key(d(2025, 2, 14), Period::Month), (202502, "Feb 2025".into()));
    assert_eq!(period_key(d(2025, 8, 1), Period::Quarter), (20253, "Q3 2025".into()));
    assert_eq!(period_key(d(2025, 8, 1), Period::Year), (2025, "2025".into()));
    assert_eq!(period_bounds(d(2024, 2, 10), Period::Month), (d(2024, 2, 1), d(2024, 2, 29)));
    assert_eq!(period_bounds(d(2025, 11, 30), Period::Quarter), (d(2025, 10, 1), d(2025, 12, 31)));
    assert_eq!(period_bounds(d(2025, 6, 30), Period::Year), (d(2025, 1, 1), d(2025, 12, 31)));
}

#[test]
fn signs() {
    // A $10 credit to an income account, then a $10 debit to an expense account.
    assert_eq!(signed(-1_000, false, Sign::Natural), 1_000);
    assert_eq!(signed(-1_000, false, Sign::Debit), -1_000);
    assert_eq!(signed(-1_000, false, Sign::Credit), 1_000);
    assert_eq!(signed(1_000, true, Sign::Natural), 1_000);
    assert_eq!(signed(1_000, true, Sign::Credit), -1_000);
}

#[test]
fn presets_are_named_once_and_runnable_settings() {
    let presets = presets();
    let mut ids = HashSet::new();
    for p in &presets {
        assert!(ids.insert(p.id), "{} listed twice", p.id);
        assert_eq!(p.settings.mode == Mode::Detail, !p.settings.columns.is_empty(), "{}: detail presets have columns", p.id);
        // What the screen sends back deserializes into the same settings.
        let round: ReportSettings = serde_json::from_value(serde_json::to_value(&p.settings).unwrap()).unwrap();
        assert_eq!(round, p.settings, "{}", p.id);
    }
    assert_eq!(presets.len(), 24);
}

#[test]
fn settings_default_what_the_screen_leaves_out() {
    let s: ReportSettings = serde_json::from_value(json!({ "columns": ["date", "amount", "openBalance"], "filters": { "memo": "rent" }, "period": "thisMonth" })).unwrap();
    assert_eq!(s.source, Source::Ledger);
    assert_eq!(s.columns, vec![Column::Date, Column::Amount, Column::OpenBalance]);
    assert_eq!(s.filters.memo.as_deref(), Some("rent"));
    assert!(serde_json::from_value::<ReportSettings>(json!({ "columns": ["nope"] })).is_err());
}

#[tokio::test]
async fn saved_reports_round_trip() {
    let pool = crate::db::test_pool().await;
    let settings = json!({ "columns": ["date", "amount"], "groupBy": "name", "period": "lastMonth", "title": "Rent by vendor" });
    let saved = save(&pool, "  Rent by vendor ", Some("expenses-vendor-detail"), &settings, false).await.unwrap();
    assert_eq!(saved.name, "Rent by vendor");
    assert_eq!(saved.preset.as_deref(), Some("expenses-vendor-detail"));
    assert_eq!(saved.settings, settings);

    // Names are unique ignoring case; replacing keeps the id.
    let err = save(&pool, "RENT BY VENDOR", None, &settings, false).await.unwrap_err();
    assert!(err.starts_with("RPT_SAVE_EXISTS"), "{err}");
    let changed = json!({ "columns": ["date"], "period": "thisMonth" });
    let replaced = save(&pool, "rent by vendor", None, &changed, true).await.unwrap();
    assert_eq!(replaced.id, saved.id);
    assert_eq!(replaced.settings, changed);

    save(&pool, "A first", None, &settings, false).await.unwrap();
    let names: Vec<String> = list_saved(&pool).await.unwrap().into_iter().map(|r| r.name).collect();
    assert_eq!(names, ["A first", "Rent by vendor"]);

    assert!(save(&pool, " ", None, &settings, false).await.unwrap_err().starts_with("RPT_SAVE_NAME"));
    assert!(save(&pool, &"x".repeat(101), None, &settings, false).await.unwrap_err().starts_with("RPT_SAVE_NAME"));
    assert!(save(&pool, "Bad", None, &json!({ "mode": "sideways" }), false).await.unwrap_err().starts_with("RPT_SAVE_SETTINGS"));

    delete_saved(&pool, saved.id).await.unwrap();
    assert!(delete_saved(&pool, saved.id).await.unwrap_err().starts_with("RPT_SAVED_NOT_FOUND"));
    assert_eq!(list_saved(&pool).await.unwrap().len(), 1);
}

#[tokio::test]
async fn rejects_bad_settings() {
    let pool = crate::db::test_pool().await;
    let today = d(2025, 6, 30);
    let backwards = ReportSettings { start_date: Some(d(2025, 7, 1)), columns: vec![Column::Date], ..ReportSettings::default() };
    assert!(run(&pool, &backwards, today).await.unwrap_err().starts_with("RPT_BAD_RANGE"));
    assert!(run(&pool, &ReportSettings::default(), today).await.unwrap_err().starts_with("RPT_NO_COLUMNS"));
    // An empty company runs to just a total.
    let empty = run(&pool, &ReportSettings { columns: vec![Column::Amount], ..ReportSettings::default() }, today).await.unwrap();
    assert_eq!(empty.end_date, today);
    assert_eq!(empty.line_count, 0);
    assert_eq!(empty.rows.len(), 1);
}
