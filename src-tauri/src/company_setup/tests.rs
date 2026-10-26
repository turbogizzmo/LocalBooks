use std::collections::HashSet;

use super::*;
use crate::db::test_pool;

fn setup(industry: &str, entity_type: &str, accounts: &[&str]) -> CompanySetup {
    CompanySetup {
        industry: industry.into(),
        entity_type: entity_type.into(),
        accounts: accounts.iter().map(|a| a.to_string()).collect(),
        ..Default::default()
    }
}

/// Every account the industry offers, core and recommended.
fn everything(industry_name: &str) -> Vec<String> {
    let ind = industry(industry_name).unwrap();
    ind.core.iter().chain(&ind.recommended).cloned().collect()
}

async fn chart(pool: &SqlitePool) -> Vec<(Option<String>, String, String, bool)> {
    sqlx::query_as("SELECT account_number, full_name, account_type, is_system FROM v_chart_of_accounts ORDER BY account_number")
        .fetch_all(pool)
        .await
        .unwrap()
}

async fn number_of(pool: &SqlitePool, name: &str) -> Option<String> {
    sqlx::query_scalar("SELECT account_number FROM chart_of_accounts WHERE name = ?1").bind(name).fetch_optional(pool).await.unwrap().flatten()
}

#[test]
fn the_catalog_matches_quickbooks_shape() {
    let c = catalog();
    assert_eq!(c.industries.len(), 33);
    assert_eq!(c.industries.last().unwrap().name, "Other/None");
    let numbers: HashSet<&str> = c.accounts.iter().map(|a| a.number.as_str()).collect();
    assert_eq!(numbers.len(), c.accounts.len(), "account numbers are unique");
    for a in &c.accounts {
        assert!(a.number.len() == 5 && a.number.chars().all(|ch| ch.is_ascii_digit()), "{}", a.number);
        assert!(!a.name.contains(':') && !a.description.trim().is_empty(), "{}", a.name);
        if let Some(parent) = &a.parent {
            let p = catalog_account(parent).unwrap_or_else(|| panic!("{} has no parent {parent}", a.name));
            assert_eq!(p.account_type, a.account_type, "{} is the same type as its parent", a.name);
            assert!(p.parent.is_none(), "one level of subaccounts");
        }
    }
    for ind in &c.industries {
        for n in ind.core.iter().chain(&ind.recommended) {
            assert!(numbers.contains(n.as_str()), "{} offers unknown account {n}", ind.name);
        }
    }
    for (key, _, _) in ENTITY_TYPES {
        assert!(equity_for(key).iter().any(|e| e.number == RETAINED_EARNINGS_NUMBER), "{key} names the retained-earnings account");
    }
    let restaurant = industry("Restaurant, Caterer, or Bar").unwrap();
    assert!(restaurant.sales_tax && restaurant.core.contains(&"43800".to_string()), "Food Sales is a core restaurant account");
}

#[test]
fn starter_charts_list_required_core_and_recommended_accounts() {
    let accounts = starter_chart("General Service-based Business", "s_corporation").unwrap();
    let find = |n: &str| accounts.iter().find(|a| a.number == n).unwrap_or_else(|| panic!("{n}"));
    assert_eq!((find("10100").name.as_str(), find("10100").group.as_str()), ("Checking", "required"));
    assert_eq!(find("31400").name, "Shareholder Distributions");
    assert_eq!(find("32000").name, "Retained Earnings");
    assert_eq!(find("47900").group, "core");
    assert_eq!(find("63310").parent_number.as_deref(), Some("63300"));
    assert!(accounts.windows(2).all(|w| w[0].number < w[1].number), "in number order");

    let sole = starter_chart("Other/None", "sole_proprietorship").unwrap();
    assert_eq!(sole.iter().find(|a| a.number == "32000").unwrap().name, "Owners Equity");
    assert!(sole.iter().all(|a| a.group == "required"), "Other/None offers no industry accounts");

    assert!(starter_chart("Bakery", "other").unwrap_err().starts_with("SETUP_UNKNOWN_INDUSTRY"));
    assert!(starter_chart("Rental", "trust").unwrap_err().starts_with("SETUP_UNKNOWN_ENTITY"));
}

/// Every industry's full chart (every account it offers) and every tax form's equity can be
/// created, and each gives a chart the ledger accepts.
#[tokio::test]
async fn every_industry_and_tax_form_builds_a_valid_chart() {
    for ind in &catalog().industries {
        let pool = test_pool().await;
        let all = everything(&ind.name);
        let refs: Vec<&str> = all.iter().map(String::as_str).collect();
        apply(&pool, &setup(&ind.name, "corporation", &refs)).await.unwrap_or_else(|e| panic!("{}: {e}", ind.name));
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chart_of_accounts WHERE is_system = 0").fetch_one(&pool).await.unwrap();
        // The industry's accounts, the bank account, and Capital Stock and Dividends Paid.
        assert_eq!(count as usize, all.len() + 3, "{}", ind.name);
    }
    for (key, _, tax_form) in ENTITY_TYPES {
        let pool = test_pool().await;
        apply(&pool, &setup("Professional Consulting", key, &["42700"])).await.unwrap_or_else(|e| panic!("{key}: {e}"));
        let equity: Vec<String> = sqlx::query_scalar("SELECT name FROM chart_of_accounts WHERE account_type = 'EQUITY' AND account_number <> '30000' ORDER BY account_number")
            .fetch_all(&pool)
            .await
            .unwrap();
        let mut expected: Vec<(String, String)> = equity_for(key).iter().map(|e| (e.number.clone(), e.name.clone())).collect();
        expected.sort();
        assert_eq!(equity, expected.into_iter().map(|(_, n)| n).collect::<Vec<_>>(), "{key}");
        let form = crate::preferences::get(&pool, "tax_form").await.unwrap();
        assert_eq!(form, tax_form.unwrap_or("SCHEDULE_C"), "{key}");
    }
}

