//! Is there a backup that would survive this disk, and has one been shown to restore? (gap R8)
//!
//! The daily automatic backups sit in the `backups` folder next to the company file: a failed disk
//! or a lost laptop takes them with it, and a backup that was never opened might not restore. This
//! module records the two things that guard against that and says when either is overdue:
//!
//! - **a copy off this disk**: a backup saved with Back up to…, or made by the offsite backup
//!   (`offsite.rs`), on a different drive (volume), a network share or a synced folder. A copy on
//!   the company file's own disk doesn't count, offsite or not;
//! - **a test restore**: a backup of these books opened and checked as a restore would, its schema
//!   upgrade rehearsed on a throwaway copy when it is from an older version, without replacing the
//!   books (here, or for an offsite copy in `offsite::test_restore`).
//!
//! Each is a row in `backup_safety_events` (migration 0176), with where it was. Nothing is sent
//! anywhere: the check is the file system's own answer to "which volume is this folder on".

use std::path::Path;

use chrono::{Local, NaiveDate, NaiveDateTime};
use serde::Serialize;
use sqlx::SqlitePool;

use crate::backup::{self, CompanyFileInfo};
use crate::ledger::db_err;

/// What `backup_safety_events` records (migration 0176).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A backup saved off the disk the company file is on.
    OffDiskCopy,
    /// A backup of these books passed a test restore.
    TestRestore,
}

impl Kind {
    fn code(self) -> &'static str {
        match self {
            Kind::OffDiskCopy => "OFF_DISK_COPY",
            Kind::TestRestore => "TEST_RESTORE",
        }
    }
}

/// How it was done: `BACK_UP_TO` (Back up to…), `OFFSITE` (the offsite backup) or `LOCAL` (a backup
/// file tested here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    BackUpTo,
    Offsite,
    Local,
}

impl Source {
    fn code(self) -> &'static str {
        match self {
            Source::BackUpTo => "BACK_UP_TO",
            Source::Offsite => "OFFSITE",
            Source::Local => "LOCAL",
        }
    }
}

/// The newest copy off this disk and the newest test restore, as local date-times
/// (`YYYY-MM-DDTHH:MM:SS`), with where each was (the folder or file; `None` for one carried over
/// from before LocalBooks kept it).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Safety {
    pub last_off_disk: Option<String>,
    pub last_off_disk_location: Option<String>,
    pub last_test_restore: Option<String>,
    pub last_test_restore_location: Option<String>,
    /// The offsite backup's folder is on the same disk as the company file, so its copies don't
    /// count as off this disk. Filled in by [`offsite_on_same_disk`]; [`status`] leaves it false.
    pub offsite_on_same_disk: bool,
}

fn now() -> String {
    Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// Records one event at `at`.
async fn record_at(pool: &SqlitePool, kind: Kind, source: Source, location: Option<&Path>, at: &str) -> Result<(), String> {
    sqlx::query("INSERT INTO backup_safety_events (kind, source, at, location) VALUES (?1, ?2, ?3, ?4)")
        .bind(kind.code())
        .bind(source.code())
        .bind(at)
        .bind(location.map(|p| p.display().to_string()))
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Records one event now.
pub async fn record(pool: &SqlitePool, kind: Kind, source: Source, location: Option<&Path>) -> Result<(), String> {
    record_at(pool, kind, source, location, &now()).await
}

/// A stored time: `YYYY-MM-DDTHH:MM:SS`, with or without fractional seconds and a trailing `Z`, or
/// with a space for the `T` (the forms LocalBooks writes).
fn parse_at(text: &str) -> Option<NaiveDateTime> {
    let t = text.trim().trim_end_matches('Z');
    NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M:%S%.f").or_else(|_| NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S%.f")).ok()
}

/// The newest event of `kind`, compared as times (a time that doesn't parse loses), and its location.
async fn newest(pool: &SqlitePool, kind: Kind) -> Result<(Option<String>, Option<String>), String> {
    let rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT at, location FROM backup_safety_events WHERE kind = ?1 ORDER BY id DESC").bind(kind.code()).fetch_all(pool).await.map_err(db_err)?;
    let best = rows.into_iter().fold(None::<(String, Option<String>)>, |best, row| match best {
        Some(b) if parse_at(&row.0) <= parse_at(&b.0) => Some(b),
        _ => Some(row),
    });
    Ok(best.map_or((None, None), |(at, location)| (Some(at), location)))
}

