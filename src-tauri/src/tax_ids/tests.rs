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
