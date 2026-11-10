//! Duplicate Transaction (Ctrl+O) for the forms that show a saved document read-only (T28): sales
//! receipts, estimates, credit memos, vendor credits, purchase orders, deposits and transfers.
//!
//! [`copy_source`] reads a saved document in one shape the forms load as a new, unsaved one: its
//! name, header fields and lines. Nothing is written here; the copy is saved like any new form, with
//! today's date and the next number. Invoices, bills, checks and journal entries load their own saved
//! documents for editing and duplicate from there.
//!
//! What a copy leaves out: a deposit's received payments and sales receipts (each is deposited once;
//! only its other lines come over), and the number, date and status of the original.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::item_types::FormLineView;
use crate::ledger::db_err;
use crate::sales_lists::{SalesDetailsView, SalesDoc};

/// One line of the copied document. Sales lines carry an item, quantity, rate and income account
/// (`income_account_id`, the same as `account_id`); purchase and banking lines an account and amount.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CopyLine {
    pub line_number: i64,
    pub product_service_id: Option<i64>,
    pub description: String,
    pub quantity_e4: Option<i64>,
    pub rate_cents: Option<i64>,
    pub amount_cents: i64,
    pub account_id: i64,
    pub income_account_id: i64,
    pub class_id: Option<i64>,
    pub is_taxable: bool,
    pub percent_ppm: Option<i64>,
    pub customer_id: Option<i64>,
    /// A deposit line's payer (a vendor), when it named one.
    pub vendor_id: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopySource {
    /// `SALES_RECEIPT`, `ESTIMATE`, `CREDIT_MEMO`, `VENDOR_CREDIT`, `PURCHASE_ORDER`, `DEPOSIT` or `TRANSFER`.
    pub kind: String,
    /// The original's number, for the form's "copied from" note.
    pub number: Option<String>,
    /// The customer or vendor.
    pub name: Option<String>,
    pub memo: Option<String>,
    /// A vendor credit's reference or a sales receipt's check number.
    pub reference: Option<String>,
    /// The deposit account (sales receipt), refund account (refund receipt), bank account (deposit)
    /// or the account a transfer is from.
    pub account_id: Option<i64>,
    /// The account a transfer goes to.
    pub to_account_id: Option<i64>,
    /// A transfer's amount.
    pub amount_cents: Option<i64>,
    pub payment_method: Option<String>,
    pub payment_method_id: Option<i64>,
    pub tax_rate_id: Option<i64>,
    pub discount_cents: i64,
    /// A deposit's cash back: the account, amount and memo.
    pub cash_back_account_id: Option<i64>,
    pub cash_back_cents: i64,
    pub cash_back_memo: Option<String>,
    pub lines: Vec<CopyLine>,
    /// Subtotals, discounts, charges and groups (sales receipts and estimates).
    pub form_lines: Vec<FormLineView>,
    /// Rep, shipping and the printed customer fields (sales receipts and estimates).
    pub details: Option<SalesDetailsView>,
}

const SALES_LINE: &str = "line_number, product_service_id, description, quantity_e4, rate_cents, amount_cents, income_account_id AS account_id,
     income_account_id, class_id, is_taxable, NULL AS customer_id, NULL AS vendor_id";

