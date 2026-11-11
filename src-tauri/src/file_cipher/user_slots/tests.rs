use super::*;
use crate::db::{Db, DbConfig};
use crate::file_cipher::{encrypt_into, header};
use crate::permissions::Need;
use crate::users::Role;

const MARKER: &str = "Plaintext Marker Company";

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("localbooks-encrypted-sign-in-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// A plain company file with users Ann (Admin) and Bob (View only), encrypted into `encrypted`
/// with a slot for each and one for the recovery key, which is returned.
async fn encrypted_company(dir: &std::path::Path) -> (std::path::PathBuf, String) {
    let plain = dir.join("plain.db");
    let pool = crate::db::open(&plain).await.unwrap();
    crate::preferences::set(&pool, "company_name", MARKER).await.unwrap();
    let (ann, recovery) = users::turn_on(&pool, "ann", None, "ann password 1").await.unwrap();
    let bob = users::add(&pool, &ann, NewUserInput { username: "bob".into(), display_name: None, role: Role::ViewOnly, can_see_sensitive: false, password: "bob password 1".into() }).await.unwrap();
    crate::db::close_pool(&pool).await;

    let key = keys::PageKey::generate().unwrap();
    let file_id = keys::new_file_id().unwrap();
    let slots = vec![
        keys::password_slot(&key, &file_id, ann.user.id, "ann", "ann password 1").await.unwrap(),
        keys::password_slot(&key, &file_id, bob.id, "bob", "bob password 1").await.unwrap(),
        keys::recovery_slot(&key, &file_id, recovery.as_str()).await.unwrap(),
    ];
    let encrypted = dir.join("books.db");
    encrypt_into(&plain, &encrypted, &key, &header::Header { file_id, page_size: 4096, slots }).await.unwrap();
    (encrypted, recovery.as_str().to_string())
}

async fn open_db(dir: &std::path::Path, path: &std::path::Path) -> Db {
    let config = DbConfig { config_dir: dir.join("config"), default_path: path.to_path_buf(), backups_dir: dir.join("backups") };
    let db = Db::open_at(path.to_path_buf(), config).await.unwrap();
    db.refresh_gate().await;
    db
}

async fn relock(db: &Db) {
    db.relock().await;
    db.refresh_gate().await;
}

#[tokio::test]
async fn an_encrypted_file_opens_locked_and_only_a_password_unlocks_it() {
    let dir = scratch("unlock");
    let (path, _) = encrypted_company(&dir).await;
    let bytes = std::fs::read(&path).unwrap();
    assert!(header::is_encrypted(&path) && !contains(&bytes, MARKER.as_bytes()), "no plaintext in the encrypted file");

    let db = open_db(&dir, &path).await;
    assert!(db.is_locked().await);
    assert!(db.check(Need::Read, false).is_err(), "nothing is readable before sign-in");
    assert!(users::users_enabled(&db.pool().await).await.is_err(), "the stand-in database has no tables");

    for (name, password) in [("ann", "wrong password"), ("nobody", "ann password 1")] {
        assert_eq!(sign_in(&db, name, password).await.unwrap_err(), users::SIGN_IN_FAILED);
        assert!(db.is_locked().await, "a failed sign-in leaves it locked");
    }
    let ann = sign_in(&db, "Ann", "ann password 1").await.unwrap();
    assert_eq!(ann.user.username, "ann");
    assert!(!db.is_locked().await);
    let name = crate::preferences::get(&db.pool().await, "company_name").await.unwrap();
    assert_eq!(name, MARKER, "the books open once unlocked");

    relock(&db).await;
    assert!(db.is_locked().await, "signing out locks it again");
    assert!(db.check(Need::Read, false).is_err());
    let bob = sign_in(&db, "bob", "bob password 1").await.unwrap();
    assert_eq!(bob.user.role, Role::ViewOnly, "every user can unlock, not only those who see sensitive data");
    relock(&db).await;
    assert!(remove_all_users(&db, "REMOVE ALL USERS").await.unwrap_err().starts_with("FILE_ENCRYPTED_NO_RESET"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn slots_follow_users_and_passwords() {
    let dir = scratch("slots");
    let (path, recovery) = encrypted_company(&dir).await;
    let db = open_db(&dir, &path).await;
    let ann = sign_in(&db, "ann", "ann password 1").await.unwrap();

    // A new user can unlock with their temporary password.
    let carol = add_user(&db, &ann, NewUserInput { username: "carol".into(), display_name: None, role: Role::Standard, can_see_sensitive: false, password: "carol temp 1".into() }).await.unwrap();
    relock(&db).await;
    let carol_session = sign_in(&db, "carol", "carol temp 1").await.unwrap();
    assert!(carol_session.user.must_change_password);

    // Turning Carol off removes her slot: her password no longer opens the file at all.
    relock(&db).await;
    let ann = sign_in(&db, "ann", "ann password 1").await.unwrap();
    let off = UserUpdate { display_name: "carol".into(), role: Role::Standard, can_see_sensitive: false, is_active: false, new_password: None };
    update_user(&db, &ann, carol.id, off).await.unwrap();
    relock(&db).await;
    assert_eq!(sign_in(&db, "carol", "carol temp 1").await.unwrap_err(), users::SIGN_IN_FAILED);

    // Turning her back on needs a temporary password, which gives her a slot again.
    let ann = sign_in(&db, "ann", "ann password 1").await.unwrap();
    let back = UserUpdate { display_name: "carol".into(), role: Role::Standard, can_see_sensitive: false, is_active: true, new_password: None };
    assert!(update_user(&db, &ann, carol.id, back.clone()).await.unwrap_err().starts_with("USER_NEEDS_PASSWORD"));
    update_user(&db, &ann, carol.id, UserUpdate { new_password: Some("carol again 1".into()), ..back }).await.unwrap();
    relock(&db).await;
    sign_in(&db, "carol", "carol again 1").await.unwrap();
    relock(&db).await;

    // An Admin's reset gives Bob a new slot and retires the old one.
    let ann = sign_in(&db, "ann", "ann password 1").await.unwrap();
    let reset = UserUpdate { display_name: "bob".into(), role: Role::ViewOnly, can_see_sensitive: false, is_active: true, new_password: Some("bob reset 1".into()) };
    update_user(&db, &ann, ann_bob_id(&db).await, reset).await.unwrap();
    relock(&db).await;
    assert_eq!(sign_in(&db, "bob", "bob password 1").await.unwrap_err(), users::SIGN_IN_FAILED);
    sign_in(&db, "bob", "bob reset 1").await.unwrap();

    // Ann changes her own password.
    relock(&db).await;
    let mut ann = sign_in(&db, "ann", "ann password 1").await.unwrap();
    change_own_password(&db, &mut ann, "ann password 1", "ann password 2").await.unwrap();
    relock(&db).await;
    assert_eq!(sign_in(&db, "ann", "ann password 1").await.unwrap_err(), users::SIGN_IN_FAILED);
    let ann = sign_in(&db, "ann", "ann password 2").await.unwrap();
    let header_now = {
        let pool = db.pool().await;
        let mut conn = pool.acquire().await.unwrap();
        header::read_open(&mut conn).await.unwrap()
    };
    assert_eq!(header_now.slots.iter().filter(|s| s.user_id == Some(ann.user.id)).count(), 1, "the old slot is gone once the change is saved");

    // A new recovery key replaces the old one's slot.
    let fresh = replace_recovery_key(&db, &ann).await.unwrap();
    relock(&db).await;
    assert!(recover(&db, &recovery, "ann", "ann password 3").await.unwrap_err().starts_with("KEY_WRONG_RECOVERY"));
    assert!(db.is_locked().await);
    let recovered = recover(&db, fresh.as_str(), "ann", "ann password 3").await.unwrap();
    assert_eq!(recovered.user.username, "ann");
    relock(&db).await;
    assert_eq!(sign_in(&db, "ann", "ann password 2").await.unwrap_err(), users::SIGN_IN_FAILED);
    sign_in(&db, "ann", "ann password 3").await.unwrap();

    let bytes = std::fs::read(&path).unwrap();
    assert!(!contains(&bytes, MARKER.as_bytes()), "still no plaintext after all that");
    let _ = std::fs::remove_dir_all(&dir);
}

async fn ann_bob_id(db: &Db) -> i64 {
    users::list(&db.pool().await).await.unwrap().into_iter().find(|u| u.username == "bob").unwrap().id
}

#[tokio::test]
async fn a_backup_of_an_encrypted_file_is_encrypted_and_opens_with_the_same_passwords() {
    let dir = scratch("backup");
    let (path, _) = encrypted_company(&dir).await;
    let db = open_db(&dir, &path).await;
    sign_in(&db, "ann", "ann password 1").await.unwrap();
    let copy = dir.join("copy.localbooks");
    crate::backup::snapshot(&db.pool().await, &path, &copy).await.unwrap();
    let bytes = std::fs::read(&copy).unwrap();
    assert!(header::is_encrypted(&copy), "the copy has the key header");
    assert!(!contains(&bytes, MARKER.as_bytes()), "and no plaintext");

    // With the books locked again (their key gone from this process), the copy opens locked too.
    db.relock().await;
    let other = open_db(&dir, &copy).await;
    assert!(other.is_locked().await);
    sign_in(&other, "bob", "bob password 1").await.unwrap();
    assert_eq!(crate::preferences::get(&other.pool().await, "company_name").await.unwrap(), MARKER);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every command but the sign-in screen's is refused while an encrypted file is locked, and every
/// command that runs without a sign-in has been checked against a locked file. A new command that
/// runs without a sign-in fails here until it is added to the right list below.
#[tokio::test]
async fn a_locked_file_refuses_every_command_except_unlock() {
    let dir = scratch("refuses");
    let (path, _) = encrypted_company(&dir).await;
    let db = open_db(&dir, &path).await;
    assert!(db.is_locked().await);

    for name in crate::commands::all_commands() {
        let need = crate::permissions::need(name);
        if need != Need::Open {
            assert!(db.check(need, false).is_err(), "{name} ({need:?}) ran on a locked file");
        }
    }

    // Commands that run without a sign-in. Those touching the open file's books:
    // - unlock it (with a password or the recovery key) and nothing else;
    let unlock = ["sign_in", "sign_in_remembered", "recover_with_recovery_key"];
    // - or are refused on an encrypted file, or need a signed-in user, or only read what the sign-in
    //   screen needs (no business data: the locked file's stand-in database is empty);
    let refused_or_harmless = ["turn_on_users", "remove_all_users", "change_my_password", "sign_out", "sign_out_if_idle", "forget_remembered_sign_in", "get_sign_in_state", "get_storage_status"];
    // - and the rest don't touch the open file's books at all (windows, files, the start screen).
    let elsewhere = [
        "get_startup_failure",
        // Which file is opening and its step (gap R28): names a file, reads no books.
        "get_opening_state",
        "get_third_party_notices",
        "get_start_screen",
        "open_company_file",
        "close_company_file",
        "create_company_file",
        "get_company_setup_options",
        "get_starter_chart",
        "forget_recent_company",
        "find_company_files",
        "restore_backup_as",
        "open_window",
        "report_child_windows",
        "arrange_windows",
        "take_desktop",
        "set_desktop_view",
        "set_window_preferences",
        "set_menu_view_enabled",
    ];
    for name in crate::permissions::open_commands() {
        assert!(unlock.contains(name) || refused_or_harmless.contains(name) || elsewhere.contains(name), "{name} runs without a sign-in: check what it does on a locked encrypted file, then list it here");
    }

    assert!(turn_on(&db, "eve", None, "eve password 1").await.unwrap_err().starts_with("USERS_ALREADY_ON"));
    assert!(remove_all_users(&db, "REMOVE ALL USERS").await.unwrap_err().starts_with("FILE_ENCRYPTED_NO_RESET"));
    assert!(db.update_key_header(|h| h).await.unwrap_err().starts_with("SIGN_IN_REQUIRED"), "the header can't change while locked");
    assert!(db.is_locked().await, "and none of that unlocked it");

    let _ = std::fs::remove_dir_all(&dir);
}

/// A file made before encryption existed opens and behaves exactly as before: no lock, no header,
/// nothing written in front of the database, and every user change works as it did.
#[tokio::test]
async fn a_plain_file_is_unchanged() {
    let dir = scratch("plain");
    let path = dir.join("books.db");
    let pool = crate::db::open(&path).await.unwrap();
    users::turn_on(&pool, "ann", None, "ann password 1").await.unwrap();
    crate::db::close_pool(&pool).await;
    let before = std::fs::read(&path).unwrap();
    assert!(before.starts_with(b"SQLite format 3\0"));

    let db = open_db(&dir, &path).await;
    assert!(!db.is_encrypted().await && !db.is_locked().await);
    assert_eq!(db.encrypted_file_id().await, None);
    let mut ann = sign_in(&db, "ann", "ann password 1").await.unwrap();
    let bob = add_user(&db, &ann, NewUserInput { username: "bob".into(), display_name: None, role: Role::Standard, can_see_sensitive: false, password: "bob password 1".into() }).await.unwrap();
    let reset = UserUpdate { display_name: "bob".into(), role: Role::Standard, can_see_sensitive: false, is_active: true, new_password: Some("bob reset 1".into()) };
    update_user(&db, &ann, bob.id, reset).await.unwrap();
    change_own_password(&db, &mut ann, "ann password 1", "ann password 2").await.unwrap();
    replace_recovery_key(&db, &ann).await.unwrap();
    db.update_key_header(|_| panic!("never called for a plain file")).await.unwrap();
    assert!(db.new_password_slot(1, "ann", "x").await.unwrap().is_none());
    relock(&db).await;
    assert!(!db.is_locked().await, "signing out doesn't lock a plain file");
    sign_in(&db, "bob", "bob reset 1").await.unwrap();

    let copy = dir.join("copy.localbooks");
    crate::backup::snapshot(&db.pool().await, &path, &copy).await.unwrap();
    assert!(std::fs::read(&copy).unwrap().starts_with(b"SQLite format 3\0"), "a plain file's backup is plain, as before");
    assert!(!header::is_encrypted(&copy));
    drop(db);
    assert!(std::fs::read(&path).unwrap().starts_with(b"SQLite format 3\0"), "nothing was written in front of the database");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A password change interrupted after its new slot was written, but before the users table
/// changed: the old password still signs in, and the tidy-up keeps the old slot (the one the users
/// table agrees with), not the newer one whose password was never saved.
#[tokio::test]
async fn an_interrupted_password_change_never_locks_anyone_out() {
    let dir = scratch("interrupted");
    let (path, _) = encrypted_company(&dir).await;
    let db = open_db(&dir, &path).await;
    let ann = sign_in(&db, "ann", "ann password 1").await.unwrap();
    // The first half of a change: the new slot beside the old, and then the "crash".
    let slot = db.new_password_slot(ann.user.id, "ann", "never saved 1").await.unwrap().unwrap();
    db.update_key_header(|header| keys::adding_user_slot(&header, slot)).await.unwrap();
    relock(&db).await;

    sign_in(&db, "ann", "ann password 1").await.unwrap();
    let left = db.key_header().await.unwrap().unwrap();
    assert_eq!(left.slots.iter().filter(|s| s.user_id == Some(ann.user.id)).count(), 1, "one slot is left");
    relock(&db).await;
    sign_in(&db, "ann", "ann password 1").await.expect("and it is the password the users table holds");
    relock(&db).await;
    assert_eq!(sign_in(&db, "ann", "never saved 1").await.unwrap_err(), users::SIGN_IN_FAILED);
    let _ = std::fs::remove_dir_all(&dir);
}
