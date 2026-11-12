use super::*;
use crate::db::test_pool;
use crate::expenses::ExpenseInput;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn pay(pool: &SqlitePool, vendor: &str, date: &str, cents: i64) {
    let (checking, contractors) = (account(pool, "1010").await, account(pool, "6400").await);
    let check: ExpenseInput =
        serde_json::from_value(serde_json::json!({ "kind": "CHECK", "payeeName": vendor, "paymentAccountId": checking, "expenseDate": date,
        "checkNumber": null, "paymentMethod": null, "memo": null, "lines": [{ "accountId": contractors, "amountCents": cents, "memo": null }] }))
        .unwrap();
    crate::expenses::post_expense(pool, check).await.unwrap();
}

#[test]
fn tins_are_checked_without_repeating_them() {
    assert_eq!(check_tin(TinType::Ssn, "123456789").unwrap(), "123-45-6789");
    assert_eq!(check_tin(TinType::Ssn, "123-45-6789").unwrap(), "123-45-6789");
    assert_eq!(check_tin(TinType::Ssn, "912-70-1234").unwrap(), "912-70-1234", "an ITIN");
    assert_eq!(check_tin(TinType::Ein, "12 3456789").unwrap(), "12-3456789");
    for (kind, bad) in [
        (TinType::Ssn, "000-12-3456"),
        (TinType::Ssn, "666-12-3456"),
        (TinType::Ssn, "123-00-4567"),
        (TinType::Ssn, "123-45-0000"),
        (TinType::Ssn, "912-40-1234"),
        (TinType::Ssn, "12345678"),
        (TinType::Ssn, "123-45-678X"),
        (TinType::Ein, "07-1234567"),
        (TinType::Ein, "89-1234567"),
        (TinType::Ein, "1234567890"),
    ] {
        let err = check_tin(kind, bad).unwrap_err();
        assert!(err.starts_with("TIN_"), "{bad}: {err}");
        let digits: String = bad.chars().filter(char::is_ascii_digit).collect();
        assert!(!err.contains(bad) && !err.contains(&digits[..digits.len().min(9)]), "the message does not repeat the number: {err}");
    }
}

#[tokio::test]
async fn a_w9_keeps_its_details_and_stores_the_tin_on_the_vendor() {
    let pool = test_pool().await;
    let v = crate::expenses::create_vendor(&pool, "Ridgeline Carpentry", None).await.unwrap();
    let input = W9Input {
        tin_type: Some(TinType::Ein),
        tin: Some("41 2345678".into()),
        legal_name: Some("Ridgeline Carpentry LLC".into()),
        tax_classification: Some("LLC_P".into()),
        received_on: NaiveDate::from_ymd_opt(2026, 1, 9),
        ..Default::default()
    };
    let w = set_w9(&pool, Access::OPEN, v.id, &input).await.unwrap();
    assert_eq!((w.tin.as_deref(), w.tin_masked.as_deref(), w.tin_type.as_deref()), (Some("41-2345678"), Some("**-***5678"), Some("EIN")));
    assert_eq!(w.legal_name.as_deref(), Some("Ridgeline Carpentry LLC"));

    // Bad input is refused with nothing saved.
    let bad = W9Input { tin_type: Some(TinType::Ein), tin: Some("00-0000000".into()), ..Default::default() };
    assert!(set_w9(&pool, Access::OPEN, v.id, &bad).await.unwrap_err().starts_with("TIN_INVALID"));
    assert!(set_w9(&pool, Access::OPEN, v.id, &W9Input { tin: Some("412345678".into()), ..Default::default() }).await.unwrap_err().starts_with("TIN_TYPE"));
    assert!(set_w9(&pool, Access::OPEN, v.id, &W9Input { tax_classification: Some("PIRATE".into()), keep_tin: true, ..Default::default() })
        .await
        .unwrap_err()
        .starts_with("W9_CLASSIFICATION"));
    assert_eq!(get_w9(&pool, Access::OPEN, v.id).await.unwrap().tin.as_deref(), Some("41-2345678"));

    // keep_tin leaves the number alone; the audit trail notes the change but never the number.
    set_w9(&pool, Access::OPEN, v.id, &W9Input { keep_tin: true, legal_name: Some("Ridgeline Carpentry, LLC".into()), ..input.clone() }).await.unwrap();
    assert_eq!(get_w9(&pool, Access::OPEN, v.id).await.unwrap().tin.as_deref(), Some("41-2345678"));
    let audit: Vec<String> = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log").fetch_all(&pool).await.unwrap();
    assert!(audit.iter().any(|a| a.contains("tax ID")));
    assert!(audit.iter().all(|a| !a.contains("2345678")), "no TIN in the audit trail");
}

