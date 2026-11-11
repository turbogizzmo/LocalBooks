//! Two-step sign-in (gap R26; design in `docs/plans/multi-factor-sign-in.md`; migration 0161).
//!
//! **Optional and off by default**, like whole-file encryption. An Admin turns it on for the
//! company ([`Policy`]); each user may then add an authenticator app, and from then on signing in
//! asks that user for a 6-digit code after the password. A stricter policy makes Admins and
//! External Accountants, or everyone, set one up before they can use the books.
//!
//! - **Codes** are TOTP (RFC 6238): HMAC-SHA-1 over 30-second steps, 6 digits, which every
//!   authenticator app reads from the QR code LocalBooks draws itself. The step before and after
//!   the current one are accepted for clock drift, and a step at or before the last accepted one is
//!   refused, so a code works once on this file.
//! - **The secret** behind the codes is sealed under a key derived by Argon2id from the user's
//!   password ([`crate::crypto::seal_with_password`], the key-slot format). The company file without
//!   the password can't make codes; changing your own password re-seals it in the same transaction.
//! - **Ten backup codes** are shown once at set-up; each signs in once in place of a code. They are
//!   kept as salted SHA-256 hashes.
//! - **The way back** when the phone is lost: a backup code; an Admin's **Reset two-step sign-in**;
//!   a password reset by an Admin (the secret is sealed under the old password, which the Admin
//!   doesn't know, so it goes with it); or the company's recovery key, which resets an Admin's
//!   password and removes their factor ([`crate::users::recover`]).
//! - **The order** on an encrypted company file: the password unlocks the file first (its key
//!   slots), then the factor is checked on the opened books. The code never feeds any key.
//!
//! Nothing here writes a secret, a code or a password to the audit trail, a log or an error.

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{SqliteConnection, SqlitePool};
use zeroize::Zeroizing;

use crate::crypto::{self, PasswordSealed};
use crate::ledger::db_err;
use crate::users::{Role, Session};

/// Seconds per code.
pub const STEP_SECONDS: u64 = 30;
/// Digits per code.
const DIGITS: usize = 6;
/// 160 bits, the size RFC 4226 recommends for HMAC-SHA-1.
const SECRET_BYTES: usize = 20;
/// Backup codes made at set-up.
pub const BACKUP_CODES: usize = 10;
/// Characters per backup code: 10 Crockford base32 characters, 50 random bits.
const BACKUP_CHARS: usize = 10;
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

pub const CODE_WRONG: &str = "MFA_CODE_WRONG: that code is not correct; enter the current code from your authenticator app, or a backup code";
pub const CODE_REQUIRED: &str = "MFA_CODE_REQUIRED: enter the 6-digit code from your authenticator app";

/// The company's two-step sign-in setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Policy {
    /// The default: nobody is asked for a code, and nobody can set one up.
    Off,
    /// Anyone may set up an authenticator app; nobody has to.
    Optional,
    /// Admins and External Accountants must set one up; anyone else may.
    Admins,
    /// Everyone must set one up.
    All,
}

impl Policy {
    fn as_str(self) -> &'static str {
        match self {
            Policy::Off => "OFF",
            Policy::Optional => "OPTIONAL",
            Policy::Admins => "ADMINS",
            Policy::All => "ALL",
        }
    }

    pub fn parse(s: &str) -> Policy {
        match s {
            "OPTIONAL" => Policy::Optional,
            "ADMINS" => Policy::Admins,
            "ALL" => Policy::All,
            _ => Policy::Off,
        }
    }

    /// Whether a user with `role` has to set up a factor before using the books.
    pub fn required_for(self, role: Role) -> bool {
        match self {
            Policy::All => true,
            Policy::Admins => matches!(role, Role::Admin | Role::ExternalAccountant),
            Policy::Off | Policy::Optional => false,
        }
    }
}

/// The company's setting; `Off` for a file without the table's row.
pub async fn policy(conn: &mut SqliteConnection) -> Result<Policy, String> {
    let value: Option<String> = sqlx::query_scalar("SELECT policy FROM two_step_policy WHERE id = 1").fetch_optional(&mut *conn).await.map_err(db_err)?;
    Ok(value.as_deref().map(Policy::parse).unwrap_or(Policy::Off))
}

