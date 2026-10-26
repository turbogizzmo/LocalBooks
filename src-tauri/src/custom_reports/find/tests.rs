use super::*;
use crate::custom_reports::NameRef;
use crate::db::test_pool;

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

/// A check (or card expense, with no number) from one account to another, to `payee` for `cents`.
async fn pay(pool: &SqlitePool, (from, to): (i64, i64), payee: &str, date: &str, number: Option<&str>, cents: i64, memo: &str) -> i64 {
    let input = serde_json::from_value(serde_json::json!({ "kind": if number.is_some() { "CHECK" } else { "EXPENSE" }, "payeeName": payee,
        "paymentAccountId": from, "expenseDate": date, "checkNumber": number, "paymentMethod": null, "memo": null,
        "lines": [{ "accountId": to, "amountCents": cents, "memo": memo }] }))
    .unwrap();
    let id = crate::expenses::post_expense(pool, input).await.unwrap().id;
    sqlx::query_scalar("SELECT journal_entry_id FROM expenses WHERE id = ?1").bind(id).fetch_one(pool).await.unwrap()
}

fn ids(result: &FindResult) -> Vec<i64> {
    result.rows.iter().map(|r| r.journal_entry_id).collect()
}

#[tokio::test]
async fn finds_transactions_by_the_core_filters() {
    let pool = test_pool().await;
    let (checking, card, rent, supplies, services) =
        (account(&pool, "1010").await, account(&pool, "2100").await, account(&pool, "6300").await, account(&pool, "6250").await, account(&pool, "4100").await);
    let jan_rent = pay(&pool, (checking, rent), "Landlord", "2026-01-01", Some("501"), 120_000, "January rent").await;
    let feb_rent = pay(&pool, (checking, rent), "Landlord", "2026-02-01", Some("502"), 120_000, "February rent").await;
    let paper = pay(&pool, (card, supplies), "Staples", "2026-02-10", None, 4_250, "Printer paper").await;
    let invoice: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({ "customerName": "Acme", "issueDate": "2026-02-15", "dueDate": "2026-03-15",
        "lines": [{ "description": "Consulting", "quantityE4": 10_000, "rateCents": 50_000, "incomeAccountId": services, "isTaxable": false }] }))
    .unwrap();
    let inv = crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap();
    let inv_entry = inv.journal_entry_id.unwrap();
    let today = d(6, 30);
    let run = |query: FindQuery| {
        let pool = pool.clone();
        async move { find(&pool, &query, today).await.unwrap() }
    };
    let filters = |f: Filters| FindQuery { filters: Filters { exclude_voided: true, ..f }, ..FindQuery::default() };

    // Nothing chosen: every transaction, newest first, with a count and a total.
    let all = run(filters(Filters::default())).await;
    assert_eq!(ids(&all), [inv_entry, paper, feb_rent, jan_rent]);
    assert_eq!((all.match_count, all.total_cents, all.end_date), (4, 120_000 * 2 + 4_250 + 50_000, today));
    let check = &all.rows[2];
    assert_eq!((check.type_label.as_str(), check.num.as_deref(), check.name.as_deref(), check.name_kind), ("Check", Some("502"), Some("Landlord"), Some(NameKind::Vendor)));
    assert_eq!(all.rows[0].name_kind, Some(NameKind::Customer));

    // Dates.
    let feb = run(FindQuery { start_date: Some(d(2, 1)), end_date: Some(d(2, 28)), ..filters(Filters::default()) }).await;
    assert_eq!(ids(&feb), [inv_entry, paper, feb_rent]);

    // Types, names and accounts (the row shows the account that matched).
    assert_eq!(ids(&run(filters(Filters { transaction_types: vec!["CHECK".into()], ..Filters::default() })).await), [feb_rent, jan_rent]);
    let acme: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Acme'").fetch_one(&pool).await.unwrap();
    assert_eq!(ids(&run(filters(Filters { names: vec![NameRef { kind: NameKind::Customer, id: acme }], ..Filters::default() })).await), [inv_entry]);
    let by_account = run(filters(Filters { account_ids: vec![supplies], ..Filters::default() })).await;
    assert_eq!(ids(&by_account), [paper]);
    assert_eq!(by_account.rows[0].memo.as_deref(), Some("Printer paper"));
    assert!(by_account.rows[0].account.contains("Supplies"), "{}", by_account.rows[0].account);

    // Memo (any line, ignoring case) and number (ignoring a leading #).
    assert_eq!(ids(&run(filters(Filters { memo: Some("RENT".into()), ..Filters::default() })).await), [feb_rent, jan_rent]);
    assert_eq!(ids(&run(FindQuery { number: Some(" #501 ".into()), ..filters(Filters::default()) }).await), [jan_rent]);
    assert_eq!(ids(&run(FindQuery { number: Some(inv.invoice_number.to_string()), ..filters(Filters::default()) }).await), [inv_entry]);

    // Amounts bound the transaction's total, not a line.
    let exact = run(filters(Filters { amount_min_cents: Some(120_000), amount_max_cents: Some(120_000), ..Filters::default() })).await;
    assert_eq!(ids(&exact), [feb_rent, jan_rent]);
    assert_eq!(ids(&run(filters(Filters { amount_max_cents: Some(50_000), ..Filters::default() })).await), [inv_entry, paper]);

    // Free text: the name, number or a line's memo.
    assert_eq!(ids(&run(FindQuery { text: Some("landlord".into()), ..filters(Filters::default()) }).await), [feb_rent, jan_rent]);
    assert_eq!(ids(&run(FindQuery { text: Some("PAPER".into()), ..filters(Filters::default()) }).await), [paper]);
    assert_eq!(ids(&run(FindQuery { text: Some("502".into()), ..filters(Filters::default()) }).await), [feb_rent]);

    // The limit trims rows, not the count or total.
    let limited = run(FindQuery { limit: Some(1), ..filters(Filters::default()) }).await;
    assert_eq!((limited.rows.len(), limited.match_count, limited.total_cents), (1, 4, all.total_cents));

    // Voids are left out unless asked for; asked for, the void shows beside the original.
    crate::ledger::void_transaction(&pool, paper).await.unwrap();
    assert_eq!(ids(&run(filters(Filters::default())).await), [inv_entry, feb_rent, jan_rent]);
    let with_voids = run(FindQuery::default()).await;
    assert_eq!(with_voids.match_count, 5);
    assert!(with_voids.rows.iter().any(|r| r.journal_entry_id == paper && r.is_voided));
    assert!(with_voids.rows.iter().any(|r| r.is_reversal && r.type_label == "Void"));

    // Bad bounds.
    let backwards = FindQuery { start_date: Some(d(3, 1)), end_date: Some(d(2, 1)), ..FindQuery::default() };
    assert!(find(&pool, &backwards, today).await.unwrap_err().starts_with("RPT_BAD_RANGE"));
    let crossed = filters(Filters { amount_min_cents: Some(500), amount_max_cents: Some(100), ..Filters::default() });
    assert!(find(&pool, &crossed, today).await.unwrap_err().starts_with("FIND_BAD_AMOUNT"));
}

#[test]
fn the_screen_can_leave_everything_out() {
    let q: FindQuery = serde_json::from_value(serde_json::json!({ "filters": { "transactionTypes": ["BILL"] }, "number": "12" })).unwrap();
    assert_eq!(q.filters.transaction_types, ["BILL"]);
    assert_eq!((q.start_date, q.limit, q.number.as_deref()), (None, None, Some("12")));
}
