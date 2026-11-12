use super::*;
use crate::db::test_pool;
use crate::mfa::{self, Secret};
use crate::users::{self, NewUserInput, Role, UserUpdate};

const T: u64 = 1_900_000_020;

/// A credential store of its own, in a file, standing in for one computer's Keychain.
fn computer(name: &str) -> (SecretStore, std::path::PathBuf) {
    let path = std::env::temp_dir().join(format!("localbooks-two-step-computer-{name}-{}.json", std::process::id()));
    let _ = std::fs::remove_file(&path);
    (SecretStore::in_file(path.clone(), "LocalBooks two-step sign-in", "this computer's two-step sign-in"), path)
}

async fn company() -> (SqlitePool, Session) {
    let pool = test_pool().await;
    let (session, _) = users::turn_on(&pool, "owner", None, "owner password").await.unwrap();
    mfa::set_policy(&pool, &session, Policy::Optional).await.unwrap();
    (pool, session)
}

async fn enrol_app(pool: &SqlitePool, session: &Session, password: &str) -> Secret {
    let (setup, _) = mfa::begin_setup(pool, session, password).await.unwrap();
    mfa::finish_setup(pool, &setup, &mfa::test_code(setup.secret(), T), T).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    mfa::open_secret(&mut conn, session.user.id, password).await.unwrap().unwrap()
}

async fn computers(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM user_device_factors").fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn a_computer_is_added_beside_the_app_and_signs_in_only_where_its_secret_is() {
    let (pool, owner) = company().await;
    let (here, here_path) = computer("here");
    let (elsewhere, elsewhere_path) = computer("elsewhere");

    // Not without the app: the app is what works on every other computer.
    assert!(add(&pool, &here, &owner, Kind::TouchId, "Mac").await.unwrap_err().starts_with("MFA_DEVICE_NEEDS_APP"));
    // Nor on a computer with nothing LocalBooks can use.
    enrol_app(&pool, &owner, "owner password").await;
    assert_eq!(add(&pool, &here, &owner, Kind::None, "Linux box").await.unwrap_err(), crate::platform_auth::UNSUPPORTED);
    assert_eq!(computers(&pool).await, 0);

    let added = add(&pool, &here, &owner, Kind::TouchId, "  Ann's MacBook  ").await.unwrap();
    assert!(added.here && added.label == "Ann's MacBook" && added.kind == "TOUCH_ID" && added.last_used_at.is_none());

    // This computer signs her in; another computer (or a copy of the file there) can't.
    let mut conn = pool.acquire().await.unwrap();
    assert!(set_up_here(&mut conn, &here, owner.user.id).await.unwrap());
    assert!(!set_up_here(&mut conn, &elsewhere, owner.user.id).await.unwrap());
    drop(conn);
    check(&pool, &here, owner.user.id).await.unwrap();
    assert_eq!(check(&pool, &elsewhere, owner.user.id).await.unwrap_err(), NOT_HERE);
    let mut conn = pool.acquire().await.unwrap();
    let listed = list(&mut conn, &here, owner.user.id).await.unwrap();
    assert!(listed.len() == 1 && listed[0].last_used_at.is_some(), "the list shows when it was last used");
    assert!(!list(&mut conn, &elsewhere, owner.user.id).await.unwrap()[0].here);
    drop(conn);

    // The file keeps only a hash: the secret is in the computer's store, nowhere in the file.
    let (credential_id, hash): (Vec<u8>, Vec<u8>) =
        sqlx::query_as("SELECT credential_id, secret_hash FROM user_device_factors").fetch_one(&pool).await.unwrap();
    let saved = here.get(&store_key(&credential_id)).await.unwrap().unwrap();
    let secret = unhex(&saved).unwrap();
    assert_eq!(secret.len(), SECRET_BYTES);
    assert_ne!(hash, secret.to_vec());
    let dump: Vec<Vec<u8>> = sqlx::query_scalar("SELECT CAST(payload_snapshot AS BLOB) FROM ledger_audit_log").fetch_all(&pool).await.unwrap();
    assert!(dump.iter().all(|p| !String::from_utf8_lossy(p).contains(&saved)), "the secret is not in the audit trail");
    assert!(dump.iter().any(|p| String::from_utf8_lossy(p).contains("MFA_DEVICE_ADDED")));

    // A secret that doesn't match the file's hash (changed outside LocalBooks) signs nobody in.
    here.put(&store_key(&credential_id), &hex(&[7u8; SECRET_BYTES])).await.unwrap();
    assert_eq!(check(&pool, &here, owner.user.id).await.unwrap_err(), NOT_HERE);
    here.put(&store_key(&credential_id), &saved).await.unwrap();
    // Nor is a row moved to another user: the hash is bound to the user.
    let clerk = users::add(
        &pool,
        &owner,
        NewUserInput { username: "clerk".into(), display_name: None, role: Role::Standard, can_see_sensitive: false, password: "clerk password".into() },
    )
    .await
    .unwrap();
    let clerk_session = users::sign_in(&pool, "clerk", "clerk password").await.unwrap();
    enrol_app(&pool, &clerk_session, "clerk password").await;
    sqlx::query("UPDATE user_device_factors SET user_id = ?1").bind(clerk.id).execute(&pool).await.unwrap();
    assert_eq!(check(&pool, &here, clerk.id).await.unwrap_err(), NOT_HERE);
    sqlx::query("UPDATE user_device_factors SET user_id = ?1").bind(owner.user.id).execute(&pool).await.unwrap();

    // Adding this computer again replaces its row and its secret.
    add(&pool, &here, &owner, Kind::TouchId, "Ann's MacBook").await.unwrap();
    assert_eq!(computers(&pool).await, 1);
    assert!(here.get(&store_key(&credential_id)).await.unwrap().is_none(), "the old secret is gone");
    check(&pool, &here, owner.user.id).await.unwrap();

    // A second computer for the same user.
    add(&pool, &elsewhere, &owner, Kind::WindowsHello, "Office PC").await.unwrap();
    assert_eq!(computers(&pool).await, 2);
    check(&pool, &elsewhere, owner.user.id).await.unwrap();

    let _ = std::fs::remove_file(here_path);
    let _ = std::fs::remove_file(elsewhere_path);
}

