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

/// Stars every digit but the last four, keeping the dashes: `123-45-6789` → `***-**-6789`.
pub fn mask(plain: &str) -> String {
    let digits = plain.chars().filter(char::is_ascii_alphanumeric).count();
    let mut seen = 0;
    plain
        .chars()
        .map(|c| {
            if !c.is_ascii_alphanumeric() {
                return c;
            }
            seen += 1;
            if seen + 4 > digits { c } else { '*' }
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

/// Encrypts every plain tax ID, and drops the copies imports staged for vendors already created.
/// Returns how many were encrypted.
pub async fn protect_all(conn: &mut SqliteConnection, key: &DataKey) -> Result<usize, String> {
    let plain: Vec<(i64, String)> = sqlx::query_as("SELECT id, tax_id FROM vendors WHERE tax_id IS NOT NULL AND tax_id NOT LIKE 'lb1:%'")
        .fetch_all(&mut *conn)
        .await
        .map_err(db_err)?;
    let access = Access { protected: true, key: Some(key) };
    for (id, value) in &plain {
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
    Ok(plain.len())
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
mod tests {
    use super::*;
    use crate::db::test_pool;

    #[test]
    fn masks_keep_the_last_four_and_the_shape() {
        assert_eq!(mask("123-45-6789"), "***-**-6789");
        assert_eq!(mask("12-3456789"), "**-***6789");
        assert_eq!(mask("123"), "123");
        assert!(looks_like_tax_id("12-3456789") && looks_like_tax_id("123 45 6789"));
        assert!(!looks_like_tax_id("AQAAANCMnd8BFdERjHoAwE/Cl+sBAAAAexample") && !looks_like_tax_id("n/a"));
    }

    #[tokio::test]
    async fn a_file_without_users_stores_and_shows_plain_ids() {
        let (stored, masked) = store(Access::OPEN, 1, Some(" 12-3456789 ")).unwrap();
        assert_eq!((stored.as_deref(), masked.as_deref()), (Some("12-3456789"), Some("**-***6789")));
        let view = show(Access::OPEN, 1, stored.as_deref(), masked.as_deref()).unwrap();
        assert_eq!(view.full.as_deref(), Some("12-3456789"));
        assert_eq!(store(Access::OPEN, 1, Some("  ")).unwrap(), (None, None));
        assert_eq!(show(Access::OPEN, 1, None, None).unwrap(), TaxIdView::default());
    }

    #[tokio::test]
    async fn protected_ids_show_only_with_the_key() {
        let pool = test_pool().await;
        let (session, _) = crate::users::turn_on(&pool, "Owner", None, "owner password").await.unwrap();
        let key = session.key.as_ref().unwrap();
        let with_key = Access { protected: true, key: Some(key) };
        let without = Access { protected: true, key: None };

        let (stored, masked) = store(with_key, 4, Some("123-45-6789")).unwrap();
        assert!(crypto::is_encrypted(stored.as_deref().unwrap()));
        assert_eq!(show(with_key, 4, stored.as_deref(), masked.as_deref()).unwrap().full.as_deref(), Some("123-45-6789"));
        let hidden = show(without, 4, stored.as_deref(), masked.as_deref()).unwrap();
        assert_eq!((hidden.full, hidden.masked.as_deref()), (None, Some("***-**-6789")));
        assert!(store(without, 4, Some("123-45-6789")).unwrap_err().starts_with("TAX_ID_LOCKED"));
        assert!(show(with_key, 5, stored.as_deref(), masked.as_deref()).is_err(), "bound to its vendor");

        // A plain value (an import not yet protected) is still masked for users without the key.
        let plain = show(without, 4, Some("98-7654321"), None).unwrap();
        assert_eq!((plain.full, plain.masked.as_deref()), (None, Some("**-***4321")));
    }

    #[tokio::test]
    async fn protect_all_encrypts_plain_ids_once() {
        let pool = test_pool().await;
        let a: i64 = sqlx::query_scalar("INSERT INTO vendors (name, tax_id) VALUES ('Acme', '12-3456789') RETURNING id").fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO vendors (name) VALUES ('No ID')").execute(&pool).await.unwrap();
        let (session, _) = crate::users::turn_on(&pool, "Owner", None, "owner password").await.unwrap();
        let key = session.key.as_ref().unwrap();

        let (stored, masked): (String, String) = sqlx::query_as("SELECT tax_id, tax_id_masked FROM vendors WHERE id = ?1").bind(a).fetch_one(&pool).await.unwrap();
        assert!(crypto::is_encrypted(&stored), "turning users on protected the existing ID");
        assert_eq!(masked, "**-***6789");
        assert_eq!(key.decrypt_field(TABLE, COLUMN, a, &stored).unwrap(), "12-3456789");
        assert_eq!(protect_all(&mut pool.acquire().await.unwrap(), key).await.unwrap(), 0, "nothing left to protect");
    }
}
