//! Classes: an optional second dimension on income and expense lines (department, location, line of
//! business), separate from the chart of accounts. Each journal line may carry one class, and the
//! Profit & Loss by Class report (in `statements`) splits every account across them.
//!
//! Classes nest as "Parent:Child" subclasses, up to five levels. A class that has been used on a
//! line can be renamed, moved or made inactive, but not deleted: posted lines keep their class
//! forever. Inactive classes stay on the lines that already use them but cannot be chosen again.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{db_err, post_entry_in_tx, rollback, EntrySource, JournalEntry, JournalLine};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ClassView {
    pub id: i64,
    pub name: String,
    pub full_name: String,
    pub parent_id: Option<i64>,
    pub depth: i64,
    pub is_active: bool,
    /// Used on at least one journal line or document line, so it can no longer be deleted.
    pub is_used: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassInput {
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<i64>,
}

/// Tables whose lines may carry a class.
const CLASS_USERS: &[&str] = &[
    "journal_lines", "invoice_items", "sales_receipt_lines", "estimate_lines", "bill_lines", "expense_lines",
    "deposit_lines", "bank_transactions", "time_activities",
];

fn is_used_sql() -> String {
    CLASS_USERS.iter().map(|t| format!("EXISTS (SELECT 1 FROM {t} WHERE class_id = c.id)")).collect::<Vec<_>>().join(" OR ")
}

/// Every class, ordered as a tree (each parent followed by its subclasses).
pub async fn list_classes(pool: &SqlitePool) -> Result<Vec<ClassView>, String> {
    sqlx::query_as::<_, ClassView>(&format!(
        "SELECT c.id, c.name, c.full_name, c.parent_id, c.depth, c.is_active, ({}) AS is_used
         FROM v_classes c ORDER BY replace(lower(c.full_name), ':', char(1))",
        is_used_sql()
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

async fn get_class(pool: &SqlitePool, id: i64) -> Result<ClassView, String> {
    list_classes(pool).await?.into_iter().find(|c| c.id == id).ok_or_else(|| format!("CLASS_NOT_FOUND: class {id} does not exist"))
}

fn clean_name(input: &ClassInput) -> Result<&str, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("CLASS_NAME_REQUIRED: enter a class name".into());
    }
    if name.contains(':') {
        return Err("CLASS_NAME_COLON: a class name cannot contain ':' (it separates a class from its subclass)".into());
    }
    Ok(name)
}

/// Turns the sibling-name unique index violation into a readable message.
fn name_err(err: sqlx::Error, name: &str) -> String {
    let msg = db_err(err);
    if msg.contains("UNIQUE") {
        format!("CLASS_DUPLICATE: there is already a class named '{name}' here")
    } else {
        msg
    }
}

async fn check_parent(tx: &mut Transaction<'_, Sqlite>, parent_id: Option<i64>) -> Result<(), String> {
    let Some(parent) = parent_id else { return Ok(()) };
    let active: Option<bool> = sqlx::query_scalar("SELECT is_active FROM classes WHERE id = ?1")
        .bind(parent)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    match active {
        None => Err("CLASS_PARENT_NOT_FOUND: the parent class does not exist".into()),
        Some(false) => Err("CLASS_PARENT_INACTIVE: the parent class is inactive".into()),
        Some(true) => Ok(()),
    }
}

pub async fn create_class(pool: &SqlitePool, input: ClassInput) -> Result<ClassView, String> {
    let name = clean_name(&input)?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    if let Err(err) = check_parent(&mut tx, input.parent_id).await {
        return Err(rollback(tx, err).await);
    }
    let id: i64 = match sqlx::query_scalar("INSERT INTO classes (name, parent_id) VALUES (?1, ?2) RETURNING id")
        .bind(name)
        .bind(input.parent_id)
        .fetch_one(&mut *tx)
        .await
    {
        Ok(id) => id,
        Err(err) => return Err(rollback(tx, name_err(err, name)).await),
    };
    tx.commit().await.map_err(db_err)?;
    get_class(pool, id).await
}

/// Renames a class or moves it under another parent (or to the top level).
pub async fn update_class(pool: &SqlitePool, id: i64, input: ClassInput) -> Result<ClassView, String> {
    let name = clean_name(&input)?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    if let Err(err) = check_parent(&mut tx, input.parent_id).await {
        return Err(rollback(tx, err).await);
    }
    let updated = match sqlx::query("UPDATE classes SET name = ?1, parent_id = ?2 WHERE id = ?3")
        .bind(name)
        .bind(input.parent_id)
        .bind(id)
        .execute(&mut *tx)
        .await
    {
        Ok(done) => done.rows_affected(),
        Err(err) => return Err(rollback(tx, name_err(err, name)).await),
    };
    if updated == 0 {
        return Err(rollback(tx, format!("CLASS_NOT_FOUND: class {id} does not exist")).await);
    }
    tx.commit().await.map_err(db_err)?;
    get_class(pool, id).await
}

/// Makes a class inactive together with all of its subclasses, or active again (which needs an
/// active parent). Lines that already use an inactive class keep it.
pub async fn set_class_active(pool: &SqlitePool, id: i64, active: bool) -> Result<ClassView, String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let result = async {
        let parent: Option<Option<i64>> =
            sqlx::query_scalar("SELECT parent_id FROM classes WHERE id = ?1").bind(id).fetch_optional(&mut *tx).await.map_err(db_err)?;
        let parent = parent.ok_or_else(|| format!("CLASS_NOT_FOUND: class {id} does not exist"))?;
        if active {
            check_parent(&mut tx, parent).await?;
            sqlx::query("UPDATE classes SET is_active = 1 WHERE id = ?1").bind(id).execute(&mut *tx).await.map_err(db_err)?;
        } else {
            sqlx::query("UPDATE classes SET is_active = 0 WHERE id IN (SELECT class_id FROM v_class_ancestry WHERE ancestor_id = ?1)")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
        Ok::<(), String>(())
    }
    .await;
    if let Err(err) = result {
        return Err(rollback(tx, err).await);
    }
    tx.commit().await.map_err(db_err)?;
    get_class(pool, id).await
}

/// Deletes a class that has never been used and has no subclasses.
pub async fn delete_class(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let class = get_class(pool, id).await?;
    if class.is_used {
        return Err(format!("CLASS_IN_USE: '{}' is used on transactions; make it inactive instead", class.full_name));
    }
    let has_children: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM classes WHERE parent_id = ?1)").bind(id).fetch_one(pool).await.map_err(db_err)?;
    if has_children {
        return Err(format!("CLASS_HAS_SUBCLASSES: '{}' has subclasses; delete or move them first", class.full_name));
    }
    sqlx::query("DELETE FROM classes WHERE id = ?1").bind(id).execute(pool).await.map(|_| ()).map_err(db_err)
}

