//! File Dentist: a check-up of the live company file, and a rebuild that repairs what can be
//! repaired safely.
//!
//! The check-up only reads. It runs SQLite's full `integrity_check` and `foreign_key_check`,
//! confirms the schema steps match this build, compares every index, trigger and view with the
//! definitions this build creates (the triggers are what enforce double-entry, so a missing one
//! matters), finds unbalanced entries, replays the posting snapshots
//! ([`ledger::verify_ledger_audit`]), and recomputes the stored totals that triggers normally keep
//! in step (amounts paid on invoices and bills, stock on hand).
//!
//! The rebuild refuses a file that fails `integrity_check` (that needs a backup restored, not a
//! rebuild). Otherwise it saves a verified `pre-rebuild-*.db` snapshot, then recreates missing or
//! altered indexes, triggers and views from this build's definitions and drops triggers this build
//! does not define, all in one transaction. It finishes with `REINDEX`, `ANALYZE`,
//! `PRAGMA optimize` and `VACUUM`. Problems in the data itself (broken links, unbalanced or changed
//! entries, stored totals that disagree) are reported but never rewritten here: those come from
//! edits made outside LocalBooks, and the fix is a reviewed correction or a restore.

use std::collections::BTreeMap;
use std::str::FromStr;

use serde::Serialize;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Connection, SqlitePool};

use crate::backup::{self, BackupFile};
use crate::db::{self, Db, MIGRATOR};
use crate::ledger;

