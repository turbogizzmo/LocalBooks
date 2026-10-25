use super::*;
use crate::db::test_pool;

async fn with_admin() -> (SqlitePool, Session, RecoveryKey) {
    let pool = test_pool().await;
    let (session, recovery) = turn_on(&pool, " Owner ", Some("Pat Owner"), "owner password").await.unwrap();
    (pool, session, recovery)
}

fn new_user(username: &str, role: Role, sensitive: bool) -> NewUserInput {
    NewUserInput { username: username.into(), display_name: None, role, can_see_sensitive: sensitive, password: "temporary pw".into() }
}

fn unchanged(user: &UserView) -> UserUpdate {
    UserUpdate { display_name: user.display_name.clone(), role: user.role, can_see_sensitive: user.can_see_sensitive, is_active: user.is_active, new_password: None }
}

#[tokio::test]
async fn turning_users_on_makes_a_signed_in_admin() {
    let pool = test_pool().await;
    assert!(!users_enabled(&pool).await.unwrap());
    let (session, recovery) = turn_on(&pool, " Owner ", Some("Pat Owner"), "owner password").await.unwrap();
    assert!(users_enabled(&pool).await.unwrap());
    assert_eq!((session.user.username.as_str(), session.user.display_name.as_str(), session.user.role), ("Owner", "Pat Owner", Role::Admin));
    assert!(session.user.can_see_sensitive && session.key.is_some() && !session.user.must_change_password);
    assert_eq!(recovery.as_str().len(), 39);
    assert!(turn_on(&pool, "Other", None, "owner password").await.unwrap_err().starts_with("USERS_ALREADY_ON"));
}

#[tokio::test]
async fn signing_in_checks_the_password_and_unlocks_the_key() {
    let (pool, admin, _) = with_admin().await;
    let session = sign_in(&pool, "owner", "owner password").await.unwrap();
    assert_eq!(session.user.id, admin.user.id, "user names are not case-sensitive");
    assert_eq!(session.key.unwrap().key_id(), admin.key.as_ref().unwrap().key_id());
    assert!(session.user.last_signed_in_at.is_some());

    assert_eq!(sign_in(&pool, "Owner", "wrong password").await.unwrap_err(), SIGN_IN_FAILED);
    assert_eq!(sign_in(&pool, "Nobody", "owner password").await.unwrap_err(), SIGN_IN_FAILED, "the same answer for unknown names");
}

#[tokio::test]
async fn users_without_sensitive_access_get_no_key() {
    let (pool, admin, _) = with_admin().await;
    let clerk = add(&pool, &admin, new_user("clerk", Role::Standard, false)).await.unwrap();
    assert!(clerk.must_change_password && !clerk.can_see_sensitive);
    let session = sign_in(&pool, "clerk", "temporary pw").await.unwrap();
    assert!(session.key.is_none());
    let slots: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM key_slots WHERE user_id = ?1").bind(clerk.id).fetch_one(&pool).await.unwrap();
    assert_eq!(slots, 0);

    assert!(add(&pool, &admin, new_user("CLERK", Role::ViewOnly, false)).await.unwrap_err().starts_with("USER_EXISTS"));
    assert!(add(&pool, &session, new_user("x", Role::Standard, false)).await.is_ok(), "the function trusts its caller; commands check the role");
    assert!(Session::require_admin(Some(&session)).unwrap_err().starts_with("USER_ADMIN_ONLY"));
    assert!(Session::require_admin(None).unwrap_err().starts_with("SIGN_IN_REQUIRED"));
}

#[tokio::test]
async fn granting_sensitive_access_needs_a_password() {
    let (pool, admin, _) = with_admin().await;
    let clerk = add(&pool, &admin, new_user("clerk", Role::Standard, false)).await.unwrap();
    let grant = UserUpdate { can_see_sensitive: true, ..unchanged(&clerk) };
    assert!(update(&pool, &admin, clerk.id, grant.clone()).await.unwrap_err().starts_with("SENSITIVE_NEEDS_PASSWORD"));
    let still: i64 = sqlx::query_scalar("SELECT can_see_sensitive FROM users WHERE id = ?1").bind(clerk.id).fetch_one(&pool).await.unwrap();
    assert_eq!(still, 0, "the refused update was rolled back");

    update(&pool, &admin, clerk.id, UserUpdate { new_password: Some("new temp pw".into()), ..grant }).await.unwrap();
    let session = sign_in(&pool, "clerk", "new temp pw").await.unwrap();
    assert!(session.key.is_some() && session.user.must_change_password);

    let revoked = update(&pool, &admin, clerk.id, UserUpdate { can_see_sensitive: false, ..unchanged(&session.user) }).await.unwrap();
    assert!(!revoked.can_see_sensitive);
    assert!(sign_in(&pool, "clerk", "new temp pw").await.unwrap().key.is_none(), "the slot went with the access");
}