/// A saved document of `kind` read as a new form loads it.
pub async fn copy_source(pool: &SqlitePool, kind: &str, id: i64) -> Result<CopySource, String> {
    let missing = || format!("DUP_NOT_FOUND: that {} does not exist", kind.to_lowercase().replace('_', " "));
    let lines = |sql: String| async move { sqlx::query_as::<_, CopyLine>(&sql).bind(id).fetch_all(pool).await.map_err(db_err) };
    let mut copy = CopySource { kind: kind.to_string(), ..Default::default() };
    match kind {
        "SALES_RECEIPT" => {
            type Head = (i64, Option<String>, i64, String, Option<i64>, Option<String>, Option<String>, Option<i64>, i64);
            let head: Head = sqlx::query_as(
                "SELECT r.receipt_number, c.name, r.deposit_account_id, r.payment_method, r.payment_method_id, r.reference, r.memo, r.tax_rate_id, r.discount_cents
                 FROM sales_receipts r LEFT JOIN customers c ON c.id = r.customer_id WHERE r.id = ?1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?
            .ok_or_else(missing)?;
            copy.number = Some(head.0.to_string());
            copy.name = head.1;
            copy.account_id = Some(head.2);
            copy.payment_method = Some(head.3);
            copy.payment_method_id = head.4;
            copy.reference = head.5;
            copy.memo = head.6;
            copy.tax_rate_id = head.7;
            copy.discount_cents = head.8;
            copy.lines = lines(format!("SELECT {SALES_LINE}, percent_ppm FROM sales_receipt_lines WHERE sales_receipt_id = ?1 ORDER BY line_number")).await?;
            copy.form_lines = crate::item_types::form_lines_of(pool, SalesDoc::SalesReceipt, id).await?;
            copy.details = Some(crate::sales_lists::document_details(pool, SalesDoc::SalesReceipt, id).await?);
        }
        "ESTIMATE" => {
            let head: (i64, String, Option<String>, Option<i64>, i64) = sqlx::query_as(
                "SELECT e.estimate_number, c.name, e.memo, e.tax_rate_id, e.discount_cents FROM estimates e JOIN customers c ON c.id = e.customer_id WHERE e.id = ?1",
            )
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?
            .ok_or_else(missing)?;
            copy.number = Some(head.0.to_string());
            copy.name = Some(head.1);
            copy.memo = head.2;
            copy.tax_rate_id = head.3;
            copy.discount_cents = head.4;
            copy.lines = lines(format!("SELECT {SALES_LINE}, percent_ppm FROM estimate_lines WHERE estimate_id = ?1 ORDER BY line_number")).await?;
            copy.form_lines = crate::item_types::form_lines_of(pool, SalesDoc::Estimate, id).await?;
            copy.details = Some(crate::sales_lists::document_details(pool, SalesDoc::Estimate, id).await?);
        }
        "CREDIT_MEMO" => {
            let head: (i64, String, Option<String>, Option<i64>, Option<i64>) =
                sqlx::query_as("SELECT m.credit_number, c.name, m.memo, m.tax_rate_id, m.refund_account_id FROM credit_memos m JOIN customers c ON c.id = m.customer_id WHERE m.id = ?1")
                    .bind(id)
                    .fetch_optional(pool)
                    .await
                    .map_err(db_err)?
                    .ok_or_else(missing)?;
            copy.number = Some(head.0.to_string());
            copy.name = Some(head.1);
            copy.memo = head.2;
            copy.tax_rate_id = head.3;
            // A refund receipt: the account the refund was paid from.
            copy.account_id = head.4;
            copy.lines = lines(format!("SELECT {SALES_LINE}, NULL AS percent_ppm FROM credit_memo_lines WHERE credit_memo_id = ?1 ORDER BY line_number")).await?;
        }
        "VENDOR_CREDIT" => {
            let head: (String, Option<String>, Option<String>) =
                sqlx::query_as("SELECT v.name, c.reference, c.memo FROM vendor_credits c JOIN vendors v ON v.id = c.vendor_id WHERE c.id = ?1")
                    .bind(id)
                    .fetch_optional(pool)
                    .await
                    .map_err(db_err)?
                    .ok_or_else(missing)?;
            copy.name = Some(head.0);
            copy.number = head.1.clone();
            copy.reference = head.1;
            copy.memo = head.2;
            copy.lines = lines(
                "SELECT line_number, NULL AS product_service_id, COALESCE(memo, '') AS description, NULL AS quantity_e4, NULL AS rate_cents, amount_cents,
                        account_id, account_id AS income_account_id, class_id, 0 AS is_taxable, NULL AS percent_ppm, customer_id, NULL AS vendor_id
                 FROM vendor_credit_lines WHERE vendor_credit_id = ?1 ORDER BY line_number"
                    .into(),
            )
            .await?;
        }
        "PURCHASE_ORDER" => {
            let head: (i64, String, Option<String>) =
                sqlx::query_as("SELECT o.po_number, v.name, o.memo FROM purchase_orders o JOIN vendors v ON v.id = o.vendor_id WHERE o.id = ?1")
                    .bind(id)
                    .fetch_optional(pool)
                    .await
                    .map_err(db_err)?
                    .ok_or_else(missing)?;
            copy.number = Some(head.0.to_string());
            copy.name = Some(head.1);
            copy.memo = head.2;
            copy.lines = lines(
                "SELECT line_number, product_service_id, COALESCE(description, '') AS description, quantity_e4, rate_cents, amount_cents,
                        account_id, account_id AS income_account_id, class_id, 0 AS is_taxable, NULL AS percent_ppm, customer_id, NULL AS vendor_id
                 FROM purchase_order_lines WHERE purchase_order_id = ?1 ORDER BY line_number"
                    .into(),
            )
            .await?;
        }
        "DEPOSIT" => {
            let head: (i64, Option<String>, Option<i64>, i64, Option<String>) =
                sqlx::query_as("SELECT bank_account_id, memo, cash_back_account_id, cash_back_cents, cash_back_memo FROM deposits WHERE id = ?1")
                    .bind(id)
                    .fetch_optional(pool)
                    .await
                    .map_err(db_err)?
                    .ok_or_else(missing)?;
            copy.account_id = Some(head.0);
            copy.memo = head.1;
            copy.cash_back_account_id = head.2;
            copy.cash_back_cents = head.3;
            copy.cash_back_memo = head.4;
            // Payments and sales receipts are deposited once; only the other lines come over.
            copy.lines = lines(
                "SELECT line_number, NULL AS product_service_id, COALESCE(memo, '') AS description, NULL AS quantity_e4, NULL AS rate_cents, amount_cents,
                        account_id, account_id AS income_account_id, class_id, 0 AS is_taxable, NULL AS percent_ppm, customer_id, vendor_id
                 FROM deposit_lines WHERE deposit_id = ?1 AND is_voided = 0 AND invoice_payment_id IS NULL AND sales_receipt_id IS NULL AND account_id IS NOT NULL
                 ORDER BY line_number"
                    .into(),
            )
            .await?;
        }
        "TRANSFER" => {
            let head: (i64, i64, i64, Option<String>) = sqlx::query_as("SELECT from_account_id, to_account_id, amount_cents, memo FROM transfers WHERE id = ?1")
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(db_err)?
                .ok_or_else(missing)?;
            copy.account_id = Some(head.0);
            copy.to_account_id = Some(head.1);
            copy.amount_cents = Some(head.2);
            copy.memo = head.3;
        }
        other => return Err(format!("DUP_BAD_KIND: '{other}' isn't duplicated here")),
    }
    Ok(copy)
}

#[cfg(test)]
mod tests;
