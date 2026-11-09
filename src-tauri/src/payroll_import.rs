//! Payroll import: a payroll service's journal export becomes balanced journal entries.
//!
//! LocalBooks does not run payroll. A payroll service pays the people and files the taxes, and
//! almost every service can export each payroll as a journal: rows with a date, an account, and a
//! debit and credit (or one signed amount), often with a payroll or journal id that says which rows
//! belong together. This module reads that export (CSV or a workbook, through
//! [`crate::sheet_import::read`]), lets the user match its columns and its account names to
//! LocalBooks accounts, shows every entry it would post, and posts them all in one write
//! transaction through [`crate::ledger::post_entry_in_tx`]. Nothing posts while any row has a
//! problem or any entry is out of balance.
//!
//! - **Entries.** Rows sharing the Entry column's value (a payroll id, check date or batch) form one
//!   entry; without that column, rows sharing a date do. Each entry must balance and fall on one date.
//! - **Accounts.** Each account name in the file is matched to a LocalBooks account: the account
//!   it was matched to last time (kept in `payroll_account_map`), else an account whose name, full
//!   name or number is the same. The user can change any match on the preview.
//! - **Once only.** The file's SHA-256 is kept with the import (`payroll_imports`), so the same
//!   file cannot be posted twice. A mistaken import is undone by voiding its entries.

use std::collections::{BTreeMap, HashMap};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::bank::{detect_date_order, parse_amount_cents, parse_date_ordered, DateOrder};
use crate::ledger::{db_err, Cents, EntrySource, JournalEntry, JournalLine};
use crate::sheet_import::{Mapping, Problem, Sheet};

/// A column the export can be matched to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    pub id: &'static str,
    pub label: &'static str,
    pub required: bool,
    #[serde(skip)]
    aliases: &'static [&'static str],
}

pub const FIELDS: &[Field] = &[
    Field { id: "date", label: "Date", required: true, aliases: &["checkdate", "paydate", "paymentdate", "postingdate", "journaldate", "transactiondate", "entrydate", "period end"] },
    Field { id: "entry", label: "Entry", required: false, aliases: &["entryid", "entrynumber", "journalid", "journalentry", "journal", "payrollid", "payroll", "payrun", "payrunid", "batch", "batchid", "reference", "ref", "referencenumber"] },
    Field { id: "account", label: "Account", required: true, aliases: &["accountname", "glaccount", "glaccountname", "ledgeraccount", "accounttitle", "accountdescription", "glname"] },
    Field { id: "debit", label: "Debit", required: false, aliases: &["debitamount", "debits", "dr"] },
    Field { id: "credit", label: "Credit", required: false, aliases: &["creditamount", "credits", "cr"] },
    Field { id: "amount", label: "Amount", required: false, aliases: &["netamount", "value", "total"] },
    Field { id: "memo", label: "Memo", required: false, aliases: &["description", "lineitem", "notes", "note", "item", "category", "lineitemdescription"] },
];

