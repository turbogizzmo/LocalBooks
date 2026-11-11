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
//! - **Saved as hashes.** People sometimes type their password into the user-name box, so names
//!   are never written in clear: each count is filed under a SHA-256 of what it counts, and at
//!   most [`MAX_ENTRIES`] counts are kept (the oldest go first).
//! - **Counted before the check runs**, so tries started at the same moment can't all slip past
//!   the wait; a try that turns out right, or fails for another reason, gives its count back.
//! - A try made while a wait is running is refused without checking the password, and doesn't add
//!   to the count. A successful sign-in clears the user's count; counts untouched for a day expire.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Wrong tries allowed before any wait.
pub const FREE_TRIES: u32 = 4;
/// The longest wait, however many tries have failed.
pub const MAX_WAIT: Duration = Duration::from_secs(5 * 60);
/// A count with no failure for this long starts again from zero.
const FORGET_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
/// The most counts kept; enough for every user of a file many times over.
const MAX_ENTRIES: usize = 500;

/// What a count is for: a user name's sign-ins (and password checks), or the file's recovery key.
#[derive(Debug, Clone, Copy)]
pub enum Who<'a> {
    User(&'a str),
    RecoveryKey,
    /// A user name's two-step sign-in codes (gap R26), counted apart from its passwords, so a
    /// right password doesn't clear the count of wrong codes.
    Code(&'a str),
}
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

    /// A hash, so neither the file's id nor a name (or a password typed as one) is saved in clear.
    fn key(file: &str, who: Who) -> String {
        let what = match who {
            Who::User(name) => format!("user\n{file}\n{}", name.trim().to_lowercase()),
            Who::RecoveryKey => format!("recovery\n{file}"),
            Who::Code(name) => format!("code\n{file}\n{}", name.trim().to_lowercase()),
        };
        Sha256::digest(what.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
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
    pub fn check(&self, file: &str, who: Who, now: SystemTime) -> Result<(), String> {
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

    /// How many wrong tries `who` has made that still count.
    pub fn failures(&self, file: &str, who: Who) -> u32 {
        let counts = self.counts.lock().expect("sign-in waits");
        counts.get(&Self::key(file, who)).map(|c| c.failures).unwrap_or(0)
    }

    /// Records a wrong try.
    pub fn failed(&self, file: &str, who: Who, now: SystemTime) {
        let mut counts = self.counts.lock().expect("sign-in waits");
        let now_secs = secs(now);
        counts.retain(|_, c| now_secs.saturating_sub(c.last) < FORGET_AFTER.as_secs());
        while counts.len() >= MAX_ENTRIES {
            let Some(oldest) = counts.iter().min_by_key(|(_, c)| c.last).map(|(k, _)| k.clone()) else { break };
            counts.remove(&oldest);
        }
        let count = counts.entry(Self::key(file, who)).or_default();
        count.failures = count.failures.saturating_add(1);
        count.last = now_secs;
        self.save(&counts);
    }

    /// Clears the count after a successful try.
    pub fn succeeded(&self, file: &str, who: Who) {
        let mut counts = self.counts.lock().expect("sign-in waits");
        if counts.remove(&Self::key(file, who)).is_some() {
            self.save(&counts);
        }
    }

    /// Gives back a count taken by [`Self::failed`] for a try that turned out not to be a guess.
    fn give_back(&self, file: &str, who: Who) {
        let mut counts = self.counts.lock().expect("sign-in waits");
        let key = Self::key(file, who);
        if let Some(count) = counts.get_mut(&key) {
            count.failures = count.failures.saturating_sub(1);
            if count.failures == 0 {
                counts.remove(&key);
            }
            self.save(&counts);
        }
    }

    /// Runs `attempt` under the throttle: refused while a wait runs; counted as a failure before it
    /// runs, so simultaneous tries can't all pass the check; cleared when it succeeds; and given
    /// back when it fails with an error `counts` doesn't accept (a rule about the new password, not
    /// a wrong password or key).
    pub async fn attempt<T, F>(&self, file: &str, who: Who<'_>, counts: impl Fn(&str) -> bool, attempt: F) -> Result<T, String>
    where
        F: std::future::Future<Output = Result<T, String>>,
    {
        self.check(file, who, SystemTime::now())?;
        self.failed(file, who, SystemTime::now());
        match attempt.await {
            Ok(value) => {
                self.succeeded(file, who);
                Ok(value)
            }
            Err(e) => {
                if !counts(&e) {
                    self.give_back(file, who);
                }
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests;