#[tokio::test]
async fn the_1096_summary_lists_forms_over_the_threshold_and_what_is_missing() {
    let pool = test_pool().await;
    for (name, is_1099) in [("Ridgeline Carpentry", 1), ("Small Jobs Co", 1), ("Not Tracked", 0)] {
        let v = crate::expenses::create_vendor(&pool, name, None).await.unwrap();
        sqlx::query("UPDATE vendors SET is_1099 = ?1, address = CASE WHEN ?2 = 'Ridgeline Carpentry' THEN '12 Fir Lane, Bend, OR 97701' END WHERE id = ?3")
            .bind(is_1099)
            .bind(name)
            .bind(v.id)
            .execute(&pool)
            .await
            .unwrap();
        if name == "Ridgeline Carpentry" {
            let input = W9Input {
                tin_type: Some(TinType::Ssn),
                tin: Some("123-45-6789".into()),
                legal_name: Some("Dana Ridge".into()),
                received_on: NaiveDate::from_ymd_opt(2026, 1, 9),
                ..Default::default()
            };
            set_w9(&pool, Access::OPEN, v.id, &input).await.unwrap();
        }
    }
    pay(&pool, "Ridgeline Carpentry", "2026-03-01", 150_000).await;
    pay(&pool, "Ridgeline Carpentry", "2026-06-01", 100_000).await;
    pay(&pool, "Ridgeline Carpentry", "2025-12-31", 99_000).await;
    pay(&pool, "Small Jobs Co", "2026-03-01", 199_999).await;
    pay(&pool, "Not Tracked", "2026-03-01", 500_000).await;

    let s = nec_summary(&pool, 2026, Access::OPEN).await.unwrap();
    assert_eq!((s.form_count, s.total_cents, s.threshold_cents), (1, 250_000, 200_000));
    let f = &s.forms[0];
    assert_eq!(
        (f.recipient_name.as_str(), f.tin.as_deref(), f.tin_masked.as_deref(), f.tin_type.as_deref()),
        ("Dana Ridge", Some("123-45-6789"), Some("***-**-6789"), Some("SSN"))
    );
    assert!(f.missing.is_empty(), "{:?}", f.missing);
    assert_eq!(s.under_threshold, vec![("Small Jobs Co".to_string(), 199_999)]);
    assert_eq!(s.payer_missing, vec!["company EIN".to_string(), "company address".to_string()]);

    crate::preferences::set(&pool, "company_ein", "41-1234567").await.unwrap();
    crate::preferences::set(&pool, "company_address", "1 Main St, Bend, OR 97701").await.unwrap();
    pay(&pool, "Small Jobs Co", "2026-09-01", 1).await;
    let s = nec_summary(&pool, 2026, Access::OPEN).await.unwrap();
    assert!(s.payer_missing.is_empty());
    assert_eq!((s.form_count, s.total_cents), (2, 450_000));
    let small = s.forms.iter().find(|f| f.recipient_name == "Small Jobs Co").unwrap();
    assert_eq!(small.missing, vec!["TIN", "W-9 received date", "address"]);
    let last_year = nec_summary(&pool, 2025, Access::OPEN).await.unwrap();
    assert_eq!((last_year.form_count, last_year.total_cents, last_year.threshold_cents), (1, 99_000, 60_000), "2025 payments go by 2025's $600 threshold");
}
