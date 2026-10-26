use super::*;
use crate::db::test_pool;

/// A protected file with users 1, 3, 7, 8 and 9 (key slots need an existing user).
async fn protected() -> (sqlx::SqlitePool, DataKey, RecoveryKey) {
    let pool = test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    add_users(&mut conn).await;
    let (key, recovery) = create(&mut conn).await.unwrap();
    drop(conn);
    (pool, key, recovery)
}

async fn add_users(conn: &mut SqliteConnection) {
    for id in [1, 3, 7, 8, 9] {
        sqlx::query("INSERT INTO users (id, username, display_name, role, password_verifier) VALUES (?1, ?2, ?2, 'STANDARD', 'argon2id$test')")
            .bind(id)
            .bind(format!("user{id}"))
            .execute(&mut *conn)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn a_password_slot_unlocks_the_same_key() {
    let (pool, key, _) = protected().await;
    let mut conn = pool.acquire().await.unwrap();
    set_password(&mut conn, &key, 7, "corr3ct horse").await.unwrap();
    let stored = key.encrypt_field("vendors", "tax_id", 12, "123-45-6789").unwrap();

    let unlocked = unlock_with_password(&mut conn, 7, "corr3ct horse").await.unwrap();
    assert_eq!(unlocked.key_id(), key.key_id());
    assert_eq!(unlocked.decrypt_field("vendors", "tax_id", 12, &stored).unwrap(), "123-45-6789");
}

#[tokio::test]
async fn wrong_passwords_and_missing_slots_are_refused() {
    let (pool, key, _) = protected().await;
    let mut conn = pool.acquire().await.unwrap();
    set_password(&mut conn, &key, 7, "corr3ct horse").await.unwrap();
    assert!(unlock_with_password(&mut conn, 7, "Corr3ct horse").await.unwrap_err().starts_with("KEY_WRONG_PASSWORD"));
    assert!(unlock_with_password(&mut conn, 8, "corr3ct horse").await.unwrap_err().starts_with("KEY_NO_SLOT"));
    assert!(set_password(&mut conn, &key, 8, "short").await.unwrap_err().starts_with("PASSWORD_TOO_SHORT"));

    sqlx::query("UPDATE users SET can_see_sensitive = 0 WHERE id = 7").execute(&mut *conn).await.unwrap();
    assert!(unlock_with_password(&mut conn, 7, "corr3ct horse").await.unwrap_err().starts_with("KEY_NO_SLOT"), "losing access removes the slot");
}

#[tokio::test]
async fn changing_a_password_replaces_the_slot() {
    let (pool, key, _) = protected().await;
    let mut conn = pool.acquire().await.unwrap();
    set_password(&mut conn, &key, 7, "first password").await.unwrap();
    set_password(&mut conn, &key, 7, "sécond pässword").await.unwrap();
    assert!(unlock_with_password(&mut conn, 7, "first password").await.is_err());
    assert!(unlock_with_password(&mut conn, 7, "sécond pässword").await.is_ok());
    let slots: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM key_slots WHERE user_id = 7").fetch_one(&mut *conn).await.unwrap();
    assert_eq!(slots, 1);
}

#[tokio::test]
async fn a_slot_moved_to_another_user_does_not_open() {
    let (pool, key, _) = protected().await;
    let mut conn = pool.acquire().await.unwrap();
    set_password(&mut conn, &key, 7, "corr3ct horse").await.unwrap();
    sqlx::query("UPDATE key_slots SET user_id = 9 WHERE user_id = 7").execute(&mut *conn).await.unwrap();
    assert!(unlock_with_password(&mut conn, 9, "corr3ct horse").await.unwrap_err().starts_with("KEY_WRONG_PASSWORD"));
}

#[tokio::test]
async fn the_recovery_key_unlocks_and_can_be_replaced() {
    let (pool, key, recovery) = protected().await;
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(recovery.as_str().len(), 39, "eight groups of four and seven dashes");
    let typed = recovery.as_str().to_lowercase().replace('-', " ").replace('0', "o");
    assert_eq!(unlock_with_recovery_key(&mut conn, &typed).await.unwrap().key_id(), key.key_id());

    let fresh = replace_recovery_key(&mut conn, &key).await.unwrap();
    assert!(unlock_with_recovery_key(&mut conn, recovery.as_str()).await.unwrap_err().starts_with("KEY_WRONG_RECOVERY"));
    assert!(unlock_with_recovery_key(&mut conn, fresh.as_str()).await.is_ok());
    assert!(unlock_with_recovery_key(&mut conn, "ABCD-EFGH").await.unwrap_err().starts_with("KEY_RECOVERY_FORMAT"));
    assert!(unlock_with_recovery_key(&mut conn, &fresh.as_str().replace('-', "U")).await.unwrap_err().starts_with("KEY_RECOVERY_FORMAT"));
}

#[tokio::test]
async fn fields_are_bound_to_their_place_and_tamper_evident() {
    let (_pool, key, _) = protected().await;
    let a = key.encrypt_field("vendors", "tax_id", 12, "12-3456789").unwrap();
    let b = key.encrypt_field("vendors", "tax_id", 12, "12-3456789").unwrap();
    assert!(is_encrypted(&a) && a != b, "every encryption uses a fresh nonce");
    assert!(!a.contains("3456789"));

    assert!(key.decrypt_field("vendors", "tax_id", 13, &a).unwrap_err().starts_with("FIELD_UNREADABLE"));
    assert!(key.decrypt_field("customers", "tax_id", 12, &a).unwrap_err().starts_with("FIELD_UNREADABLE"));
    let mut tampered = a.clone().into_bytes();
    let last = tampered.len() - 3;
    tampered[last] = if tampered[last] == b'A' { b'B' } else { b'A' };
    assert!(key.decrypt_field("vendors", "tax_id", 12, &String::from_utf8(tampered).unwrap()).is_err());
    assert!(key.decrypt_field("vendors", "tax_id", 12, "12-3456789").unwrap_err().starts_with("FIELD_NOT_ENCRYPTED"));
}

#[tokio::test]
async fn another_file_s_key_cannot_read_a_field() {
    let (_a, key, _) = protected().await;
    let (_b, other, _) = protected().await;
    let stored = key.encrypt_field("vendors", "tax_id", 1, "123-45-6789").unwrap();
    assert!(other.decrypt_field("vendors", "tax_id", 1, &stored).is_err());
}

#[tokio::test]
async fn protection_is_created_once_and_slots_need_a_key() {
    let pool = test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    add_users(&mut conn).await;
    assert!(!is_protected(&mut conn).await.unwrap());
    assert!(unlock_with_password(&mut conn, 1, "whatever123").await.unwrap_err().starts_with("DATA_KEY_MISSING"));
    let err = sqlx::query("INSERT INTO key_slots (kind, user_id, memory_kib, iterations, parallelism, salt, nonce, wrapped_key) VALUES ('PASSWORD', 1, 8, 1, 1, zeroblob(16), zeroblob(12), zeroblob(48))")
        .execute(&mut *conn)
        .await
        .unwrap_err();
    assert!(db_err(err).starts_with("DATA_KEY_MISSING"));

    let (key, _) = create(&mut conn).await.unwrap();
    assert!(is_protected(&mut conn).await.unwrap());
    assert!(create(&mut conn).await.unwrap_err().starts_with("DATA_KEY_EXISTS"));
    let err = sqlx::query("UPDATE data_key SET key_id = ?1").bind("0".repeat(32)).execute(&mut *conn).await.unwrap_err();
    assert!(db_err(err).starts_with("DATA_KEY_IMMUTABLE"));
    assert!(format!("{key:?}").contains(key.key_id()) && !format!("{key:?}").contains("key:"));
}

#[tokio::test]
async fn a_slot_keeps_the_cost_it_was_made_with() {
    let (pool, key, _) = protected().await;
    let mut conn = pool.acquire().await.unwrap();
    let stronger = Cost { memory_kib: 64, iterations: 2, parallelism: 2 };
    add_slot(&mut conn, &key, "PASSWORD", Some(3), Zeroizing::new(b"older slot pw".to_vec()), stronger).await.unwrap();
    assert!(unlock_with_password(&mut conn, 3, "older slot pw").await.is_ok());
}

#[tokio::test]
async fn a_copied_company_file_unlocks_on_its_own() {
    let dir = std::env::temp_dir().join(format!("localbooks-crypto-{}-{}", std::process::id(), chrono::Local::now().format("%H%M%S%f")));
    std::fs::create_dir_all(&dir).unwrap();
    let (original, copy) = (dir.join("original.localbooks"), dir.join("copy.localbooks"));
    let pool = crate::db::open(&original).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    add_users(&mut conn).await;
    let (key, recovery) = create(&mut conn).await.unwrap();
    set_password(&mut conn, &key, 1, "owner password").await.unwrap();
    let stored = key.encrypt_field("vendors", "tax_id", 5, "98-7654321").unwrap();
    drop(conn);
    sqlx::query("VACUUM INTO ?1").bind(copy.to_string_lossy().to_string()).execute(&pool).await.unwrap();
    pool.close().await;

    let pool = crate::db::open(&copy).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let by_password = unlock_with_password(&mut conn, 1, "owner password").await.unwrap();
    let by_recovery = unlock_with_recovery_key(&mut conn, recovery.as_str()).await.unwrap();
    assert_eq!(by_password.decrypt_field("vendors", "tax_id", 5, &stored).unwrap(), "98-7654321");
    assert_eq!(by_recovery.decrypt_field("vendors", "tax_id", 5, &stored).unwrap(), "98-7654321");
    drop(conn);
    pool.close().await;
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn password_verifiers_check_the_password() {
    let verifier = password_verifier("corr3ct horse").await.unwrap();
    assert!(verifier.starts_with("argon2id$") && !verifier.contains("corr3ct"));
    assert!(verify_password("corr3ct horse", &verifier).await.unwrap());
    assert!(!verify_password("corr3ct horsE", &verifier).await.unwrap());
    assert_ne!(verifier, password_verifier("corr3ct horse").await.unwrap(), "salted");
    assert!(verify_password("x", "argon2id$1$2").await.unwrap_err().starts_with("CRYPTO_VERIFIER"));
}

#[tokio::test]
async fn a_tampered_slot_cost_is_refused_not_obeyed() {
    let (pool, key, _) = protected().await;
    let mut conn = pool.acquire().await.unwrap();
    set_password(&mut conn, &key, 7, "corr3ct horse").await.unwrap();
    sqlx::query("UPDATE key_slots SET memory_kib = 1073741824 WHERE user_id = 7").execute(&mut *conn).await.unwrap();
    assert!(unlock_with_password(&mut conn, 7, "corr3ct horse").await.unwrap_err().starts_with("CRYPTO_KDF"));
    assert!(verify_password("x", "argon2id$99999999$3$1$AAAAAAAAAAAAAAAAAAAAAA$AAAA").await.unwrap_err().starts_with("CRYPTO_KDF"));
    assert!(set_password(&mut conn, &key, 7, &"x".repeat(MAX_PASSWORD_CHARS + 1)).await.unwrap_err().starts_with("PASSWORD_TOO_LONG"));
}

#[tokio::test]
async fn slots_need_an_existing_user() {
    let pool = test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let (key, _) = create(&mut conn).await.unwrap();
    assert!(set_password(&mut conn, &key, 42, "corr3ct horse").await.unwrap_err().starts_with("KEY_SLOT_NO_USER"));
}

#[test]
fn base32_round_trips() {
    let bytes: [u8; RECOVERY_BYTES] = core::array::from_fn(|i| (i * 37 + 11) as u8);
    assert_eq!(&parse_recovery_key(&encode_base32(&bytes)).unwrap()[..], &bytes[..]);
}

/// How long one unlock takes at the production cost: run with `--release -- --ignored
/// measures_the_production_cost --nocapture` on the slowest computer you support.
#[tokio::test]
#[ignore]
async fn measures_the_production_cost() {
    let started = std::time::Instant::now();
    derive(Zeroizing::new(b"a password".to_vec()), [7; SALT_LEN], PRODUCTION_COST).await.unwrap();
    println!("one unlock: {:?}", started.elapsed());
}
