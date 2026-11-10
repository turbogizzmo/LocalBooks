//! Approvals for bills and purchase orders (design: `docs/plans/approvals.md`).
//!
//! A bill or purchase order that matches an approval rule is not recorded when it is entered. It
//! becomes an approval request holding what was entered; approving it creates the bill (posted
//! through the ledger like any bill) or the purchase order, in the same write transaction that
//! marks the request approved. Rejecting keeps the request with the approver's note and creates
//! nothing. So an unapproved bill is never posted, and Pay Bills, A/P and Open Purchase Orders see
//! only approved documents.
//!
//! Who may approve is decided in `permissions.rs` (`approve_request` and `reject_request` are
//! Admin commands).

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{SqliteConnection, SqlitePool};

use crate::expenses::BillInput;
use crate::ledger::{db_err, format_cents};
use crate::workflows::PurchaseOrderInput;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Kind {
    Bill,
    PurchaseOrder,
}

impl Kind {
    pub fn code(self) -> &'static str {
        match self {
            Kind::Bill => "BILL",
            Kind::PurchaseOrder => "PURCHASE_ORDER",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Kind::Bill => "bill",
            Kind::PurchaseOrder => "purchase order",
        }
    }
}

/// An approval rule: documents of `kind` with a total of at least `min_total_cents` (from the one
/// vendor, when set) need approval.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRule {
    pub id: i64,
    pub kind: String,
    pub min_total_cents: i64,
    pub vendor_id: Option<i64>,
    pub vendor_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleInput {
    pub kind: Kind,
    pub min_total_cents: i64,
    #[serde(default)]
    pub vendor_id: Option<i64>,
}

pub async fn list_rules(pool: &SqlitePool) -> Result<Vec<ApprovalRule>, String> {
    sqlx::query_as(
        "SELECT r.id, r.kind, r.min_total_cents, r.vendor_id, v.name AS vendor_name
         FROM approval_rules r LEFT JOIN vendors v ON v.id = r.vendor_id ORDER BY r.kind, r.min_total_cents, r.id",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn add_rule(pool: &SqlitePool, input: &RuleInput) -> Result<ApprovalRule, String> {
    if !(0..=crate::ledger::MAX_LINE_CENTS).contains(&input.min_total_cents) {
        return Err("APPROVAL_RULE_AMOUNT: the amount must be zero or more".into());
    }
    if let Some(v) = input.vendor_id {
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM vendors WHERE id = ?1)").bind(v).fetch_one(pool).await.map_err(db_err)?;
        if !exists {
            return Err("APPROVAL_RULE_VENDOR: that vendor does not exist".into());
        }
    }
    let id: i64 = sqlx::query_scalar("INSERT INTO approval_rules (kind, min_total_cents, vendor_id) VALUES (?1, ?2, ?3) RETURNING id")
        .bind(input.kind.code())
        .bind(input.min_total_cents)
        .bind(input.vendor_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
    list_rules(pool).await?.into_iter().find(|r| r.id == id).ok_or_else(|| "APPROVAL_RULE: not saved".into())
}

pub async fn delete_rule(pool: &SqlitePool, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM approval_rules WHERE id = ?1").bind(id).execute(pool).await.map_err(db_err)?;
    Ok(())
}

/// The rule a document matches, if any: the one with the highest minimum.
pub async fn matching_rule(conn: &mut SqliteConnection, kind: Kind, vendor_name: &str, total_cents: i64) -> Result<Option<ApprovalRule>, String> {
    sqlx::query_as(
        "SELECT r.id, r.kind, r.min_total_cents, r.vendor_id, v.name AS vendor_name
         FROM approval_rules r LEFT JOIN vendors v ON v.id = r.vendor_id
         WHERE r.kind = ?1 AND r.min_total_cents <= ?2
           AND (r.vendor_id IS NULL OR r.vendor_id = (SELECT id FROM vendors WHERE name = ?3 COLLATE NOCASE))
         ORDER BY r.min_total_cents DESC, r.id LIMIT 1",
    )
    .bind(kind.code())
    .bind(total_cents)
    .bind(vendor_name.trim())
    .fetch_optional(&mut *conn)
    .await
    .map_err(db_err)
}

/// Refuses with `APPROVAL_REQUIRED` when a rule holds this document; the form then submits it.
pub async fn check_not_held(pool: &SqlitePool, kind: Kind, vendor_name: &str, total_cents: i64) -> Result<(), String> {
    let mut conn = pool.acquire().await.map_err(db_err)?;
    match matching_rule(&mut conn, kind, vendor_name, total_cents).await? {
        None => Ok(()),
        Some(rule) => {
            let why = match (&rule.vendor_name, rule.min_total_cents) {
                (Some(v), 0) => format!("{}s from {v}", kind.noun()),
                (Some(v), min) => format!("{}s from {v} of {} or more", kind.noun(), format_cents(min)),
                (None, 0) => format!("every {}", kind.noun()),
                (None, min) => format!("{}s of {} or more", kind.noun(), format_cents(min)),
            };
            Err(format!("APPROVAL_REQUIRED: {why} need approval before they are recorded; submit it for approval"))
        }
    }
}

/// An approval request: a document waiting for, or decided by, an approver.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub id: i64,
    pub kind: String,
    pub vendor_name: String,
    pub total_cents: i64,
    pub document_date: NaiveDate,
    pub reference: Option<String>,
    pub status: String,
    pub requested_by: String,
    pub requested_at: String,
    pub decided_by: Option<String>,
    pub decided_at: Option<String>,
    pub note: Option<String>,
    pub bill_id: Option<i64>,
    pub purchase_order_id: Option<i64>,
    /// The form as entered (JSON).
    pub input_json: String,
}

