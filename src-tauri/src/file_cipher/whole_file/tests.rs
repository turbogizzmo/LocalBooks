use super::*;
use crate::db::DbConfig;
use crate::file_cipher::{header, user_slots};
use crate::users::{NewUserInput, Role};

const MARKER: &str = "Plaintext Marker Company";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("localbooks-whole-file-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// A plain company (named [`MARKER`]) with Ann (Admin) and Bob (Standard), open in a `Db` with Ann
/// signed in. `fill` adds books before users are turned on.
async fn plain_company<F, Fut>(dir: &Path, fill: F) -> (Db, Session)
where
    F: FnOnce(sqlx::SqlitePool) -> Fut,
    Fut: std::future::Future<Output = sqlx::SqlitePool>,
{
    let path = dir.join("books.db");
    let pool = crate::db::open(&path).await.unwrap();
    let pool = fill(pool).await;
    crate::preferences::set(&pool, "company_name", MARKER).await.unwrap();
    let (ann, _) = users::turn_on(&pool, "ann", None, "ann password 1").await.unwrap();
    users::add(&pool, &ann, NewUserInput { username: "bob".into(), display_name: None, role: Role::Standard, can_see_sensitive: false, password: "bob password 1".into() }).await.unwrap();
    crate::db::close_pool(&pool).await;
    let config = DbConfig { config_dir: dir.join("config"), default_path: path.clone(), backups_dir: dir.join("backups") };
    let db = Db::open_at(path, config).await.unwrap();
    db.refresh_gate().await;
    let ann = user_slots::sign_in(&db, "ann", "ann password 1").await.unwrap();
    (db, ann)
}

async fn bob_id(db: &Db) -> i64 {
    users::list(&db.pool().await).await.unwrap().into_iter().find(|u| u.username == "bob").unwrap().id
}

fn turn_on(password: &str, bob: i64) -> EncryptInput {
    EncryptInput { password: password.into(), temporary_passwords: vec![TemporaryPassword { user_id: bob, password: "bob temp 1".into() }], understood: true }
}

async fn company_name(db: &Db) -> String {
    crate::preferences::get(&db.pool().await, "company_name").await.unwrap()
}

#[tokio::test]
async fn turning_encryption_on_and_off() {
    let dir = scratch("on-off");
    let (db, ann) = plain_company(&dir, |pool| async { pool }).await;
    let path = db.path().await.unwrap();
    let plain_backup = crate::backup::backup_now(&db).await.unwrap();
    let bob = bob_id(&db).await;

    // Refused without the tick, without Bob's temporary password, or with a wrong password.
    assert!(encrypt(&db, &ann, EncryptInput { understood: false, ..turn_on("ann password 1", bob) }).await.unwrap_err().starts_with("ENCRYPT_NOT_CONFIRMED"));
    assert!(encrypt(&db, &ann, EncryptInput { temporary_passwords: vec![], ..turn_on("ann password 1", bob) }).await.unwrap_err().starts_with("ENCRYPT_NEEDS_PASSWORDS"));
    assert!(encrypt(&db, &ann, turn_on("wrong password", bob)).await.unwrap_err().starts_with("ENCRYPT_WRONG_PASSWORD"));
    let plain_bytes = [std::fs::read(&path).unwrap(), std::fs::read(crate::wal_guard::wal_path(&path)).unwrap_or_default()].concat();
    assert!(!header::is_encrypted(&path) && contains(&plain_bytes, MARKER.as_bytes()), "nothing changed: still a plain file");

    let (session, recovery) = encrypt(&db, &ann, turn_on("ann password 1", bob)).await.unwrap();
    assert_eq!(session.user.username, "ann");
    assert!(header::is_encrypted(&path) && db.is_encrypted().await && !db.is_locked().await);
    assert!(!contains(&std::fs::read(&path).unwrap(), MARKER.as_bytes()), "no plaintext in the company file");
    assert_eq!(company_name(&db).await, MARKER, "the books are all there");
    // Another company's plain backup in the same backups folder is not this company's to delete.
    let other_dir = scratch("on-off-other");
    let other_path = other_dir.join("other.db");
    let other_pool = crate::db::open(&other_path).await.unwrap();
    users::turn_on(&other_pool, "zed", None, "zed password 1").await.unwrap();
    crate::backup::snapshot(&other_pool, &other_path, &db.backups_dir().join("auto-other-company.db")).await.unwrap();
    crate::db::close_pool(&other_pool).await;
    let plain: Vec<_> = unencrypted_backups(&db).await.unwrap().into_iter().map(|b| b.path).collect();
    assert_eq!(plain, vec![plain_backup.path.clone()], "only this company's backup made before is listed; the swap's own safety copy is gone");
    let _ = std::fs::remove_dir_all(&other_dir);

    // Everyone gets back in: Ann with her password, Bob with his temporary one, and the new recovery key.
    db.relock().await;
    assert_eq!(user_slots::sign_in(&db, "bob", "bob password 1").await.unwrap_err(), users::SIGN_IN_FAILED);
    assert!(user_slots::sign_in(&db, "bob", "bob temp 1").await.unwrap().user.must_change_password);
    db.relock().await;
    let ann = user_slots::recover(&db, recovery.as_str(), "ann", "ann password 2").await.unwrap();

    // The plain backup from before can't quietly turn encryption off by being restored.
    let refused = crate::backup::restore(&db, Path::new(&plain_backup.path)).await.unwrap_err();
    assert!(refused.starts_with("RESTORE_UNENCRYPTED"), "{refused}");
    assert!(db.is_encrypted().await && header::is_encrypted(&path), "still encrypted");
    let encrypted_backup = crate::backup::backup_now(&db).await.unwrap();

    // And off again.
    let back = decrypt(&db, &ann, "ann password 2").await.unwrap();
    assert_eq!(back.user.username, "ann");
    assert!(!header::is_encrypted(&path) && !db.is_encrypted().await);
    assert!(std::fs::read(&path).unwrap().starts_with(b"SQLite format 3\0"));
    assert_eq!(company_name(&db).await, MARKER);
    db.relock().await;
    assert!(!db.is_locked().await, "a plain file doesn't lock");
    // An encrypted backup over the now plain books needs its key, which left with the encryption.
    let err = crate::backup::restore(&db, Path::new(&encrypted_backup.path)).await.unwrap_err();
    assert!(err.contains("FILE_ENCRYPTED_LOCKED"), "{err}");
    assert!(!header::is_encrypted(&path), "the books stayed plain");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The coordinator's check for 3b: every kind of snapshot made from an encrypted company file is
/// encrypted, and restores. These are the copies that would otherwise fail without a word.
#[tokio::test]
async fn every_kind_of_snapshot_restores_from_an_encrypted_file() {
    let dir = scratch("snapshots");
    let (db, ann) = plain_company(&dir, |pool| async {
        let pool = crate::harness::fixture::harbor_lane_in(pool).await;
        let lock = crate::closing::ClosingInput { lock_date: Some(chrono::NaiveDate::from_ymd_opt(2025, 3, 31).unwrap()), current_passcode: None, new_passcode: None };
        crate::closing::set_closing_date(&pool, &lock).await.unwrap();
        pool
    })
    .await;
    let path = db.path().await.unwrap();
    let bob = bob_id(&db).await;
    let (_, _) = encrypt(&db, &ann, turn_on("ann password 1", bob)).await.unwrap();
    let pool = db.pool().await;

    let mut copies: Vec<(&str, PathBuf)> = Vec::new();
    copies.push(("manual", PathBuf::from(crate::backup::backup_now(&db).await.unwrap().path)));
    if let Some(auto) = crate::backup::auto_backup(&db).await.unwrap() {
        copies.push(("automatic", PathBuf::from(auto.path)));
    }
    crate::backup::pre_upgrade_snapshot(&pool, &path, db.backups_dir(), 1).await.unwrap();
    let pre_upgrade = crate::backup::list_backups(db.backups_dir()).unwrap().into_iter().find(|b| b.kind == "PRE_UPGRADE").unwrap();
    copies.push(("pre-upgrade", PathBuf::from(pre_upgrade.path)));
    let condensed = crate::condense::condense_live(&db, chrono::NaiveDate::from_ymd_opt(2025, 3, 31).unwrap()).await.unwrap();
    copies.push(("before condense", PathBuf::from(condensed.safety_backup.path)));
    copies.push(("condense archive", PathBuf::from(condensed.archive.path)));
    let restored = crate::backup::restore(&db, &copies[0].1).await.unwrap();
    copies.push(("pre-restore", PathBuf::from(restored.safety_backup.path)));

    for (kind, copy) in &copies {
        assert!(header::is_encrypted(copy), "the {kind} copy is encrypted");
        assert!(!contains(&std::fs::read(copy).unwrap(), MARKER.as_bytes()), "the {kind} copy has no plaintext");
        crate::backup::restore(&db, copy).await.unwrap_or_else(|e| panic!("the {kind} copy restores: {e}"));
        assert!(db.is_encrypted().await && !db.is_locked().await, "after restoring the {kind} copy the books are encrypted and open");
        assert_eq!(company_name(&db).await, MARKER, "and readable");
        assert!(!contains(&std::fs::read(&path).unwrap(), MARKER.as_bytes()));
    }

    // Offsite: the copy is the encrypted snapshot inside the offsite container.
    let store = crate::secrets::SecretStore::in_file(dir.join("offsite-keys.json"), crate::offsite::KEYCHAIN_SERVICE, "the offsite backup key");
    let folder = dir.join("offsite");
    std::fs::create_dir_all(&folder).unwrap();
    crate::offsite::set_up(&db.pool().await, &store, &folder, "offsite password 1", 3).await.unwrap();
    let offsite = crate::offsite::back_up_now(&db, &store).await.unwrap();
    assert!(!contains(&std::fs::read(&offsite.path).unwrap(), MARKER.as_bytes()));
    crate::offsite::restore(&db, Path::new(&offsite.path), "offsite password 1").await.unwrap();
    assert!(db.is_encrypted().await && !db.is_locked().await);
    assert_eq!(company_name(&db).await, MARKER);

    // After all that, the passwords still open it.
    db.relock().await;
    user_slots::sign_in(&db, "ann", "ann password 1").await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// A backup of another encrypted company can't be restored here without its password; it says so
/// instead of failing obscurely.
#[tokio::test]
async fn another_companys_encrypted_backup_asks_to_be_opened_and_signed_in_to() {
    let dir = scratch("other");
    let (db, ann) = plain_company(&dir, |pool| async { pool }).await;
    let bob = bob_id(&db).await;
    encrypt(&db, &ann, turn_on("ann password 1", bob)).await.unwrap();
    let copy = PathBuf::from(crate::backup::backup_now(&db).await.unwrap().path);
    db.relock().await; // the key leaves the keyring, as when another company is open instead
    let elsewhere = scratch("other-company");
    let (other, _) = plain_company(&elsewhere, |pool| async { pool }).await;
    let err = crate::backup::restore(&other, &copy).await.unwrap_err();
    assert!(err.contains("FILE_ENCRYPTED_LOCKED"), "{err}");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&elsewhere);
}

/// R1's acceptance test: an attachment in an encrypted company file shows nothing readable in the
/// file or its backup, and opens as before. Also, File Repair's checkup and the diagnostics bundle
/// read an encrypted file as they read a plain one (SQLCipher answers `page_size` as text).
#[tokio::test]
async fn attachments_checkup_and_diagnostics_on_an_encrypted_file() {
    const RECEIPT: &[u8] = b"%PDF-1.4 Plaintext receipt for invoice 4471, paid by card";
    let dir = scratch("checkup");
    let (db, ann) = plain_company(&dir, |pool| async { crate::harness::fixture::harbor_lane_in(pool).await }).await;
    let entry: i64 = sqlx::query_scalar("SELECT MIN(id) FROM journal_entries").fetch_one(&db.pool().await).await.unwrap();
    let attachment = crate::workflows::add_attachment(&db.pool().await, entry, "receipt.pdf", RECEIPT).await.unwrap();
    assert!(contains(&std::fs::read(db.path().await.unwrap()).unwrap(), RECEIPT) || contains(&std::fs::read(crate::wal_guard::wal_path(&db.path().await.unwrap())).unwrap_or_default(), RECEIPT), "plain before");
    let bob = bob_id(&db).await;
    encrypt(&db, &ann, turn_on("ann password 1", bob)).await.unwrap();
    let path = db.path().await.unwrap();
    let backup = crate::backup::backup_now(&db).await.unwrap();
    for file in [path.clone(), PathBuf::from(&backup.path)] {
        let bytes = std::fs::read(&file).unwrap();
        assert!(!contains(&bytes, RECEIPT) && !contains(&bytes, b"receipt.pdf"), "no readable attachment in {}", file.display());
    }
    assert_eq!(crate::workflows::attachment_content(&db.pool().await, attachment.id).await.unwrap().1, RECEIPT, "the attachment opens as before");
    crate::file_repair::checkup_live(&db).await.unwrap();
    crate::diagnostics::bundle(Some(&db.pool().await), Some(&path), None, "test").await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

/// Moving an encrypted company to another folder keeps it open and usable: its copy there opens
/// with the same key, and backups of it still work afterwards.
#[tokio::test]
async fn a_moved_encrypted_company_keeps_working() {
    let dir = scratch("moved");
    let (db, ann) = plain_company(&dir, |pool| async { pool }).await;
    let bob = bob_id(&db).await;
    encrypt(&db, &ann, turn_on("ann password 1", bob)).await.unwrap();
    let elsewhere = dir.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    db.relocate(&elsewhere, crate::db::RelocateMode::MoveCurrent).await.unwrap();
    assert!(db.is_encrypted().await && !db.is_locked().await, "the moved copy opens with the same key");
    assert!(header::is_encrypted(&db.path().await.unwrap()));
    crate::backup::backup_now(&db).await.expect("and its key is still known, so it backs up");
    let _ = std::fs::remove_dir_all(&dir);
}
