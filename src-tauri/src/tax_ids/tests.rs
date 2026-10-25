use super::*;
use crate::db::test_pool;

#[test]
fn masks_keep_the_last_four_and_the_shape() {
    assert_eq!(mask("123-45-6789"), "***-**-6789");
    assert_eq!(mask("12-3456789"), "**-***6789");
    assert_eq!(mask("123"), "***");
    assert_eq!(mask("12-34567"), "**-*****", "too short to show the last four");
    assert!(looks_like_tax_id("12-3456789") && looks_like_tax_id("123 45 6789"));
    assert!(!looks_like_tax_id("AQAAANCMnd8BFdERjHoAwE/Cl+sBAAAAexample") && !looks_like_tax_id("n/a"));
}

#[tokio::test]
async fn a_file_without_users_stores_and_shows_plain_ids() {
    let (stored, masked) = store(Access::OPEN, 1, Some(" 12-3456789 ")).unwrap();
    assert_eq!((stored.as_deref(), masked.as_deref()), (Some("12-3456789"), Some("**-***6789")));
    let view = show(Access::OPEN, 1, stored.as_deref(), masked.as_deref()).unwrap();
    assert_eq!(view.full.as_deref(), Some("12-3456789"));
    assert_eq!(store(Access::OPEN, 1, Some("  ")).unwrap(), (None, None));
    assert_eq!(show(Access::OPEN, 1, None, None).unwrap(), TaxIdView::default());
}

#[tokio::test]
async fn protected_ids_show_only_with_the_key() {
    let pool = test_pool().await;
    let (session, _) = crate::users::turn_on(&pool, "Owner", None, "owner password").await.unwrap();
    let key = session.key.as_ref().unwrap();
    let with_key = Access { protected: true, key: Some(key) };
    let without = Access { protected: true, key: None };

    let (stored, masked) = store(with_key, 4, Some("123-45-6789")).unwrap();
    assert!(crypto::is_encrypted(stored.as_deref().unwrap()));
    assert_eq!(show(with_key, 4, stored.as_deref(), masked.as_deref()).unwrap().full.as_deref(), Some("123-45-6789"));
    let hidden = show(without, 4, stored.as_deref(), masked.as_deref()).unwrap();
    assert_eq!((hidden.full, hidden.masked.as_deref()), (None, Some("***-**-6789")));
    assert!(store(without, 4, Some("123-45-6789")).unwrap_err().starts_with("TAX_ID_LOCKED"));
    assert!(show(with_key, 5, stored.as_deref(), masked.as_deref()).is_err(), "bound to its vendor");

    // A plain value (an import not yet protected) is still masked for users without the key.
    let plain = show(without, 4, Some("98-7654321"), None).unwrap();
    assert_eq!((plain.full, plain.masked.as_deref()), (None, Some("**-***4321")));
}

#[tokio::test]
async fn no_plain_tax_id_is_left_in_the_file_after_turning_users_on() {
    let dir = std::env::temp_dir().join(format!("localbooks-plain-{}-{}", std::process::id(), chrono::Local::now().format("%H%M%S%f")));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("books.localbooks");
    let pool = crate::db::open(&path).await.unwrap();
    for i in 0..200 {
        sqlx::query("INSERT INTO vendors (name, tax_id) VALUES (?1, ?2)").bind(format!("Vendor {i}")).bind(format!("987-65-{i:04}")).execute(&pool).await.unwrap();
    }
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)").execute(&pool).await.unwrap();
    crate::users::turn_on(&pool, "Owner", None, "owner password").await.unwrap();
    crate::db::compact(&pool).await.unwrap();
    pool.close().await;

    let mut bytes = std::fs::read(&path).unwrap();
    for sidecar in ["-wal", "-journal"] {
        bytes.extend(std::fs::read(format!("{}{sidecar}", path.display())).unwrap_or_default());
    }
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("987-65-0042"), "a plain tax ID survived in the file or its journal");
    assert!(text.contains("***-**-0042"), "the masked copy is there");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn protect_all_encrypts_plain_ids_once() {
    let pool = test_pool().await;
    let a: i64 = sqlx::query_scalar("INSERT INTO vendors (name, tax_id) VALUES ('Acme', '12-3456789') RETURNING id").fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO vendors (name) VALUES ('No ID')").execute(&pool).await.unwrap();
    let (session, _) = crate::users::turn_on(&pool, "Owner", None, "owner password").await.unwrap();
    let key = session.key.as_ref().unwrap();

    let (stored, masked): (String, String) = sqlx::query_as("SELECT tax_id, tax_id_masked FROM vendors WHERE id = ?1").bind(a).fetch_one(&pool).await.unwrap();
    assert!(crypto::is_encrypted(&stored), "turning users on protected the existing ID");
    assert_eq!(masked, "**-***6789");
    assert_eq!(key.decrypt_field(TABLE, COLUMN, a, &stored).unwrap(), "12-3456789");
    assert_eq!(protect_all(&mut pool.acquire().await.unwrap(), key).await.unwrap(), 0, "nothing left to protect");
}

