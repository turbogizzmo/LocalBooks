use super::*;
use crate::db;
use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};
use chrono::NaiveDate;

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("localbooks-offsite-{tag}-{}-{}", std::process::id(), Local::now().format("%H%M%S%f")));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn post_sale(pool: &SqlitePool, cents: i64) {
    let id = |n: &'static str| async move { sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(pool).await.unwrap() };
    let (checking, services) = (id("1010").await, id("4100").await);
    let entry = JournalEntry { id: None, date: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(), description: "Sale to Quillfeather Studio".into(), lines: vec![JournalLine::debit(checking, cents), JournalLine::credit(services, cents)], currency: None };
    post_journal_entry(pool, entry).await.unwrap();
}

async fn total_posted(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents), 0) FROM v_posted_lines").fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn chunks_round_trip_and_tampering_is_caught() {
    let tmp = TempDir::new("chunks");
    let salt = [7u8; SALT_LEN];
    let cost = KdfCost::for_new_keys();
    let key = crypto::derive_backup_key("correct horse battery", salt, cost).await.unwrap();
    // Exactly one chunk, a chunk and a bit, and empty: the edges of the chunking.
    for size in [0usize, CHUNK, CHUNK + 5, 3 * CHUNK - 1] {
        let plain = tmp.0.join("plain.bin");
        let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        std::fs::write(&plain, &data).unwrap();
        let sealed = tmp.0.join("sealed.lbbackup");
        encrypt_file(&plain, &sealed, &key, &salt, cost).unwrap();
        let bytes = std::fs::read(&sealed).unwrap();
        let chunks = size.div_ceil(CHUNK).max(1);
        assert_eq!(bytes.len(), HEADER_LEN + size + TAG_LEN * chunks, "size {size}");
        let out = tmp.0.join("out.bin");
        decrypt_file(&sealed, "correct horse battery", &out).await.unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), data, "size {size}");
        assert!(decrypt_file(&sealed, "wrong horse battery", &out).await.unwrap_err().starts_with("OFFSITE_DECRYPT"));
        if size > CHUNK {
            // Cut off after the first chunk: the first chunk was not sealed as the last.
            std::fs::write(&sealed, &bytes[..HEADER_LEN + CHUNK + TAG_LEN]).unwrap();
            assert!(decrypt_file(&sealed, "correct horse battery", &out).await.unwrap_err().starts_with("OFFSITE_DECRYPT"), "truncated");
        }
        assert!(!out.exists(), "a failed decrypt leaves no partial plaintext");
        let mut flipped = bytes.clone();
        let last = flipped.len() - 1;
        flipped[last] ^= 1;
        std::fs::write(&sealed, &flipped).unwrap();
        assert!(decrypt_file(&sealed, "correct horse battery", &out).await.unwrap_err().starts_with("OFFSITE_DECRYPT"), "flipped bit");
    }
    // A header whose Argon2 cost is out of range is not a backup.
    let mut bad_cost = std::fs::read(tmp.0.join("sealed.lbbackup")).unwrap();
    bad_cost[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
    std::fs::write(tmp.0.join("bad.lbbackup"), &bad_cost).unwrap();
    assert!(decrypt_file(&tmp.0.join("bad.lbbackup"), "correct horse battery", &tmp.0.join("o")).await.unwrap_err().starts_with("OFFSITE_NOT_A_BACKUP"));
    std::fs::write(tmp.0.join("x.lbbackup"), b"SQLite format 3\0 and more").unwrap();
    assert!(decrypt_file(&tmp.0.join("x.lbbackup"), "correct horse battery", &tmp.0.join("o")).await.unwrap_err().starts_with("OFFSITE_NOT_A_BACKUP"));
}

#[tokio::test]
async fn a_recovery_key_opens_a_copy_however_it_is_typed() {
    let salt = [1u8; SALT_LEN];
    let cost = KdfCost::for_new_keys();
    let a = crypto::derive_backup_key("ABCD-EFGH-JKMN-PQRS-TVWX-YZ01-2345-6789", salt, cost).await.unwrap();
    let b = crypto::derive_backup_key("abcd efgh jkmn pqrs tvwx yz01 2345 6789", salt, cost).await.unwrap();
    assert_eq!(a.as_slice(), b.as_slice());
}

