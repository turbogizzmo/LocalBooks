//! Accountant & Taxes reports that are not presets of the custom report engine:
//!
//! - **Previous reconciliation** Detail and Summary: a completed reconciliation reprinted, with the
//!   transactions it cleared and those still outstanding (or cleared later) as of its statement date.
//! - **Reconciliation Discrepancy**: cleared transactions changed, voided or deleted after their
//!   reconciliation was completed, found by comparing each one with the snapshot taken when it was
//!   completed (migration 0056). LocalBooks itself refuses such changes, so a discrepancy means the
//!   company file was changed by something else. Reconciliations completed before snapshots were
//!   kept are checked by their totals only.
//! - **Income Tax Summary and Detail**: totals per income tax return line for the company's tax form,
//!   from the tax line assigned to each account.
//! - **Collections**: overdue invoices by customer, with the customer's contact details.
//! - **Open Purchase Orders**.
//!
//! Every report comes back in the custom report engine's shape ([`CustomReport`]), so the screen
//! shows, prints and exports it the same way. Money is integer cents throughout.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::custom_reports::{type_label, ColumnHead, ColumnKind, CustomReport, ReportRow, RowKind};
use crate::ledger::{db_err, DISPLAY_SOURCE_TYPE};

/// A report and the notes printed under it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountantReport {
    #[serde(flatten)]
    pub report: CustomReport,
    pub notes: Vec<String>,
}

fn head(label: &str, kind: ColumnKind) -> ColumnHead {
    ColumnHead { label: label.into(), kind }
}

fn row(kind: RowKind, depth: u32, cells: Vec<Value>) -> ReportRow {
    ReportRow { kind, depth, cells, journal_entry_id: None, account_id: None, drill: None }
}

/// A row with `label` in the first column, `values` in the last ones and blanks between.
fn labeled(kind: RowKind, depth: u32, width: usize, label: &str, values: &[Value]) -> ReportRow {
    let mut cells = vec![json!(label)];
    cells.resize(width - values.len(), Value::Null);
    cells.extend_from_slice(values);
    row(kind, depth, cells)
}

fn section(depth: u32, width: usize, label: &str) -> ReportRow {
    labeled(RowKind::Section, depth, width, label, &[])
}

fn report(start_date: Option<NaiveDate>, end_date: NaiveDate, columns: Vec<ColumnHead>, rows: Vec<ReportRow>, notes: Vec<String>) -> AccountantReport {
    let line_count = rows.iter().filter(|r| r.kind == RowKind::Detail && (r.journal_entry_id.is_some() || r.account_id.is_some())).count();
    AccountantReport { report: CustomReport { start_date, end_date, columns, rows, line_count }, notes }
}

fn date(text: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|e| format!("RPT_BAD_DATE: {text}: {e}"))
}

/// An entry's type as reports show it: the entry that voids another shows as `Void`.
fn type_text(source_type: &str, is_reversal: bool) -> String {
    if is_reversal {
        "Void".into()
    } else {
        type_label(source_type).to_string()
    }
}

// ------------------------------------------------------------------------------------------------
// Previous reconciliations
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CompletedReconciliation {
    pub id: i64,
    pub account_id: i64,
    pub account_name: String,
    pub statement_end_date: String,
    pub statement_ending_balance_cents: i64,
    pub locked_at: Option<String>,
    /// Completed after LocalBooks began keeping snapshots, so the discrepancy check is line by line.
    pub has_snapshot: bool,
}

