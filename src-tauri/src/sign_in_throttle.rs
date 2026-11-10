//! A growing wait after failed sign-ins and failed recovery-key attempts (gap R27).
//!
//! Without it, someone at the sign-in screen can try passwords as fast as Argon2id allows. With it,
//! the first [`FREE_TRIES`] wrong tries cost nothing extra, then each one makes the next try wait
//! longer, up to [`MAX_WAIT`]. There is **no lockout**: after the wait anyone may try again, so the
//! only Admin can never be shut out of their own books by someone else's guessing.
//!
//! - **Kept per company file and per user name.** The file is named by its data key's id, which a
//!   copy of the file shares; the user name is compared without case, as the users table does. An
//!   unknown user name is counted like a real one, so the wait reveals nothing about which names
//!   exist. Recovery-key attempts have their own count for the file ([`RECOVERY`]).
//! - **Kept outside the company file**, in an owner-only file in LocalBooks' settings folder, so a
//!   restart doesn't reset the count and the sign-in screen still writes nothing to the books. If
//!   that file can't be written the count still works for this run; it never blocks a sign-in.
//! - A try made while a wait is running is refused without checking the password, and doesn't add
//!   to the count. A successful sign-in clears the user's count; counts untouched for a day expire.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// Wrong tries allowed before any wait.
pub const FREE_TRIES: u32 = 4;
/// The longest wait, however many tries have failed.
pub const MAX_WAIT: Duration = Duration::from_secs(5 * 60);
/// A count with no failure for this long starts again from zero.
const FORGET_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
/// The count for recovery-key attempts on a file.
pub const RECOVERY: &str = "(recovery key)";
pub const FILE_NAME: &str = "sign-in-waits.json";

/// The wait after `failures` wrong tries: none for the free tries, then 30 s, 1, 2 and 5 minutes.
pub fn wait_after(failures: u32) -> Duration {
    match failures.saturating_sub(FREE_TRIES) {
        0 => Duration::ZERO,
        1 => Duration::from_secs(30),
        2 => Duration::from_secs(60),
        3 => Duration::from_secs(120),
        _ => MAX_WAIT,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Count {
    failures: u32,
    /// Seconds since 1970 of the last failure.
    last: u64,
}

fn secs(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Where the counts live, loaded once and saved after each change.
pub struct Throttle {
    path: Option<PathBuf>,
    counts: Mutex<BTreeMap<String, Count>>,
}

impl Throttle {
    /// Counts kept in `config_dir`, or only in memory when `config_dir` is None (tests).
    pub fn new(config_dir: Option<&Path>) -> Self {
        let path = config_dir.map(|d| d.join(FILE_NAME));
        let counts = path.as_deref().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Throttle { path, counts: Mutex::new(counts) }
    }

    fn key(file: &str, who: &str) -> String {
        format!("{file}\n{}", who.trim().to_lowercase())
    }

    fn save(&self, counts: &BTreeMap<String, Count>) {
        let Some(path) = &self.path else { return };
        if let Ok(bytes) = serde_json::to_vec(counts) {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            // Best effort: an unwritable settings folder must never stop anyone signing in.
            let _ = crate::instance::write_private(path, &bytes);
        }
    }

    /// Ok when `who` may try now; otherwise the error to show, naming how long to wait.
    pub fn check(&self, file: &str, who: &str, now: SystemTime) -> Result<(), String> {
        let counts = self.counts.lock().expect("sign-in waits");
        let Some(count) = counts.get(&Self::key(file, who)) else { return Ok(()) };
        // Measured from the last failure and never more than the schedule allows, so a clock set
        // back can't stretch the wait beyond MAX_WAIT.
        let waited = Duration::from_secs(secs(now).saturating_sub(count.last));
        let remaining = wait_after(count.failures).saturating_sub(waited).min(MAX_WAIT);
        if remaining.is_zero() || secs(now) < count.last {
            return Ok(());
        }
        let seconds = remaining.as_secs().max(1);
        let when = if seconds < 90 { format!("{seconds} seconds") } else { format!("{} minutes", seconds.div_ceil(60)) };
        Err(format!("SIGN_IN_WAIT: too many wrong tries; try again in {when}"))
    }

    /// Records a wrong try.
    pub fn failed(&self, file: &str, who: &str, now: SystemTime) {
        let mut counts = self.counts.lock().expect("sign-in waits");
        let now_secs = secs(now);
        counts.retain(|_, c| now_secs.saturating_sub(c.last) < FORGET_AFTER.as_secs());
        let count = counts.entry(Self::key(file, who)).or_default();
        count.failures = count.failures.saturating_add(1);
        count.last = now_secs;
        self.save(&counts);
    }

    /// Clears the count after a successful try.
    pub fn succeeded(&self, file: &str, who: &str) {
        let mut counts = self.counts.lock().expect("sign-in waits");
        if counts.remove(&Self::key(file, who)).is_some() {
            self.save(&counts);
        }
    }

    /// Runs `attempt` under the throttle: refused while a wait runs, counted when it fails with an
    /// error `counts` accepts (a wrong password or key, not a rule about the new password), and
    /// clearing the count when it succeeds.
    pub async fn attempt<T, F>(&self, file: &str, who: &str, counts: impl Fn(&str) -> bool, attempt: F) -> Result<T, String>
    where
        F: std::future::Future<Output = Result<T, String>>,
    {
        self.check(file, who, SystemTime::now())?;
        match attempt.await {
            Ok(value) => {
                self.succeeded(file, who);
                Ok(value)
            }
            Err(e) => {
                if counts(&e) {
                    self.failed(file, who, SystemTime::now());
                }
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests;
