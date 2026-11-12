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
    assert_eq!(presets.len(), 33);
}

#[test]
fn settings_default_what_the_screen_leaves_out() {
    let s: ReportSettings =
        serde_json::from_value(json!({ "columns": ["date", "amount", "openBalance"], "filters": { "memo": "rent" }, "period": "thisMonth" })).unwrap();
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

/// Gap T84: a custom report by item lists the discount and charge items' lines, on sales and
/// purchase forms alike.
#[tokio::test]
async fn the_items_source_lists_discount_and_charge_lines() {
    use crate::money::Cents;
    let pool = crate::db::test_pool().await;
    let id = |number: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(&pool).await.unwrap() }
    };
    let (services, supplies, freight) = (id("4100").await, id("6250").await, id("6400").await);
    let item = |name: &str, kind: &str, account: i64| {
        crate::sales::create_item(
            &pool,
            crate::sales::ProductInput { name: name.into(), description: None, rate_cents: Cents::ZERO, income_account_id: services, is_taxable: false },
            crate::sales::ItemKind { item_type: kind.into(), percent_ppm: None, expense_account_id: Some(account) },
        )
    };
    let loyalty = item("Loyalty", "DISCOUNT", supplies).await.unwrap();
    let shipping = item("Shipping", "OTHER_CHARGE", freight).await.unwrap();
    let rebate = item("Rebate", "DISCOUNT", supplies).await.unwrap();
    crate::sales::create_customer(&pool, "Acme", None).await.unwrap();

    let invoice: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
        "customerName": "Acme", "issueDate": "2026-05-01", "dueDate": "2026-05-31",
        "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": 10_000, "incomeAccountId": services }],
        "formLines": [{ "afterLines": 1, "kind": "DISCOUNT", "amountCents": 1_000, "accountId": services, "productServiceId": loyalty.id, "description": "Loyalty" }]
    }))
    .unwrap();
    crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap();
    let bill: crate::expenses::BillInput = serde_json::from_value(serde_json::json!({
        "vendorName": "Supplier", "billNumber": "B-1", "billDate": "2026-05-02", "dueDate": "2026-05-31", "memo": null, "totalCents": 20_200,
        "lines": [{ "accountId": supplies, "amountCents": 20_000, "memo": "Lumber" }, { "accountId": supplies, "amountCents": 1_000, "memo": "Nails" }],
        "formLines": [
            { "afterLine": 1, "kind": "DISCOUNT", "amountCents": 2_000, "accountId": supplies, "productServiceId": rebate.id, "description": "Rebate" },
            { "afterLine": 2, "kind": "CHARGE", "amountCents": 1_200, "accountId": freight, "productServiceId": shipping.id, "description": "Freight" }
        ]
    }))
    .unwrap();
    crate::expenses::post_vendor_bill(&pool, bill).await.unwrap();

    let lines = item_lines(&pool, None, d(2026, 5, 31)).await.unwrap();
    let mut by_item: Vec<(String, i64)> = lines.iter().filter_map(|l| Some((l.item_name.clone()?, l.item_amount_cents?))).collect();
    by_item.sort();
    assert_eq!(by_item, [("Loyalty".to_string(), -1_000), ("Rebate".to_string(), -2_000), ("Shipping".to_string(), 1_200)]);
}
