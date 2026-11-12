//! Help › Reinstall Previous Version (gap R13): goes back to the release before this one, through
//! the same signed updater as a normal update, without putting the books at risk.
//!
//! 1. The previous release is the newest published (not draft, not pre-release) GitHub release
//!    older than this version.
//! 2. An older app can open a company file only if it knows every schema migration the file has.
//!    The migrations that release shipped are listed from the repository at its tag; when the open
//!    company file has one it doesn't know, reinstalling is refused, with the reason.
//! 3. Before anything is installed, the open company file is backed up (a manual snapshot).
//! 4. The release is installed by the updater from that release's own `latest.json`, so its
//!    signature is checked against the key built into this app, exactly as for an update.
//!
//! Network use is only what the user asks for here: GitHub's release list, the migration list at
//! the tag, and the release itself.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::db_err;

/// The repository releases come from (as `plugins.updater.endpoints` in tauri.conf.json).
pub const REPO: &str = "turbogizzmo/LocalBooks";

/// `major.minor.patch` from "v0.12.3" or "0.12.3"; None for anything else (pre-releases included).
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let v = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
    parts.next().is_none().then_some(v)
}

/// A release as GitHub lists it (only the fields used).
#[derive(Debug, Clone, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub published_at: Option<String>,
}

/// The release to go back to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviousRelease {
    pub version: String,
    pub tag: String,
    pub published_at: Option<String>,
}

/// The newest published release older than `current`.
pub fn previous_release(releases: &[GithubRelease], current: &str) -> Option<PreviousRelease> {
    let now = parse_version(current)?;
    releases
        .iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| parse_version(&r.tag_name).filter(|v| *v < now).map(|v| (v, r)))
        .max_by_key(|(v, _)| *v)
        .map(|((a, b, c), r)| PreviousRelease { version: format!("{a}.{b}.{c}"), tag: r.tag_name.clone(), published_at: r.published_at.clone() })
}

/// Migration versions from file names such as `0094_payment_reminders.sql`.
pub fn migration_versions<'a>(names: impl IntoIterator<Item = &'a str>) -> BTreeSet<i64> {
    names.into_iter().filter(|n| n.ends_with(".sql")).filter_map(|n| n.split_once('_').and_then(|(num, _)| num.parse::<i64>().ok())).collect()
}

/// The migrations the open company file has that `known` (the older release's) doesn't.
pub async fn unknown_migrations(pool: &SqlitePool, known: &BTreeSet<i64>) -> Result<Vec<i64>, String> {
    let applied: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM _sqlx_migrations WHERE success = 1 ORDER BY version").fetch_all(pool).await.map_err(db_err)?;
    Ok(applied.into_iter().filter(|v| !known.contains(v)).collect())
}

/// Why the previous version can't open the company file, or None when it can.
pub fn schema_blocker(previous: &PreviousRelease, unknown: &[i64]) -> Option<String> {
    let first = unknown.first()?;
    Some(format!(
        "ROLLBACK_SCHEMA: Tillhouse {} can't open this company file: this version updated it ({} change{} that version doesn't have, starting with {first:04}). \
         Reinstalling it would leave these books unreadable. To go back, restore a backup taken before this version opened the file (Settings › File & Data), \
         or keep this version.",
        previous.version,
        unknown.len(),
        if unknown.len() == 1 { "" } else { "s" }
    ))
}

// ------------------------------------------------------------------------------------------------
// GitHub
// ------------------------------------------------------------------------------------------------

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(concat!("Tillhouse/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("ROLLBACK_NETWORK: {e}"))
}

async fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    let response = client()?
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("ROLLBACK_NETWORK: can't reach GitHub ({e}). Check the internet connection and try again"))?;
    if !response.status().is_success() {
        return Err(format!("ROLLBACK_NETWORK: GitHub answered {} for {url}", response.status()));
    }
    response.json().await.map_err(|e| format!("ROLLBACK_NETWORK: unexpected answer from GitHub ({e})"))
}

/// The published releases (newest first, as GitHub lists them).
pub async fn fetch_releases() -> Result<Vec<GithubRelease>, String> {
    get_json(&format!("https://api.github.com/repos/{REPO}/releases?per_page=50")).await
}

/// The schema migrations the release at `tag` shipped.
pub async fn fetch_migrations(tag: &str) -> Result<BTreeSet<i64>, String> {
    #[derive(Deserialize)]
    struct Entry {
        name: String,
    }
    let entries: Vec<Entry> = get_json(&format!("https://api.github.com/repos/{REPO}/contents/src-tauri/migrations?ref={tag}")).await?;
    let versions = migration_versions(entries.iter().map(|e| e.name.as_str()));
    if versions.is_empty() {
        return Err(format!("ROLLBACK_NETWORK: no migrations listed for {tag}"));
    }
    Ok(versions)
}

/// The previous release's own update manifest, which the updater reads and verifies.
pub fn manifest_url(tag: &str) -> String {
    format!("https://github.com/{REPO}/releases/download/{tag}/latest.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    fn release(tag: &str, draft: bool, prerelease: bool) -> GithubRelease {
        GithubRelease { tag_name: tag.into(), draft, prerelease, published_at: Some("2026-11-07T16:57:26Z".into()) }
    }

    #[test]
    fn the_previous_release_is_the_newest_published_one_before_this() {
        let releases = [
            release("v0.14.0", true, false),
            release("v0.13.1-beta", false, true),
            release("v0.13.0", false, false),
            release("v0.12.0", false, false),
            release("v0.10.3", false, false),
            release("nightly", false, false),
        ];
        assert_eq!(previous_release(&releases, "0.13.1").map(|r| r.tag), Some("v0.13.0".into()));
        assert_eq!(previous_release(&releases, "0.13.0").map(|r| r.version), Some("0.12.0".into()), "drafts and pre-releases are skipped");
        assert_eq!(previous_release(&releases, "0.10.3"), None, "nothing older");
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert!(manifest_url("v0.12.0").ends_with("/releases/download/v0.12.0/latest.json"));
    }

    #[tokio::test]
    async fn a_company_file_with_migrations_the_older_release_lacks_is_refused() {
        let pool = test_pool().await;
        let applied: BTreeSet<i64> = sqlx::query_scalar("SELECT version FROM _sqlx_migrations").fetch_all(&pool).await.unwrap().into_iter().collect();
        let newest = *applied.iter().max().unwrap();
        let previous = PreviousRelease { version: "0.12.0".into(), tag: "v0.12.0".into(), published_at: None };

        // The older release knew every migration: nothing stands in the way.
        let names: Vec<String> = applied.iter().map(|v| format!("{v:04}_x.sql")).chain(["README.md".to_string()]).collect();
        let known = migration_versions(names.iter().map(String::as_str));
        assert_eq!(known, applied);
        assert!(unknown_migrations(&pool, &known).await.unwrap().is_empty());
        assert_eq!(schema_blocker(&previous, &[]), None);

        // It lacked the newest: reinstalling is refused, and the message says why.
        let mut older = known.clone();
        older.remove(&newest);
        let unknown = unknown_migrations(&pool, &older).await.unwrap();
        assert_eq!(unknown, [newest]);
        let reason = schema_blocker(&previous, &unknown).unwrap();
        assert!(reason.starts_with("ROLLBACK_SCHEMA") && reason.contains("0.12.0") && reason.contains(&format!("{newest:04}")), "{reason}");
    }
}