/// An Admin changes the company's setting. Turning it off removes every user's authenticator and
/// backup codes (a trigger), so nobody is left being asked for a code they can't turn off.
pub async fn set_policy(pool: &SqlitePool, admin: &Session, next: Policy) -> Result<(), String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let before = policy(&mut tx).await?;
    if before == next {
        return Ok(());
    }
    let removed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_factors").fetch_one(&mut *tx).await.map_err(db_err)?;
    sqlx::query("UPDATE two_step_policy SET policy = ?1 WHERE id = 1").bind(next.as_str()).execute(&mut *tx).await.map_err(db_err)?;
    let mut details = serde_json::json!({ "by": admin.user.username, "from": before.as_str(), "to": next.as_str() });
    if next == Policy::Off {
        details["factorsRemoved"] = removed.into();
    }
    crate::users::record(&mut tx, 0, "MFA_POLICY_CHANGED", details).await?;
    crate::ledger::commit_write(tx).await.map_err(db_err)
}

/// Whether `user_id` has an authenticator set up.
pub async fn has_factor(conn: &mut SqliteConnection, user_id: i64) -> Result<bool, String> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_factors WHERE user_id = ?1").bind(user_id).fetch_one(&mut *conn).await.map_err(db_err)?;
    Ok(n > 0)
}

/// An authenticator secret, opened. Wiped from memory when dropped; never printed.
pub struct Secret(Zeroizing<Vec<u8>>);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(..)")
    }
}

/// What the sealed secret is bound to, so a sealed secret can't be moved to another user's row.
fn aad(user_id: i64) -> Vec<u8> {
    format!("localbooks two-step v1\0TOTP\0{user_id}").into_bytes()
}

/// The 30-second step `unix_seconds` falls in.
pub fn step_at(unix_seconds: u64) -> u64 {
    unix_seconds / STEP_SECONDS
}

pub fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The code for `step` (RFC 4226 HOTP with RFC 6238's time step), as a number below 10^digits.
fn hotp(secret: &[u8], step: u64, digits: usize) -> u32 {
    let mut mac = Hmac::<sha1::Sha1>::new_from_slice(secret).expect("HMAC takes any key length");
    mac.update(&step.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[19] & 0x0f) as usize;
    let binary = u32::from_be_bytes([digest[offset] & 0x7f, digest[offset + 1], digest[offset + 2], digest[offset + 3]]);
    binary % 10u32.pow(digits as u32)
}

/// The 6-digit code for `step`, as an authenticator app shows it (tests only: sign-in compares
/// numbers in [`matching_step`]).
#[cfg(test)]
pub fn code_for(secret: &Secret, step: u64) -> String {
    format!("{:0width$}", hotp(&secret.0, step, DIGITS), width = DIGITS)
}

/// The step within one of `now_step` that `code` is right for, never at or before `last_step`.
fn matching_step(secret: &[u8], code: &str, now_step: u64, last_step: u64) -> Option<u64> {
    let wanted: u32 = code.parse().ok()?;
    let mut found = None;
    // Every candidate is checked, so the time taken doesn't say which one matched.
    for step in [now_step.saturating_sub(1), now_step, now_step + 1] {
        if hotp(secret, step, DIGITS) == wanted && step > last_step && found.is_none() {
            found = Some(step);
        }
    }
    found
}

/// A TOTP code as typed: digits, with any spaces removed.
fn as_totp(code: &str) -> Option<String> {
    let digits: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    (digits.len() == DIGITS && digits.chars().all(|c| c.is_ascii_digit())).then_some(digits)
}

/// A backup code as typed: case, spaces and dashes don't matter, O reads as 0 and I or L as 1.
fn as_backup(code: &str) -> Option<Zeroizing<String>> {
    let mut out = Zeroizing::new(String::new());
    for c in code.chars().filter(|c| !c.is_whitespace() && *c != '-') {
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        if !CROCKFORD.contains(&(c as u8)) || !c.is_ascii() {
            return None;
        }
        out.push(c);
    }
    (out.len() == BACKUP_CHARS).then_some(out)
}

fn backup_hash(salt: &[u8], code: &str) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(b"localbooks backup code v1\0");
    hasher.update(salt);
    hasher.update(code.as_bytes());
    hasher.finalize().to_vec()
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// RFC 4648 base32 without padding, the form `otpauth://` URIs and authenticator apps use.
fn rfc4648_base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let (mut bits, mut count, mut out) = (0u64, 0u32, String::new());
    for &b in bytes {
        bits = (bits << 8) | u64::from(b);
        count += 8;
        while count >= 5 {
            count -= 5;
            out.push(ALPHABET[((bits >> count) & 31) as usize] as char);
        }
    }
    if count > 0 {
        out.push(ALPHABET[((bits << (5 - count)) & 31) as usize] as char);
    }
    out
}

