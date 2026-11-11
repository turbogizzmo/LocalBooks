use super::*;
use crate::db::DbConfig;
use crate::file_cipher::{encrypt_into, header, keys};
use crate::permissions::Need;
use crate::users::{NewUserInput, Role};

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("localbooks-two-step-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// An encrypted company file: Ann (Admin) has an authenticator app, Bob (View only) doesn't.
/// Returns the file and Ann's secret.
async fn encrypted_company_with_two_step(dir: &std::path::Path) -> (std::path::PathBuf, Secret) {
    let plain = dir.join("plain.db");
    let pool = crate::db::open(&plain).await.unwrap();
    let (ann, recovery) = users::turn_on(&pool, "ann", None, "ann password 1").await.unwrap();
    let bob = users::add(&pool, &ann, NewUserInput { username: "bob".into(), display_name: None, role: Role::ViewOnly, can_see_sensitive: false, password: "bob password 1".into() }).await.unwrap();
    mfa::set_policy(&pool, &ann, Policy::Optional).await.unwrap();
    let (setup, _) = mfa::begin_setup(&pool, &ann, "ann password 1").await.unwrap();
    let now = mfa::now_unix();
    mfa::finish_setup(&pool, &setup, &mfa::test_code(setup.secret(), now), now).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let secret = mfa::open_secret(&mut conn, ann.user.id, "ann password 1").await.unwrap().unwrap();
    drop(conn);
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
    (encrypted, secret)
}

/// The order on an encrypted file: the password unlocks the file, then the code is asked for, and
/// until it is right nobody is signed in. A remembered sign-in takes the same path. A user without
/// an authenticator signs in at once.
#[tokio::test]
async fn the_code_is_asked_after_the_password_unlocks_the_file() {
    let dir = scratch("order");
    let (path, secret) = encrypted_company_with_two_step(&dir).await;
    let config = DbConfig { config_dir: dir.join("config"), default_path: path.clone(), backups_dir: dir.join("backups") };
    let db = Db::open_at(path.clone(), config).await.unwrap();
    db.refresh_gate().await;
    assert!(db.is_locked().await);

    // A wrong password never reaches the code step, and leaves the file locked.
    assert!(crate::file_cipher::user_slots::sign_in(&db, "ann", "wrong password").await.is_err());
    assert!(db.is_locked().await);

    for remember in [Remember::Forget, Remember::Keep] {
        let session = crate::file_cipher::user_slots::sign_in(&db, "ann", "ann password 1").await.unwrap();
        assert!(!db.is_locked().await, "the password opened the file");
        let id = session.user.id;
        assert!(hold_for_code(&db, session, "ann password 1", remember).await.unwrap().is_none(), "Ann has to enter a code");
        db.refresh_gate().await;
        assert!(db.check(Need::Read, false).unwrap_err().starts_with("SIGN_IN_REQUIRED"), "nobody is signed in yet");
        let (pool, slot) = db.pool_and_session().await.unwrap();
        assert!(slot.lock().await.is_none());
        {
            let pending = PENDING.lock().await;
            let pending = pending.as_ref().unwrap();
            assert!(std::sync::Arc::ptr_eq(&pending.slot, &slot) && pending.session.user.username == "ann");
            // The held secret is Ann's, opened by her password on the unlocked file.
            assert_eq!(mfa::code_for(&pending.secret, 7), mfa::code_for(&secret, 7));
        }
        // A wrong code is refused; the right one would sign her in (the command does that).
        let right = mfa::test_code(&secret, mfa::now_unix() + mfa::STEP_SECONDS);
        let wrong = format!("{:06}", (right.parse::<u32>().unwrap() + 500_000) % 1_000_000);
        assert_eq!(mfa::check_code(&pool, id, &PENDING.lock().await.as_ref().unwrap().secret, &wrong, mfa::now_unix()).await.unwrap_err(), mfa::CODE_WRONG);
        // Going back locks the file again.
        PENDING.lock().await.take();
        db.relock().await;
        assert!(db.is_locked().await);
    }

    // Bob has no authenticator: his password signs him in.
    let session = crate::file_cipher::user_slots::sign_in(&db, "bob", "bob password 1").await.unwrap();
    assert!(hold_for_code(&db, session, "bob password 1", Remember::Keep).await.unwrap().is_some());
    assert!(PENDING.lock().await.is_none());

    // A secret that can't be opened (changed outside LocalBooks) is refused, never skipped.
    let (pool, _) = db.pool_and_session().await.unwrap();
    sqlx::query("UPDATE user_factors SET sealed_secret = zeroblob(40)").execute(&pool).await.unwrap();
    db.relock().await;
    let session = crate::file_cipher::user_slots::sign_in(&db, "ann", "ann password 1").await.unwrap();
    assert!(hold_for_code(&db, session, "ann password 1", Remember::Keep).await.unwrap_err().starts_with("MFA_UNREADABLE"));
    assert!(db.is_locked().await, "and the file is locked again");
    let _ = std::fs::remove_dir_all(&dir);
}