#[tokio::test]
async fn turned_off_users_cannot_sign_in() {
    let (pool, admin, _) = with_admin().await;
    let bookkeeper = add(&pool, &admin, new_user("books", Role::Standard, true)).await.unwrap();
    update(&pool, &admin, bookkeeper.id, UserUpdate { is_active: false, ..unchanged(&bookkeeper) }).await.unwrap();
    assert!(sign_in(&pool, "books", "temporary pw").await.unwrap_err().starts_with("SIGN_IN_TURNED_OFF"));
    assert!(sign_in(&pool, "books", "wrong").await.unwrap_err().starts_with("SIGN_IN_FAILED"), "no hint without the password");
    let back = UserUpdate { is_active: true, ..unchanged(&bookkeeper) };
    assert!(update(&pool, &admin, bookkeeper.id, back.clone()).await.unwrap_err().starts_with("SENSITIVE_NEEDS_PASSWORD"));
    update(&pool, &admin, bookkeeper.id, UserUpdate { new_password: Some("back again pw".into()), ..back }).await.unwrap();
    assert!(sign_in(&pool, "books", "back again pw").await.unwrap().key.is_some());
}

#[tokio::test]
async fn an_active_admin_always_remains() {
    let (pool, admin, _) = with_admin().await;
    let mut me = unchanged(&admin.user);
    me.role = Role::Standard;
    assert!(update(&pool, &admin, admin.user.id, me).await.unwrap_err().starts_with("USER_EDIT_SELF"));

    let second = add(&pool, &admin, new_user("second", Role::Admin, false)).await.unwrap();
    assert!(second.can_see_sensitive, "Admins always may see sensitive data");
    let second_session = sign_in(&pool, "second", "temporary pw").await.unwrap();
    update(&pool, &second_session, admin.user.id, UserUpdate { is_active: false, ..unchanged(&admin.user) }).await.unwrap();
    let err = sqlx::query("UPDATE users SET is_active = 0 WHERE id = ?1").bind(second.id).execute(&pool).await.unwrap_err();
    assert!(db_err(err).starts_with("USER_LAST_ADMIN"));
    let err = sqlx::query("DELETE FROM users WHERE id = ?1").bind(second.id).execute(&pool).await.unwrap_err();
    assert!(db_err(err).starts_with("USER_NO_DELETE"));
}

#[tokio::test]
async fn changing_your_own_password_keeps_your_key() {
    let (pool, admin, _) = with_admin().await;
    add(&pool, &admin, new_user("books", Role::Standard, true)).await.unwrap();
    let mut session = sign_in(&pool, "books", "temporary pw").await.unwrap();
    assert!(change_own_password(&pool, &mut session, "wrong one", "my own password").await.unwrap_err().starts_with("PASSWORD_WRONG"));
    assert!(change_own_password(&pool, &mut session, "temporary pw", "short").await.unwrap_err().starts_with("PASSWORD_TOO_SHORT"));
    let user = change_own_password(&pool, &mut session, "temporary pw", "my own password").await.unwrap();
    assert!(!user.must_change_password && !session.user.must_change_password);
    assert!(sign_in(&pool, "books", "temporary pw").await.is_err());
    assert!(sign_in(&pool, "books", "my own password").await.unwrap().key.is_some());
}

