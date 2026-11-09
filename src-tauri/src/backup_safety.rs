//! Is there a backup that would survive this disk, and has one been shown to restore? (gap R8)
//!
//! The daily automatic backups sit in the `backups` folder next to the company file: a failed disk
//! or a lost laptop takes them with it, and a backup that was never opened might not restore. This
//! module records the two things that guard against that and says when either is overdue:
//!
//! - **a copy off this disk**: an offsite backup (`offsite.rs`), or a backup saved (Create Local
//!   Backup › Save a copy…) to a different drive, a network share or a synced folder;
//! - **a test restore**: a backup opened and checked as a restore would, without replacing the books
//!   (here, or for an offsite copy in `offsite::test_restore`).

use std::path::Path;

use chrono::{Local, NaiveDate, NaiveDateTime};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::backup::{self, CompanyFileInfo};
use crate::ledger::db_err;

/// When a backup was last saved off this disk (outside the offsite backup), in `system_preferences`.
const OFF_DISK_KEY: &str = "backup_last_off_disk_at";
/// When a backup was last test-restored here (outside the offsite backup).
const TEST_RESTORE_KEY: &str = "backup_last_test_restore_at";

/// The newest copy off this disk and the newest test restore, as local date-times
/// (`YYYY-MM-DDTHH:MM:SS`), from whichever of the two ways was most recent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Safety {
    pub last_off_disk: Option<String>,
    pub last_test_restore: Option<String>,
}

