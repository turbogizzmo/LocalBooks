//! Internal: the release check's company-file probe. Not a user feature and not in any menu.
//!
//! `LocalBooks --check-file <company file> [--backups-dir <folder>] [--output <file.json>]`
//!
//! Opens the company file without a window, the way a launch with that file does (the restore
//! checks, then the schema upgrade with its verified pre-upgrade copy), runs File Dentist's
//! check-up, prints a JSON result and exits: 0 when every check passes, 1 on any problem, 2 on bad
//! arguments. Nothing else runs: no window, no hand-off to a running copy, no automatic backup, no
//! MCP server, no audit-trail anchors in the keychain.
//!
//! The pre-upgrade copy goes to `--backups-dir`, by default a `localbooks-check-backups` folder
//! beside the company file (never the installed app's own backups). `--output` also writes the
//! JSON to a file, for Windows, where the release build has no console. Used by
//! `.github/workflows/release-check.yml`; see `docs/testing/release-check.md`.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::backup::{self, BackupFile, CompanyFileInfo};
use crate::db::{self, Db, MIGRATOR};
use crate::file_dentist::{self, CheckStatus, CheckupReport};

const FLAG: &str = "--check-file";

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckOutput {
    file: String,
    ok: bool,
    /// The newest schema step this build has.
    app_schema: i64,
    /// The file's schema step before it was opened, if it could be read.
    schema_before: Option<i64>,
    schema_after: Option<i64>,
    upgraded: bool,
    /// The verified copy saved before the upgrade (only when the file needed one).
    pre_upgrade_backup: Option<BackupFile>,
    checkup: Option<CheckupReport>,
    problems: Vec<String>,
}

struct Args {
    file: PathBuf,
    backups_dir: Option<PathBuf>,
    output: Option<PathBuf>,
}

/// When LocalBooks was started with `--check-file`, runs the check and returns the exit code;
/// otherwise `None` and the app starts as usual.
pub fn run_from_args() -> Option<i32> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    if !args.iter().any(|a| a == FLAG) {
        return None;
    }
    Some(match parse(args) {
        Ok(args) => {
            let output = tauri::async_runtime::block_on(check(&args.file, args.backups_dir.as_deref()));
            let json = serde_json::to_string_pretty(&output).expect("serializable check result");
            println!("{json}");
            if let Some(path) = &args.output {
                if let Err(e) = std::fs::write(path, format!("{json}\n")) {
                    eprintln!("cannot write {}: {e}", path.display());
                    return Some(1);
                }
            }
            if output.ok { 0 } else { 1 }
        }
        Err(message) => {
            eprintln!("{message}\nusage: LocalBooks --check-file <company file> [--backups-dir <folder>] [--output <file.json>]");
            2
        }
    })
}

fn parse(args: Vec<std::ffi::OsString>) -> Result<Args, String> {
    let (mut file, mut backups_dir, mut output) = (None, None, None);
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let slot = match arg.to_str() {
            Some(FLAG) => &mut file,
            Some("--backups-dir") => &mut backups_dir,
            Some("--output") => &mut output,
            _ => return Err(format!("unknown argument {}", arg.to_string_lossy())),
        };
        *slot = Some(PathBuf::from(args.next().ok_or_else(|| format!("{} needs a value", arg.to_string_lossy()))?));
    }
    Ok(Args { file: file.ok_or("--check-file needs a value")?, backups_dir, output })
}

async fn check(file: &Path, backups_dir: Option<&Path>) -> CheckOutput {
    let app_schema = MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0);
    let mut out = CheckOutput { file: file.display().to_string(), app_schema, ..Default::default() };
    let backups = backups_dir.map(Path::to_path_buf).unwrap_or_else(|| file.with_file_name("localbooks-check-backups"));
    match run(file, &backups, &mut out).await {
        Ok(()) => {}
        Err(e) => out.problems.push(e),
    }
    out.ok = out.problems.is_empty();
    out
}