#[tokio::test]
async fn removing_a_computer_keeps_the_app_and_only_the_owner_can() {
    let (pool, owner) = company().await;
    let (here, path) = computer("remove");
    let secret = enrol_app(&pool, &owner, "owner password").await;
    let added = add(&pool, &here, &owner, Kind::WindowsHello, "Office PC").await.unwrap();

    let clerk = users::add(
        &pool,
        &owner,
        NewUserInput { username: "clerk".into(), display_name: None, role: Role::Standard, can_see_sensitive: false, password: "clerk password".into() },
    )
    .await
    .unwrap();
    let _ = clerk;
    let clerk_session = users::sign_in(&pool, "clerk", "clerk password").await.unwrap();
    assert!(remove(&pool, &here, &clerk_session, added.id).await.unwrap_err().starts_with("MFA_DEVICE_NONE"), "not someone else's computer");

    remove(&pool, &here, &owner, added.id).await.unwrap();
    assert_eq!(computers(&pool).await, 0);
    assert_eq!(check(&pool, &here, owner.user.id).await.unwrap_err(), NOT_HERE);
    let mut conn = pool.acquire().await.unwrap();
    assert!(mfa::has_factor(&mut conn, owner.user.id).await.unwrap(), "the app stays");
    drop(conn);
    mfa::check_code(&pool, owner.user.id, &secret, &mfa::test_code(&secret, T + 60), T + 60).await.unwrap();
    let file = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(!file.contains("two-step:"), "its secret left this computer's store");
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn setting_up_the_app_again_keeps_the_computers() {
    let (pool, owner) = company().await;
    let (here, path) = computer("again");
    enrol_app(&pool, &owner, "owner password").await;
    add(&pool, &here, &owner, Kind::TouchId, "Mac").await.unwrap();
    let new_secret = enrol_app(&pool, &owner, "owner password").await;
    assert_eq!(computers(&pool).await, 1);
    check(&pool, &here, owner.user.id).await.unwrap();
    // The new app's codes work; the old backup codes were replaced.
    mfa::check_code(&pool, owner.user.id, &new_secret, &mfa::test_code(&new_secret, T + 60), T + 60).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(mfa::backup_codes_left(&mut conn, owner.user.id).await.unwrap(), mfa::BACKUP_CODES as i64);
    drop(conn);
    let _ = std::fs::remove_file(path);
}

/// Whatever removes the authenticator app removes the computers with it, so every way back that
/// worked before (R26) still works, and a lost laptop can't keep signing in.
#[tokio::test]
async fn every_way_back_removes_the_computers_too() {
    let pool = test_pool().await;
    let (owner, recovery) = users::turn_on(&pool, "owner", None, "owner password").await.unwrap();
    mfa::set_policy(&pool, &owner, Policy::Optional).await.unwrap();
    let (here, path) = computer("way-back");
    let clerk = users::add(
        &pool,
        &owner,
        NewUserInput { username: "clerk".into(), display_name: None, role: Role::Standard, can_see_sensitive: false, password: "clerk temp pw".into() },
    )
    .await
    .unwrap();
    let clerk_session = users::sign_in(&pool, "clerk", "clerk temp pw").await.unwrap();

    // An Admin's reset.
    enrol_app(&pool, &clerk_session, "clerk temp pw").await;
    add(&pool, &here, &clerk_session, Kind::TouchId, "Clerk's Mac").await.unwrap();
    mfa::reset_by_admin(&pool, &owner, clerk.id).await.unwrap();
    assert_eq!(computers(&pool).await, 0);
    assert_eq!(check(&pool, &here, clerk.id).await.unwrap_err(), NOT_HERE);

    // A temporary password from an Admin.
    enrol_app(&pool, &clerk_session, "clerk temp pw").await;
    add(&pool, &here, &clerk_session, Kind::TouchId, "Clerk's Mac").await.unwrap();
    let update =
        UserUpdate { display_name: "clerk".into(), role: Role::Standard, can_see_sensitive: false, is_active: true, new_password: Some("another temp".into()) };
    users::update(&pool, &owner, clerk.id, update).await.unwrap();
    assert_eq!(computers(&pool).await, 0);

    // Turning the app off yourself.
    let clerk_session = users::sign_in(&pool, "clerk", "another temp").await.unwrap();
    enrol_app(&pool, &clerk_session, "another temp").await;
    add(&pool, &here, &clerk_session, Kind::TouchId, "Clerk's Mac").await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let keys = here_keys(&mut conn, &here, clerk.id).await.unwrap();
    drop(conn);
    assert_eq!(keys.len(), 1);
    mfa::remove_own(&pool, &clerk_session, "another temp").await.unwrap();
    forget_keys(&here, &keys).await;
    assert_eq!(computers(&pool).await, 0);
    assert!(here.get(&keys[0]).await.unwrap().is_none(), "forgotten here too");

    // The recovery key.
    enrol_app(&pool, &owner, "owner password").await;
    add(&pool, &here, &owner, Kind::TouchId, "Owner's Mac").await.unwrap();
    let recovered = users::recover(&pool, recovery.as_str(), "owner", "brand new password").await.unwrap();
    assert_eq!(computers(&pool).await, 0);

    // The company turning two-step sign-in off.
    enrol_app(&pool, &recovered, "brand new password").await;
    add(&pool, &here, &recovered, Kind::TouchId, "Owner's Mac").await.unwrap();
    mfa::set_policy(&pool, &recovered, Policy::Off).await.unwrap();
    assert_eq!(computers(&pool).await, 0);
    assert!(add(&pool, &here, &recovered, Kind::TouchId, "Owner's Mac").await.unwrap_err().starts_with("MFA_OFF"));

    // Remove all users.
    mfa::set_policy(&pool, &recovered, Policy::Optional).await.unwrap();
    enrol_app(&pool, &recovered, "brand new password").await;
    add(&pool, &here, &recovered, Kind::TouchId, "Owner's Mac").await.unwrap();
    users::remove_all_users(&pool, users::REMOVE_ALL_CONFIRMATION).await.unwrap();
    assert_eq!(computers(&pool).await, 0);

    // The schema itself refuses a computer without an app.
    let err = sqlx::query(
        "INSERT INTO user_device_factors (user_id, credential_id, label, kind, secret_hash) VALUES (1, zeroblob(16), 'x', 'TOUCH_ID', zeroblob(32))",
    )
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(err.to_string().contains("MFA_DEVICE_NEEDS_APP") || err.to_string().contains("FOREIGN KEY"), "{err}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn hex_round_trips_and_refuses_junk() {
    assert_eq!(hex(&[0, 15, 255]), "000fff");
    assert_eq!(unhex("000fff").unwrap().to_vec(), vec![0, 15, 255]);
    assert!(unhex("abc").is_none() && unhex("zz").is_none());
}