fn now() -> String {
    Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

async fn stored(pool: &SqlitePool, key: &str) -> Result<Option<String>, String> {
    sqlx::query_scalar("SELECT value FROM system_preferences WHERE key = ?1").bind(key).fetch_optional(pool).await.map_err(db_err)
}

async fn record(pool: &SqlitePool, key: &str, at: &str) -> Result<(), String> {
    sqlx::query("INSERT INTO system_preferences (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')")
        .bind(key)
        .bind(at)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

fn newest(a: Option<String>, b: Option<String>) -> Option<String> {
    // Both are `YYYY-MM-DDTHH:MM:SS`, so the later one sorts last.
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

pub async fn status(pool: &SqlitePool) -> Result<Safety, String> {
    let offsite: Option<(Option<String>, Option<String>)> = sqlx::query_as("SELECT last_backup_at, last_test_restore_at FROM offsite_backup WHERE id = 1")
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    let (offsite_copy, offsite_test) = offsite.unwrap_or_default();
    Ok(Safety {
        last_off_disk: newest(stored(pool, OFF_DISK_KEY).await?, offsite_copy),
        last_test_restore: newest(stored(pool, TEST_RESTORE_KEY).await?, offsite_test),
    })
}

/// Whether `a` and `b` are on the same disk (volume).
#[cfg(unix)]
fn same_volume(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev(),
        // Can't tell: assume the same disk, so nothing counts as off it by mistake.
        _ => true,
    }
}

/// Whether `a` and `b` are on the same disk: the same drive letter or network share.
#[cfg(not(unix))]
fn same_volume(a: &Path, b: &Path) -> bool {
    use std::path::Component;
    let root = |p: &Path| match std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).components().next() {
        Some(Component::Prefix(prefix)) => Some(prefix.as_os_str().to_string_lossy().to_ascii_lowercase()),
        _ => None,
    };
    root(a) == root(b)
}

/// Whether a backup saved at `destination` is off the disk the company file at `company` is on:
/// another drive, a network share or a synced folder.
pub fn is_off_disk(destination: &Path, company: &Path) -> bool {
    let (Some(dest_dir), Some(company_dir)) = (destination.parent(), company.parent()) else { return false };
    crate::storage::classify(dest_dir).is_shared() || !same_volume(dest_dir, company_dir)
}

/// Notes a backup saved to `destination`; counts it when it is off the company file's disk.
pub async fn saved_copy(pool: &SqlitePool, destination: &Path, company: &Path) -> Result<bool, String> {
    let off = is_off_disk(destination, company);
    if off {
        record(pool, OFF_DISK_KEY, &now()).await?;
    }
    Ok(off)
}

/// Test restore: opens the backup at `path` read-only and checks it as a restore would (a LocalBooks
/// company file, its integrity and its version), without touching the open books. Records the time
/// in the open company when it passes.
pub async fn test_restore(pool: Option<&SqlitePool>, path: &Path) -> Result<CompanyFileInfo, String> {
    let info = backup::inspect(path).await?;
    if let Some(pool) = pool {
        record(pool, TEST_RESTORE_KEY, &now()).await?;
    }
    Ok(info)
}

fn day_of(at: &Option<String>) -> Option<NaiveDate> {
    at.as_deref().and_then(|s| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok()).map(|t| t.date())
}

fn days_word(n: i64) -> String {
    if n == 1 { "1 day".into() } else { format!("{n} days") }
}

/// The reminders: one when no backup has been copied off this disk in `days` days, one when none
/// has been test-restored in that time.
pub fn due(safety: &Safety, as_of: NaiveDate, days: i64) -> Vec<crate::reminders::ReminderItem> {
    let item = |label: &str, last: Option<NaiveDate>, never: &str, ago: &str| {
        let detail = match last {
            Some(d) if (as_of - d).num_days() <= days => return None,
            Some(d) => format!("{ago} {} ago", days_word((as_of - d).num_days())),
            None => never.to_string(),
        };
        Some(crate::reminders::ReminderItem { id: None, label: label.into(), detail, date: last.map(|d| d.to_string()), amount_cents: None, entry_id: None })
    };
    [
        item(
            "Copy a backup off this computer",
            day_of(&safety.last_off_disk),
            "No backup has been saved to another drive, a network or synced folder, or an offsite backup yet",
            "The last copy off this computer was",
        ),
        item("Test-restore a backup", day_of(&safety.last_test_restore), "No backup has been test-restored yet", "The last test restore was"),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn each_reminder_comes_due_after_its_days() {
        let never = Safety::default();
        let items = due(&never, d("2026-11-09"), 30);
        assert_eq!(items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), ["Copy a backup off this computer", "Test-restore a backup"]);
        let recent = Safety { last_off_disk: Some("2026-10-20T09:00:00".into()), last_test_restore: Some("2026-09-01T09:00:00".into()) };
        let items = due(&recent, d("2026-11-09"), 30);
        assert_eq!(items.len(), 1, "the copy is 20 days old; the test restore 69");
        assert_eq!(items[0].label, "Test-restore a backup");
        assert_eq!(items[0].detail, "The last test restore was 69 days ago");
        assert_eq!(items[0].date.as_deref(), Some("2026-09-01"));
    }

    #[test]
    fn a_copy_on_the_same_disk_is_not_off_it() {
        let dir = std::env::temp_dir().join(format!("lb-backup-safety-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
        assert!(!is_off_disk(&dir.join("elsewhere").join("copy.localbooks"), &dir.join("Books.localbooks")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_newest_of_this_and_the_offsite_backup_counts_and_a_test_restore_is_recorded() {
        let pool = test_pool().await;
        assert_eq!(status(&pool).await.unwrap(), Safety::default());
        record(&pool, OFF_DISK_KEY, "2026-10-01T08:00:00").await.unwrap();
        sqlx::query(
            "INSERT INTO offsite_backup (id, folder, salt, kdf_memory_kib, kdf_iterations, kdf_parallelism, key_check, last_backup_at, last_test_restore_at)
             VALUES (1, '/offsite', 'salt', 8, 1, 1, '00000000000000000000000000000000', '2026-10-05T08:00:00', '2026-09-01T08:00:00')",
        )
        .execute(&pool)
        .await
        .unwrap();
        let s = status(&pool).await.unwrap();
        assert_eq!(s.last_off_disk.as_deref(), Some("2026-10-05T08:00:00"), "the offsite copy is newer");
        assert_eq!(s.last_test_restore.as_deref(), Some("2026-09-01T08:00:00"));

        // A real backup passes the test restore, which is then the newest.
        let dir = std::env::temp_dir().join(format!("lb-test-restore-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()));
        std::fs::create_dir_all(&dir).unwrap();
        let live = dir.join("Books.localbooks");
        let books = crate::db::open(&live).await.unwrap();
        let copy = dir.join("copy.localbooks");
        backup::snapshot(&books, &live, &copy).await.unwrap();
        crate::db::close_pool(&books).await;
        test_restore(Some(&pool), &copy).await.unwrap();
        let tested = status(&pool).await.unwrap().last_test_restore.unwrap();
        assert!(tested.as_str() > "2026-09-01T08:00:00");
        // Something that isn't a company file fails and records nothing.
        let junk = dir.join("junk.localbooks");
        std::fs::write(&junk, b"not a database").unwrap();
        assert!(test_restore(Some(&pool), &junk).await.unwrap_err().starts_with("RESTORE_NOT_SQLITE"));
        assert_eq!(status(&pool).await.unwrap().last_test_restore.unwrap(), tested);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