/// When a backup was last copied off this disk and last test-restored, and where.
pub async fn status(pool: &SqlitePool) -> Result<Safety, String> {
    let (last_off_disk, last_off_disk_location) = newest(pool, Kind::OffDiskCopy).await?;
    let (last_test_restore, last_test_restore_location) = newest(pool, Kind::TestRestore).await?;
    Ok(Safety { last_off_disk, last_off_disk_location, last_test_restore, last_test_restore_location, offsite_on_same_disk: false })
}

/// Whether the offsite backup is on and its folder is, as far as this computer can tell, on the
/// same disk as the company file at `company` (a folder that isn't there, such as an unplugged
/// drive, can't be told and isn't flagged).
pub async fn offsite_on_same_disk(pool: &SqlitePool, company: &Path) -> Result<bool, String> {
    let folder: Option<String> = sqlx::query_scalar("SELECT folder FROM offsite_backup WHERE id = 1").fetch_optional(pool).await.map_err(db_err)?;
    Ok(folder.is_some_and(|f| {
        let f = Path::new(&f);
        f.is_dir() && !is_off_disk(&f.join("copy"), company)
    }))
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
    // `C:` and `\\?\C:` (a canonical path) are the same drive.
    let root = |p: &Path| match std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf()).components().next() {
        Some(Component::Prefix(prefix)) => Some(prefix.as_os_str().to_string_lossy().to_ascii_lowercase().trim_start_matches(r"\\?\").trim_start_matches(r"unc\").to_string()),
        _ => None,
    };
    root(a) == root(b)
}

/// Whether a backup saved at `destination` is off the disk the company file at `company` is on:
/// another drive, or a network share or synced folder when the books themselves are on this
/// computer. A copy beside books that already live on a share is on that same share.
pub fn is_off_disk(destination: &Path, company: &Path) -> bool {
    let (Some(dest_dir), Some(company_dir)) = (destination.parent(), company.parent()) else { return false };
    !same_volume(dest_dir, company_dir) || (crate::storage::classify(dest_dir).is_shared() && !crate::storage::classify(company_dir).is_shared())
}

/// Notes a backup saved to `destination` by `source`; records it when it is off the company file's
/// disk, and says whether it was.
pub async fn saved_copy(pool: &SqlitePool, destination: &Path, company: &Path, source: Source) -> Result<bool, String> {
    let off = is_off_disk(destination, company);
    if off {
        record(pool, Kind::OffDiskCopy, source, Some(destination)).await?;
    }
    Ok(off)
}

/// Whether a backup described by `info` is of the books open in `pool` (the same company name).
pub async fn is_these_books(pool: &SqlitePool, info: &CompanyFileInfo) -> bool {
    let ours = crate::preferences::get(pool, "company_name").await.unwrap_or_default();
    info.company_name.as_deref().is_some_and(|n| n.trim().eq_ignore_ascii_case(ours.trim()))
}

/// Test restore: opens the backup at `path` read-only and checks it as a restore would (a LocalBooks
/// company file, its integrity, its links, balanced entries and its version), and when it is from
/// an older version also upgrades a copy of it in `scratch` (then deletes the copy), without
/// touching the open books. When it passes and is a backup of the open books, the test restore is
/// recorded in them.
pub async fn test_restore(pool: Option<&SqlitePool>, scratch: &Path, path: &Path) -> Result<CompanyFileInfo, String> {
    let info = backup::inspect(path).await?;
    if info.needs_upgrade {
        backup::rehearse_upgrade(path, scratch).await?;
    }
    if let Some(pool) = pool {
        // A passing test stays passed if noting it fails.
        if is_these_books(pool, &info).await {
            if let Err(e) = record(pool, Kind::TestRestore, Source::Local, Some(path)).await {
                crate::diagnostics::log(&format!("backup safety: could not note the test restore: {e}"));
            }
        }
    }
    Ok(info)
}

fn day_of(at: &Option<String>) -> Option<NaiveDate> {
    at.as_deref().and_then(parse_at).map(|t| t.date())
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
        let recent = Safety { last_off_disk: Some("2026-10-20T09:00:00".into()), last_test_restore: Some("2026-09-01T09:00:00".into()), ..Default::default() };
        let items = due(&recent, d("2026-11-09"), 30);
        assert_eq!(items.len(), 1, "the copy is 20 days old; the test restore 69");
        assert_eq!(items[0].label, "Test-restore a backup");
        assert_eq!(items[0].detail, "The last test restore was 69 days ago");
        assert_eq!(items[0].date.as_deref(), Some("2026-09-01"));
    }

    #[test]
    fn stored_times_parse_in_every_form_written() {
        assert_eq!(parse_at("2026-10-05T08:00:00").map(|t| t.date()), Some(d("2026-10-05")));
        assert_eq!(parse_at("2026-10-05T08:00:00.123Z").map(|t| t.date()), Some(d("2026-10-05")));
        assert_eq!(parse_at("2026-10-05 08:00:00").map(|t| t.date()), Some(d("2026-10-05")));
        assert_eq!(parse_at("soon"), None);
    }

    #[test]
    fn a_copy_on_the_same_disk_is_not_off_it() {
        let dir = std::env::temp_dir().join(format!("lb-backup-safety-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
        assert!(!is_off_disk(&dir.join("elsewhere").join("copy.localbooks"), &dir.join("Books.localbooks")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A different volume: on Linux, `/dev/shm` (tmpfs) when the temp folder is on another device.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_copy_on_another_volume_is_off_the_disk() {
        use std::os::unix::fs::MetadataExt;
        let tmp = std::env::temp_dir();
        let shm = Path::new("/dev/shm");
        let (Ok(a), Ok(b)) = (std::fs::metadata(&tmp), std::fs::metadata(shm)) else { return };
        if a.dev() == b.dev() {
            return;
        }
        assert!(is_off_disk(&shm.join("copy.localbooks"), &tmp.join("Books.localbooks")));
        assert!(!is_off_disk(&tmp.join("copy.localbooks"), &tmp.join("Books.localbooks")));
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("lb-{name}-{}-{}", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn the_newest_event_of_each_kind_counts_with_where_it_was() {
        let pool = test_pool().await;
        assert_eq!(status(&pool).await.unwrap(), Safety::default());
        record_at(&pool, Kind::OffDiskCopy, Source::BackUpTo, Some(Path::new("/media/usb/Books.localbooks")), "2026-10-05T10:00:00.500Z").await.unwrap();
        // Recorded later, but an earlier time: the time decides, not the order or the text.
        record_at(&pool, Kind::OffDiskCopy, Source::Offsite, Some(Path::new("/sync/offsite")), "2026-10-05T09:59:59").await.unwrap();
        record_at(&pool, Kind::TestRestore, Source::Local, None, "2026-09-01 08:00:00").await.unwrap();
        let s = status(&pool).await.unwrap();
        assert_eq!(s.last_off_disk.as_deref(), Some("2026-10-05T10:00:00.500Z"));
        assert_eq!(s.last_off_disk_location.as_deref(), Some("/media/usb/Books.localbooks"));
        assert_eq!(s.last_test_restore.as_deref(), Some("2026-09-01 08:00:00"));
        assert_eq!(s.last_test_restore_location, None);
        // An offsite backup by itself is no copy off this disk.
        sqlx::query(
            "INSERT INTO offsite_backup (id, folder, salt, kdf_memory_kib, kdf_iterations, kdf_parallelism, key_check, last_backup_at, last_test_restore_at)
             VALUES (1, '/offsite', 'salt', 8, 1, 1, '00000000000000000000000000000000', '2026-11-05T08:00:00', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_eq!(status(&pool).await.unwrap().last_off_disk.as_deref(), Some("2026-10-05T10:00:00.500Z"));
    }

    #[tokio::test]
    async fn a_copy_saved_on_the_same_disk_is_not_recorded_and_flags_an_offsite_folder_there() {
        let pool = test_pool().await;
        let dir = scratch("same-disk");
        let company = dir.join("Books.localbooks");
        assert!(!saved_copy(&pool, &dir.join("copy.localbooks"), &company, Source::BackUpTo).await.unwrap());
        assert_eq!(status(&pool).await.unwrap().last_off_disk, None);

        assert!(!offsite_on_same_disk(&pool, &company).await.unwrap(), "no offsite backup");
        let folder = dir.join("offsite");
        std::fs::create_dir_all(&folder).unwrap();
        sqlx::query("INSERT INTO offsite_backup (id, folder, salt, kdf_memory_kib, kdf_iterations, kdf_parallelism, key_check) VALUES (1, ?1, 'salt', 8, 1, 1, '00000000000000000000000000000000')")
            .bind(folder.display().to_string())
            .execute(&pool)
            .await
            .unwrap();
        assert!(offsite_on_same_disk(&pool, &company).await.unwrap());
        // A folder that isn't there (an unplugged drive) can't be told, and isn't flagged.
        sqlx::query("UPDATE offsite_backup SET folder = ?1").bind(dir.join("gone").display().to_string()).execute(&pool).await.unwrap();
        assert!(!offsite_on_same_disk(&pool, &company).await.unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    async fn books_named(path: &Path, name: &str) -> SqlitePool {
        let books = crate::db::open(path).await.unwrap();
        sqlx::query("INSERT INTO system_preferences (key, value) VALUES ('company_name', ?1) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
            .bind(name)
            .execute(&books)
            .await
            .unwrap();
        books
    }

    #[tokio::test]
    async fn a_test_restore_of_these_books_is_recorded_and_others_are_not() {
        let pool = test_pool().await;
        let ours = crate::preferences::get(&pool, "company_name").await.unwrap();
        let dir = scratch("test-restore");
        let live = dir.join("Books.localbooks");
        let books = books_named(&live, &ours).await;
        let copy = dir.join("copy.localbooks");
        backup::snapshot(&books, &live, &copy).await.unwrap();
        crate::db::close_pool(&books).await;

        let info = test_restore(Some(&pool), &dir.join("scratch"), &copy).await.unwrap();
        assert!(!info.needs_upgrade);
        let s = status(&pool).await.unwrap();
        assert!(s.last_test_restore.is_some());
        assert_eq!(s.last_test_restore_location.as_deref(), Some(copy.display().to_string().as_str()));

        // Something that isn't a company file fails and records nothing.
        let junk = dir.join("junk.localbooks");
        std::fs::write(&junk, b"not a database").unwrap();
        assert!(test_restore(Some(&pool), &dir, &junk).await.unwrap_err().starts_with("RESTORE_NOT_SQLITE"));
        // A backup of other books opens fine but doesn't count as these books' test restore.
        let other = dir.join("Other.localbooks");
        let others = books_named(&other, "Someone Else Ltd").await;
        let other_copy = dir.join("other-copy.localbooks");
        backup::snapshot(&others, &other, &other_copy).await.unwrap();
        crate::db::close_pool(&others).await;
        test_restore(Some(&pool), &dir, &other_copy).await.unwrap();
        let events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM backup_safety_events WHERE kind = 'TEST_RESTORE'").fetch_one(&pool).await.unwrap();
        assert_eq!(events, 1, "only the passing test of these books");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A backup from an older version is upgraded on a copy as a restore would, and the copy is
    /// gone afterwards; the backup itself is unchanged.
    #[tokio::test]
    async fn a_test_restore_of_an_older_backup_rehearses_the_upgrade() {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        let pool = test_pool().await;
        let dir = scratch("rehearse");
        // A backup from the previous release: every migration but the latest.
        let copy = dir.join("old-copy.localbooks");
        let latest = crate::db::MIGRATOR.iter().map(|m| m.version).max().unwrap();
        {
            let options = SqliteConnectOptions::new().filename(&copy).create_if_missing(true).foreign_keys(true);
            let old = SqlitePoolOptions::new().max_connections(1).connect_with(options).await.unwrap();
            let previous = sqlx::migrate::Migrator {
                migrations: crate::db::MIGRATOR.iter().filter(|m| m.version < latest).cloned().collect::<Vec<_>>().into(),
                ..sqlx::migrate::Migrator::DEFAULT
            };
            crate::db::migrate_with(&old, &previous).await.unwrap();
            let ours = crate::preferences::get(&pool, "company_name").await.unwrap();
            sqlx::query("INSERT INTO system_preferences (key, value) VALUES ('company_name', ?1) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
                .bind(ours)
                .execute(&old)
                .await
                .unwrap();
            crate::db::close_pool(&old).await;
        }

        let scratch_dir = dir.join("scratch");
        let info = test_restore(Some(&pool), &scratch_dir, &copy).await.unwrap();
        assert!(info.needs_upgrade);
        assert_eq!(std::fs::read_dir(&scratch_dir).unwrap().count(), 0, "the rehearsal copy is deleted");
        assert!(backup::inspect(&copy).await.unwrap().needs_upgrade, "the backup itself is not upgraded");
        assert!(status(&pool).await.unwrap().last_test_restore.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