/// Checks that a class chosen for a new line exists and is active. `context` prefixes the error
/// code and names the line, e.g. "INV: line 2" gives "INV_UNKNOWN_CLASS: line 2 ...".
pub(crate) async fn check_usable(tx: &mut Transaction<'_, Sqlite>, class_id: i64, context: &str) -> Result<(), String> {
    let (code, line) = context.split_once(": ").unwrap_or((context, "a line"));
    let row: Option<(bool, String)> = sqlx::query_as("SELECT is_active, full_name FROM v_classes WHERE id = ?1")
        .bind(class_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    match row {
        None => Err(format!("{code}_UNKNOWN_CLASS: {line} uses a class that does not exist")),
        Some((false, name)) => Err(format!("{code}_INACTIVE_CLASS: {line} uses inactive class '{name}'")),
        Some((true, _)) => Ok(()),
    }
}

// ------------------------------------------------------------------------------------------------
// Reclassifying posted lines
// ------------------------------------------------------------------------------------------------

/// A posted income or expense line that can be moved to another class.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ReclassLine {
    pub line_id: i64,
    pub journal_entry_id: i64,
    pub entry_number: i64,
    pub entry_date: String,
    pub source_type: String,
    pub memo: Option<String>,
    pub account_id: i64,
    pub account_name: String,
    /// Debit minus credit, in home currency.
    pub amount_cents: i64,
    /// The line's class now, after any earlier reclassification.
    pub class_id: Option<i64>,
    pub class_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReclassQuery {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    #[serde(default)]
    pub account_id: Option<i64>,
    /// Only lines currently in this class (with its subclasses)...
    #[serde(default)]
    pub class_id: Option<i64>,
    /// ...or, when true, only lines with no class.
    #[serde(default)]
    pub unclassified: bool,
}

/// Income and expense lines in the range that can be reclassified: posted, not voided, not a void's
/// reversal, and not themselves part of a reclassifying entry.
pub async fn reclass_candidates(pool: &SqlitePool, query: &ReclassQuery) -> Result<Vec<ReclassLine>, String> {
    sqlx::query_as::<_, ReclassLine>(
        "SELECT l.id AS line_id, e.id AS journal_entry_id, e.entry_number, e.entry_date, e.source_type,
                COALESCE(l.memo, e.memo) AS memo, l.account_id, a.full_name AS account_name,
                l.debit_cents - l.credit_cents AS amount_cents, x.class_id, k.full_name AS class_name
         FROM journal_lines l
         JOIN journal_entries e ON e.id = l.journal_entry_id
         JOIN v_chart_of_accounts a ON a.id = l.account_id
         JOIN v_line_effective_class x ON x.line_id = l.id
         LEFT JOIN v_classes k ON k.id = x.class_id
         WHERE e.status = 'POSTED' AND e.is_voided = 0 AND e.reverses_entry_id IS NULL
           AND NOT EXISTS (SELECT 1 FROM class_reclassifications r WHERE r.journal_entry_id = e.id)
           AND a.classification IN ('INCOME', 'EXPENSE')
           AND e.entry_date BETWEEN ?1 AND ?2
           AND (?3 IS NULL OR l.account_id = ?3)
           AND (?4 IS NULL OR x.class_id IN (SELECT class_id FROM v_class_ancestry WHERE ancestor_id = ?4))
           AND (?5 = 0 OR x.class_id IS NULL)
         ORDER BY e.entry_date, e.entry_number, l.line_number",
    )
    .bind(query.start_date)
    .bind(query.end_date)
    .bind(query.account_id)
    .bind(query.class_id)
    .bind(query.unclassified)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Moves posted lines to `to_class_id` (or to no class). Posted lines never change, so each line's
/// amount is moved with a reclassifying journal entry on the line's own date: the same account,
/// out of its current class and into the new one. Lines already in the target class are skipped.
/// Returns how many lines moved.
pub async fn reclassify(pool: &SqlitePool, line_ids: &[i64], to_class_id: Option<i64>) -> Result<usize, String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    match write_reclassification(&mut tx, line_ids, to_class_id).await {
        Ok(moved) => tx.commit().await.map(|()| moved).map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

async fn write_reclassification(tx: &mut Transaction<'_, Sqlite>, line_ids: &[i64], to_class_id: Option<i64>) -> Result<usize, String> {
    if line_ids.is_empty() {
        return Err("RECLASS_NO_LINES: choose at least one line to reclassify".into());
    }
    if let Some(class) = to_class_id {
        check_usable(tx, class, "RECLASS: the new class").await?;
    }
    let target_name: String = match to_class_id {
        Some(id) => sqlx::query_scalar("SELECT full_name FROM v_classes WHERE id = ?1").bind(id).fetch_one(&mut **tx).await.map_err(db_err)?,
        None => "no class".into(),
    };

    // (date, lines to post, (line id, from class)) per entry date, in first-appearance order.
    let mut by_date: Vec<(NaiveDate, Vec<JournalLine>, Vec<(i64, Option<i64>)>)> = Vec::new();
    for &line_id in line_ids {
        let row: Option<(NaiveDate, i64, i64, i64, Option<i64>, bool)> = sqlx::query_as(
            "SELECT e.entry_date, l.account_id, l.debit_cents, l.credit_cents, x.class_id,
                    (e.status = 'POSTED' AND e.is_voided = 0 AND e.reverses_entry_id IS NULL
                     AND NOT EXISTS (SELECT 1 FROM class_reclassifications r WHERE r.journal_entry_id = e.id)
                     AND a.classification IN ('INCOME', 'EXPENSE'))
             FROM journal_lines l
             JOIN journal_entries e ON e.id = l.journal_entry_id
             JOIN v_chart_of_accounts a ON a.id = l.account_id
             JOIN v_line_effective_class x ON x.line_id = l.id
             WHERE l.id = ?1",
        )
        .bind(line_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        let (date, account, debit, credit, from, movable) = row.ok_or_else(|| format!("RECLASS_NOT_FOUND: line {line_id} does not exist"))?;
        if !movable {
            return Err(format!("RECLASS_NOT_ALLOWED: line {line_id} is not a posted income or expense line that can be reclassified"));
        }
        if from == to_class_id {
            continue;
        }
        let (out_of, into) = if debit > 0 {
            (JournalLine::credit(account, debit), JournalLine::debit(account, debit))
        } else {
            (JournalLine::debit(account, credit), JournalLine::credit(account, credit))
        };
        let group = match by_date.iter_mut().position(|(d, _, _)| *d == date) {
            Some(i) => &mut by_date[i],
            None => {
                by_date.push((date, Vec::new(), Vec::new()));
                by_date.last_mut().unwrap()
            }
        };
        group.1.push(out_of.with_class(from));
        group.1.push(into.with_class(to_class_id));
        group.2.push((line_id, from));
    }

    let mut moved = 0;
    for (date, lines, sources) in by_date {
        let entry = JournalEntry { id: None, date, description: format!("Reclassify to {target_name}"), lines, currency: None };
        let entry_id = post_entry_in_tx(tx, &entry, EntrySource::Reclassification).await?;
        for (line_id, from) in sources {
            sqlx::query("INSERT INTO class_reclassifications (line_id, from_class_id, to_class_id, journal_entry_id) VALUES (?1, ?2, ?3, ?4)")
                .bind(line_id)
                .bind(from)
                .bind(to_class_id)
                .bind(entry_id)
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
            moved += 1;
        }
    }
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};

    fn input(name: &str, parent_id: Option<i64>) -> ClassInput {
        ClassInput { name: name.into(), parent_id }
    }

    #[tokio::test]
    async fn classes_nest_and_refuse_duplicates_and_cycles() {
        let pool = test_pool().await;
        let east = create_class(&pool, input("East", None)).await.unwrap();
        let retail = create_class(&pool, input(" Retail ", Some(east.id))).await.unwrap();
        assert_eq!((retail.full_name.as_str(), retail.depth), ("East:Retail", 1));

        assert!(create_class(&pool, input("east", None)).await.unwrap_err().starts_with("CLASS_DUPLICATE"));
        assert!(create_class(&pool, input("A:B", None)).await.unwrap_err().starts_with("CLASS_NAME_COLON"));
        // The same name is fine under a different parent.
        create_class(&pool, input("Retail", None)).await.unwrap();

        let err = update_class(&pool, east.id, input("East", Some(retail.id))).await.unwrap_err();
        assert!(err.starts_with("CLASS_CYCLE"), "{err}");

        // Listed as a tree: each class directly followed by its subclasses.
        create_class(&pool, input("East Coast", None)).await.unwrap();
        let order: Vec<String> = list_classes(&pool).await.unwrap().into_iter().map(|c| c.full_name).collect();
        assert_eq!(order, ["East", "East:Retail", "East Coast", "Retail"]);

        let moved = update_class(&pool, retail.id, input("Stores", None)).await.unwrap();
        assert_eq!((moved.full_name.as_str(), moved.parent_id), ("Stores", None));
    }

    #[tokio::test]
    async fn inactive_cascades_and_used_classes_cannot_be_deleted() {
        let pool = test_pool().await;
        let east = create_class(&pool, input("East", None)).await.unwrap();
        let retail = create_class(&pool, input("Retail", Some(east.id))).await.unwrap();

        set_class_active(&pool, east.id, false).await.unwrap();
        assert!(!get_class(&pool, retail.id).await.unwrap().is_active);
        let err = set_class_active(&pool, retail.id, true).await.unwrap_err();
        assert!(err.starts_with("CLASS_PARENT_INACTIVE"), "{err}");
        set_class_active(&pool, east.id, true).await.unwrap();
        set_class_active(&pool, retail.id, true).await.unwrap();

        assert!(delete_class(&pool, east.id).await.unwrap_err().starts_with("CLASS_HAS_SUBCLASSES"));
        let account = |n: &'static str| sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n);
        let (checking, rent) = (account("1010").fetch_one(&pool).await.unwrap(), account("6300").fetch_one(&pool).await.unwrap());
        let date = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let lines = vec![JournalLine::debit(rent, 500).with_class(Some(retail.id)), JournalLine::credit(checking, 500)];
        post_journal_entry(&pool, JournalEntry { id: None, date, description: String::new(), lines, currency: None }).await.unwrap();
        assert!(delete_class(&pool, retail.id).await.unwrap_err().starts_with("CLASS_IN_USE"));

        // Inactive classes cannot be chosen for new lines.
        set_class_active(&pool, retail.id, false).await.unwrap();
        let lines = vec![JournalLine::debit(rent, 500).with_class(Some(retail.id)), JournalLine::credit(checking, 500)];
        let err = post_journal_entry(&pool, JournalEntry { id: None, date, description: String::new(), lines, currency: None }).await.unwrap_err();
        assert!(err.starts_with("JE_INACTIVE_CLASS: line 1"), "{err}");

        let unused = create_class(&pool, input("Spare", None)).await.unwrap();
        delete_class(&pool, unused.id).await.unwrap();
        assert!(list_classes(&pool).await.unwrap().iter().all(|c| c.id != unused.id));
    }

    #[tokio::test]
    async fn reclassifying_moves_lines_between_classes_and_voids_in_order() {
        let pool = test_pool().await;
        let account = |n: &'static str| sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n);
        let (checking, rent) = (account("1010").fetch_one(&pool).await.unwrap(), account("6300").fetch_one(&pool).await.unwrap());
        let east = create_class(&pool, input("East", None)).await.unwrap().id;
        let west = create_class(&pool, input("West", None)).await.unwrap().id;
        let date = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
        let lines = vec![JournalLine::debit(rent, 800).with_class(Some(east)), JournalLine::credit(checking, 800)];
        let original = post_journal_entry(&pool, JournalEntry { id: None, date, description: "Rent".into(), lines, currency: None }).await.unwrap();
        let class_net = |class: i64| {
            sqlx::query_scalar::<_, i64>("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1 AND class_id = ?2").bind(rent).bind(class)
        };
        let query = ReclassQuery { start_date: date, end_date: date, account_id: None, class_id: None, unclassified: false };
        let candidates = reclass_candidates(&pool, &query).await.unwrap();
        assert_eq!(candidates.len(), 1, "only the expense line; the bank line is not income or expense");
        let line = candidates[0].line_id;

        // East → West, then West → no class: each move is its own entry and nothing doubles up.
        set_class_active(&pool, east, false).await.unwrap(); // moving out of an inactive class is allowed
        assert_eq!(reclassify(&pool, &[line], Some(west)).await.unwrap(), 1);
        assert_eq!((class_net(east).fetch_one(&pool).await.unwrap(), class_net(west).fetch_one(&pool).await.unwrap()), (0, 800));
        assert_eq!(reclassify(&pool, &[line], Some(west)).await.unwrap(), 0, "already in West");
        assert_eq!(reclass_candidates(&pool, &query).await.unwrap()[0].class_id, Some(west));
        assert_eq!(reclassify(&pool, &[line], None).await.unwrap(), 1);
        assert_eq!(class_net(west).fetch_one(&pool).await.unwrap(), 0);
        assert_eq!(reclass_candidates(&pool, &query).await.unwrap()[0].class_id, None);

        // The original cannot be voided while moved; reclassifying entries are undone newest first.
        let err = crate::ledger::void_transaction(&pool, original).await.unwrap_err();
        assert!(err.starts_with("VOID_RECLASSIFIED"), "{err}");
        let reclass_entries: Vec<i64> =
            sqlx::query_scalar("SELECT DISTINCT journal_entry_id FROM class_reclassifications ORDER BY id").fetch_all(&pool).await.unwrap();
        let err = crate::ledger::void_transaction(&pool, reclass_entries[0]).await.unwrap_err();
        assert!(err.starts_with("VOID_RECLASS_ORDER"), "{err}");
        crate::ledger::void_transaction(&pool, reclass_entries[1]).await.unwrap();
        assert_eq!(reclass_candidates(&pool, &query).await.unwrap()[0].class_id, Some(west));
        crate::ledger::void_transaction(&pool, reclass_entries[0]).await.unwrap();
        assert_eq!(reclass_candidates(&pool, &query).await.unwrap()[0].class_id, Some(east));
        crate::ledger::void_transaction(&pool, original).await.unwrap();
        assert_eq!(class_net(east).fetch_one(&pool).await.unwrap(), 0);
        assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
    }
}