/// Percent-encodes a label part of an `otpauth://` URI.
fn uri_part(text: &str) -> String {
    text.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

/// What the set-up screen shows: the QR code, and the same secret as text for typing in.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupView {
    /// The secret in groups of four, for an app that can't scan.
    pub secret_text: String,
    /// The QR code of the `otpauth://` URI, as an SVG image.
    pub qr_svg: String,
}

/// A set-up waiting for its first code: the new secret, already sealed under the user's password.
/// Kept in memory only, until the code proves the app saved it.
pub struct PendingSetup {
    pub user_id: i64,
    secret: Secret,
    sealed: PasswordSealed,
}

impl std::fmt::Debug for PendingSetup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingSetup").field("user_id", &self.user_id).finish_non_exhaustive()
    }
}

impl PendingSetup {
    #[cfg(test)]
    pub fn secret(&self) -> &Secret {
        &self.secret
    }
}

async fn password_verifier(conn: &mut SqliteConnection, user_id: i64) -> Result<String, String> {
    sqlx::query_scalar("SELECT password_verifier FROM users WHERE id = ?1")
        .bind(user_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("USER_NOT_FOUND: no user {user_id}"))
}

/// Starts setting up an authenticator for the signed-in user, after their password again.
/// Setting up again replaces the old authenticator and backup codes once the new one is confirmed.
pub async fn begin_setup(pool: &SqlitePool, session: &Session, password: &str) -> Result<(PendingSetup, SetupView), String> {
    let mut conn = pool.acquire().await.map_err(db_err)?;
    if policy(&mut conn).await? == Policy::Off {
        return Err("MFA_OFF: an Admin has to turn on two-step sign-in for this company first".into());
    }
    let user_id = session.user.id;
    if !crypto::verify_password(password, &password_verifier(&mut conn, user_id).await?).await? {
        return Err("PASSWORD_WRONG: your password is not correct".into());
    }
    let secret = Secret(Zeroizing::new(crypto::random::<SECRET_BYTES>()?.to_vec()));
    let sealed = crypto::seal_with_password(password, &aad(user_id), &secret.0).await?;
    let text = Zeroizing::new(rfc4648_base32(&secret.0));
    let company = crate::preferences::get(pool, "company_name").await.unwrap_or_default();
    let account = if company.trim().is_empty() { session.user.username.clone() } else { format!("{} ({})", session.user.username, company.trim()) };
    let uri = Zeroizing::new(format!(
        "otpauth://totp/Tillhouse:{}?secret={}&issuer=Tillhouse&algorithm=SHA1&digits={DIGITS}&period={STEP_SECONDS}",
        uri_part(&account),
        text.as_str()
    ));
    let qr = qrcode::QrCode::new(uri.as_bytes()).map_err(|e| format!("MFA_QR: could not draw the QR code ({e})"))?;
    let qr_svg = qr.render::<qrcode::render::svg::Color<'_>>().min_dimensions(200, 200).quiet_zone(true).build();
    let secret_text = text.as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join(" ");
    Ok((PendingSetup { user_id, secret, sealed }, SetupView { secret_text, qr_svg }))
}

fn new_backup_code() -> Result<Zeroizing<String>, String> {
    // 256 is a multiple of 32, so each byte's low five bits pick a character evenly.
    let bytes = Zeroizing::new(crypto::random::<BACKUP_CHARS>()?);
    Ok(Zeroizing::new(bytes.iter().map(|b| CROCKFORD[(b & 31) as usize] as char).collect()))
}