/// How many example rows a finding lists; the count is always complete.
const EXAMPLES: usize = 20;
/// Free pages above this share of the file are worth reclaiming with a rebuild.
const FREE_PAGE_WARNING_PERCENT: i64 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckStatus {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    /// Stable identifier (`INTEGRITY`, `SCHEMA`, `STRUCTURE`, `LINKS`, `BALANCE`, `AUDIT`, `TOTALS`, `SPACE`).
    pub id: &'static str,
    pub title: &'static str,
    pub status: CheckStatus,
    pub summary: String,
    /// Up to [`EXAMPLES`] specifics.
    pub details: Vec<String>,
    /// True when Rebuild repairs this problem.
    pub fixed_by_rebuild: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckupReport {
    pub status: CheckStatus,
    pub checks: Vec<CheckResult>,
    pub size_bytes: u64,
    pub checked_at: String,
    /// False when the file is damaged and Rebuild would refuse.
    pub can_rebuild: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RebuildResult {
    pub safety_backup: BackupFile,
    pub actions: Vec<String>,
    pub size_before: u64,
    pub size_after: u64,
    pub report: CheckupReport,
}

fn check(id: &'static str, title: &'static str, status: CheckStatus, summary: impl Into<String>, details: Vec<String>, fixed_by_rebuild: bool) -> CheckResult {
    CheckResult { id, title, status, summary: summary.into(), details, fixed_by_rebuild }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn err(e: sqlx::Error) -> String {
    format!("DENTIST_QUERY: {e}")
}

// ------------------------------------------------------------------------------------------------
// Schema objects compared with this build's definitions
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
struct SchemaObject {
    kind: String,
    name: String,
    sql: String,
}

/// What differs between the company file's schema and this build's.
#[derive(Debug, Default)]
struct SchemaDrift {
    missing_tables: Vec<String>,
    /// Indexes, triggers and views to (re)create from the reference, in creation order.
    recreate: Vec<SchemaObject>,
    /// Of `recreate`, the ones that exist in the file with different SQL.
    altered: Vec<String>,
    /// Triggers this build does not define.
    extra_triggers: Vec<String>,
}

/// SQL compared without case, quoting or whitespace differences, which SQLite's table renames
/// can introduce when older files were upgraded by an older SQLite.
fn normalize(sql: &str) -> String {
    sql.chars()
        .filter(|c| !matches!(c, '"' | '`' | '[' | ']'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

async fn schema_objects(pool: &SqlitePool) -> Result<Vec<SchemaObject>, String> {
    let rows: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT type, name, sql FROM sqlite_schema
         WHERE sql IS NOT NULL AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations'
         ORDER BY rowid",
    )
    .fetch_all(pool)
    .await
    .map_err(err)?;
    Ok(rows.into_iter().map(|(kind, name, sql)| SchemaObject { kind, name, sql }).collect())
}

/// The schema a new company file gets from this build, built in memory.
async fn reference_schema() -> Result<Vec<SchemaObject>, String> {
    let options = SqliteConnectOptions::from_str("sqlite::memory:").map_err(|e| e.to_string())?.foreign_keys(true);
    let pool = SqlitePoolOptions::new().max_connections(1).connect_with(options).await.map_err(|e| e.to_string())?;
    let built = match db::migrate(&pool).await {
        Ok(()) => schema_objects(&pool).await,
        Err(e) => Err(format!("DENTIST_REFERENCE: {e}")),
    };
    pool.close().await;
    built
}

async fn schema_drift(pool: &SqlitePool) -> Result<SchemaDrift, String> {
    let reference = reference_schema().await?;
    let live: BTreeMap<(String, String), String> =
        schema_objects(pool).await?.into_iter().map(|o| ((o.kind, o.name), normalize(&o.sql))).collect();

    let mut drift = SchemaDrift::default();
    for object in &reference {
        match live.get(&(object.kind.clone(), object.name.clone())) {
            None if object.kind == "table" => drift.missing_tables.push(object.name.clone()),
            None => drift.recreate.push(object.clone()),
            Some(sql) if object.kind != "table" && *sql != normalize(&object.sql) => {
                drift.altered.push(object.name.clone());
                drift.recreate.push(object.clone());
            }
            Some(_) => {}
        }
    }
    drift.extra_triggers = live
        .keys()
        .filter(|(kind, name)| kind == "trigger" && !reference.iter().any(|o| o.kind == "trigger" && &o.name == name))
        .map(|(_, name)| name.clone())
        .collect();
    Ok(drift)
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

// ------------------------------------------------------------------------------------------------
// Check-up
// ------------------------------------------------------------------------------------------------

async fn check_integrity(pool: &SqlitePool) -> Result<CheckResult, String> {
    let rows: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check").fetch_all(pool).await.map_err(err)?;
    Ok(if rows.len() == 1 && rows[0] == "ok" {
        check("INTEGRITY", "File structure", CheckStatus::Pass, "SQLite's full integrity check found no damage.", vec![], false)
    } else {
        check(
            "INTEGRITY",
            "File structure",
            CheckStatus::Fail,
            "The file is damaged. Restore the most recent backup that passes this check; Rebuild cannot repair damage at this level.",
            rows.into_iter().take(EXAMPLES).collect(),
            false,
        )
    })
}

async fn check_schema_steps(pool: &SqlitePool) -> Result<CheckResult, String> {
    let applied: Vec<(i64, Vec<u8>, bool)> =
        sqlx::query_as("SELECT version, checksum, success FROM _sqlx_migrations ORDER BY version").fetch_all(pool).await.map_err(err)?;
    let mut problems = Vec::new();
    for migration in MIGRATOR.iter() {
        match applied.iter().find(|a| a.0 == migration.version) {
            None => problems.push(format!("Step {} ({}) was never applied", migration.version, migration.description)),
            Some((_, _, false)) => problems.push(format!("Step {} ({}) never finished", migration.version, migration.description)),
            Some((_, checksum, _)) if checksum.as_slice() != migration.checksum.as_ref() => {
                problems.push(format!("Step {} ({}) does not match this build; it may have been edited", migration.version, migration.description))
            }
            Some(_) => {}
        }
    }
    let latest = MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0);
    Ok(if problems.is_empty() {
        check("SCHEMA", "Schema version", CheckStatus::Pass, format!("All {} schema steps match this build (v{latest}).", MIGRATOR.iter().count()), vec![], false)
    } else {
        let summary = format!("{} out of step with this build. Restore a backup.", plural(problems.len(), "schema step is", "schema steps are"));
        check("SCHEMA", "Schema version", CheckStatus::Fail, summary, problems, false)
    })
}

fn check_structure(drift: &SchemaDrift) -> CheckResult {
    if !drift.missing_tables.is_empty() {
        let summary = format!("{} missing. Restore a backup; Rebuild does not recreate tables.", plural(drift.missing_tables.len(), "table is", "tables are"));
        let details = drift.missing_tables.iter().take(EXAMPLES).map(|t| format!("Missing table {t}")).collect();
        return check("STRUCTURE", "Safeguards and indexes", CheckStatus::Fail, summary, details, false);
    }
    let missing: Vec<&SchemaObject> = drift.recreate.iter().filter(|o| !drift.altered.contains(&o.name)).collect();
    if missing.is_empty() && drift.altered.is_empty() && drift.extra_triggers.is_empty() {
        return check("STRUCTURE", "Safeguards and indexes", CheckStatus::Pass, "Every index, trigger and view matches this build.", vec![], false);
    }
    let mut details: Vec<String> = missing.iter().map(|o| format!("Missing {} {}", o.kind, o.name)).collect();
    details.extend(drift.altered.iter().map(|n| format!("Changed {n}")));
    details.extend(drift.extra_triggers.iter().map(|n| format!("Unknown trigger {n}")));
    let count = details.len();
    details.truncate(EXAMPLES);
    let summary = format!(
        "{} differ from this build. Triggers are what enforce double-entry rules, so Rebuild restores them.",
        plural(count, "database safeguard or index", "database safeguards or indexes")
    );
    check("STRUCTURE", "Safeguards and indexes", CheckStatus::Fail, summary, details, true)
}

async fn check_links(pool: &SqlitePool) -> Result<CheckResult, String> {
    let rows: Vec<(String, i64)> = sqlx::query_as("SELECT \"table\", COUNT(*) FROM pragma_foreign_key_check GROUP BY \"table\" ORDER BY \"table\"")
        .fetch_all(pool)
        .await
        .map_err(err)?;
    let total: i64 = rows.iter().map(|r| r.1).sum();
    Ok(if total == 0 {
        check("LINKS", "Record links", CheckStatus::Pass, "Every record points to data that exists.", vec![], false)
    } else {
        check(
            "LINKS",
            "Record links",
            CheckStatus::Fail,
            format!("{} point to data that no longer exists. Restore a backup taken before this happened.", plural(total as usize, "record", "records")),
            rows.into_iter().take(EXAMPLES).map(|(table, n)| format!("{table}: {n}")).collect(),
            false,
        )
    })
}

async fn check_balance(pool: &SqlitePool) -> Result<CheckResult, String> {
    let rows: Vec<(i64, Option<i64>, i64)> = sqlx::query_as(
        "SELECT e.id, e.entry_number, SUM(l.debit_cents) - SUM(l.credit_cents)
         FROM journal_entries e JOIN journal_lines l ON l.journal_entry_id = e.id
         WHERE e.status <> 'DRAFT'
         GROUP BY e.id HAVING SUM(l.debit_cents) <> SUM(l.credit_cents)
         ORDER BY e.id",
    )
    .fetch_all(pool)
    .await
    .map_err(err)?;
    let (debits, credits): (i64, i64) =
        sqlx::query_as("SELECT COALESCE(SUM(debit_cents), 0), COALESCE(SUM(credit_cents), 0) FROM v_posted_lines").fetch_one(pool).await.map_err(err)?;
    Ok(if rows.is_empty() && debits == credits {
        check("BALANCE", "Debits equal credits", CheckStatus::Pass, "Every posted entry balances, and so does the trial balance.", vec![], false)
    } else {
        let details = rows
            .iter()
            .take(EXAMPLES)
            .map(|(id, number, diff)| format!("Entry {} is off by {} cents", number.map_or(format!("id {id}"), |n| format!("#{n}")), diff))
            .collect();
        let summary = format!("{} out of balance. Review them on the Audit Log or restore a backup.", plural(rows.len().max(1), "posted entry is", "posted entries are"));
        check("BALANCE", "Debits equal credits", CheckStatus::Fail, summary, details, false)
    })
}

async fn check_audit(pool: &SqlitePool) -> Result<CheckResult, String> {
    let verification = ledger::verify_ledger_audit(pool).await?;
    Ok(if verification.intact {
        check(
            "AUDIT",
            "Posting history",
            CheckStatus::Pass,
            format!("{} match the snapshots taken when they were posted.", plural(verification.entries_checked as usize, "posted entry", "posted entries")),
            vec![],
            false,
        )
    } else {
        let summary = format!(
            "{} changed after posting. Accounting › Audit Log shows each one and the correcting entry that would restore it.",
            plural(verification.findings.len(), "entry was", "entries were")
        );
        let details = verification
            .findings
            .iter()
            .take(EXAMPLES)
            .map(|f| format!("Entry {}: {}", f.entry_number.map_or(format!("id {}", f.entry_id), |n| format!("#{n}")), f.problem))
            .collect();
        check("AUDIT", "Posting history", CheckStatus::Fail, summary, details, false)
    })
}

async fn check_stored_totals(pool: &SqlitePool) -> Result<CheckResult, String> {
    let checks: [(&str, &str); 3] = [
        (
            "Invoice",
            "SELECT CAST(i.invoice_number AS TEXT), i.amount_paid_cents,
                    COALESCE((SELECT SUM(amount_cents) FROM invoice_payments WHERE invoice_id = i.id AND is_voided = 0), 0)
                  + COALESCE((SELECT SUM(amount_cents) FROM credit_applications WHERE invoice_id = i.id AND is_voided = 0), 0) AS expected
             FROM invoices i WHERE i.amount_paid_cents <> expected ORDER BY i.id",
        ),
        (
            "Bill",
            "SELECT COALESCE(b.bill_number, 'id ' || b.id), b.amount_paid_cents,
                    COALESCE((SELECT SUM(amount_cents) FROM bill_payments WHERE bill_id = b.id AND is_voided = 0), 0)
                  + COALESCE((SELECT SUM(amount_cents) FROM vendor_credit_applications WHERE bill_id = b.id AND is_voided = 0), 0) AS expected
             FROM bills b WHERE b.amount_paid_cents <> expected ORDER BY b.id",
        ),
        (
            "Stock on hand for item",
            "SELECT COALESCE(i.sku, 'id ' || i.id), i.qty_on_hand_e4, COALESCE(s.qty_after_e4, 0) AS expected
             FROM inventory_items i
             LEFT JOIN stock_ledger_entries s ON s.id = (SELECT MAX(id) FROM stock_ledger_entries WHERE item_id = i.id)
             WHERE i.qty_on_hand_e4 <> COALESCE(s.qty_after_e4, 0)
                OR i.value_cents <> COALESCE(s.value_after_cents, 0)
             ORDER BY i.id",
        ),
    ];
    let mut details = Vec::new();
    let mut count = 0usize;
    for (label, sql) in checks {
        let rows: Vec<(String, i64, i64)> = sqlx::query_as(sql).fetch_all(pool).await.map_err(err)?;
        count += rows.len();
        details.extend(rows.into_iter().map(|(name, stored, expected)| format!("{label} {name}: stored {stored}, records add up to {expected}")));
    }
    details.truncate(EXAMPLES);
    Ok(if count == 0 {
        check("TOTALS", "Stored totals", CheckStatus::Pass, "Amounts paid on invoices and bills, and stock on hand, agree with their records.", vec![], false)
    } else {
        let summary = format!(
            "{} disagree with the records behind them. LocalBooks never writes these out of step, so the file was edited elsewhere; restore a backup.",
            plural(count, "stored total", "stored totals")
        );
        check("TOTALS", "Stored totals", CheckStatus::Fail, summary, details, false)
    })
}

async fn check_space(pool: &SqlitePool) -> Result<CheckResult, String> {
    let (pages, free, page_size): (i64, i64, i64) =
        sqlx::query_as("SELECT (SELECT page_count FROM pragma_page_count), (SELECT freelist_count FROM pragma_freelist_count), (SELECT page_size FROM pragma_page_size)")
            .fetch_one(pool)
            .await
            .map_err(err)?;
    let percent = if pages > 0 { free * 100 / pages } else { 0 };
    let reclaimable = free * page_size / 1024;
    Ok(if percent > FREE_PAGE_WARNING_PERCENT {
        check("SPACE", "Unused space", CheckStatus::Warn, format!("{percent}% of the file ({reclaimable} KB) is unused. Rebuild compacts it."), vec![], true)
    } else {
        check("SPACE", "Unused space", CheckStatus::Pass, format!("{percent}% of the file is unused ({reclaimable} KB)."), vec![], false)
    })
}

/// Runs every check on `pool`. Nothing is written.
pub async fn checkup(pool: &SqlitePool, size_bytes: u64) -> Result<CheckupReport, String> {
    let integrity = check_integrity(pool).await?;
    let can_rebuild = integrity.status == CheckStatus::Pass;
    let mut checks = vec![integrity];
    // A damaged file can make every later query fail or mislead, so stop at the first check.
    if can_rebuild {
        checks.push(check_schema_steps(pool).await?);
        checks.push(check_structure(&schema_drift(pool).await?));
        checks.push(check_links(pool).await?);
        checks.push(check_balance(pool).await?);
        checks.push(check_audit(pool).await?);
        checks.push(check_stored_totals(pool).await?);
        checks.push(check_space(pool).await?);
    }
    let status = checks.iter().map(|c| c.status).max().unwrap_or(CheckStatus::Pass);
    let checked_at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    Ok(CheckupReport { status, checks, size_bytes, checked_at, can_rebuild })
}

fn file_size(path: &std::path::Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Check-up of the live company file.
pub async fn checkup_live(db: &Db) -> Result<CheckupReport, String> {
    checkup(&db.pool().await, file_size(&db.path().await?)).await
}

// ------------------------------------------------------------------------------------------------
// Rebuild
// ------------------------------------------------------------------------------------------------

/// Restores this build's indexes, triggers and views in one transaction, then compacts the file.
/// Returns what was done.
pub async fn rebuild_pool(pool: &SqlitePool) -> Result<Vec<String>, String> {
    let drift = schema_drift(pool).await?;
    if !drift.missing_tables.is_empty() {
        return Err(format!("REBUILD_MISSING_TABLES: {} missing; restore a backup instead", plural(drift.missing_tables.len(), "table is", "tables are")));
    }

    let mut actions = Vec::new();
    let mut conn = pool.acquire().await.map_err(|e| e.to_string())?;
    if !drift.recreate.is_empty() || !drift.extra_triggers.is_empty() {
        let mut tx = conn.begin().await.map_err(err)?;
        for name in &drift.extra_triggers {
            sqlx::query(&format!("DROP TRIGGER {}", quote_ident(name))).execute(&mut *tx).await.map_err(err)?;
            actions.push(format!("Removed unknown trigger {name}"));
        }
        // Drop altered objects first (triggers before the views they may read), then create
        // everything in the order this build created it.
        for kind in ["trigger", "view", "index"] {
            for object in drift.recreate.iter().filter(|o| o.kind == kind && drift.altered.contains(&o.name)) {
                sqlx::query(&format!("DROP {} {}", kind.to_uppercase(), quote_ident(&object.name))).execute(&mut *tx).await.map_err(err)?;
            }
        }
        for object in &drift.recreate {
            sqlx::query(&object.sql)
                .execute(&mut *tx)
                .await
                .map_err(|e| format!("REBUILD_RECREATE: could not restore {} {} ({e}); nothing was changed", object.kind, object.name))?;
            let verb = if drift.altered.contains(&object.name) { "Restored changed" } else { "Recreated missing" };
            actions.push(format!("{verb} {} {}", object.kind, object.name));
        }
        tx.commit().await.map_err(err)?;
    }

    for (sql, done) in [
        ("REINDEX", "Rebuilt every index"),
        ("ANALYZE", "Refreshed query statistics"),
        ("PRAGMA optimize", "Optimized queries"),
        ("VACUUM", "Compacted the file"),
    ] {
        sqlx::query(sql).execute(&mut *conn).await.map_err(|e| format!("REBUILD_MAINTENANCE: {sql} failed ({e})"))?;
        actions.push(done.into());
    }
    Ok(actions)
}

/// Saves a verified snapshot, rebuilds the live file, and checks it again.
pub async fn rebuild_live(db: &Db) -> Result<RebuildResult, String> {
    let pool = db.pool().await;
    let path = db.path().await?;
    let size_before = file_size(&path);
    if check_integrity(&pool).await?.status != CheckStatus::Pass {
        return Err("REBUILD_CORRUPT: the file is damaged at a level Rebuild cannot repair; restore a backup instead".into());
    }
    let safety_backup = backup::pre_rebuild_snapshot(db).await?;
    let actions = rebuild_pool(&pool).await?;
    let size_after = file_size(&path);
    let report = checkup(&pool, size_after).await?;
    Ok(RebuildResult { safety_backup, actions, size_before, size_after, report })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};
    use chrono::NaiveDate;

    async fn account(pool: &SqlitePool, number: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
    }

    async fn post_sale(pool: &SqlitePool, cents: i64) -> i64 {
        let (checking, sales) = (account(pool, "1010").await, account(pool, "4000").await);
        post_journal_entry(
            pool,
            JournalEntry {
                id: None,
                date: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
                description: "Sale".into(),
                lines: vec![JournalLine::debit(checking, cents), JournalLine::credit(sales, cents)],
                currency: None,
            },
        )
        .await
        .unwrap()
    }

    fn result<'a>(report: &'a CheckupReport, id: &str) -> &'a CheckResult {
        report.checks.iter().find(|c| c.id == id).unwrap_or_else(|| panic!("no {id} check"))
    }

    #[tokio::test]
    async fn a_fresh_company_file_passes_every_check() {
        let pool = test_pool().await;
        post_sale(&pool, 12_500).await;
        let report = checkup(&pool, 0).await.unwrap();
        let failing: Vec<_> = report.checks.iter().filter(|c| c.status != CheckStatus::Pass).collect();
        assert!(failing.is_empty(), "{failing:#?}");
        assert_eq!(report.status, CheckStatus::Pass);
        assert!(report.can_rebuild);
        assert_eq!(report.checks.len(), 8);
    }

    #[tokio::test]
    async fn rebuild_restores_a_dropped_safeguard_trigger_and_a_changed_index() {
        let pool = test_pool().await;
        let trigger: String = sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type = 'trigger' ORDER BY name LIMIT 1").fetch_one(&pool).await.unwrap();
        let (index, table): (String, String) =
            sqlx::query_as("SELECT name, tbl_name FROM sqlite_schema WHERE type = 'index' AND sql IS NOT NULL ORDER BY name LIMIT 1").fetch_one(&pool).await.unwrap();
        sqlx::query(&format!("DROP TRIGGER {}", quote_ident(&trigger))).execute(&pool).await.unwrap();
        sqlx::query(&format!("DROP INDEX {}", quote_ident(&index))).execute(&pool).await.unwrap();
        sqlx::query(&format!("CREATE INDEX {} ON {} (id)", quote_ident(&index), quote_ident(&table))).execute(&pool).await.unwrap();
        sqlx::query("CREATE TRIGGER sneaky AFTER INSERT ON journal_entries BEGIN SELECT 1; END").execute(&pool).await.unwrap();

        let before = checkup(&pool, 0).await.unwrap();
        let structure = result(&before, "STRUCTURE");
        assert_eq!(structure.status, CheckStatus::Fail);
        assert!(structure.fixed_by_rebuild);
        assert!(structure.details.contains(&format!("Missing trigger {trigger}")), "{:?}", structure.details);
        assert!(structure.details.contains(&format!("Changed {index}")), "{:?}", structure.details);
        assert!(structure.details.contains(&"Unknown trigger sneaky".to_string()), "{:?}", structure.details);

        let actions = rebuild_pool(&pool).await.unwrap();
        assert!(actions.contains(&format!("Recreated missing trigger {trigger}")), "{actions:?}");
        assert!(actions.contains(&format!("Restored changed index {index}")), "{actions:?}");
        assert!(actions.contains(&"Removed unknown trigger sneaky".to_string()), "{actions:?}");
        assert!(actions.contains(&"Compacted the file".to_string()));

        let after = checkup(&pool, 0).await.unwrap();
        assert_eq!(after.status, CheckStatus::Pass, "{:#?}", after.checks);
    }

    #[tokio::test]
    async fn rebuilding_a_healthy_file_changes_no_data() {
        let pool = test_pool().await;
        post_sale(&pool, 4_200).await;
        let count = |pool: SqlitePool| async move {
            sqlx::query_scalar::<_, i64>("SELECT (SELECT COUNT(*) FROM journal_lines) * 1000 + (SELECT COUNT(*) FROM ledger_audit_log)").fetch_one(&pool).await.unwrap()
        };
        let before = count(pool.clone()).await;
        let actions = rebuild_pool(&pool).await.unwrap();
        assert_eq!(actions, ["Rebuilt every index", "Refreshed query statistics", "Optimized queries", "Compacted the file"]);
        assert_eq!(count(pool.clone()).await, before);
    }

    /// Runs `sql` with `table`'s triggers removed, the way a tool editing the file outside
    /// LocalBooks would, then puts them back.
    async fn edit_behind_safeguards(pool: &SqlitePool, table: &str, sql: &str, bind: i64) {
        let triggers: Vec<(String, String)> =
            sqlx::query_as("SELECT name, sql FROM sqlite_schema WHERE type = 'trigger' AND tbl_name = ?1").bind(table).fetch_all(pool).await.unwrap();
        for (name, _) in &triggers {
            sqlx::query(&format!("DROP TRIGGER {}", quote_ident(name))).execute(pool).await.unwrap();
        }
        sqlx::query(sql).bind(bind).execute(pool).await.unwrap();
        for (_, sql) in &triggers {
            sqlx::query(sql).execute(pool).await.unwrap();
        }
    }

    #[tokio::test]
    async fn an_entry_edited_outside_localbooks_fails_balance_and_history_but_is_not_rewritten() {
        let pool = test_pool().await;
        let entry = post_sale(&pool, 10_000).await;
        edit_behind_safeguards(&pool, "journal_lines", "UPDATE journal_lines SET debit_cents = 9000 WHERE journal_entry_id = ?1 AND debit_cents > 0", entry).await;

        let report = checkup(&pool, 0).await.unwrap();
        assert_eq!(result(&report, "BALANCE").status, CheckStatus::Fail);
        assert!(!result(&report, "BALANCE").fixed_by_rebuild);
        assert_eq!(result(&report, "AUDIT").status, CheckStatus::Fail);
        assert_eq!(result(&report, "STRUCTURE").status, CheckStatus::Pass);

        rebuild_pool(&pool).await.unwrap();
        let debit: i64 = sqlx::query_scalar("SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = ?1").bind(entry).fetch_one(&pool).await.unwrap();
        assert_eq!(debit, 9000, "rebuild never rewrites ledger data");
    }

    #[tokio::test]
    async fn an_invoice_paid_total_out_of_step_with_its_payments_is_reported() {
        use crate::invoices::{self, InvoiceInput, InvoiceLineInput, PaymentInput};
        let pool = test_pool().await;
        let day = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let invoice = invoices::create_and_post_invoice(
            &pool,
            InvoiceInput {
                customer_name: "Acme".into(),
                issue_date: day,
                due_date: day,
                lines: vec![InvoiceLineInput {
                    description: "Work".into(),
                    quantity_e4: 10_000,
                    rate_cents: 5_000,
                    income_account_id: account(&pool, "4000").await,
                    product_service_id: None,
                    class_id: None,
                    is_taxable: false,
                }],
                currency: None,
                tax_rate_id: None,
                discount_cents: 0,
            },
        )
        .await
        .unwrap();
        invoices::record_payment(
            &pool,
            PaymentInput {
                invoice_id: invoice.id,
                deposit_account_id: account(&pool, "1010").await,
                payment_date: day,
                amount_cents: 2_000,
                payment_method: "CHECK".into(),
                reference: None,
                exchange_rate_e6: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(result(&checkup(&pool, 0).await.unwrap(), "TOTALS").status, CheckStatus::Pass);

        edit_behind_safeguards(&pool, "invoice_payments", "UPDATE invoice_payments SET amount_cents = 1500 WHERE invoice_id = ?1", invoice.id).await;
        let report = checkup(&pool, 0).await.unwrap();
        let totals = result(&report, "TOTALS");
        assert_eq!(totals.status, CheckStatus::Fail);
        assert!(!totals.fixed_by_rebuild);
        assert_eq!(totals.details, [format!("Invoice {}: stored 2000, records add up to 1500", invoice.invoice_number)]);
    }
}
