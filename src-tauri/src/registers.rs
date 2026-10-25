//! Account registers: every posted line of one account (optionally with its subaccounts) in date
//! order, with the other side of each transaction, the customer or vendor, its class, whether it
//! is cleared or reconciled, and a running balance on the account's normal side.

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::{db_err, DISPLAY_SOURCE_TYPE};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RegisterRow {
    pub line_id: i64,
    pub journal_entry_id: i64,
    pub entry_number: i64,
    pub entry_date: String,
    pub source_type: String,
    pub source_id: Option<i64>,
    pub party_name: Option<String>,
    pub memo: Option<String>,
    pub account_name: String,
    /// The other account, or "-Split-" when the transaction has several.
    pub split: Option<String>,
    pub class_name: Option<String>,
    pub debit_cents: i64,
    pub credit_cents: i64,
    /// Balance after this line, on the account's normal side.
    #[sqlx(skip)]
    pub balance_cents: i64,
    pub is_voided: bool,
    pub is_reversal: bool,
    pub cleared: bool,
    pub reconciled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Register {
    pub account_id: i64,
    pub account_name: String,
    pub account_type_name: String,
    pub is_debit_normal: bool,
    pub include_subaccounts: bool,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub opening_balance_cents: i64,
    pub ending_balance_cents: i64,
    pub rows: Vec<RegisterRow>,
}

/// The register of `account_id` between the dates (either may be open-ended).
pub async fn account_register(
    pool: &SqlitePool,
    account_id: i64,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    include_subaccounts: bool,
) -> Result<Register, String> {
    let head: Option<(String, String, String)> =
        sqlx::query_as("SELECT full_name, account_type_name, normal_balance FROM v_chart_of_accounts WHERE id = ?1")
            .bind(account_id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    let (account_name, account_type_name, normal) = head.ok_or_else(|| format!("REG_NOT_FOUND: account {account_id} does not exist"))?;
    let debit_normal = normal == "DEBIT";
    // The register's account ids: the account itself, or it and every subaccount below it.
    let scope = if include_subaccounts {
        "SELECT account_id FROM v_account_ancestry WHERE ancestor_id = ?1"
    } else {
        "SELECT ?1"
    };

    let opening: i64 = match start_date {
        Some(start) => sqlx::query_scalar(&format!(
            "SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id IN ({scope}) AND entry_date < ?2"
        ))
        .bind(account_id)
        .bind(start)
        .fetch_one(pool)
        .await
        .map_err(db_err)?,
        None => 0,
    };
    let mut rows: Vec<RegisterRow> = sqlx::query_as(&format!(
        "SELECT l.id AS line_id, e.id AS journal_entry_id, e.entry_number, e.entry_date, {DISPLAY_SOURCE_TYPE} AS source_type, e.source_id,
                p.party_name, COALESCE(l.memo, e.memo) AS memo, a.full_name AS account_name,
                (SELECT CASE WHEN COUNT(*) = 1 THEN MAX(o_a.full_name) WHEN COUNT(*) > 1 THEN '-Split-' END
                 FROM journal_lines o JOIN v_chart_of_accounts o_a ON o_a.id = o.account_id
                 WHERE o.journal_entry_id = e.id AND o.account_id NOT IN ({scope})) AS split,
                k.full_name AS class_name, l.debit_cents, l.credit_cents,
                e.is_voided, e.reverses_entry_id IS NOT NULL AS is_reversal,
                l.reconciliation_id IS NOT NULL AS cleared, l.reconciled_at IS NOT NULL AS reconciled
         FROM journal_lines l
         JOIN journal_entries e ON e.id = l.journal_entry_id
         JOIN v_chart_of_accounts a ON a.id = l.account_id
         LEFT JOIN v_entry_party p ON p.journal_entry_id = e.id
         LEFT JOIN v_classes k ON k.id = l.class_id
         WHERE e.status = 'POSTED' AND l.account_id IN ({scope})
           AND (?2 IS NULL OR e.entry_date >= ?2) AND (?3 IS NULL OR e.entry_date <= ?3)
         ORDER BY e.entry_date, e.entry_number, l.line_number"
    ))
    .bind(account_id)
    .bind(start_date)
    .bind(end_date)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let natural = |net: i64| if debit_normal { net } else { -net };
    let mut balance = natural(opening);
    for row in &mut rows {
        balance += natural(row.debit_cents - row.credit_cents);
        row.balance_cents = balance;
    }
    Ok(Register {
        account_id,
        account_name,
        account_type_name,
        is_debit_normal: debit_normal,
        include_subaccounts,
        start_date,
        end_date,
        opening_balance_cents: natural(opening),
        ending_balance_cents: balance,
        rows,
    })
}

#[cfg(test)]
mod tests;