/// Finishes a set-up with the first code from the app: saves the authenticator and returns the ten
/// backup codes, shown once (`XXXXX-XXXXX`).
pub async fn finish_setup(pool: &SqlitePool, setup: &PendingSetup, code: &str, now: u64) -> Result<Vec<String>, String> {
    let code = as_totp(code).ok_or_else(|| CODE_WRONG.to_string())?;
    let step = matching_step(&setup.secret.0, &code, step_at(now), 0).ok_or_else(|| CODE_WRONG.to_string())?;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    if policy(&mut tx).await? == Policy::Off {
        return Err("MFA_OFF: an Admin turned two-step sign-in off for this company".into());
    }
    let replaced = has_factor(&mut tx, setup.user_id).await?;
    remove(&mut tx, setup.user_id).await?;
    let s = &setup.sealed;
    sqlx::query(
        "INSERT INTO user_factors (user_id, kind, memory_kib, iterations, parallelism, salt, nonce, sealed_secret, last_step)
         VALUES (?1, 'TOTP', ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )
    .bind(setup.user_id)
    .bind(s.memory_kib)
    .bind(s.iterations)
    .bind(s.parallelism)
    .bind(&s.salt)
    .bind(&s.nonce)
    .bind(&s.sealed)
    .bind(i64::try_from(step).map_err(|_| CODE_WRONG.to_string())?)
    .execute(&mut *tx)
    .await
    .map_err(db_err)?;
    let mut codes = Vec::with_capacity(BACKUP_CODES);
    for _ in 0..BACKUP_CODES {
        let code = new_backup_code()?;
        let salt: [u8; 16] = crypto::random()?;
        sqlx::query("INSERT INTO user_backup_codes (user_id, salt, code_hash) VALUES (?1, ?2, ?3)")
            .bind(setup.user_id)
            .bind(&salt[..])
            .bind(backup_hash(&salt, &code))
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        codes.push(format!("{}-{}", &code[..5], &code[5..]));
    }
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = ?1").bind(setup.user_id).fetch_one(&mut *tx).await.map_err(db_err)?;
    crate::users::record(&mut tx, setup.user_id, "MFA_ENROLLED", serde_json::json!({ "username": username, "kind": "TOTP", "replaced": replaced })).await?;
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(codes)
}

#[derive(sqlx::FromRow)]
struct FactorRow {
    memory_kib: i64,
    iterations: i64,
    parallelism: i64,
    salt: Vec<u8>,
    nonce: Vec<u8>,
    sealed_secret: Vec<u8>,
}

impl FactorRow {
    fn sealed(self) -> PasswordSealed {
        PasswordSealed {
            memory_kib: self.memory_kib,
            iterations: self.iterations,
            parallelism: self.parallelism,
            salt: self.salt,
            nonce: self.nonce,
            sealed: self.sealed_secret,
        }
    }
}

async fn factor_row(conn: &mut SqliteConnection, user_id: i64) -> Result<Option<FactorRow>, String> {
    sqlx::query_as("SELECT memory_kib, iterations, parallelism, salt, nonce, sealed_secret FROM user_factors WHERE user_id = ?1")
        .bind(user_id)
        .fetch_optional(&mut *conn)
        .await
        .map_err(db_err)
}

/// After the password signed `user_id` in: their authenticator secret, opened with that password,
/// or `None` when they have no authenticator. A secret that doesn't open (the file was changed
/// outside LocalBooks) is an error, never a way past the code.
pub async fn open_secret(conn: &mut SqliteConnection, user_id: i64, password: &str) -> Result<Option<Secret>, String> {
    let Some(row) = factor_row(conn, user_id).await? else { return Ok(None) };
    let plain = crypto::open_with_password(password, &aad(user_id), &row.sealed())
        .await?
        .ok_or_else(|| "MFA_UNREADABLE: this user's two-step sign-in can't be read; an Admin can reset it, or use the recovery key".to_string())?;
    Ok(Some(Secret(plain)))
}

/// Checks the code typed at sign-in: a current authenticator code (used up on this file) or an
/// unused backup code (used up). [`CODE_WRONG`] when it is neither.
pub async fn check_code(pool: &SqlitePool, user_id: i64, secret: &Secret, code: &str, now: u64) -> Result<(), String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    if let Some(code) = as_totp(code) {
        let last: i64 = sqlx::query_scalar("SELECT last_step FROM user_factors WHERE user_id = ?1")
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(db_err)?
            .ok_or_else(|| CODE_WRONG.to_string())?;
        let step = matching_step(&secret.0, &code, step_at(now), u64::try_from(last).unwrap_or(0)).ok_or_else(|| CODE_WRONG.to_string())?;
        sqlx::query("UPDATE user_factors SET last_step = ?1 WHERE user_id = ?2")
            .bind(i64::try_from(step).map_err(|_| CODE_WRONG.to_string())?)
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        return crate::ledger::commit_write(tx).await.map_err(db_err);
    }
    let code = as_backup(code).ok_or_else(|| CODE_WRONG.to_string())?;
    let unused: Vec<(i64, Vec<u8>, Vec<u8>)> = sqlx::query_as("SELECT id, salt, code_hash FROM user_backup_codes WHERE user_id = ?1 AND used_at IS NULL ORDER BY id")
        .bind(user_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
    let mut matched = None;
    for (id, salt, hash) in &unused {
        if same(&backup_hash(salt, &code), hash) && matched.is_none() {
            matched = Some(*id);
        }
    }
    let id = matched.ok_or_else(|| CODE_WRONG.to_string())?;
    sqlx::query("UPDATE user_backup_codes SET used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1").bind(id).execute(&mut *tx).await.map_err(db_err)?;
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = ?1").bind(user_id).fetch_one(&mut *tx).await.map_err(db_err)?;
    crate::users::record(&mut tx, user_id, "MFA_BACKUP_CODE_USED", serde_json::json!({ "username": username, "backupCodesLeft": unused.len() - 1 })).await?;
    crate::ledger::commit_write(tx).await.map_err(db_err)
}

/// Records wrong codes in the audit trail once the free tries are used up, so guessing shows.
/// Nobody is signed in yet: the trail names the computer's user, and this the user name tried.
pub async fn record_failed(pool: &SqlitePool, user_id: i64, username: &str) -> Result<(), String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    crate::users::record(&mut tx, user_id, "MFA_FAILED", serde_json::json!({ "username": username })).await?;
    crate::ledger::commit_write(tx).await.map_err(db_err)
}