/// Every completed reconciliation, by account and then newest statement first.
pub async fn list_completed(pool: &SqlitePool) -> Result<Vec<CompletedReconciliation>, String> {
    sqlx::query_as(
        "SELECT r.id, r.account_id, a.full_name AS account_name, r.statement_end_date, r.statement_ending_balance_cents, r.locked_at,
                EXISTS (SELECT 1 FROM reconciliation_snapshots s WHERE s.reconciliation_id = r.id) AS has_snapshot
         FROM reconciliations r JOIN v_chart_of_accounts a ON a.id = r.account_id
         WHERE r.is_locked = 1
         ORDER BY a.full_name COLLATE NOCASE, r.statement_end_date DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct RecHeader {
    id: i64,
    account_id: i64,
    account_name: String,
    account_type: String,
    debit_normal: bool,
    statement_end_date: String,
    statement_ending_balance_cents: i64,
    beginning_balance_cents: i64,
}

async fn rec_header(pool: &SqlitePool, reconciliation_id: i64) -> Result<RecHeader, String> {
    sqlx::query_as(
        "SELECT r.id, r.account_id, a.full_name AS account_name, a.account_type, a.normal_balance = 'DEBIT' AS debit_normal,
                r.statement_end_date, r.statement_ending_balance_cents, r.beginning_balance_cents
         FROM reconciliations r JOIN v_chart_of_accounts a ON a.id = r.account_id
         WHERE r.id = ?1 AND r.is_locked = 1",
    )
    .bind(reconciliation_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?
    .ok_or_else(|| format!("REC_NOT_COMPLETED: reconciliation {reconciliation_id} is not a completed reconciliation"))
}

/// Effect of a journal line (`l`, account type `t`) on its account's normal-side balance.
const NORMAL_SIGNED: &str = "CASE t.normal_balance WHEN 'DEBIT' THEN l.debit_cents - l.credit_cents ELSE l.credit_cents - l.debit_cents END";

/// The document number people know a transaction by, where it has one.
const NUM: &str = "COALESCE(x.check_number, bp.reference, ip.reference, bl.bill_number, CAST(inv.invoice_number AS TEXT),
                            CAST(sr.receipt_number AS TEXT), CASE WHEN e.source_type = 'JOURNAL' THEN CAST(e.entry_number AS TEXT) END)";
const NUM_JOINS: &str = "
    LEFT JOIN expenses x ON e.source_type IN ('EXPENSE', 'CHECK') AND x.id = e.source_id
    LEFT JOIN bill_payments bp ON e.source_type = 'BILL_PAYMENT' AND bp.id = e.source_id
    LEFT JOIN invoice_payments ip ON e.source_type = 'RECEIVE_PAYMENT' AND ip.id = e.source_id
    LEFT JOIN bills bl ON e.source_type = 'BILL' AND bl.id = e.source_id
    LEFT JOIN invoices inv ON e.source_type = 'INVOICE' AND inv.id = e.source_id
    LEFT JOIN sales_receipts sr ON e.source_type = 'SALES_RECEIPT' AND sr.id = e.source_id
    LEFT JOIN v_entry_party p ON p.journal_entry_id = e.id";

#[derive(Debug, Clone, sqlx::FromRow)]
struct RecLine {
    journal_entry_id: i64,
    date: String,
    source_type: String,
    is_reversal: bool,
    num: Option<String>,
    name: Option<String>,
    memo: Option<String>,
    amount_cents: i64,
    /// Cleared in this reconciliation.
    cleared_here: bool,
    /// Statement date of the later reconciliation that cleared it, if one did.
    cleared_later: Option<String>,
    /// Ticked in the reconciliation now in progress.
    ticked: bool,
}

/// The lines cleared in the reconciliation, and every other line on the account dated on or before
/// its statement that was not cleared in an earlier one. Voided entries and the entries that void
/// them net to zero and are never offered for clearing, so they are left out too.
async fn rec_lines(pool: &SqlitePool, h: &RecHeader) -> Result<Vec<RecLine>, String> {
    sqlx::query_as(&format!(
        "SELECT e.id AS journal_entry_id, e.entry_date AS date, {DISPLAY_SOURCE_TYPE} AS source_type,
                e.reverses_entry_id IS NOT NULL AS is_reversal, {NUM} AS num, p.party_name AS name,
                COALESCE(l.memo, e.memo) AS memo, {NORMAL_SIGNED} AS amount_cents,
                COALESCE(l.reconciliation_id = ?2, 0) AS cleared_here,
                CASE WHEN r.is_locked = 1 AND r.id <> ?2 THEN r.statement_end_date END AS cleared_later,
                COALESCE(r.is_locked = 0, 0) AS ticked
         FROM journal_lines l
         JOIN journal_entries e ON e.id = l.journal_entry_id
         JOIN chart_of_accounts a ON a.id = l.account_id
         JOIN account_types t ON t.code = a.account_type
         LEFT JOIN reconciliations r ON r.id = l.reconciliation_id
         {NUM_JOINS}
         WHERE l.account_id = ?1 AND e.status = 'POSTED'
           AND (l.reconciliation_id = ?2
                OR (e.entry_date <= ?3 AND e.is_voided = 0 AND e.reverses_entry_id IS NULL
                    AND (l.reconciliation_id IS NULL OR r.is_locked = 0 OR r.statement_end_date > ?3)))
         ORDER BY e.entry_date, e.entry_number, l.line_number"
    ))
    .bind(h.account_id)
    .bind(h.id)
    .bind(&h.statement_end_date)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Everything posted to the account through the statement date, on its normal side.
async fn register_balance(pool: &SqlitePool, account_id: i64, as_of: &str) -> Result<i64, String> {
    sqlx::query_scalar(&format!(
        "SELECT COALESCE(SUM({NORMAL_SIGNED}), 0)
         FROM journal_lines l
         JOIN journal_entries e ON e.id = l.journal_entry_id
         JOIN chart_of_accounts a ON a.id = l.account_id
         JOIN account_types t ON t.code = a.account_type
         WHERE l.account_id = ?1 AND e.status = 'POSTED' AND e.entry_date <= ?2"
    ))
    .bind(account_id)
    .bind(as_of)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

/// Names of the two halves of an account's transactions: (decreases, increases).
fn halves(h: &RecHeader) -> (&'static str, &'static str) {
    match (h.debit_normal, h.account_type.as_str()) {
        (true, _) => ("Checks and Payments", "Deposits and Credits"),
        (false, "CREDIT_CARD") => ("Payments and Credits", "Charges and Cash Advances"),
        (false, _) => ("Payments and Decreases", "Increases"),
    }
}

fn items(n: usize) -> String {
    if n == 1 {
        "1 item".into()
    } else {
        format!("{n} items")
    }
}

/// A completed reconciliation reprinted: in `detail`, every transaction; otherwise counts and totals.
pub async fn previous_reconciliation(pool: &SqlitePool, reconciliation_id: i64, detail: bool) -> Result<AccountantReport, String> {
    let h = rec_header(pool, reconciliation_id).await?;
    let end = date(&h.statement_end_date)?;
    let lines = rec_lines(pool, &h).await?;
    let register = register_balance(pool, h.account_id, &h.statement_end_date).await?;
    let (down, up) = halves(&h);
    let (cleared, uncleared): (Vec<&RecLine>, Vec<&RecLine>) = lines.iter().partition(|l| l.cleared_here);
    let cleared_total: i64 = cleared.iter().map(|l| l.amount_cents).sum();
    let cleared_balance = h.beginning_balance_cents + cleared_total;
    let money = |c: i64| json!(c);

    let mut notes = vec![format!("{} · statement ending {}", h.account_name, h.statement_end_date)];
    if cleared_balance != h.statement_ending_balance_cents {
        notes.push(format!(
            "The cleared balance no longer equals the statement's ending balance of {}: see the Reconciliation Discrepancy report.",
            crate::ledger::format_cents(h.statement_ending_balance_cents)
        ));
    }

    let (columns, rows) = if detail {
        let columns = vec![
            head("Type", ColumnKind::Text),
            head("Date", ColumnKind::Date),
            head("Num", ColumnKind::Text),
            head("Name", ColumnKind::Text),
            head("Memo", ColumnKind::Text),
            head("Status", ColumnKind::Text),
            head("Amount", ColumnKind::Money),
            head("Balance", ColumnKind::Money),
        ];
        let w = columns.len();
        let mut rows = vec![labeled(RowKind::Detail, 0, w, "Beginning Balance", &[Value::Null, money(h.beginning_balance_cents)])];
        let mut balance = h.beginning_balance_cents;
        let mut block = |rows: &mut Vec<ReportRow>, title: &str, list: &[&RecLine], cleared: bool| {
            rows.push(section(0, w, title));
            for (label, increases) in [(down, false), (up, true)] {
                let part: Vec<&&RecLine> = list.iter().filter(|l| (l.amount_cents > 0) == increases).collect();
                if part.is_empty() {
                    continue;
                }
                rows.push(section(1, w, &format!("{label} - {}", items(part.len()))));
                for l in &part {
                    balance += l.amount_cents;
                    let status = if cleared {
                        "Cleared".to_string()
                    } else if let Some(later) = &l.cleared_later {
                        format!("Cleared {later}")
                    } else if l.ticked {
                        "Ticked (in progress)".into()
                    } else {
                        "Outstanding".into()
                    };
                    let mut r = row(
                        RowKind::Detail,
                        2,
                        vec![json!(type_text(&l.source_type, l.is_reversal)), json!(l.date), json!(l.num), json!(l.name), json!(l.memo), json!(status), money(l.amount_cents), money(balance)],
                    );
                    r.journal_entry_id = Some(l.journal_entry_id);
                    rows.push(r);
                }
                let total: i64 = part.iter().map(|l| l.amount_cents).sum();
                rows.push(labeled(RowKind::Subtotal, 1, w, &format!("Total {label}"), &[money(total), money(balance)]));
            }
            let total: i64 = list.iter().map(|l| l.amount_cents).sum();
            rows.push(labeled(RowKind::Subtotal, 0, w, &format!("Total {title}"), &[money(total), money(balance)]));
        };
        block(&mut rows, "Cleared Transactions", &cleared, true);
        rows.push(labeled(RowKind::Subtotal, 0, w, "Cleared Balance", &[Value::Null, money(cleared_balance)]));
        if !uncleared.is_empty() {
            block(&mut rows, &format!("Uncleared Transactions as of {}", h.statement_end_date), &uncleared, false);
        }
        rows.push(labeled(RowKind::Total, 0, w, &format!("Register Balance as of {}", h.statement_end_date), &[Value::Null, money(register)]));
        (columns, rows)
    } else {
        let columns = vec![head("", ColumnKind::Text), head("Items", ColumnKind::Number), head("Amount", ColumnKind::Money)];
        let w = columns.len();
        let count = |list: &[&RecLine], increases: bool| {
            let part: Vec<&&RecLine> = list.iter().filter(|l| (l.amount_cents > 0) == increases).collect();
            (part.len(), part.iter().map(|l| l.amount_cents).sum::<i64>())
        };
        let mut rows = vec![labeled(RowKind::Detail, 0, w, "Beginning Balance", &[Value::Null, money(h.beginning_balance_cents)])];
        let block = |rows: &mut Vec<ReportRow>, title: &str, list: &[&RecLine]| {
            rows.push(section(0, w, title));
            for (label, increases) in [(down, false), (up, true)] {
                let (n, total) = count(list, increases);
                rows.push(labeled(RowKind::Detail, 1, w, label, &[json!(n), money(total)]));
            }
            let total: i64 = list.iter().map(|l| l.amount_cents).sum();
            rows.push(labeled(RowKind::Subtotal, 0, w, &format!("Total {title}"), &[json!(list.len()), money(total)]));
        };
        block(&mut rows, "Cleared Transactions", &cleared);
        rows.push(labeled(RowKind::Subtotal, 0, w, "Cleared Balance", &[Value::Null, money(cleared_balance)]));
        rows.push(labeled(RowKind::Detail, 0, w, "Statement Ending Balance", &[Value::Null, money(h.statement_ending_balance_cents)]));
        block(&mut rows, &format!("Uncleared Transactions as of {}", h.statement_end_date), &uncleared);
        rows.push(labeled(RowKind::Total, 0, w, &format!("Register Balance as of {}", h.statement_end_date), &[Value::Null, money(register)]));
        (columns, rows)
    };
    let mut out = report(None, end, columns, rows, notes);
    out.report.line_count = lines.len();
    Ok(out)
}

// ------------------------------------------------------------------------------------------------
// Reconciliation discrepancy
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
struct SnapshotCheck {
    journal_entry_id: i64,
    entry_number: i64,
    entry_date: String,
    source_type: String,
    memo: Option<String>,
    amount_cents: i64,
    /// The line as it is now (all `None` when it is gone).
    now_exists: bool,
    now_reconciliation_id: Option<i64>,
    now_account_id: Option<i64>,
    now_date: Option<String>,
    now_amount_cents: Option<i64>,
    now_voided: Option<bool>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct AddedLine {
    journal_entry_id: i64,
    entry_number: i64,
    entry_date: String,
    source_type: String,
    memo: Option<String>,
    amount_cents: i64,
}

struct Discrepancy {
    kind: String,
    date: Option<String>,
    num: Option<String>,
    memo: Option<String>,
    change: String,
    reconciled: i64,
    current: i64,
    journal_entry_id: Option<i64>,
}

/// Cleared transactions changed, voided, deleted or added after their reconciliation was completed,
/// for one account or (`None`) all of them.
pub async fn discrepancy(pool: &SqlitePool, account_id: Option<i64>, today: NaiveDate) -> Result<AccountantReport, String> {
    #[derive(sqlx::FromRow)]
    struct Rec {
        id: i64,
        account_id: i64,
        account_name: String,
        statement_end_date: String,
        statement_ending_balance_cents: i64,
        beginning_balance_cents: i64,
        has_snapshot: bool,
        cleared_now_cents: i64,
    }
    let recs: Vec<Rec> = sqlx::query_as(&format!(
        "SELECT r.id, r.account_id, ca.full_name AS account_name, r.statement_end_date, r.statement_ending_balance_cents, r.beginning_balance_cents,
                EXISTS (SELECT 1 FROM reconciliation_snapshots s WHERE s.reconciliation_id = r.id) AS has_snapshot,
                COALESCE((SELECT SUM({NORMAL_SIGNED}) FROM journal_lines l
                          JOIN chart_of_accounts a ON a.id = l.account_id JOIN account_types t ON t.code = a.account_type
                          WHERE l.reconciliation_id = r.id), 0) AS cleared_now_cents
         FROM reconciliations r JOIN v_chart_of_accounts ca ON ca.id = r.account_id
         WHERE r.is_locked = 1 AND (?1 IS NULL OR r.account_id = ?1)
         ORDER BY ca.full_name COLLATE NOCASE, r.account_id, r.statement_end_date"
    ))
    .bind(account_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let columns = vec![
        head("Type", ColumnKind::Text),
        head("Date", ColumnKind::Date),
        head("Entry", ColumnKind::Text),
        head("Memo", ColumnKind::Text),
        head("Change", ColumnKind::Text),
        head("Reconciled Amount", ColumnKind::Money),
        head("Current Amount", ColumnKind::Money),
        head("Effect on Reconciled Balance", ColumnKind::Money),
    ];
    let w = columns.len();
    let mut rows = Vec::new();
    let mut found = 0usize;
    let mut totals_only = 0usize;
    let mut grand = 0i64;
    let mut previous_ending: Option<(i64, i64)> = None; // (account, ending balance)
    for r in &recs {
        let mut found_here = Vec::new();
        // Each statement starts where the one before it ended.
        if let Some((account, ending)) = previous_ending {
            if account == r.account_id && ending != r.beginning_balance_cents {
                found_here.push(Discrepancy {
                    kind: "Beginning balance".into(),
                    date: Some(r.statement_end_date.clone()),
                    num: None,
                    memo: None,
                    change: "Differs from the previous statement's ending balance".into(),
                    reconciled: ending,
                    current: r.beginning_balance_cents,
                    journal_entry_id: None,
                });
            }
        }
        previous_ending = Some((r.account_id, r.statement_ending_balance_cents));
        if r.has_snapshot {
            let checks: Vec<SnapshotCheck> = sqlx::query_as(&format!(
                "SELECT s.journal_entry_id, s.entry_number, s.entry_date, s.source_type, s.memo, s.amount_cents,
                        l.id IS NOT NULL AS now_exists, l.reconciliation_id AS now_reconciliation_id, l.account_id AS now_account_id,
                        e.entry_date AS now_date, CASE WHEN l.id IS NOT NULL THEN {NORMAL_SIGNED} END AS now_amount_cents,
                        e.is_voided = 1 OR e.status <> 'POSTED' AS now_voided
                 FROM reconciliation_snapshot_lines s
                 LEFT JOIN journal_lines l ON l.id = s.journal_line_id
                 LEFT JOIN journal_entries e ON e.id = l.journal_entry_id
                 LEFT JOIN chart_of_accounts a ON a.id = l.account_id
                 LEFT JOIN account_types t ON t.code = a.account_type
                 WHERE s.reconciliation_id = ?1
                 ORDER BY s.entry_date, s.entry_number, s.journal_line_id"
            ))
            .bind(r.id)
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
            for c in checks {
                let base = |change: &str, current: i64, entry: Option<i64>| Discrepancy {
                    kind: type_label(&c.source_type).to_string(),
                    date: Some(c.entry_date.clone()),
                    num: Some(c.entry_number.to_string()),
                    memo: c.memo.clone(),
                    change: change.into(),
                    reconciled: c.amount_cents,
                    current,
                    journal_entry_id: entry,
                };
                if !c.now_exists {
                    found_here.push(base("Deleted", 0, None));
                    continue;
                }
                let entry = Some(c.journal_entry_id);
                if c.now_account_id != Some(r.account_id) {
                    found_here.push(base("Moved to another account", 0, entry));
                } else if c.now_voided == Some(true) {
                    found_here.push(base("Voided", 0, entry));
                } else if c.now_reconciliation_id != Some(r.id) {
                    found_here.push(base("No longer cleared", 0, entry));
                } else {
                    let now = c.now_amount_cents.unwrap_or(0);
                    if now != c.amount_cents {
                        found_here.push(base("Amount changed", now, entry));
                    }
                    if c.now_date.as_deref() != Some(c.entry_date.as_str()) {
                        let mut d = base(&format!("Date changed to {}", c.now_date.clone().unwrap_or_default()), now, entry);
                        d.reconciled = now;
                        found_here.push(d);
                    }
                }
            }
            let added: Vec<AddedLine> = sqlx::query_as(&format!(
                "SELECT e.id AS journal_entry_id, e.entry_number, e.entry_date, {DISPLAY_SOURCE_TYPE} AS source_type,
                        COALESCE(l.memo, e.memo) AS memo, {NORMAL_SIGNED} AS amount_cents
                 FROM journal_lines l
                 JOIN journal_entries e ON e.id = l.journal_entry_id
                 JOIN chart_of_accounts a ON a.id = l.account_id
                 JOIN account_types t ON t.code = a.account_type
                 WHERE l.reconciliation_id = ?1
                   AND NOT EXISTS (SELECT 1 FROM reconciliation_snapshot_lines s WHERE s.reconciliation_id = ?1 AND s.journal_line_id = l.id)
                 ORDER BY e.entry_date, e.entry_number, l.line_number"
            ))
            .bind(r.id)
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
            for a in added {
                found_here.push(Discrepancy {
                    kind: type_label(&a.source_type).to_string(),
                    date: Some(a.entry_date),
                    num: Some(a.entry_number.to_string()),
                    memo: a.memo,
                    change: "Cleared after the reconciliation was completed".into(),
                    reconciled: 0,
                    current: a.amount_cents,
                    journal_entry_id: Some(a.journal_entry_id),
                });
            }
        } else {
            totals_only += 1;
            let reconciled = r.statement_ending_balance_cents - r.beginning_balance_cents;
            if r.cleared_now_cents != reconciled {
                found_here.push(Discrepancy {
                    kind: "Cleared total".into(),
                    date: Some(r.statement_end_date.clone()),
                    num: None,
                    memo: None,
                    change: "Cleared transactions no longer add up to the statement (checked by total only)".into(),
                    reconciled,
                    current: r.cleared_now_cents,
                    journal_entry_id: None,
                });
            }
        }
        if found_here.is_empty() {
            continue;
        }
        rows.push(section(0, w, &format!("{} · statement ending {}", r.account_name, r.statement_end_date)));
        let mut total = 0;
        for d in found_here {
            let effect = d.current - d.reconciled;
            total += effect;
            found += 1;
            let mut out = row(
                RowKind::Detail,
                1,
                vec![json!(d.kind), json!(d.date), json!(d.num), json!(d.memo), json!(d.change), json!(d.reconciled), json!(d.current), json!(effect)],
            );
            out.journal_entry_id = d.journal_entry_id;
            rows.push(out);
        }
        grand += total;
        rows.push(labeled(RowKind::Subtotal, 0, w, &format!("Total {} · {}", r.account_name, r.statement_end_date), &[json!(total)]));
    }
    if found > 0 {
        rows.push(labeled(RowKind::Total, 0, w, "TOTAL", &[json!(grand)]));
    }
    let mut notes = vec![format!(
        "{} completed reconciliation{} checked; {} change{} found.",
        recs.len(),
        if recs.len() == 1 { "" } else { "s" },
        found,
        if found == 1 { "" } else { "s" }
    )];
    if totals_only > 0 {
        notes.push(format!(
            "{totals_only} of them {} completed before LocalBooks kept a copy of each reconciliation's cleared transactions, so {} checked by {} totals only.",
            if totals_only == 1 { "was" } else { "were" },
            if totals_only == 1 { "it is" } else { "they are" },
            if totals_only == 1 { "its" } else { "their" },
        ));
    }
    let mut out = report(None, today, columns, rows, notes);
    out.report.line_count = found;
    Ok(out)
}

// ------------------------------------------------------------------------------------------------
// Tax lines
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TaxLineKind {
    /// Counted credits minus debits for the year.
    Income,
    /// Counted debits minus credits for the year.
    Deduction,
    /// A Schedule L line: the account's balance at the end of the year.
    BalanceSheet,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxLine {
    /// `<form>:<key>`, the form being a `preferences::TAX_FORMS` code.
    pub code: String,
    pub form: &'static str,
    /// The line as the form numbers it (`8`, `16a`, `Sch. K 13a`, `Sch. L`).
    pub line: &'static str,
    pub label: &'static str,
    pub kind: TaxLineKind,
}

impl TaxLine {
    /// "8 · Advertising".
    pub fn title(&self) -> String {
        format!("{} · {}", self.line, self.label)
    }
}

use TaxLineKind::{BalanceSheet as B, Deduction as D, Income as I};

const SCHEDULE_C: &[(&str, &str, TaxLineKind)] = &[
    ("1", "Gross receipts or sales", I),
    ("2", "Returns and allowances", I),
    ("4", "Cost of goods sold", D),
    ("6", "Other income", I),
    ("8", "Advertising", D),
    ("9", "Car and truck expenses", D),
    ("10", "Commissions and fees", D),
    ("11", "Contract labor", D),
    ("12", "Depletion", D),
    ("13", "Depreciation and section 179 expense", D),
    ("14", "Employee benefit programs", D),
    ("15", "Insurance (other than health)", D),
    ("16a", "Interest: mortgage", D),
    ("16b", "Interest: other", D),
    ("17", "Legal and professional services", D),
    ("18", "Office expense", D),
    ("19", "Pension and profit-sharing plans", D),
    ("20a", "Rent or lease: vehicles, machinery and equipment", D),
    ("20b", "Rent or lease: other business property", D),
    ("21", "Repairs and maintenance", D),
    ("22", "Supplies", D),
    ("23", "Taxes and licenses", D),
    ("24a", "Travel", D),
    ("24b", "Deductible meals", D),
    ("25", "Utilities", D),
    ("26", "Wages", D),
    ("27a", "Other expenses", D),
    ("30", "Business use of home", D),
];

const FORM_1065: &[(&str, &str, TaxLineKind)] = &[
    ("1a", "Gross receipts or sales", I),
    ("1b", "Returns and allowances", I),
    ("2", "Cost of goods sold", D),
    ("6", "Net gain (loss) from Form 4797", I),
    ("7", "Other income", I),
    ("9", "Salaries and wages", D),
    ("10", "Guaranteed payments to partners", D),
    ("11", "Repairs and maintenance", D),
    ("12", "Bad debts", D),
    ("13", "Rent", D),
    ("14", "Taxes and licenses", D),
    ("15", "Interest", D),
    ("16a", "Depreciation", D),
    ("17", "Depletion", D),
    ("18", "Retirement plans", D),
    ("19", "Employee benefit programs", D),
    ("21", "Other deductions", D),
    ("Sch. K 5", "Interest income", I),
    ("Sch. K 6a", "Ordinary dividends", I),
    ("Sch. K 12", "Section 179 deduction", D),
    ("Sch. K 13a", "Charitable contributions", D),
];

const FORM_1120S: &[(&str, &str, TaxLineKind)] = &[
    ("1a", "Gross receipts or sales", I),
    ("1b", "Returns and allowances", I),
    ("2", "Cost of goods sold", D),
    ("4", "Net gain (loss) from Form 4797", I),
    ("5", "Other income", I),
    ("7", "Compensation of officers", D),
    ("8", "Salaries and wages", D),
    ("9", "Repairs and maintenance", D),
    ("10", "Bad debts", D),
    ("11", "Rents", D),
    ("12", "Taxes and licenses", D),
    ("13", "Interest", D),
    ("14", "Depreciation", D),
    ("15", "Depletion", D),
    ("16", "Advertising", D),
    ("17", "Pension, profit-sharing, etc., plans", D),
    ("18", "Employee benefit programs", D),
    ("20", "Other deductions", D),
    ("Sch. K 4", "Interest income", I),
    ("Sch. K 5a", "Ordinary dividends", I),
    ("Sch. K 11", "Section 179 deduction", D),
    ("Sch. K 12a", "Charitable contributions", D),
];

const FORM_1120: &[(&str, &str, TaxLineKind)] = &[
    ("1a", "Gross receipts or sales", I),
    ("1b", "Returns and allowances", I),
    ("2", "Cost of goods sold", D),
    ("4", "Dividends and inclusions", I),
    ("5", "Interest", I),
    ("6", "Gross rents", I),
    ("7", "Gross royalties", I),
    ("8", "Capital gain net income", I),
    ("9", "Net gain (loss) from Form 4797", I),
    ("10", "Other income", I),
    ("12", "Compensation of officers", D),
    ("13", "Salaries and wages", D),
    ("14", "Repairs and maintenance", D),
    ("15", "Bad debts", D),
    ("16", "Rents", D),
    ("17", "Taxes and licenses", D),
    ("18", "Interest", D),
    ("19", "Charitable contributions", D),
    ("20", "Depreciation", D),
    ("21", "Depletion", D),
    ("22", "Advertising", D),
    ("23", "Pension, profit-sharing, etc., plans", D),
    ("24", "Employee benefit programs", D),
    ("26", "Other deductions", D),
];

/// Form 990: revenue (Part VIII), functional expenses (Part IX) and the balance sheet (Part X).
const FORM_990: &[(&str, &str, TaxLineKind)] = &[
    ("VIII 1a", "Federated campaigns", I),
    ("VIII 1b", "Membership dues", I),
    ("VIII 1c", "Fundraising events (contributions)", I),
    ("VIII 1d", "Related organizations", I),
    ("VIII 1e", "Government grants (contributions)", I),
    ("VIII 1f", "All other contributions, gifts and grants", I),
    ("VIII 2", "Program service revenue", I),
    ("VIII 3", "Investment income", I),
    ("VIII 5", "Royalties", I),
    ("VIII 6", "Net rental income", I),
    ("VIII 7", "Net gain (loss) from sales of assets other than inventory", I),
    ("VIII 8", "Net income from fundraising events", I),
    ("VIII 9", "Net income from gaming activities", I),
    ("VIII 10", "Net income from sales of inventory", I),
    ("VIII 11", "Miscellaneous revenue", I),
    ("IX 1", "Grants to domestic organizations and governments", D),
    ("IX 2", "Grants to domestic individuals", D),
    ("IX 4", "Benefits paid to or for members", D),
    ("IX 5", "Compensation of officers, directors and key employees", D),
    ("IX 7", "Other salaries and wages", D),
    ("IX 8", "Pension plan contributions", D),
    ("IX 9", "Other employee benefits", D),
    ("IX 10", "Payroll taxes", D),
    ("IX 11a", "Management fees", D),
    ("IX 11b", "Legal fees", D),
    ("IX 11c", "Accounting fees", D),
    ("IX 11e", "Professional fundraising services", D),
    ("IX 11g", "Other fees for services", D),
    ("IX 12", "Advertising and promotion", D),
    ("IX 13", "Office expenses", D),
    ("IX 14", "Information technology", D),
    ("IX 16", "Occupancy", D),
    ("IX 17", "Travel", D),
    ("IX 19", "Conferences, conventions and meetings", D),
    ("IX 20", "Interest", D),
    ("IX 21", "Payments to affiliates", D),
    ("IX 22", "Depreciation, depletion and amortization", D),
    ("IX 23", "Insurance", D),
    ("IX 24", "Other expenses", D),
    ("X 1", "Cash (non-interest-bearing)", B),
    ("X 2", "Savings and temporary cash investments", B),
    ("X 3", "Pledges and grants receivable", B),
    ("X 4", "Accounts receivable", B),
    ("X 8", "Inventories for sale or use", B),
    ("X 9", "Prepaid expenses and deferred charges", B),
    ("X 10", "Land, buildings and equipment, net", B),
    ("X 11", "Investments", B),
    ("X 15", "Other assets", B),
    ("X 17", "Accounts payable and accrued expenses", B),
    ("X 18", "Grants payable", B),
    ("X 19", "Deferred revenue", B),
    ("X 23", "Secured mortgages and notes payable", B),
    ("X 25", "Other liabilities", B),
    ("X 27", "Net assets without donor restrictions", B),
    ("X 28", "Net assets with donor restrictions", B),
];

/// Schedule L (balance sheet) lines, shared by the 1065, 1120-S and 1120. `{owners}` lines differ.
const SCHEDULE_L: &[(&str, &str, TaxLineKind)] = &[
    ("L-cash", "Cash", B),
    ("L-receivables", "Trade notes and accounts receivable", B),
    ("L-inventories", "Inventories", B),
    ("L-other-current-assets", "Other current assets", B),
    ("L-depreciable", "Buildings and other depreciable assets", B),
    ("L-accumulated-depreciation", "Less accumulated depreciation", B),
    ("L-land", "Land (net of any amortization)", B),
    ("L-other-assets", "Other assets", B),
    ("L-payables", "Accounts payable", B),
    ("L-notes-short", "Mortgages, notes, bonds payable in less than 1 year", B),
    ("L-other-current-liabilities", "Other current liabilities", B),
    ("L-owner-loans", "Loans from {owners}", B),
    ("L-notes-long", "Mortgages, notes, bonds payable in 1 year or more", B),
    ("L-other-liabilities", "Other liabilities", B),
];

/// Every tax line LocalBooks knows, form by form in `preferences::TAX_FORMS` order.
pub fn tax_lines() -> Vec<TaxLine> {
    let mut out = Vec::new();
    let mut add = |form: &'static str, lines: &[(&'static str, &'static str, TaxLineKind)]| {
        for (line, label, kind) in lines {
            out.push(TaxLine { code: format!("{form}:{line}"), form, line, label, kind: *kind });
        }
    };
    add("SCHEDULE_C", SCHEDULE_C);
    add("1065", FORM_1065);
    add("1120S", FORM_1120S);
    add("1120", FORM_1120);
    add("990", FORM_990);
    for (form, owners, equity) in [
        ("1065", "partners", &[("L-capital", "Partners' capital accounts")][..]),
        ("1120S", "shareholders", &[("L-capital-stock", "Capital stock"), ("L-retained-earnings", "Retained earnings"), ("L-distributions", "Shareholder distributions")][..]),
        ("1120", "shareholders", &[("L-capital-stock", "Capital stock"), ("L-retained-earnings", "Retained earnings")][..]),
    ] {
        for (key, label, kind) in SCHEDULE_L.iter().copied().chain(equity.iter().map(|(k, l)| (*k, *l, B))) {
            // Static labels for the two spellings of the owners' loans line.
            let label: &'static str = match (key, owners) {
                ("L-owner-loans", "partners") => "Loans from partners",
                ("L-owner-loans", _) => "Loans from shareholders",
                _ => label,
            };
            out.push(TaxLine { code: format!("{form}:{key}"), form, line: "Sch. L", label, kind });
        }
    }
    out
}

/// How an imported QuickBooks tax line number is kept until it is matched to a LocalBooks line.
pub fn quickbooks_tax_line(number: i64) -> String {
    format!("QuickBooks tax line {number}")
}

fn simplify(text: &str) -> String {
    text.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase()
}

/// The tax line an import names, as text like `Sch C: Advertising` or `Form 1120S: Rents`: the
/// matching LocalBooks code, or the text itself when nothing matches. `None` for unassigned.
pub fn imported_tax_line(text: &str) -> Option<String> {
    let text = text.trim();
    let simple = simplify(text);
    if simple.is_empty() || simple == "unassigned" || simple.starts_with("nottaxrelated") || simple == "0" {
        return None;
    }
    let (prefix, label) = text.split_once(':').map_or(("", text), |(p, l)| (p, l));
    let prefix = simplify(prefix);
    let form = if prefix.contains("schc") || prefix.contains("schedulec") {
        Some("SCHEDULE_C")
    } else if prefix.contains("1065") {
        Some("1065")
    } else if prefix.contains("1120s") {
        Some("1120S")
    } else if prefix.contains("1120") {
        Some("1120")
    } else if prefix.contains("990") {
        Some("990")
    } else {
        None
    };
    let label = simplify(label);
    let found = tax_lines().into_iter().find(|t| form.is_some_and(|f| t.form == f) && simplify(t.label) == label);
    Some(found.map_or_else(|| text.chars().take(200).collect(), |t| t.code))
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AccountTaxLine {
    pub account_id: i64,
    pub account_name: String,
    pub account_type: String,
    pub classification: String,
    pub is_active: bool,
    pub tax_line: Option<String>,
}

/// Every account with its tax line, in chart of accounts order.
pub async fn account_tax_lines(pool: &SqlitePool) -> Result<Vec<AccountTaxLine>, String> {
    sqlx::query_as(
        "SELECT a.id AS account_id, a.full_name AS account_name, a.account_type, a.classification, a.is_active = 1 AS is_active, c.tax_line
         FROM v_chart_of_accounts a JOIN chart_of_accounts c ON c.id = a.id
         ORDER BY a.type_sort_order, a.account_number, a.full_name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Assigns a LocalBooks tax line to an account, or clears it (`None`). Balance sheet (Schedule L)
/// lines are for asset, liability and equity accounts; the others for income and expense accounts.
pub async fn set_account_tax_line(pool: &SqlitePool, account_id: i64, tax_line: Option<&str>) -> Result<(), String> {
    let classification: String = sqlx::query_scalar("SELECT classification FROM v_chart_of_accounts WHERE id = ?1")
        .bind(account_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("COA_NOT_FOUND: account {account_id} does not exist"))?;
    let code = tax_line.map(str::trim).filter(|c| !c.is_empty());
    if let Some(code) = code {
        let line = tax_lines().into_iter().find(|t| t.code == code).ok_or_else(|| format!("TAX_LINE_UNKNOWN: '{code}' is not a tax line"))?;
        let balance_sheet = matches!(classification.as_str(), "ASSET" | "LIABILITY" | "EQUITY");
        if balance_sheet != (line.kind == TaxLineKind::BalanceSheet) {
            return Err(if balance_sheet {
                "TAX_LINE_KIND: a balance sheet account takes a Schedule L line".into()
            } else {
                "TAX_LINE_KIND: an income or expense account takes an income or deduction line, not a Schedule L line".into()
            });
        }
    }
    sqlx::query("UPDATE chart_of_accounts SET tax_line = ?1 WHERE id = ?2").bind(code).bind(account_id).execute(pool).await.map_err(db_err)?;
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Income tax reports
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
struct TaxAccount {
    account_id: i64,
    account_name: String,
    classification: String,
    debit_normal: bool,
    tax_line: Option<String>,
    /// Debits minus credits in the period (income and expense) or through its end (balance sheet).
    net_cents: i64,
}

/// Where an account lands on the report.
enum Placement {
    Line(usize),
    /// A line of another form, or imported text no LocalBooks line matches.
    Other(String),
    Unassigned,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct TaxDetailLine {
    journal_entry_id: i64,
    account_id: i64,
    date: String,
    source_type: String,
    is_reversal: bool,
    num: Option<String>,
    name: Option<String>,
    memo: Option<String>,
    net_cents: i64,
}

/// Income Tax Summary (`detail` = false: one row per tax line) or Detail (each transaction, grouped
/// by tax line) for `form`, a `preferences::TAX_FORMS` code. Income and deduction lines total the
/// period's activity; Schedule L lines show balances at its end. Accounts with activity but no tax
/// line are listed under Unassigned, and those with another form's line or an unmatched imported
/// one under Other tax lines. `only` limits the detail to one tax line.
pub async fn income_tax(pool: &SqlitePool, form: &str, start: NaiveDate, end: NaiveDate, detail: bool, only: Option<&str>) -> Result<AccountantReport, String> {
    if !crate::preferences::TAX_FORMS.iter().any(|(code, _)| *code == form) {
        return Err(format!("PREF_BAD_TAX_FORM: unknown tax form '{form}'"));
    }
    if start > end {
        return Err("RPT_BAD_RANGE: the start date is after the end date".into());
    }
    let accounts: Vec<TaxAccount> = sqlx::query_as(
        "SELECT a.id AS account_id, a.full_name AS account_name, a.classification, a.normal_balance = 'DEBIT' AS debit_normal, c.tax_line,
                COALESCE((SELECT SUM(l.debit_cents - l.credit_cents) FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
                          WHERE l.account_id = a.id AND e.status = 'POSTED' AND e.entry_date <= ?2
                            AND (a.classification IN ('ASSET', 'LIABILITY', 'EQUITY') OR e.entry_date >= ?1)), 0) AS net_cents
         FROM v_chart_of_accounts a JOIN chart_of_accounts c ON c.id = a.id
         ORDER BY a.type_sort_order, a.account_number, a.full_name COLLATE NOCASE",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let all = tax_lines();
    let lines: Vec<&TaxLine> = all.iter().filter(|t| t.form == form).collect();
    let label_of = |code: &str| all.iter().find(|t| t.code == code).map_or_else(|| code.to_string(), |t| format!("{} ({})", t.title(), form_name(t.form)));

    let place = |a: &TaxAccount| -> Option<Placement> {
        let balance_sheet = matches!(a.classification.as_str(), "ASSET" | "LIABILITY" | "EQUITY");
        match a.tax_line.as_deref() {
            Some(code) => Some(match lines.iter().position(|t| t.code == code) {
                Some(i) => Placement::Line(i),
                None => Placement::Other(label_of(code)),
            }),
            // Only income and expense accounts must have a line; unmapped balance sheet accounts are left out.
            None if !balance_sheet => Some(Placement::Unassigned),
            None => None,
        }
    };
    // Amount on the line's side: income credits minus debits, deductions debits minus credits,
    // Schedule L balances on the account's normal side.
    let side = |kind: Option<TaxLineKind>, a: &TaxAccount, net: i64| match kind {
        Some(TaxLineKind::Income) => -net,
        Some(TaxLineKind::Deduction) => net,
        Some(TaxLineKind::BalanceSheet) | None if a.debit_normal => net,
        _ => -net,
    };
    let kind_of = |a: &TaxAccount| a.tax_line.as_deref().and_then(|c| all.iter().find(|t| t.code == c)).map(|t| t.kind);

    // Buckets in report order: the form's lines, then other lines by label, then unassigned.
    let mut by_line: BTreeMap<usize, Vec<&TaxAccount>> = BTreeMap::new();
    let mut other: BTreeMap<String, Vec<&TaxAccount>> = BTreeMap::new();
    let mut unassigned: Vec<&TaxAccount> = Vec::new();
    for a in &accounts {
        if a.net_cents == 0 {
            continue;
        }
        match place(a) {
            Some(Placement::Line(i)) => by_line.entry(i).or_default().push(a),
            Some(Placement::Other(label)) => other.entry(label).or_default().push(a),
            Some(Placement::Unassigned) => unassigned.push(a),
            None => {}
        }
    }
    let keep = |code: Option<&str>| only.is_none_or(|o| code == Some(o));

    let detail_lines: Vec<TaxDetailLine> = if detail {
        sqlx::query_as(&format!(
            "SELECT e.id AS journal_entry_id, l.account_id, e.entry_date AS date, {DISPLAY_SOURCE_TYPE} AS source_type,
                    e.reverses_entry_id IS NOT NULL AS is_reversal, {NUM} AS num, p.party_name AS name,
                    COALESCE(l.memo, e.memo) AS memo, l.debit_cents - l.credit_cents AS net_cents
             FROM journal_lines l
             JOIN journal_entries e ON e.id = l.journal_entry_id
             JOIN v_chart_of_accounts a ON a.id = l.account_id
             {NUM_JOINS}
             WHERE e.status = 'POSTED' AND e.entry_date BETWEEN ?1 AND ?2 AND a.classification IN ('INCOME', 'EXPENSE')
             ORDER BY e.entry_date, e.entry_number, l.line_number"
        ))
        .bind(start)
        .bind(end)
        .fetch_all(pool)
        .await
        .map_err(db_err)?
    } else {
        Vec::new()
    };

    let columns = if detail {
        vec![
            head("Type", ColumnKind::Text),
            head("Date", ColumnKind::Date),
            head("Num", ColumnKind::Text),
            head("Name", ColumnKind::Text),
            head("Memo", ColumnKind::Text),
            head("Account", ColumnKind::Text),
            head("Amount", ColumnKind::Money),
        ]
    } else {
        vec![head("Tax line", ColumnKind::Text), head("Amount", ColumnKind::Money)]
    };
    let w = columns.len();
    let mut rows = Vec::new();
    let mut detail_count = 0usize;
    // One block per tax line: its accounts (summary) or transactions (detail), and its total.
    let mut block = |rows: &mut Vec<ReportRow>, title: &str, kind: Option<TaxLineKind>, accts: &[&TaxAccount]| {
        let total: i64 = accts.iter().map(|a| side(kind.or(kind_of(a)), a, a.net_cents)).sum();
        if !detail {
            let mut r = labeled(RowKind::Detail, 1, w, title, &[json!(total)]);
            r.drill = Some(crate::custom_reports::Drill { account_ids: accts.iter().map(|a| a.account_id).collect(), start_date: Some(start), end_date: Some(end), ..Default::default() });
            rows.push(r);
            return;
        }
        rows.push(section(1, w, title));
        for a in accts {
            let k = kind.or(kind_of(a));
            if a.classification != "INCOME" && a.classification != "EXPENSE" {
                let mut r = labeled(RowKind::Detail, 2, w, &format!("Balance as of {end}"), &[json!(a.account_name), json!(side(k, a, a.net_cents))]);
                r.account_id = Some(a.account_id);
                rows.push(r);
                detail_count += 1;
                continue;
            }
            for l in detail_lines.iter().filter(|l| l.account_id == a.account_id) {
                let mut r = row(
                    RowKind::Detail,
                    2,
                    vec![json!(type_text(&l.source_type, l.is_reversal)), json!(l.date), json!(l.num), json!(l.name), json!(l.memo), json!(a.account_name), json!(side(k, a, l.net_cents))],
                );
                r.journal_entry_id = Some(l.journal_entry_id);
                rows.push(r);
                detail_count += 1;
            }
        }
        rows.push(labeled(RowKind::Subtotal, 1, w, &format!("Total {title}"), &[json!(total)]));
    };
    let balance_sheet = if form == "990" { "Balance sheet (Part X)" } else { "Balance sheet (Schedule L)" };
    for (heading, kind) in [("Income", TaxLineKind::Income), ("Deductions", TaxLineKind::Deduction), (balance_sheet, TaxLineKind::BalanceSheet)] {
        let blocks: Vec<(&usize, &Vec<&TaxAccount>)> = by_line.iter().filter(|(i, _)| lines[**i].kind == kind && keep(Some(lines[**i].code.as_str()))).collect();
        if blocks.is_empty() {
            continue;
        }
        rows.push(section(0, w, heading));
        let mut sum = 0;
        for (i, accts) in blocks {
            let t = lines[*i];
            sum += accts.iter().map(|a| side(Some(kind), a, a.net_cents)).sum::<i64>();
            block(&mut rows, &t.title(), Some(kind), accts);
        }
        if kind != TaxLineKind::BalanceSheet {
            rows.push(labeled(RowKind::Subtotal, 0, w, &format!("Total {heading}"), &[json!(sum)]));
        }
    }
    if only.is_none() && !other.is_empty() {
        rows.push(section(0, w, "Other tax lines (another form, or imported and not yet matched)"));
        for (label, accts) in &other {
            block(&mut rows, label, None, accts);
        }
    }
    if only.is_none() && !unassigned.is_empty() {
        rows.push(section(0, w, "Unassigned (income and expense accounts with no tax line)"));
        for a in &unassigned {
            block(&mut rows, &a.account_name, None, std::slice::from_ref(a));
        }
    }

    let mut notes = vec![format!("{} · {} through {}", form_name(form), start, end)];
    if !unassigned.is_empty() {
        notes.push(format!(
            "{} income or expense account{} with activity {} no tax line; assign one under Tax lines.",
            unassigned.len(),
            if unassigned.len() == 1 { "" } else { "s" },
            if unassigned.len() == 1 { "has" } else { "have" }
        ));
    }
    notes.push("Amounts are from the books on the accrual basis; adjust for your return as your tax preparer advises.".into());
    let mut out = report(Some(start), end, columns, rows, notes);
    out.report.line_count = if detail { detail_count } else { out.report.rows.iter().filter(|r| r.kind == RowKind::Detail).count() };
    Ok(out)
}

fn form_name(form: &str) -> &'static str {
    match form {
        "SCHEDULE_C" => "Schedule C (Form 1040)",
        "1065" => "Form 1065",
        "1120S" => "Form 1120-S",
        "1120" => "Form 1120",
        "990" => "Form 990",
        _ => "Other form",
    }
}

// ------------------------------------------------------------------------------------------------
// Collections and open purchase orders
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
struct Overdue {
    customer_id: i64,
    customer: String,
    company_name: Option<String>,
    phone: Option<String>,
    email: Option<String>,
    journal_entry_id: i64,
    number: String,
    date: String,
    due_date: String,
    open_cents: i64,
}

/// Invoices past due and still open as of `as_of`, by customer, with how to reach each one.
pub async fn collections(pool: &SqlitePool, as_of: NaiveDate) -> Result<AccountantReport, String> {
    // Open balances follow `report_center::aging`.
    let overdue: Vec<Overdue> = sqlx::query_as(
        "SELECT * FROM (
            SELECT c.id AS customer_id, c.name AS customer, c.company_name, c.phone, c.email, i.journal_entry_id,
                   CAST(i.invoice_number AS TEXT) AS number, i.issue_date AS date, i.due_date,
                   COALESCE(i.home_total_cents, i.total_cents)
                   - COALESCE((SELECT SUM(COALESCE(p.ar_relief_cents, p.amount_cents)) FROM invoice_payments p
                               WHERE p.invoice_id = i.id AND p.is_voided = 0 AND p.payment_date <= ?1), 0)
                   - COALESCE((SELECT SUM(a.amount_cents) FROM credit_applications a WHERE a.invoice_id = i.id AND a.is_voided = 0 AND a.applied_date <= ?1), 0)
                   AS open_cents
            FROM invoices i JOIN customers c ON c.id = i.customer_id
            WHERE i.is_voided = 0 AND i.journal_entry_id IS NOT NULL AND i.issue_date <= ?1 AND i.due_date < ?1
         ) WHERE open_cents > 0 ORDER BY customer COLLATE NOCASE, customer_id, due_date, number",
    )
    .bind(as_of)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let columns = vec![
        head("Type", ColumnKind::Text),
        head("Date", ColumnKind::Date),
        head("Num", ColumnKind::Text),
        head("Due Date", ColumnKind::Date),
        head("Aging", ColumnKind::Number),
        head("Open Balance", ColumnKind::Money),
    ];
    let w = columns.len();
    let mut rows = Vec::new();
    let mut total = 0;
    let mut customers = 0;
    let mut i = 0;
    while i < overdue.len() {
        let first = &overdue[i];
        let mut contact = vec![first.customer.clone()];
        contact.extend(first.company_name.iter().filter(|c| !c.eq_ignore_ascii_case(&first.customer)).map(|c| format!("Contact: {c}")));
        contact.extend(first.phone.iter().map(|p| format!("Phone: {p}")));
        contact.extend(first.email.iter().map(|e| format!("Email: {e}")));
        rows.push(section(0, w, &contact.join(" · ")));
        let mut sub = 0;
        while i < overdue.len() && overdue[i].customer_id == first.customer_id {
            let o = &overdue[i];
            let days = (as_of - date(&o.due_date)?).num_days();
            let mut r = row(RowKind::Detail, 1, vec![json!("Invoice"), json!(o.date), json!(o.number), json!(o.due_date), json!(days), json!(o.open_cents)]);
            r.journal_entry_id = Some(o.journal_entry_id);
            rows.push(r);
            sub += o.open_cents;
            i += 1;
        }
        rows.push(labeled(RowKind::Subtotal, 0, w, &format!("Total {}", first.customer), &[json!(sub)]));
        total += sub;
        customers += 1;
    }
    if !overdue.is_empty() {
        rows.push(labeled(RowKind::Total, 0, w, "TOTAL", &[json!(total)]));
    }
    let notes = vec![format!(
        "{} overdue invoice{} from {customers} customer{} as of {as_of}; Aging is days past the due date.",
        overdue.len(),
        if overdue.len() == 1 { "" } else { "s" },
        if customers == 1 { "" } else { "s" }
    )];
    Ok(report(None, as_of, columns, rows, notes))
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct OpenPo {
    vendor_id: i64,
    vendor: String,
    po_number: i64,
    po_date: String,
    expected_date: Option<String>,
    memo: Option<String>,
    total_cents: i64,
}

/// Purchase orders dated on or before `as_of` that are still open (not billed or closed), by vendor.
pub async fn open_purchase_orders(pool: &SqlitePool, as_of: NaiveDate) -> Result<AccountantReport, String> {
    let pos: Vec<OpenPo> = sqlx::query_as(
        "SELECT v.id AS vendor_id, v.name AS vendor, p.po_number, p.po_date, p.expected_date, p.memo, p.total_cents
         FROM purchase_orders p JOIN vendors v ON v.id = p.vendor_id
         WHERE p.status = 'OPEN' AND p.po_date <= ?1
         ORDER BY v.name COLLATE NOCASE, v.id, p.po_date, p.po_number",
    )
    .bind(as_of)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let columns = vec![
        head("Type", ColumnKind::Text),
        head("Date", ColumnKind::Date),
        head("Num", ColumnKind::Text),
        head("Memo", ColumnKind::Text),
        head("Expected", ColumnKind::Date),
        head("Amount", ColumnKind::Money),
    ];
    let w = columns.len();
    let mut rows = Vec::new();
    let mut total = 0;
    let mut i = 0;
    while i < pos.len() {
        let vendor = &pos[i];
        rows.push(section(0, w, &vendor.vendor));
        let mut sub = 0;
        while i < pos.len() && pos[i].vendor_id == vendor.vendor_id {
            let p = &pos[i];
            rows.push(row(RowKind::Detail, 1, vec![json!("Purchase Order"), json!(p.po_date), json!(p.po_number.to_string()), json!(p.memo), json!(p.expected_date), json!(p.total_cents)]));
            sub += p.total_cents;
            i += 1;
        }
        rows.push(labeled(RowKind::Subtotal, 0, w, &format!("Total {}", vendor.vendor), &[json!(sub)]));
        total += sub;
    }
    if !pos.is_empty() {
        rows.push(labeled(RowKind::Total, 0, w, "TOTAL", &[json!(total)]));
    }
    let notes = vec![format!("{} open purchase order{} as of {as_of}. Purchase orders do not post to the books.", pos.len(), if pos.len() == 1 { "" } else { "s" })];
    let mut out = report(None, as_of, columns, rows, notes);
    out.report.line_count = pos.len();
    Ok(out)
}

#[cfg(test)]
mod tests;