#[tokio::test]
async fn setup_replaces_the_standard_chart_and_saves_the_answers() {
    let pool = test_pool().await;
    let answers = CompanySetup {
        bank_account_name: Some(" Operating ".into()),
        legal_name: Some("Hearth Kitchen LLC".into()),
        ein: Some("123456789".into()),
        fiscal_year_start: Some("07-01".into()),
        sales_tax: true,
        inventory: true,
        // A General Liability Insurance subaccount brings Insurance Expense along.
        ..setup("Restaurant, Caterer, or Bar", "llc_single", &["43800", "50900", "63310"])
    };
    apply(&pool, &answers).await.unwrap();

    let rows = chart(&pool).await;
    let names: Vec<(&str, &str)> = rows.iter().map(|(n, name, _, _)| (n.as_deref().unwrap_or(""), name.as_str())).collect();
    assert_eq!(
        names,
        vec![
            ("10100", "Operating"),
            ("11000", "Accounts Receivable (A/R)"),
            ("12000", "Undeposited Funds"),
            ("12100", "Inventory Asset"),
            ("20000", "Accounts Payable (A/P)"),
            ("25500", "Sales Tax Payable"),
            ("30000", "Opening Balance Equity"),
            ("30700", "Members Draw"),
            ("32000", "Members Equity"),
            ("43800", "Food Sales"),
            ("50000", "Cost of Goods Sold"),
            ("50900", "Food Purchases"),
            ("63300", "Insurance Expense"),
            ("63310", "Insurance Expense:General Liability Insurance"),
            ("80800", "Exchange Gain or Loss"),
            ("80900", "Unrealized Exchange Gain or Loss"),
        ]
    );
    let prefs = crate::preferences::get_all(&pool).await.unwrap();
    for (key, value) in [("tax_form", "SCHEDULE_C"), ("sales_tax_enabled", "true"), ("company_legal_name", "Hearth Kitchen LLC"), ("company_ein", "12-3456789"), ("fiscal_year_start", "07-01")] {
        assert_eq!(prefs.get(key).map(String::as_str), Some(value), "{key}");
    }

    // The posting engine still finds its system accounts.
    let ar: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chart_of_accounts WHERE is_system = 1").fetch_one(&pool).await.unwrap();
    assert_eq!(ar, 7);
}

#[tokio::test]
async fn setup_refuses_accounts_the_industry_does_not_offer_and_used_files() {
    let pool = test_pool().await;
    let err = apply(&pool, &setup("Legal Services", "corporation", &["43800"])).await.unwrap_err();
    assert!(err.starts_with("SETUP_UNKNOWN_ACCOUNT"), "{err}");
    assert_eq!(number_of(&pool, "Checking").await.as_deref(), Some("1010"), "nothing changed");

    let id = |n: &'static str| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(&pool).await.unwrap() }
    };
    let (checking, services) = (id("1010").await, id("4100").await);
    crate::ledger::post_journal_entry(
        &pool,
        crate::ledger::JournalEntry {
            id: None,
            date: chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            description: String::new(),
            lines: vec![crate::ledger::JournalLine::debit(checking, 100), crate::ledger::JournalLine::credit(services, 100)],
            currency: None,
        },
    )
    .await
    .unwrap();
    let err = apply(&pool, &setup("Legal Services", "corporation", &[])).await.unwrap_err();
    assert!(err.starts_with("SETUP_NOT_NEW"), "{err}");
}

#[tokio::test]
async fn standard_accounts_follow_the_charts_numbering() {
    // The standard four-digit chart: a missing Inventory Asset comes back as 1400.
    let pool = test_pool().await;
    sqlx::query("DELETE FROM chart_of_accounts WHERE name = 'Inventory Asset'").execute(&pool).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let id = ensure_standard_account(&mut conn, INVENTORY_ASSET).await.unwrap();
    assert_eq!(ensure_standard_account(&mut conn, INVENTORY_ASSET).await.unwrap(), id, "found the second time");
    drop(conn);
    assert_eq!(number_of(&pool, "Inventory Asset").await.as_deref(), Some("1400"));

    // A QuickBooks-style chart: the first sales tax rate creates Sales Tax Payable as 25500.
    let pool = test_pool().await;
    apply(&pool, &setup("Professional Consulting", "corporation", &["42700", "24400"])).await.unwrap();
    let rate = crate::sales_tax::create_tax_rate(&pool, crate::sales_tax::TaxRateInput { name: "State".into(), rate_ppm: 60_000, agency_name: None, liability_account_id: None })
        .await
        .unwrap();
    assert_eq!(rate.liability_account_name, "Sales Tax Payable", "not Client Deposits");
    assert_eq!(number_of(&pool, "Sales Tax Payable").await.as_deref(), Some("25500"));
}