#[tokio::test]
async fn offsite_copies_are_encrypted_pruned_tested_and_restored() {
    let tmp = TempDir::new("flow");
    let live = tmp.0.join("live.db");
    let synced = tmp.0.join("Cloud Drive");
    std::fs::create_dir_all(&synced).unwrap();
    let db = Db::open_at(live.clone(), db::test_config(&tmp.0)).await.unwrap();
    let store = SecretStore::in_file(tmp.0.join("keys.json"), KEYCHAIN_SERVICE, "the offsite backup key");
    let pool = db.pool().await;
    crate::preferences::set(&pool, "company_name", "Quillfeather Studio").await.unwrap();
    post_sale(&pool, 10_000).await;

    assert!(back_up_now(&db, &store).await.unwrap_err().starts_with("OFFSITE_OFF"));
    assert!(run_if_due(&db, &store).await.unwrap().is_none(), "off: nothing is due");
    assert!(set_up(&pool, &store, &synced, "short", 3).await.unwrap_err().starts_with("OFFSITE_PASSWORD_TOO_SHORT"));
    assert!(set_up(&pool, &store, &tmp.0.join("missing"), "a long password", 3).await.unwrap_err().starts_with("OFFSITE_FOLDER"));
    let s = set_up(&pool, &store, &synced, "a long password", 2).await.unwrap();
    assert_eq!((s.keep, s.last_backup_at.is_none()), (2, true));
    let keys = std::fs::read_to_string(tmp.0.join("keys.json")).unwrap();
    assert!(!keys.contains("a long password"), "the password itself is never stored");

    let first = run_if_due(&db, &store).await.unwrap().expect("due on first run");
    assert!(first.file_name.starts_with("Quillfeather-Studio-") && first.file_name.ends_with(".lbbackup"));
    assert!(run_if_due(&db, &store).await.unwrap().is_none(), "a fresh copy is not due again");
    let bytes = std::fs::read(&first.path).unwrap();
    assert!(!bytes.windows(15).any(|w| w == b"SQLite format 3"), "no plain database header");
    assert!(!bytes.windows(11).any(|w| w == b"Quillfeathe"), "no readable business data");
    assert!(std::fs::read_dir(db.backups_dir()).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().starts_with(".offsite")), "the plain temp copy is gone");

    // Three copies with two to keep: the oldest goes.
    for _ in 0..2 {
        tokio::time::sleep(Duration::from_millis(1100)).await;
        back_up_now(&db, &store).await.unwrap();
    }
    let copies = list_copies(&synced);
    assert_eq!(copies.len(), 2);
    assert!(!copies.iter().any(|c| c.file_name == first.file_name));

    // A test restore checks the newest copy without touching the books.
    post_sale(&pool, 5_000).await;
    assert!(test_restore(&db, Path::new(&copies[0].path), "not the password").await.unwrap_err().starts_with("OFFSITE_DECRYPT"));
    let info = test_restore(&db, Path::new(&copies[0].path), "a long password").await.unwrap();
    assert_eq!((info.company_name.as_deref(), info.journal_entries), (Some("Quillfeather Studio"), 1));
    assert_eq!(total_posted(&db.pool().await).await, 15_000);
    assert!(settings(&db.pool().await).await.unwrap().unwrap().last_test_restore_at.is_some());

    // Restoring brings the books back to the copy.
    restore(&db, Path::new(&copies[0].path), "a long password").await.unwrap();
    assert_eq!(total_posted(&db.pool().await).await, 10_000);

    // Without the key on this computer, copies stop with a clear error until the password is given again.
    let other = SecretStore::in_file(tmp.0.join("other.json"), KEYCHAIN_SERVICE, "the offsite backup key");
    let st = status(&db.pool().await, &other).await.unwrap();
    assert!(!st.has_key);
    assert!(back_up_now(&db, &other).await.unwrap_err().starts_with("OFFSITE_KEY_MISSING"));
    assert!(settings(&db.pool().await).await.unwrap().unwrap().last_error.unwrap().starts_with("OFFSITE_KEY_MISSING"));

    turn_off(&db.pool().await, &store).await.unwrap();
    assert!(status(&db.pool().await, &store).await.unwrap().settings.is_none());
    assert_eq!(list_copies(&synced).len(), 2, "copies stay in the folder");
}