#[tokio::test]
async fn the_recovery_key_resets_an_admin_password() {
    let (pool, admin, recovery) = with_admin().await;
    add(&pool, &admin, new_user("clerk", Role::Standard, false)).await.unwrap();
    assert!(recover(&pool, recovery.as_str(), "clerk", "fresh password").await.unwrap_err().starts_with("RECOVER_NOT_ADMIN"));
    assert!(recover(&pool, &"A".repeat(32), "Owner", "fresh password").await.unwrap_err().starts_with("KEY_WRONG_RECOVERY"));

    let session = recover(&pool, recovery.as_str(), "owner", "fresh password").await.unwrap();
    let trail: Vec<String> = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE action_type = 'USER_CHANGE'").fetch_all(&pool).await.unwrap();
    assert!(trail.iter().any(|p| p.contains("RECOVERY_KEY_USED")));
    assert!(trail.iter().all(|p| !p.contains("password\":\"") && !p.contains("fresh password") && !p.contains(recovery.as_str())), "no secrets in the trail");
    assert_eq!(session.user.id, admin.user.id);
    assert!(sign_in(&pool, "Owner", "owner password").await.is_err());
    assert!(sign_in(&pool, "Owner", "fresh password").await.unwrap().key.is_some());

    let fresh = replace_recovery_key(&pool, &session).await.unwrap();
    assert!(recover(&pool, recovery.as_str(), "Owner", "another password").await.is_err());
    assert!(recover(&pool, fresh.as_str(), "Owner", "another password").await.is_ok());
}

#[tokio::test]
async fn removing_all_users_keeps_the_books_and_drops_protected_values() {
    let (pool, admin, _) = with_admin().await;
    add(&pool, &admin, new_user("clerk", Role::Standard, false)).await.unwrap();
    let vendor: i64 = sqlx::query_scalar("INSERT INTO vendors (name, tax_id) VALUES ('Acme', 'plain-123') RETURNING id").fetch_one(&pool).await.unwrap();
    let other: i64 = sqlx::query_scalar("INSERT INTO vendors (name) VALUES ('Beta') RETURNING id").fetch_one(&pool).await.unwrap();
    let stored = admin.key.as_ref().unwrap().encrypt_field("vendors", "tax_id", other, "123-45-6789").unwrap();
    sqlx::query("UPDATE vendors SET tax_id = ?1 WHERE id = ?2").bind(stored).bind(other).execute(&pool).await.unwrap();

    assert!(remove_all_users(&pool, "remove").await.unwrap_err().starts_with("USERS_REMOVE_CONFIRM"));
    remove_all_users(&pool, REMOVE_ALL_CONFIRMATION).await.unwrap();
    assert!(remove_all_users(&pool, REMOVE_ALL_CONFIRMATION).await.unwrap_err().starts_with("USERS_NONE"));
    let events: Vec<String> = sqlx::query_scalar("SELECT json_extract(payload_snapshot, '$.event') FROM ledger_audit_log WHERE action_type = 'USER_CHANGE' ORDER BY id")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(events, ["USERS_TURNED_ON", "USER_ADDED", "ALL_USERS_REMOVED"]);
    let removed: String = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE json_extract(payload_snapshot, '$.event') = 'ALL_USERS_REMOVED'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(removed.contains("\"clerk\"") && removed.contains("\"taxIdsErased\":1"), "{removed}");
    let unsealed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ledger_audit_log WHERE row_hash IS NULL").fetch_one(&pool).await.unwrap();
    assert_eq!(unsealed, 0, "user changes are sealed into the chain");
    let ledger = crate::ledger::verify_ledger_audit(&pool).await.unwrap();
    assert!(ledger.intact, "{:?}", ledger.findings);
    assert!(crate::audit_chain::verify(&pool).await.unwrap().problems.is_empty());
    assert!(!users_enabled(&pool).await.unwrap());
    let taxes: Vec<Option<String>> = sqlx::query_scalar("SELECT tax_id FROM vendors WHERE id IN (?1, ?2) ORDER BY id").bind(vendor).bind(other).fetch_all(&pool).await.unwrap();
    assert_eq!(taxes, vec![Some("plain-123".into()), None]);
    let err = sqlx::query("DELETE FROM users").execute(&pool).await;
    assert!(err.is_ok(), "nothing left to delete");

    // Users can be turned on again, with a new data key.
    let (again, _) = turn_on(&pool, "Owner", None, "owner password").await.unwrap();
    assert_ne!(again.key.unwrap().key_id(), admin.key.unwrap().key_id());
}