async fn run(file: &Path, backups: &Path, out: &mut CheckOutput) -> Result<(), String> {
    let before: CompanyFileInfo = backup::inspect(file).await?;
    out.schema_before = Some(before.schema_version);
    let earlier: Vec<String> = backup::list_backups(backups)?.into_iter().map(|b| b.path).collect();

    // The same path as a launch with this file (and File › Open): restore checks, then open_live,
    // which saves the pre-upgrade copy and migrates.
    let config = db::DbConfig { config_dir: backups.to_path_buf(), default_path: backups.join("unused.localbooks"), backups_dir: backups.to_path_buf() };
    let database = Db::closed(config);
    database.mount(file).await?;
    let report = file_dentist::checkup_live(&database).await;
    database.shutdown().await;
    let report = report?;

    let after = backup::inspect(file).await?;
    out.schema_after = Some(after.schema_version);
    out.upgraded = after.schema_version > before.schema_version;
    if after.schema_version != out.app_schema {
        out.problems.push(format!("schema is at step {} after opening, expected {}", after.schema_version, out.app_schema));
    }

    let saved: Vec<BackupFile> = backup::list_backups(backups)?.into_iter().filter(|b| b.kind == "PRE_UPGRADE" && !earlier.contains(&b.path)).collect();
    match (before.needs_upgrade, saved.into_iter().next()) {
        (true, None) => out.problems.push("the file was upgraded but no pre-upgrade copy was saved".into()),
        (true, Some(copy)) => {
            match backup::inspect(Path::new(&copy.path)).await {
                Ok(info) if info.schema_version == before.schema_version && info.journal_entries == before.journal_entries => {}
                Ok(info) => out.problems.push(format!(
                    "the pre-upgrade copy has schema step {} and {} entries, expected {} and {}",
                    info.schema_version, info.journal_entries, before.schema_version, before.journal_entries
                )),
                Err(e) => out.problems.push(format!("the pre-upgrade copy does not open: {e}")),
            }
            out.pre_upgrade_backup = Some(copy);
        }
        (false, Some(copy)) => out.problems.push(format!("a pre-upgrade copy was saved for a file that needed no upgrade: {}", copy.path)),
        (false, None) => {}
    }

    // WARN is only ever SPACE (unused pages), which is housekeeping, not a problem with the books.
    for c in report.checks.iter().filter(|c| c.status == CheckStatus::Fail) {
        out.problems.push(format!("File Dentist {}: {}", c.id, c.summary));
    }
    out.checkup = Some(report);
    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    use super::*;

    fn os(args: &[&str]) -> Vec<std::ffi::OsString> {
        args.iter().map(Into::into).collect()
    }

    #[test]
    fn parses_the_flags_and_refuses_strays() {
        let args = parse(os(&["--check-file", "a.localbooks", "--output", "r.json"])).unwrap();
        assert_eq!(args.file, PathBuf::from("a.localbooks"));
        assert_eq!(args.output, Some(PathBuf::from("r.json")));
        assert!(args.backups_dir.is_none());
        assert!(parse(os(&["--check-file"])).is_err());
        assert!(parse(os(&["--check-file", "a", "b"])).is_err());
    }

    #[tokio::test]
    async fn upgrades_an_older_file_with_a_verified_copy_and_passes() {
        let dir = std::env::temp_dir().join(format!("lb-release-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Old.localbooks");
        // A company file as the previous release left it: every schema step but the newest.
        let latest = MIGRATOR.iter().map(|m| m.version).max().unwrap();
        let previous_version = MIGRATOR.iter().map(|m| m.version).filter(|v| *v < latest).max().unwrap();
        {
            let options = SqliteConnectOptions::new().filename(&file).create_if_missing(true).foreign_keys(true);
            let pool = SqlitePoolOptions::new().max_connections(1).connect_with(options).await.unwrap();
            let previous = sqlx::migrate::Migrator {
                migrations: MIGRATOR.iter().filter(|m| m.version < latest).cloned().collect::<Vec<_>>().into(),
                ..sqlx::migrate::Migrator::DEFAULT
            };
            db::migrate_with(&pool, &previous).await.unwrap();
            pool.close().await;
        }

        let out = check(&file, None).await;
        assert!(out.ok, "{:?}", out.problems);
        assert_eq!((out.schema_before, out.schema_after, out.upgraded), (Some(previous_version), Some(latest), true));
        let copy = out.pre_upgrade_backup.expect("a pre-upgrade copy");
        assert!(Path::new(&copy.path).starts_with(dir.join("localbooks-check-backups")), "{}", copy.path);

        // Opened again it is already current: no copy, still clean.
        let again = check(&file, None).await;
        assert!(again.ok && !again.upgraded && again.pre_upgrade_backup.is_none(), "{:?}", again.problems);

        // Not a company file: a problem, not a crash.
        std::fs::write(dir.join("junk.localbooks"), b"not sqlite").unwrap();
        let junk = check(&dir.join("junk.localbooks"), None).await;
        assert!(!junk.ok && junk.problems[0].starts_with("RESTORE_NOT_SQLITE"), "{:?}", junk.problems);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