/// Shaped like the value a newer QuickBooks file holds in place of a vendor's tax ID (base64).
const QB_VALUE: &str = "AQAAANCMnd8BFdERjHoAwE/Cl+sBAAAAexampleOnlyNotAKey12==";

#[test]
fn only_quickbooks_encrypted_values_match() {
    assert!(is_quickbooks_encrypted(QB_VALUE));
    assert!(is_quickbooks_encrypted(&format!("  {QB_VALUE} ")));
    for kept in ["12-3456789", "123-45-6789", "n/a", "W-9 requested 2026/01/05", "123456789012345678901234567890", "ABCDEFGHIJKLMNOPQRSTUVWXYZ12", "Pending: see email from pat@x.com"] {
        assert!(!is_quickbooks_encrypted(kept), "{kept}");
    }
}

/// Migration 0046 and `is_quickbooks_encrypted` must agree on every value.
#[tokio::test]
async fn the_migration_clears_exactly_what_the_rule_matches() {
    let pool = test_pool().await;
    let samples = [QB_VALUE, "12-3456789", "n/a", "W-9 requested 2026/01/05", "123456789012345678901234567890", "ABCDEFGHIJKLMNOPQRSTUVWXYZ12", "abc/def+ghi=jkl/mno+pqr1=", "lb1:not-checked-by-sql"];
    for (n, value) in samples.iter().enumerate() {
        sqlx::query("INSERT INTO vendors (name, tax_id, tax_id_masked) VALUES (?1, ?2, 'masked')").bind(format!("Vendor {n}")).bind(value).execute(&pool).await.unwrap();
    }
    sqlx::raw_sql(include_str!("../../migrations/0046_clear_quickbooks_encrypted_tax_ids.sql")).execute(&pool).await.unwrap();
    for (n, value) in samples.iter().enumerate() {
        let (tax_id, masked): (Option<String>, Option<String>) =
            sqlx::query_as("SELECT tax_id, tax_id_masked FROM vendors WHERE name = ?1").bind(format!("Vendor {n}")).fetch_one(&pool).await.unwrap();
        let cleared = !value.starts_with("lb1:") && is_quickbooks_encrypted(value);
        assert_eq!(tax_id.is_none(), cleared, "{value}");
        assert_eq!(masked.is_none(), cleared, "{value}");
    }
}

#[tokio::test]
async fn turning_users_on_clears_quickbooks_values_and_encrypts_real_ids() {
    let pool = test_pool().await;
    let qb: i64 = sqlx::query_scalar("INSERT INTO vendors (name, tax_id) VALUES ('Imported', ?1) RETURNING id").bind(QB_VALUE).fetch_one(&pool).await.unwrap();
    let real: i64 = sqlx::query_scalar("INSERT INTO vendors (name, tax_id) VALUES ('Real', '12-3456789') RETURNING id").fetch_one(&pool).await.unwrap();
    let (session, _) = crate::users::turn_on(&pool, "Owner", None, "owner password").await.unwrap();
    let get = |id: i64| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, Option<String>>("SELECT tax_id FROM vendors WHERE id = ?1").bind(id).fetch_one(&pool).await.unwrap() }
    };
    assert_eq!(get(qb).await, None, "QuickBooks' value is cleared, not encrypted");
    let stored = get(real).await.unwrap();
    assert_eq!(session.key.as_ref().unwrap().decrypt_field(TABLE, COLUMN, real, &stored).unwrap(), "12-3456789");
}

#[tokio::test]
async fn quickbooks_values_encrypted_before_the_check_are_cleared_at_the_next_unlock() {
    let pool = test_pool().await;
    let (session, _) = crate::users::turn_on(&pool, "Owner", None, "owner password").await.unwrap();
    let key = session.key.as_ref().unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO vendors (name) VALUES ('Imported') RETURNING id").fetch_one(&pool).await.unwrap();
    // As v0.5.0 stored it: QuickBooks' value, encrypted and masked like a real ID.
    let (stored, masked) = store(Access { protected: true, key: Some(key) }, id, Some(QB_VALUE)).unwrap();
    sqlx::query("UPDATE vendors SET tax_id = ?1, tax_id_masked = ?2 WHERE id = ?3").bind(stored).bind(masked).bind(id).execute(&pool).await.unwrap();

    assert_eq!(protect_all(&mut pool.acquire().await.unwrap(), key).await.unwrap(), 0);
    let (tax_id, masked): (Option<String>, Option<String>) = sqlx::query_as("SELECT tax_id, tax_id_masked FROM vendors WHERE id = ?1").bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!((tax_id, masked), (None, None));
}

