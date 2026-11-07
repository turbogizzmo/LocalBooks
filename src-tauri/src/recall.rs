//! Recalling a vendor's last transaction (My Preferences › General): a new check, expense or bill
//! for a vendor can start with the lines of the last one you entered for them.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::db_err;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RecalledLine {
    pub account_id: i64,
    pub amount_cents: i64,
    pub memo: Option<String>,
    pub class_id: Option<i64>,
    pub customer_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recalled {
    /// `EXPENSE` (a check, expense or card charge) or `BILL`.
    pub kind: String,
    pub date: String,
    pub lines: Vec<RecalledLine>,
}

/// The lines of the vendor's most recent live check, expense or bill (by date, then the one
/// entered last), or `None` when they have none. Card credits are left out: they run the other way.
pub async fn last_for_vendor(pool: &SqlitePool, vendor_name: &str) -> Result<Option<Recalled>, String> {
    let latest: Option<(String, i64, String)> = sqlx::query_as(
        "SELECT kind, id, d FROM (
            SELECT 'EXPENSE' AS kind, x.id, x.expense_date AS d FROM expenses x JOIN vendors v ON v.id = x.vendor_id
            WHERE v.name = ?1 COLLATE NOCASE AND x.is_voided = 0 AND x.kind <> 'CARD_CREDIT'
            UNION ALL
            SELECT 'BILL', b.id, b.bill_date FROM bills b JOIN vendors v ON v.id = b.vendor_id
            WHERE v.name = ?1 COLLATE NOCASE AND b.is_voided = 0
         ) ORDER BY d DESC, kind = 'BILL', id DESC LIMIT 1",
    )
    .bind(vendor_name.trim())
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let Some((kind, id, date)) = latest else { return Ok(None) };
    let sql = if kind == "BILL" {
        "SELECT account_id, amount_cents, memo, class_id, customer_id FROM bill_lines WHERE bill_id = ?1 ORDER BY line_number"
    } else {
        "SELECT account_id, amount_cents, memo, class_id, customer_id FROM expense_lines WHERE expense_id = ?1 ORDER BY line_number"
    };
    let lines = sqlx::query_as(sql).bind(id).fetch_all(pool).await.map_err(db_err)?;
    Ok(Some(Recalled { kind, date, lines }))
}

#[cfg(test)]
mod tests;
