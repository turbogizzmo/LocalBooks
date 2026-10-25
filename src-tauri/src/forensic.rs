//! Reviewed forensic corrections for entries that no longer match their posting snapshot.
//!
//! A posted entry that was changed or deleted outside LocalBooks (detected by
//! [`crate::ledger::verify_ledger_audit`]) can sometimes be neutralised by a new correcting entry:
//! for each account, post the difference between what the snapshot recorded and what the ledger
//! now holds. That works only when those differences balance (the change moved amounts between
//! accounts, or removed a whole balanced entry). When the change left the books out of balance, no
//! balanced entry can repair it, and the right fix is restoring a backup; the proposal says so.
//!
//! Nothing posts automatically. The snapshot lives in the same file as the ledger and could itself
//! have been altered, so a person reviews each proposed correction before [`apply_correction`]
//! posts it (through the normal ledger checks, including the closing date) and records a
//! FORENSIC_HEAL event. The tampered rows are left in place as evidence.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{SqliteConnection, SqlitePool};

use crate::ledger::{self, db_err, EntrySource, JournalEntry, JournalLine};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CorrectionStatus {
    /// A balanced correcting entry restores every account to its snapshot amount.
    Fixable,
    /// Already corrected by a reviewed correcting entry.
    Corrected,
    /// The change left debits and credits unequal; only a backup can repair it.
    OutOfBalance,
    /// Amounts per account are unchanged (e.g. the date or memo was edited); review the entry.
    NoBalanceEffect,
    /// The snapshot refers to an account that no longer exists.
    MissingAccount,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionLine {
    pub account_id: i64,
    #[serde(default)]
    pub account_name: String,
    pub debit_cents: i64,
    pub credit_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionProposal {
    pub entry_id: i64,
    pub entry_number: Option<i64>,
    pub entry_deleted: bool,
    pub status: CorrectionStatus,
    pub explanation: String,
    /// The correcting entry that would be posted (empty unless `Fixable`).
    pub lines: Vec<CorrectionLine>,
    /// Debits minus credits of the change itself (non-zero when `OutOfBalance`).
    pub imbalance_cents: i64,
    pub corrected_by_entry_number: Option<i64>,
}

/// Net (debit − credit) per account.
type Nets = BTreeMap<i64, i64>;

fn snapshot_nets(snapshot: &Value) -> Nets {
    let mut nets = Nets::new();
    for line in snapshot["lines"].as_array().into_iter().flatten() {
        let (Some(account), Some(debit), Some(credit)) = (line["accountId"].as_i64(), line["debitCents"].as_i64(), line["creditCents"].as_i64()) else {
            continue;
        };
        *nets.entry(account).or_insert(0) += debit - credit;
    }
    nets
}

async fn live_nets(conn: &mut SqliteConnection, entry_id: i64) -> Result<Nets, String> {
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT account_id, SUM(debit_cents) - SUM(credit_cents) FROM journal_lines WHERE journal_entry_id = ?1 GROUP BY account_id",
    )
    .bind(entry_id)
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().collect())
}

async fn propose_for(conn: &mut SqliteConnection, entry_id: i64, snapshot: &Value) -> Result<CorrectionProposal, String> {
    let entry_number = snapshot["entryNumber"].as_i64();
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM journal_entries WHERE id = ?1)").bind(entry_id).fetch_one(&mut *conn).await.map_err(db_err)?;
    let mut proposal = CorrectionProposal {
        entry_id,
        entry_number,
        entry_deleted: !exists,
        status: CorrectionStatus::Fixable,
        explanation: String::new(),
        lines: Vec::new(),
        imbalance_cents: 0,
        corrected_by_entry_number: None,
    };

    let corrected: Option<i64> = sqlx::query_scalar(
        "SELECT e.entry_number FROM ledger_audit_log a
         JOIN journal_entries e ON e.id = json_extract(a.payload_snapshot, '$.correctionEntryId')
         WHERE a.action_type = 'FORENSIC_HEAL' AND a.target_entity_id = ?1
           AND e.status = 'POSTED' AND e.is_voided = 0
         ORDER BY a.id DESC LIMIT 1",
    )
    .bind(entry_id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(db_err)?;
    if let Some(number) = corrected {
        proposal.status = CorrectionStatus::Corrected;
        proposal.corrected_by_entry_number = Some(number);
        proposal.explanation = format!("Corrected by entry #{number}. The altered rows are kept as evidence.");
        return Ok(proposal);
    }

    let recorded = snapshot_nets(snapshot);
    let live = if exists { live_nets(conn, entry_id).await? } else { Nets::new() };
    let mut deltas = Nets::new();
    for account in recorded.keys().chain(live.keys()) {
        let delta = recorded.get(account).copied().unwrap_or(0) - live.get(account).copied().unwrap_or(0);
        if delta != 0 {
            deltas.insert(*account, delta);
        }
    }
    let imbalance: i64 = deltas.values().sum();

    if deltas.is_empty() {
        proposal.status = CorrectionStatus::NoBalanceEffect;
        proposal.explanation =
            "Every account still carries the amount recorded at posting, so no correcting entry is needed. Another detail (such as the date or memo) was changed; compare the entry with a backup.".into();
        return Ok(proposal);
    }
    if imbalance != 0 {
        proposal.status = CorrectionStatus::OutOfBalance;
        proposal.imbalance_cents = -imbalance;
        proposal.explanation = format!(
            "The change left debits and credits unequal by {}. A correcting entry must itself balance, so it cannot repair this. Restore a backup taken before the change.",
            ledger::format_cents(imbalance.abs())
        );
        return Ok(proposal);
    }

    for (account, delta) in &deltas {
        let name: Option<String> = sqlx::query_scalar("SELECT full_name FROM v_chart_of_accounts WHERE id = ?1").bind(account).fetch_optional(&mut *conn).await.map_err(db_err)?;
        let Some(account_name) = name else {
            proposal.status = CorrectionStatus::MissingAccount;
            proposal.lines.clear();
            proposal.explanation = format!("Account #{account} recorded in the snapshot no longer exists. Restore a backup taken before the change.");
            return Ok(proposal);
        };
        let (debit_cents, credit_cents) = if *delta > 0 { (*delta, 0) } else { (0, -delta) };
        proposal.lines.push(CorrectionLine { account_id: *account, account_name, debit_cents, credit_cents });
    }
    proposal.explanation = if exists {
        "Restores each account to the amount recorded when this entry was posted.".into()
    } else {
        "This entry was deleted. The correction re-records its effect on each account.".into()
    };
    Ok(proposal)
}