fn key(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

/// The columns the headings suggest: each field to the first heading that is its label or id,
/// then to one of its other usual names; no heading used twice.
pub fn suggest(headers: &[String]) -> Mapping {
    let keys: Vec<String> = headers.iter().map(|h| key(h)).collect();
    let mut out = Mapping::new();
    for pass in 0..2 {
        for f in FIELDS {
            if out.contains_key(f.id) {
                continue;
            }
            let wanted: Vec<String> = if pass == 0 { vec![key(f.label)] } else { f.aliases.iter().map(|a| key(a)).collect() };
            if let Some(i) = keys.iter().enumerate().position(|(i, k)| !k.is_empty() && !out.values().any(|&u| u == i) && wanted.contains(k)) {
                out.insert(f.id.to_string(), i);
            }
        }
    }
    out
}

/// One of the file's account names and the LocalBooks account it goes to.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SourceAccount {
    pub name: String,
    pub account_id: Option<i64>,
    /// `chosen` (on this preview), `saved` (last import), `matched` (same name or number) or `none`.
    pub how: &'static str,
    pub rows: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewLine {
    pub row: usize,
    pub source_account: String,
    pub account_id: Option<i64>,
    pub debit_cents: i64,
    pub credit_cents: i64,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PreviewEntry {
    /// The Entry column's value, or the date.
    pub key: String,
    pub date: NaiveDate,
    pub description: String,
    pub lines: Vec<PreviewLine>,
    pub debit_cents: i64,
    pub credit_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PayrollPreview {
    pub file_name: String,
    pub headers: Vec<String>,
    pub fields: Vec<Field>,
    pub mapping: Mapping,
    pub source_accounts: Vec<SourceAccount>,
    pub entries: Vec<PreviewEntry>,
    /// Debits of every entry.
    pub total_cents: i64,
    pub problems: Vec<Problem>,
    /// When this file was posted before (UTC timestamp), if it was.
    pub already_imported: Option<String>,
}

/// What the user chose on the preview. `mapping` `None` takes the suggested columns.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PayrollChoices {
    #[serde(default)]
    pub mapping: Option<Mapping>,
    #[serde(default)]
    pub date_order: Option<DateOrder>,
    /// File account name → LocalBooks account id.
    #[serde(default)]
    pub accounts: HashMap<String, i64>,
}

#[derive(sqlx::FromRow)]
struct Account {
    id: i64,
    name: String,
    full_name: String,
    account_number: Option<String>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// One row as read.
struct Row {
    row: usize,
    group: String,
    date: NaiveDate,
    account: String,
    /// Debit minus credit.
    net_cents: i64,
    memo: Option<String>,
}

fn cell<'a>(mapping: &Mapping, cells: &'a [String], field: &str) -> Option<&'a str> {
    let i = *mapping.get(field)?;
    cells.get(i).map(|c| c.trim()).filter(|c| !c.is_empty())
}

fn money(raw: Option<&str>) -> Result<i64, String> {
    raw.map_or(Ok(0), parse_amount_cents)
}

/// Reads and checks the export: every entry it would post, what each account name goes to, and
/// every problem that stops it posting.
pub async fn preview(pool: &SqlitePool, file_name: &str, bytes: &[u8], choices: &PayrollChoices) -> Result<PayrollPreview, String> {
    let sheet: Sheet = crate::sheet_import::read(file_name, bytes)?;
    let mapping = choices.mapping.clone().unwrap_or_else(|| suggest(&sheet.headers));
    let mut problems = Vec::new();
    for f in FIELDS.iter().filter(|f| f.required) {
        if !mapping.contains_key(f.id) {
            problems.push(Problem { row: 0, message: format!("Match a column to {}", f.label) });
        }
    }
    let has_amounts = mapping.contains_key("amount") || mapping.contains_key("debit") || mapping.contains_key("credit");
    if !has_amounts {
        problems.push(Problem { row: 0, message: "Match a column to Debit and Credit, or to Amount".into() });
    }

    let order = choices.date_order.unwrap_or_else(|| {
        detect_date_order(sheet.rows.iter().filter_map(|(_, r)| cell(&mapping, r, "date"))).0
    });
    let mut rows = Vec::new();
    if problems.is_empty() {
        for (n, cells) in &sheet.rows {
            let date = match cell(&mapping, cells, "date").map(|d| parse_date_ordered(d, order)) {
                Some(Ok(d)) => d,
                Some(Err(e)) => {
                    problems.push(Problem { row: *n, message: format!("Date: {e}") });
                    continue;
                }
                None => {
                    // Payroll exports end with total rows and blank-dated summary rows: a row with
                    // no date is skipped when it has no amount or is a total (a cell starting
                    // "Total"); any other row with an amount and no date is a problem.
                    let any_amount = ["debit", "credit", "amount"].iter().any(|f| money(cell(&mapping, cells, f)).is_ok_and(|c| c != 0));
                    let total_row = cells.iter().any(|c| key(c).starts_with("total"));
                    if any_amount && !total_row {
                        problems.push(Problem { row: *n, message: "No date".into() });
                    }
                    continue;
                }
            };
            let net = if mapping.contains_key("amount") && !mapping.contains_key("debit") && !mapping.contains_key("credit") {
                money(cell(&mapping, cells, "amount"))
            } else {
                money(cell(&mapping, cells, "debit")).and_then(|d| money(cell(&mapping, cells, "credit")).and_then(|c| d.checked_sub(c).ok_or_else(|| "amount out of range".into())))
            };
            let net = match net {
                Ok(v) => v,
                Err(e) => {
                    problems.push(Problem { row: *n, message: format!("Amount: {e}") });
                    continue;
                }
            };
            if net == 0 {
                continue;
            }
            let Some(account) = cell(&mapping, cells, "account") else {
                problems.push(Problem { row: *n, message: "No account".into() });
                continue;
            };
            let group = cell(&mapping, cells, "entry").map_or_else(|| date.to_string(), str::to_string);
            rows.push(Row { row: *n, group, date, account: account.chars().take(200).collect(), net_cents: net, memo: cell(&mapping, cells, "memo").map(|m| m.chars().take(500).collect()) });
        }
        if rows.is_empty() && problems.is_empty() {
            problems.push(Problem { row: 0, message: "The file has no rows with an amount".into() });
        }
    }

    // Accounts: chosen here, else saved from last time, else the same name or number.
    let accounts: Vec<Account> = sqlx::query_as("SELECT id, name, full_name, account_number FROM v_chart_of_accounts").fetch_all(pool).await.map_err(db_err)?;
    let saved: HashMap<String, i64> = sqlx::query_as::<_, (String, i64)>("SELECT source_account, account_id FROM payroll_account_map").fetch_all(pool).await.map_err(db_err)?.into_iter().collect();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for r in &rows {
        *counts.entry(r.account.clone()).or_default() += 1;
    }
    let source_accounts: Vec<SourceAccount> = counts
        .into_iter()
        .map(|(name, rows)| {
            let exists = |id: &i64| accounts.iter().any(|a| a.id == *id);
            let (account_id, how) = if let Some(id) = choices.accounts.get(&name).filter(|id| exists(id)) {
                (Some(*id), "chosen")
            } else if let Some(id) = saved.get(&key(&name)).filter(|id| exists(id)) {
                (Some(*id), "saved")
            } else {
                let k = key(&name);
                let found = accounts.iter().find(|a| key(&a.full_name) == k || key(&a.name) == k || a.account_number.as_deref().is_some_and(|n| key(n) == k || key(&format!("{n} {}", a.name)) == k));
                found.map_or((None, "none"), |a| (Some(a.id), "matched"))
            };
            SourceAccount { name, account_id, how, rows }
        })
        .collect();
    for s in source_accounts.iter().filter(|s| s.account_id.is_none()) {
        problems.push(Problem { row: 0, message: format!("Choose the LocalBooks account for '{}'", s.name) });
    }
    let account_of = |name: &str| source_accounts.iter().find(|s| s.name == name).and_then(|s| s.account_id);

    // Entries, in the order they first appear.
    let mut order_seen: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<&Row>> = HashMap::new();
    for r in &rows {
        if !groups.contains_key(&r.group) {
            order_seen.push(r.group.clone());
        }
        groups.entry(r.group.clone()).or_default().push(r);
    }
    let overflow = || "PAYROLL_OVERFLOW: the amounts are out of range".to_string();
    let mut entries = Vec::new();
    let mut total: i64 = 0;
    for g in order_seen {
        let group = &groups[&g];
        let first = group[0];
        if let Some(other) = group.iter().find(|r| r.date != first.date) {
            problems.push(Problem { row: other.row, message: format!("Entry {g} has rows dated {} and {}", first.date, other.date) });
        }
        let mut debit: i64 = 0;
        let mut credit: i64 = 0;
        let mut lines = Vec::new();
        for r in group {
            let (d, c) = if r.net_cents > 0 { (r.net_cents, 0) } else { (0, r.net_cents.checked_neg().ok_or_else(overflow)?) };
            debit = debit.checked_add(d).ok_or_else(overflow)?;
            credit = credit.checked_add(c).ok_or_else(overflow)?;
            lines.push(PreviewLine { row: r.row, source_account: r.account.clone(), account_id: account_of(&r.account), debit_cents: d, credit_cents: c, memo: r.memo.clone() });
        }
        if debit != credit {
            let diff = debit.checked_sub(credit).ok_or_else(overflow)?;
            problems.push(Problem {
                row: first.row,
                message: format!("Entry {g} is out of balance: debits {} and credits {} differ by {}", crate::ledger::format_cents(debit), crate::ledger::format_cents(credit), crate::ledger::format_cents(diff.abs())),
            });
        }
        if lines.len() < 2 {
            problems.push(Problem { row: first.row, message: format!("Entry {g} has only one line") });
        }
        total = total.checked_add(debit).ok_or_else(overflow)?;
        let description = if g == first.date.to_string() { format!("Payroll {g}") } else { format!("Payroll {g} ({})", first.date) };
        entries.push(PreviewEntry { key: g, date: first.date, description, lines, debit_cents: debit, credit_cents: credit });
    }
    problems.sort_by_key(|p| p.row);

    let already_imported: Option<String> = sqlx::query_scalar(
        "SELECT p.imported_at FROM payroll_imports p WHERE p.file_sha256 = ?1
           AND EXISTS (SELECT 1 FROM payroll_import_entries x JOIN journal_entries e ON e.id = x.journal_entry_id WHERE x.import_id = p.id AND e.is_voided = 0)
         ORDER BY p.id DESC LIMIT 1",
    ).bind(sha256_hex(bytes)).fetch_optional(pool).await.map_err(db_err)?;
    let file_name = std::path::Path::new(file_name).file_name().map_or_else(|| file_name.to_string(), |n| n.to_string_lossy().into_owned());
    Ok(PayrollPreview { file_name, headers: sheet.headers, fields: FIELDS.to_vec(), mapping, source_accounts, entries, total_cents: total, problems, already_imported })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PostedPayroll {
    pub import_id: i64,
    pub journal_entry_ids: Vec<i64>,
    pub total_cents: i64,
}

/// Posts the export's entries, all or none, once the preview has no problems. Remembers each
/// account name's match for the next import.
pub async fn post(pool: &SqlitePool, file_name: &str, bytes: &[u8], choices: &PayrollChoices) -> Result<PostedPayroll, String> {
    let p = preview(pool, file_name, bytes, choices).await?;
    if let Some(at) = &p.already_imported {
        return Err(format!("PAYROLL_ALREADY_IMPORTED: this file was imported on {at}; void those entries first to import it again"));
    }
    if let Some(first) = p.problems.first() {
        let at = if first.row > 0 { format!("row {}: ", first.row) } else { String::new() };
        return Err(format!("PAYROLL_PROBLEMS: {} problem(s) to fix first ({at}{})", p.problems.len(), first.message));
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result: Result<PostedPayroll, String> = async {
        let mut ids = Vec::new();
        for e in &p.entries {
            let lines = e
                .lines
                .iter()
                .map(|l| {
                    let account_id = l.account_id.ok_or_else(|| format!("PAYROLL_PROBLEMS: row {}: '{}' has no LocalBooks account", l.row, l.source_account))?;
                    Ok(JournalLine { account_id, debit: Cents(l.debit_cents), credit: Cents(l.credit_cents), memo: l.memo.clone(), class_id: None })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let entry = JournalEntry { id: None, date: e.date, description: e.description.clone(), lines, currency: None };
            ids.push(crate::ledger::post_entry_in_tx(&mut tx, &entry, EntrySource::Journal).await?);
        }
        let import_id: i64 = sqlx::query_scalar("INSERT INTO payroll_imports (file_name, file_sha256, entry_count, total_cents) VALUES (?1, ?2, ?3, ?4) RETURNING id")
            .bind(&p.file_name)
            .bind(sha256_hex(bytes))
            .bind(i64::try_from(ids.len()).unwrap_or(i64::MAX))
            .bind(p.total_cents)
            .fetch_one(&mut *tx)
            .await
            .map_err(db_err)?;
        for (id, e) in ids.iter().zip(&p.entries) {
            sqlx::query("INSERT INTO payroll_import_entries (import_id, journal_entry_id, entry_key) VALUES (?1, ?2, ?3)").bind(import_id).bind(id).bind(&e.key).execute(&mut *tx).await.map_err(db_err)?;
        }
        for s in p.source_accounts.iter().filter(|s| !key(&s.name).is_empty()) {
            sqlx::query("INSERT INTO payroll_account_map (source_account, account_id) VALUES (?1, ?2) ON CONFLICT (source_account) DO UPDATE SET account_id = excluded.account_id")
                .bind(key(&s.name))
                .bind(s.account_id)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
        Ok(PostedPayroll { import_id, journal_entry_ids: ids, total_cents: p.total_cents })
    }
    .await;
    match result {
        Ok(done) => crate::ledger::commit_write(tx).await.map(|()| done).map_err(db_err),
        Err(err) => Err(crate::ledger::rollback(tx, err).await),
    }
}

/// A past payroll import.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PayrollImport {
    pub id: i64,
    pub file_name: String,
    pub imported_at: String,
    pub entry_count: i64,
    pub total_cents: i64,
    /// Entries of this import since voided.
    pub voided: i64,
}

/// Past payroll imports, newest first.
pub async fn history(pool: &SqlitePool) -> Result<Vec<PayrollImport>, String> {
    sqlx::query_as(
        "SELECT p.id, p.file_name, p.imported_at, p.entry_count, p.total_cents,
                (SELECT COUNT(*) FROM payroll_import_entries x JOIN journal_entries e ON e.id = x.journal_entry_id
                 WHERE x.import_id = p.id AND e.is_voided = 1) AS voided
         FROM payroll_imports p ORDER BY p.id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests;
