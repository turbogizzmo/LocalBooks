use super::*;
use crate::db::test_pool;
use crate::users::{self, NewUserInput, UserUpdate};

/// A fixed, synthetic moment, so the codes are the same on every run.
const T: u64 = 1_900_000_020;

async fn company() -> (SqlitePool, Session) {
    let pool = test_pool().await;
    let (session, _) = users::turn_on(&pool, "owner", None, "owner password").await.unwrap();
    (pool, session)
}

/// Sets up an authenticator for `session` at time `at`; returns the secret and the backup codes.
async fn enrol(pool: &SqlitePool, session: &Session, password: &str, at: u64) -> (Secret, Vec<String>) {
    let (setup, view) = begin_setup(pool, session, password).await.unwrap();
    assert!(view.qr_svg.starts_with("<?xml") || view.qr_svg.contains("<svg"), "a QR code drawn as SVG");
    let codes = finish_setup(pool, &setup, &test_code(setup.secret(), at), at).await.unwrap();
    let secret = Secret(setup.secret().0.clone());
    (secret, codes)
}

async fn audit_payloads(pool: &SqlitePool) -> Vec<String> {
    sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE action_type = 'USER_CHANGE' ORDER BY id").fetch_all(pool).await.unwrap()
}

#[test]
fn codes_match_the_rfc_6238_test_vectors() {
    // RFC 6238 appendix B, SHA-1: the 20-byte ASCII secret "12345678901234567890", 8 digits.
    let secret = b"12345678901234567890";
    for (time, code) in [(59, 94287082), (1111111109, 7081804), (1111111111, 14050471), (1234567890, 89005924), (2000000000, 69279037), (20000000000, 65353130)] {
        assert_eq!(hotp(secret, step_at(time), 8), code, "time {time}");
    }
    assert_eq!(rfc4648_base32(secret), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
    assert_eq!(code_for(&Secret(Zeroizing::new(secret.to_vec())), step_at(59)), "287082", "6 digits, leading zeros kept");
}

#[test]
fn codes_and_backup_codes_read_as_typed() {
    assert_eq!(as_totp(" 123 456 ").as_deref(), Some("123456"));
    assert!(as_totp("12345").is_none() && as_totp("12345a").is_none());
    assert_eq!(as_backup("abcde-fghjk").unwrap().as_str(), "ABCDEFGHJK");
    assert_eq!(as_backup("o1l1i 23456").unwrap().as_str(), "0111123456");
    assert!(as_backup("ABCDE-FGHJ").is_none() && as_backup("ABCDE-FGHJU").is_none() && as_backup("ABCDE-FGHJé").is_none());
}

#[tokio::test]
async fn off_by_default_and_an_admin_turns_it_on() {
    let (pool, owner) = company().await;
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(policy(&mut conn).await.unwrap(), Policy::Off);
    drop(conn);
    assert!(begin_setup(&pool, &owner, "owner password").await.unwrap_err().starts_with("MFA_OFF"));
    assert!(!owner.user.two_step && !owner.user.must_set_up_two_step);

    set_policy(&pool, &owner, Policy::Optional).await.unwrap();
    assert!(begin_setup(&pool, &owner, "wrong password").await.unwrap_err().starts_with("PASSWORD_WRONG"));
    let (setup, view) = begin_setup(&pool, &owner, "owner password").await.unwrap();
    assert_eq!(view.secret_text.replace(' ', "").len(), 32, "160 bits in base32");
    let right = test_code(setup.secret(), T);
    let wrong = format!("{:06}", (right.parse::<u32>().unwrap() + 1) % 1_000_000);
    assert_eq!(finish_setup(&pool, &setup, &wrong, T).await.unwrap_err(), CODE_WRONG, "a wrong first code saves nothing");
    let mut conn = pool.acquire().await.unwrap();
    assert!(!has_factor(&mut conn, owner.user.id).await.unwrap());
    drop(conn);
    let codes = finish_setup(&pool, &setup, &right, T).await.unwrap();
    assert_eq!(codes.len(), BACKUP_CODES);
    assert!(codes.iter().all(|c| c.len() == 11 && c.as_bytes()[5] == b'-'));
    let me = users::list(&pool).await.unwrap().into_iter().find(|u| u.id == owner.user.id).unwrap();
    assert!(me.two_step);
}

#[tokio::test]
async fn the_secret_is_sealed_under_the_password_and_never_written_in_clear() {
    let (pool, owner) = company().await;
    set_policy(&pool, &owner, Policy::Optional).await.unwrap();
    let (secret, codes) = enrol(&pool, &owner, "owner password", T).await;

    let stored: Vec<u8> = sqlx::query_scalar("SELECT sealed_secret FROM user_factors WHERE user_id = ?1").bind(owner.user.id).fetch_one(&pool).await.unwrap();
    assert!(!stored.windows(secret.0.len()).any(|w| w == &secret.0[..]), "the secret is not stored in clear");
    let hashes: Vec<Vec<u8>> = sqlx::query_scalar("SELECT code_hash FROM user_backup_codes").fetch_all(&pool).await.unwrap();
    assert_eq!(hashes.len(), BACKUP_CODES);

    let mut conn = pool.acquire().await.unwrap();
    assert!(open_secret(&mut conn, owner.user.id, "wrong password").await.unwrap_err().starts_with("MFA_UNREADABLE"));
    let opened = open_secret(&mut conn, owner.user.id, "owner password").await.unwrap().unwrap();
    assert_eq!(&opened.0[..], &secret.0[..]);
    drop(conn);
    // Moved to another user's row, it doesn't open: the user id is part of what is sealed.
    let clerk = users::add(&pool, &owner, NewUserInput { username: "clerk".into(), display_name: None, role: Role::Standard, can_see_sensitive: false, password: "owner password".into() })
        .await
        .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("INSERT INTO user_factors (user_id, memory_kib, iterations, parallelism, salt, nonce, sealed_secret) SELECT ?1, memory_kib, iterations, parallelism, salt, nonce, sealed_secret FROM user_factors WHERE user_id = ?2")
        .bind(clerk.id)
        .bind(owner.user.id)
        .execute(&mut *conn)
        .await
        .unwrap();
    assert!(open_secret(&mut conn, clerk.id, "owner password").await.unwrap_err().starts_with("MFA_UNREADABLE"));
    drop(conn);

    // Nothing secret reaches the audit trail.
    let text = audit_payloads(&pool).await.join("\n");
    assert!(text.contains("MFA_ENROLLED") && text.contains("MFA_POLICY_CHANGED"));
    for code in &codes {
        assert!(!text.contains(code.as_str()) && !text.contains(&code.replace('-', "")));
    }
    assert!(!text.contains(&rfc4648_base32(&secret.0)));
}

#[tokio::test]
async fn a_code_works_once_and_one_step_of_drift_is_allowed() {
    let (pool, owner) = company().await;
    set_policy(&pool, &owner, Policy::Optional).await.unwrap();
    let (secret, _) = enrol(&pool, &owner, "owner password", T).await;
    let id = owner.user.id;

    assert_eq!(check_code(&pool, id, &secret, &test_code(&secret, T), T).await.unwrap_err(), CODE_WRONG, "the set-up code is used up");
    let next = T + STEP_SECONDS;
    check_code(&pool, id, &secret, &test_code(&secret, next), next).await.unwrap();
    assert_eq!(check_code(&pool, id, &secret, &test_code(&secret, next), next).await.unwrap_err(), CODE_WRONG, "a code is refused twice");

    // The phone's clock one step behind or ahead is fine; two steps is not.
    let now = T + 10 * STEP_SECONDS;
    check_code(&pool, id, &secret, &test_code(&secret, now - STEP_SECONDS), now).await.unwrap();
    check_code(&pool, id, &secret, &test_code(&secret, now + STEP_SECONDS), now).await.unwrap();
    let later = T + 20 * STEP_SECONDS;
    let two_behind = test_code(&secret, later - 2 * STEP_SECONDS);
    let in_window = [-1i64, 0, 1].iter().any(|d| test_code(&secret, (later as i64 + d * STEP_SECONDS as i64) as u64) == two_behind);
    if !in_window {
        assert_eq!(check_code(&pool, id, &secret, &two_behind, later).await.unwrap_err(), CODE_WRONG);
    }
    assert_eq!(check_code(&pool, id, &secret, "not a code", later).await.unwrap_err(), CODE_WRONG);
}

#[tokio::test]
async fn a_backup_code_signs_in_once() {
    let (pool, owner) = company().await;
    set_policy(&pool, &owner, Policy::Optional).await.unwrap();
    let (secret, codes) = enrol(&pool, &owner, "owner password", T).await;
    let id = owner.user.id;
    check_code(&pool, id, &secret, &codes[3].to_lowercase().replace('-', " "), T + 3600).await.unwrap();
    assert_eq!(check_code(&pool, id, &secret, &codes[3], T + 3600).await.unwrap_err(), CODE_WRONG, "used up");
    check_code(&pool, id, &secret, &codes[4], T + 3600).await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    assert_eq!(backup_codes_left(&mut conn, id).await.unwrap(), (BACKUP_CODES - 2) as i64);
    drop(conn);
    assert_eq!(check_code(&pool, id, &secret, "ZZZZZ-ZZZZZ", T + 3600).await.unwrap_err(), CODE_WRONG);
    assert!(audit_payloads(&pool).await.iter().any(|p| p.contains("MFA_BACKUP_CODE_USED")));
    // Failed tries past the free ones are recorded, by user name only.
    record_failed(&pool, id, "owner").await.unwrap();
    assert!(audit_payloads(&pool).await.iter().any(|p| p.contains("MFA_FAILED")));
}

#[tokio::test]
async fn changing_your_password_reseals_the_secret() {
    let (pool, owner) = company().await;
    set_policy(&pool, &owner, Policy::Optional).await.unwrap();
    let (secret, _) = enrol(&pool, &owner, "owner password", T).await;
    let mut session = users::sign_in(&pool, "owner", "owner password").await.unwrap();
    users::change_own_password(&pool, &mut session, "owner password", "a new password").await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    assert!(open_secret(&mut conn, owner.user.id, "owner password").await.is_err(), "the old password no longer opens it");
    let opened = open_secret(&mut conn, owner.user.id, "a new password").await.unwrap().unwrap();
    assert_eq!(&opened.0[..], &secret.0[..], "the same authenticator keeps working");
}

#[tokio::test]
async fn every_way_back_removes_the_factor() {
    let pool = test_pool().await;
    let (owner, recovery) = users::turn_on(&pool, "owner", None, "owner password").await.unwrap();
    set_policy(&pool, &owner, Policy::Optional).await.unwrap();
    let clerk_input = NewUserInput { username: "clerk".into(), display_name: None, role: Role::Standard, can_see_sensitive: false, password: "clerk temp pw".into() };
    let clerk = users::add(&pool, &owner, clerk_input).await.unwrap();
    let clerk_session = users::sign_in(&pool, "clerk", "clerk temp pw").await.unwrap();
    let factors = |pool: SqlitePool| async move { sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM user_factors").fetch_one(&pool).await.unwrap() };

    // An Admin's reset, for a user who lost their phone.
    enrol(&pool, &clerk_session, "clerk temp pw", T).await;
    reset_by_admin(&pool, &owner, clerk.id).await.unwrap();
    assert_eq!(factors(pool.clone()).await, 0);
    assert!(reset_by_admin(&pool, &owner, clerk.id).await.unwrap_err().starts_with("MFA_NONE"));
    assert!(audit_payloads(&pool).await.iter().any(|p| p.contains("MFA_RESET") && p.contains("\"by\":\"owner\"")));

    // A temporary password from an Admin: the secret was sealed under the old one.
    enrol(&pool, &clerk_session, "clerk temp pw", T).await;
    let update = UserUpdate { display_name: "clerk".into(), role: Role::Standard, can_see_sensitive: false, is_active: true, new_password: Some("another temp".into()) };
    users::update(&pool, &owner, clerk.id, update).await.unwrap();
    assert_eq!(factors(pool.clone()).await, 0);

    // The recovery key, for the only Admin who lost both password and phone.
    enrol(&pool, &owner, "owner password", T).await;
    let recovered = users::recover(&pool, recovery.as_str(), "owner", "brand new password").await.unwrap();
    assert!(!recovered.user.two_step);
    assert_eq!(factors(pool.clone()).await, 0);
    assert!(audit_payloads(&pool).await.iter().any(|p| p.contains("RECOVERY_KEY_USED") && p.contains("\"twoStepReset\":true")));

    // Turning the company setting off removes everyone's.
    enrol(&pool, &recovered, "brand new password", T).await;
    set_policy(&pool, &owner, Policy::Off).await.unwrap();
    assert_eq!(factors(pool.clone()).await, 0);
    let codes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_backup_codes").fetch_one(&pool).await.unwrap();
    assert_eq!(codes, 0);

    // Remove all users takes the factors with them.
    set_policy(&pool, &owner, Policy::Optional).await.unwrap();
    enrol(&pool, &recovered, "brand new password", T).await;
    users::remove_all_users(&pool, users::REMOVE_ALL_CONFIRMATION).await.unwrap();
    assert_eq!(factors(pool.clone()).await, 0);
}

#[tokio::test]
async fn a_stricter_setting_makes_the_right_roles_set_it_up() {
    let (pool, owner) = company().await;
    let add = |name: &str, role: Role| NewUserInput { username: name.into(), display_name: None, role, can_see_sensitive: false, password: "temporary pw".into() };
    let accountant = users::add(&pool, &owner, add("cpa", Role::ExternalAccountant)).await.unwrap();
    let clerk = users::add(&pool, &owner, add("clerk", Role::Standard)).await.unwrap();
    let must = |pool: SqlitePool| async move { users::list(&pool).await.unwrap().into_iter().map(|u| (u.username, u.must_set_up_two_step)).collect::<std::collections::BTreeMap<_, _>>() };

    set_policy(&pool, &owner, Policy::Admins).await.unwrap();
    let now = must(pool.clone()).await;
    assert!(now["owner"] && now["cpa"] && !now["clerk"], "{now:?}");
    set_policy(&pool, &owner, Policy::All).await.unwrap();
    assert!(must(pool.clone()).await["clerk"]);

    // An External Accountant sets it up and signs in with it like anyone else.
    let cpa = users::sign_in(&pool, "cpa", "temporary pw").await.unwrap();
    let (secret, _) = enrol(&pool, &cpa, "temporary pw", T).await;
    assert!(!must(pool.clone()).await["cpa"]);
    let mut conn = pool.acquire().await.unwrap();
    let opened = open_secret(&mut conn, accountant.id, "temporary pw").await.unwrap().unwrap();
    drop(conn);
    check_code(&pool, accountant.id, &opened, &test_code(&secret, T + STEP_SECONDS), T + STEP_SECONDS).await.unwrap();

    // Required for them, so they can't turn it off themselves; an optional user can.
    assert!(remove_own(&pool, &cpa, "temporary pw").await.unwrap_err().starts_with("MFA_REQUIRED_BY_COMPANY"));
    set_policy(&pool, &owner, Policy::Optional).await.unwrap();
    assert!(remove_own(&pool, &cpa, "wrong password").await.unwrap_err().starts_with("PASSWORD_WRONG"));
    remove_own(&pool, &cpa, "temporary pw").await.unwrap();
    let _ = clerk;
}
