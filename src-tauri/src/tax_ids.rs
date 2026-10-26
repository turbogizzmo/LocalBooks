//! Vendor tax IDs (EINs and SSNs) as sensitive data.
//!
//! In a company file without users, a tax ID is stored and shown as typed. Once users are on
//! ([`crate::users`]), every tax ID is stored encrypted with the file's data key ([`crate::crypto`])
//! next to a masked copy (`***-**-6789`). Users who may see sensitive data get the full ID; everyone
//! else gets the masked one, and cannot change it.
//!
//! The vendor form encrypts as it saves. Imports write plain values, which [`protect_all`] encrypts
//! afterwards (it also runs when users are turned on, and when someone with the key signs in). An
//! import's staging rows drop their copy once the import has created the vendor; a staged import not
//! yet carried out keeps it until it is imported or discarded.

use serde::Serialize;
use sqlx::SqliteConnection;

use crate::crypto::{self, DataKey};
use crate::ledger::db_err;

const TABLE: &str = "vendors";
const COLUMN: &str = "tax_id";

/// Who is asking: whether the file protects sensitive data, and the unlocked key if they hold it.
#[derive(Debug, Clone, Copy)]
pub struct Access<'a> {
    pub protected: bool,
    pub key: Option<&'a DataKey>,
}

impl<'a> Access<'a> {
    /// A company file without users: everything is plain.
    #[cfg(test)]
    pub const OPEN: Access<'static> = Access { protected: false, key: None };

    pub async fn of(conn: &mut SqliteConnection, key: Option<&'a DataKey>) -> Result<Access<'a>, String> {
        Ok(Access { protected: crypto::is_protected(conn).await?, key })
    }

    pub fn can_see(&self) -> bool {
        !self.protected || self.key.is_some()
    }
}

/// A tax ID as one user may see it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxIdView {
    /// The full ID, when the user may see it.
    pub full: Option<String>,
    /// Always set when there is an ID.
    pub masked: Option<String>,
}

/// Stars every digit but the last four, keeping the dashes: `123-45-6789` → `***-**-6789`. An ID
/// shorter than eight letters and digits is starred entirely: its last four would give most of it away.
pub fn mask(plain: &str) -> String {
    let digits = plain.chars().filter(char::is_ascii_alphanumeric).count();
    let shown = if digits >= 8 { 4 } else { 0 };
    let mut seen = 0;
    plain
        .chars()
        .map(|c| {
            if !c.is_ascii_alphanumeric() {
                return c;
            }
            seen += 1;
            if seen + shown > digits { c } else { '*' }
        })
        .collect()
}

/// What to store for `plain` on vendor `vendor_id`: the value and its masked form.
pub fn store(access: Access, vendor_id: i64, plain: Option<&str>) -> Result<(Option<String>, Option<String>), String> {
    let Some(plain) = plain.map(str::trim).filter(|p| !p.is_empty()) else { return Ok((None, None)) };
    if !access.protected {
        return Ok((Some(plain.to_string()), Some(mask(plain))));
    }
    let key = access.key.ok_or_else(|| "TAX_ID_LOCKED: only users who can see sensitive data can change a tax ID".to_string())?;
    Ok((Some(key.encrypt_field(TABLE, COLUMN, vendor_id, plain)?), Some(mask(plain))))
}

/// A stored tax ID as `access` may see it.
pub fn show(access: Access, vendor_id: i64, stored: Option<&str>, masked: Option<&str>) -> Result<TaxIdView, String> {
    let Some(stored) = stored else { return Ok(TaxIdView::default()) };
    let plain = if crypto::is_encrypted(stored) {
        match access.key {
            Some(key) => Some(key.decrypt_field(TABLE, COLUMN, vendor_id, stored)?),
            None => None,
        }
    } else {
        // Plain: a file without users, or an import not yet protected.
        Some(stored.to_string())
    };
    let masked = masked.map(str::to_string).or_else(|| plain.as_deref().map(mask)).or_else(|| Some("****".into()));
    Ok(TaxIdView { full: plain.filter(|_| access.can_see()), masked })
}

/// Whether a stored "tax ID" is really QuickBooks' own encrypted value: base64 text that newer
/// QuickBooks files hold in place of the ID, readable only with QuickBooks' key. Narrow on purpose
/// (the same rule as migration 0046): at least 24 characters, nothing but base64 characters, a
/// letter and a digit, and a `/` or `+` or a trailing `=`. Real IDs and notes never match.
pub fn is_quickbooks_encrypted(text: &str) -> bool {
    let t = text.trim();
    t.len() >= 24
        && t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
        && t.chars().any(|c| c.is_ascii_alphabetic())
        && t.chars().any(|c| c.is_ascii_digit())
        && (t.contains('/') || t.contains('+') || t.ends_with('='))
}

async fn clear(conn: &mut SqliteConnection, vendor_id: i64) -> Result<(), String> {
    sqlx::query("UPDATE vendors SET tax_id = NULL, tax_id_masked = NULL WHERE id = ?1").bind(vendor_id).execute(&mut *conn).await.map_err(db_err)?;
    Ok(())
}

/// Encrypts every plain tax ID (and bank detail, see [`crate::ach::protect_all`]), and drops the copies imports staged for vendors already created.
/// QuickBooks' own encrypted values ([`is_quickbooks_encrypted`]) are cleared instead, including
/// ones encrypted before that check existed. Returns how many were encrypted.
pub async fn protect_all(conn: &mut SqliteConnection, key: &DataKey) -> Result<usize, String> {
    let encrypted: Vec<(i64, String)> = sqlx::query_as("SELECT id, tax_id FROM vendors WHERE tax_id LIKE 'lb1:%'").fetch_all(&mut *conn).await.map_err(db_err)?;
    for (id, stored) in &encrypted {
        if key.decrypt_field(TABLE, COLUMN, *id, stored).is_ok_and(|plain| is_quickbooks_encrypted(&plain)) {
            clear(conn, *id).await?;
        }
    }
    let plain: Vec<(i64, String)> = sqlx::query_as("SELECT id, tax_id FROM vendors WHERE tax_id IS NOT NULL AND tax_id NOT LIKE 'lb1:%'")
        .fetch_all(&mut *conn)
        .await
        .map_err(db_err)?;
    let access = Access { protected: true, key: Some(key) };
    let mut protected = 0;
    for (id, value) in &plain {
        if is_quickbooks_encrypted(value) {
            clear(conn, *id).await?;
            continue;
        }
        protected += 1;
        let (stored, masked) = store(access, *id, Some(value))?;
        sqlx::query("UPDATE vendors SET tax_id = ?1, tax_id_masked = ?2 WHERE id = ?3")
            .bind(stored)
            .bind(masked)
            .bind(id)
            .execute(&mut *conn)
            .await
            .map_err(db_err)?;
    }
    sqlx::query("UPDATE ledger_import_parties SET tax_id = NULL WHERE tax_id IS NOT NULL AND party_id IS NOT NULL")
        .execute(&mut *conn)
        .await
        .map_err(db_err)?;
    // Vendors' bank details are sensitive in the same way.
    crate::ach::protect_all(conn, key).await?;
    Ok(protected)
}

/// Whether text read from another program's file looks like a tax ID (digits, letters, dashes and
/// spaces, 4 to 20 of them) rather than that program's own encrypted value.
pub fn looks_like_tax_id(text: &str) -> bool {
    let text = text.trim();
    (4..=20).contains(&text.len())
        && text.chars().any(|c| c.is_ascii_digit())
        && text.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ' ')
}

#[cfg(test)]
mod tests;