/// A proposal for every posted entry whose snapshot no longer matches the ledger (or that was
/// deleted). Entries that still match are not listed.
pub async fn propose_corrections(pool: &SqlitePool) -> Result<Vec<CorrectionProposal>, String> {
    let snapshots: Vec<(i64, String)> = sqlx::query_as(
        "SELECT a.target_entity_id, a.payload_snapshot FROM ledger_audit_log a
         LEFT JOIN v_entry_snapshot s ON s.entry_id = a.target_entity_id
         WHERE a.action_type = 'POST'
           AND (s.entry_id IS NULL OR json_remove(a.payload_snapshot, '$.backfilled') <> s.snapshot)
         ORDER BY a.target_entity_id",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut conn = pool.acquire().await.map_err(db_err)?;
    let mut proposals = Vec::with_capacity(snapshots.len());
    for (entry_id, payload) in snapshots {
        let snapshot: Value = serde_json::from_str(&payload).map_err(|e| format!("AUDIT_SNAPSHOT: entry {entry_id}: {e}"))?;
        proposals.push(propose_for(&mut conn, entry_id, &snapshot).await?);
    }
    Ok(proposals)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyCorrectionInput {
    pub entry_id: i64,
    /// The lines the reviewer approved; posting proceeds only if they still match the proposal.
    pub lines: Vec<CorrectionLine>,
    pub date: NaiveDate,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedCorrection {
    pub correction_entry_id: i64,
    pub correction_entry_number: i64,
}

/// Posts the reviewed correction for one entry and records a FORENSIC_HEAL event, atomically.
pub async fn apply_correction(pool: &SqlitePool, input: &ApplyCorrectionInput) -> Result<AppliedCorrection, String> {
    // Reserve the writer before reading: competing corrections must revalidate after this commits.
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let payload: Option<String> = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE action_type = 'POST' AND target_entity_id = ?1")
        .bind(input.entry_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
    let snapshot: Value = serde_json::from_str(&payload.ok_or("HEAL_NO_SNAPSHOT: that entry has no posting snapshot")?).map_err(|e| format!("AUDIT_SNAPSHOT: {e}"))?;
    let proposal = propose_for(&mut tx, input.entry_id, &snapshot).await?;
    if proposal.status != CorrectionStatus::Fixable {
        return Err(format!("HEAL_NOT_FIXABLE: {}", proposal.explanation));
    }
    let key = |lines: &[CorrectionLine]| lines.iter().map(|l| (l.account_id, l.debit_cents, l.credit_cents)).collect::<Vec<_>>();
    if key(&proposal.lines) != key(&input.lines) {
        return Err("HEAL_STALE: the ledger changed since this correction was reviewed; review it again".into());
    }

    let label = proposal.entry_number.map(|n| format!("#{n}")).unwrap_or_else(|| format!("id {}", input.entry_id));
    let entry = JournalEntry {
        id: None,
        date: input.date,
        description: format!("Forensic correction for entry {label}: restores the amounts recorded when it was posted"),
        lines: proposal
            .lines
            .iter()
            .map(|l| if l.debit_cents > 0 { JournalLine::debit(l.account_id, l.debit_cents) } else { JournalLine::credit(l.account_id, l.credit_cents) })
            .collect(),
        currency: None,
    };

    let correction_id = match ledger::post_entry_in_tx(&mut tx, &entry, EntrySource::Journal).await {
        Ok(id) => id,
        Err(err) => return Err(ledger::rollback(tx, err).await),
    };
    let recorded = async {
        let number: i64 = sqlx::query_scalar("SELECT entry_number FROM journal_entries WHERE id = ?1").bind(correction_id).fetch_one(&mut *tx).await.map_err(db_err)?;
        let payload = json!({
            "entryId": input.entry_id,
            "entryNumber": proposal.entry_number,
            "entryDeleted": proposal.entry_deleted,
            "correctionEntryId": correction_id,
            "correctionEntryNumber": number,
            "lines": proposal.lines,
        });
        sqlx::query("INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot) VALUES ('FORENSIC_HEAL', ?1, ?2)")
            .bind(input.entry_id)
            .bind(payload.to_string())
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        Ok::<i64, String>(number)
    }
    .await;
    match recorded {
        Ok(number) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            Ok(AppliedCorrection { correction_entry_id: correction_id, correction_entry_number: number })
        }
        Err(err) => Err(ledger::rollback(tx, err).await),
    }
}

#[cfg(test)]
mod tests;