const SELECT: &str = "SELECT id, kind, vendor_name, total_cents, document_date, reference, status, requested_by, requested_at, decided_by, decided_at, note,
                             bill_id, purchase_order_id, input_json FROM approval_requests";

/// What the document would be: (vendor, total, date, reference), validated as its form would be.
fn describe(kind: Kind, input: &Value) -> Result<(String, i64, NaiveDate, Option<String>), String> {
    let bad = |e: serde_json::Error| format!("APPROVAL_BAD_INPUT: the form could not be read ({e})");
    match kind {
        Kind::Bill => {
            let bill: BillInput = serde_json::from_value(input.clone()).map_err(bad)?;
            let total = crate::expenses::validate_bill(&bill)?;
            Ok((bill.vendor_name.trim().to_string(), total, bill.bill_date, bill.bill_number.map(|n| n.trim().to_string()).filter(|n| !n.is_empty())))
        }
        Kind::PurchaseOrder => {
            let po: PurchaseOrderInput = serde_json::from_value(input.clone()).map_err(bad)?;
            let total = crate::workflows::validate_purchase_order(&po)?;
            Ok((po.vendor_name.trim().to_string(), total, po.po_date, None))
        }
    }
}

async fn get(pool: &SqlitePool, id: i64) -> Result<ApprovalRequest, String> {
    sqlx::query_as(&format!("{SELECT} WHERE id = ?1")).bind(id).fetch_optional(pool).await.map_err(db_err)?.ok_or_else(|| format!("APPROVAL_NOT_FOUND: no request {id}"))
}

/// Saves a bill or purchase order as a request waiting for approval.
pub async fn submit(pool: &SqlitePool, kind: Kind, input: &Value, requested_by: &str) -> Result<ApprovalRequest, String> {
    let (vendor, total, date, reference) = describe(kind, input)?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO approval_requests (kind, vendor_name, total_cents, document_date, reference, input_json, requested_by)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
    )
    .bind(kind.code())
    .bind(&vendor)
    .bind(total)
    .bind(date)
    .bind(reference)
    .bind(input.to_string())
    .bind(requested_by)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    get(pool, id).await
}

/// Requests with `status` (all when `None`), newest first.
pub async fn list(pool: &SqlitePool, status: Option<&str>) -> Result<Vec<ApprovalRequest>, String> {
    sqlx::query_as(&format!("{SELECT} WHERE ?1 IS NULL OR status = ?1 ORDER BY (status = 'PENDING') DESC, id DESC")).bind(status).fetch_all(pool).await.map_err(db_err)
}

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Approves a pending request: creates the bill (posted) or purchase order, and marks the request
/// approved, all in one write transaction. If the document can no longer be recorded (a closed
/// period, an inactive account) nothing changes and the request stays pending.
pub async fn approve(pool: &SqlitePool, id: i64, decided_by: &str) -> Result<ApprovalRequest, String> {
    let request = get(pool, id).await?;
    if request.status != "PENDING" {
        return Err(format!("APPROVAL_DECIDED: this request was already {}", request.status.to_lowercase()));
    }
    let kind = if request.kind == "BILL" { Kind::Bill } else { Kind::PurchaseOrder };
    if kind == Kind::PurchaseOrder {
        crate::inventory::require_enabled(pool).await?;
    }
    let bad = |e: serde_json::Error| format!("APPROVAL_BAD_INPUT: the form could not be read ({e})");
    let input: Value = serde_json::from_str(&request.input_json).map_err(bad)?;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result: Result<(), String> = async {
        let (bill_id, po_id) = match kind {
            Kind::Bill => {
                let bill: BillInput = serde_json::from_value(input.clone()).map_err(bad)?;
                (Some(crate::expenses::write_bill(&mut tx, &bill).await?), None)
            }
            Kind::PurchaseOrder => {
                let po: PurchaseOrderInput = serde_json::from_value(input.clone()).map_err(bad)?;
                let total = crate::workflows::validate_purchase_order(&po)?;
                (None, Some(crate::workflows::write_purchase_order(&mut tx, &po, total).await?))
            }
        };
        let done = sqlx::query(
            "UPDATE approval_requests SET status = 'APPROVED', decided_by = ?1, decided_at = ?2, bill_id = ?3, purchase_order_id = ?4
             WHERE id = ?5 AND status = 'PENDING'",
        )
        .bind(decided_by)
        .bind(now())
        .bind(bill_id)
        .bind(po_id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        if done.rows_affected() != 1 {
            return Err("APPROVAL_DECIDED: this request was decided meanwhile".into());
        }
        Ok(())
    }
    .await;
    match result {
        Ok(()) => crate::ledger::commit_write(tx).await.map_err(db_err)?,
        Err(e) => return Err(crate::ledger::rollback(tx, e).await),
    }
    get(pool, id).await
}

/// Rejects a pending request with a note saying why. The request is kept; nothing is recorded.
pub async fn reject(pool: &SqlitePool, id: i64, decided_by: &str, note: &str) -> Result<ApprovalRequest, String> {
    let note = note.trim();
    if note.is_empty() {
        return Err("APPROVAL_NOTE: say why it is rejected".into());
    }
    if note.chars().count() > 1000 {
        return Err("APPROVAL_NOTE: keep the note under 1,000 characters".into());
    }
    let done = sqlx::query("UPDATE approval_requests SET status = 'REJECTED', decided_by = ?1, decided_at = ?2, note = ?3 WHERE id = ?4 AND status = 'PENDING'")
        .bind(decided_by)
        .bind(now())
        .bind(note)
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if done.rows_affected() != 1 {
        let request = get(pool, id).await?;
        return Err(format!("APPROVAL_DECIDED: this request was already {}", request.status.to_lowercase()));
    }
    get(pool, id).await
}

#[cfg(test)]
mod tests;
