//! Temporary copies LocalBooks writes outside the company file, and when they go (gap R4).
//!
//! Two kinds of plain copy are made in the app's cache folder:
//!
//! - **Mail staging** (`outgoing/<stamp>/`): a form, voucher or report PDF handed to the mail app.
//!   The mail app reads it when it opens the draft, so it is kept for [`KEEP`] and then deleted.
//! - **Unpacked backups** (`localbooks-unpacked-*.qbw`): a QuickBooks backup decompressed to be
//!   read. It is deleted as soon as the import has read it; one left by a crash goes at the next
//!   sweep.
//!
//! [`sweep`] runs when LocalBooks starts and quits, and before each new mail staging. Exports the
//! user saves (CSV, Excel, PDF, zip of attachments) are theirs to keep; the screens say they are
//! plain copies.

use std::path::Path;
use std::time::{Duration, SystemTime};

/// How long a staged mail attachment or a leftover unpacked backup is kept.
pub const KEEP: Duration = Duration::from_secs(60 * 60);

/// The mail staging folder under the cache folder.
pub const OUTGOING: &str = "outgoing";
/// The name every unpacked backup starts with (see `qbw_backup::unpack`).
pub const UNPACKED_PREFIX: &str = "localbooks-unpacked-";

fn older_than(path: &Path, keep: Duration, now: SystemTime) -> bool {
    std::fs::metadata(path).and_then(|m| m.modified()).is_ok_and(|t| now.duration_since(t).is_ok_and(|age| age > keep))
}

/// Deletes staged mail attachments older than `keep` under `outgoing`; returns how many folders went.
pub fn sweep_outgoing(outgoing: &Path, keep: Duration, now: SystemTime) -> usize {
    let Ok(entries) = std::fs::read_dir(outgoing) else { return 0 };
    entries
        .flatten()
        .filter(|e| older_than(&e.path(), keep, now))
        .filter(|e| if e.path().is_dir() { std::fs::remove_dir_all(e.path()).is_ok() } else { std::fs::remove_file(e.path()).is_ok() })
        .count()
}

/// Deletes the temporary copies under the cache folder that are older than `keep`; returns how
/// many went.
pub fn sweep(cache_dir: &Path, keep: Duration, now: SystemTime) -> usize {
    let mut removed = sweep_outgoing(&cache_dir.join(OUTGOING), keep, now);
    if let Ok(entries) = std::fs::read_dir(cache_dir) {
        removed += entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(UNPACKED_PREFIX) && e.path().is_file())
            .filter(|e| older_than(&e.path(), keep, now))
            .filter(|e| std::fs::remove_file(e.path()).is_ok())
            .count();
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweeps_old_staged_mail_and_leftover_unpacked_backups_only() {
        let dir = std::env::temp_dir().join(format!("lb-temp-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let staged = dir.join(OUTGOING).join("123");
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(staged.join("Invoice 1001.pdf"), b"%PDF").unwrap();
        std::fs::write(dir.join("localbooks-unpacked-42-7.qbw"), b"qbw").unwrap();
        std::fs::write(dir.join("keep-me.txt"), b"not ours").unwrap();

        // Nothing is old yet.
        assert_eq!(sweep(&dir, KEEP, SystemTime::now()), 0);
        assert!(staged.exists());
        // Two hours on, both temporary copies go; anything else stays.
        assert_eq!(sweep(&dir, KEEP, SystemTime::now() + Duration::from_secs(2 * 60 * 60)), 2);
        assert!(!staged.exists());
        assert!(!dir.join("localbooks-unpacked-42-7.qbw").exists());
        assert!(dir.join("keep-me.txt").exists());
        assert_eq!(sweep(&dir.join("missing"), KEEP, SystemTime::now()), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