/// Removes `user_id`'s authenticator and backup codes, inside the caller's transaction.
pub async fn remove(conn: &mut SqliteConnection, user_id: i64) -> Result<bool, String> {
    let had = has_factor(&mut *conn, user_id).await?;
    sqlx::query("DELETE FROM user_factors WHERE user_id = ?1").bind(user_id).execute(&mut *conn).await.map_err(db_err)?;
    sqlx::query("DELETE FROM user_backup_codes WHERE user_id = ?1").bind(user_id).execute(&mut *conn).await.map_err(db_err)?;
    Ok(had)
}

/// An Admin's **Reset two-step sign-in** for a user who lost their phone: the user signs in with
/// their password and sets it up again (at once, when the company's setting requires it).
pub async fn reset_by_admin(pool: &SqlitePool, admin: &Session, user_id: i64) -> Result<(), String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let username: String = sqlx::query_scalar("SELECT username FROM users WHERE id = ?1")
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("USER_NOT_FOUND: no user {user_id}"))?;
    if !remove(&mut tx, user_id).await? {
        return Err(format!("MFA_NONE: {username} has no two-step sign-in to reset"));
    }
    crate::users::record(&mut tx, user_id, "MFA_RESET", serde_json::json!({ "username": username, "by": admin.user.username })).await?;
    crate::ledger::commit_write(tx).await.map_err(db_err)
}

/// The signed-in user turns their own two-step sign-in off, after their password; refused when
/// the company's setting requires it for them.
pub async fn remove_own(pool: &SqlitePool, session: &Session, password: &str) -> Result<(), String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    if policy(&mut tx).await?.required_for(session.user.role) {
        return Err("MFA_REQUIRED_BY_COMPANY: this company requires two-step sign-in for you; an Admin can reset it if you lost your phone".into());
    }
    if !crypto::verify_password(password, &password_verifier(&mut tx, session.user.id).await?).await? {
        return Err("PASSWORD_WRONG: your password is not correct".into());
    }
    if !remove(&mut tx, session.user.id).await? {
        return Err("MFA_NONE: you have no two-step sign-in to turn off".into());
    }
    crate::users::record(&mut tx, session.user.id, "MFA_REMOVED", serde_json::json!({ "username": session.user.username })).await?;
    crate::ledger::commit_write(tx).await.map_err(db_err)
}

/// Your own password changed: re-seals your authenticator secret under the new one, inside the
/// caller's transaction (the old password and the secret are both at hand only here).
pub async fn reseal(conn: &mut SqliteConnection, user_id: i64, current: &str, next: &str) -> Result<(), String> {
    let Some(secret) = open_secret(&mut *conn, user_id, current).await? else { return Ok(()) };
    let s = crypto::seal_with_password(next, &aad(user_id), &secret.0).await?;
    sqlx::query("UPDATE user_factors SET memory_kib = ?1, iterations = ?2, parallelism = ?3, salt = ?4, nonce = ?5, sealed_secret = ?6 WHERE user_id = ?7")
        .bind(s.memory_kib)
        .bind(s.iterations)
        .bind(s.parallelism)
        .bind(&s.salt)
        .bind(&s.nonce)
        .bind(&s.sealed)
        .bind(user_id)
        .execute(&mut *conn)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// How many unused backup codes `user_id` has left.
pub async fn backup_codes_left(conn: &mut SqliteConnection, user_id: i64) -> Result<i64, String> {
    sqlx::query_scalar("SELECT COUNT(*) FROM user_backup_codes WHERE user_id = ?1 AND used_at IS NULL").bind(user_id).fetch_one(&mut *conn).await.map_err(db_err)
}

#[cfg(test)]
pub(crate) fn test_code(secret: &Secret, unix_seconds: u64) -> String {
    code_for(secret, step_at(unix_seconds))
}

#[cfg(test)]
mod tests;
