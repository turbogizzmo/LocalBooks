//! Applying money automatically, as Preferences › Payments and Bills say: a customer payment to
//! the oldest open invoices, and a vendor's open credits to the bills being paid.
//!
//! Neither changes what a payment or credit posts; they only decide which documents it settles.
//! Posting still goes through `invoices::record_payments`, `credits::apply_vendor_credit_in_tx`
//! and `expenses::write_bill_payment`.

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::expenses::BillPaymentInput;
use crate::ledger::{db_err, rollback};

/// Receive Payments spreads the amount received over the open invoices, oldest due first.
pub const AUTO_APPLY_PAYMENTS: &str = "auto_apply_payments";
/// Ticking or typing on invoices works out the amount received from them.
pub const AUTO_CALCULATE_PAYMENTS: &str = "auto_calculate_payments";
/// Pay Bills uses the vendor's open credits first.
pub const AUTO_APPLY_VENDOR_CREDITS: &str = "auto_apply_vendor_credits";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Allocation {
    pub document_id: i64,
    pub amount_cents: i64,
}

/// `amount_cents` spread over `open` `(document, balance)` in order, each up to its balance. What
/// is left over after the last document is not allocated.
pub fn in_order(open: &[(i64, i64)], amount_cents: i64) -> Vec<Allocation> {
    let mut remaining = amount_cents.max(0);
    let mut out = Vec::new();
    for &(document_id, balance) in open {
        let take = remaining.min(balance.max(0));
        if take > 0 {
            out.push(Allocation { document_id, amount_cents: take });
            remaining -= take;
        }
    }
    out
}

/// How `amount_cents` received from `customer_name` (in `currency_code`, `None` for the home
/// currency) applies to their open invoices: oldest due date first, then lowest number. While the
/// amount is being typed (`on_request` false) it is empty when the company turned automatic
/// applying off; the form's Auto Apply command (`on_request`) always applies.
pub async fn customer_payment(pool: &SqlitePool, customer_name: &str, currency_code: Option<&str>, amount_cents: i64, on_request: bool) -> Result<Vec<Allocation>, String> {
    if !on_request && !crate::preferences::flag(pool, AUTO_APPLY_PAYMENTS).await? {
        return Ok(Vec::new());
    }
    let open: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT i.id, i.total_cents - i.amount_paid_cents FROM invoices i LEFT JOIN customers c ON c.id = i.customer_id
         WHERE COALESCE(c.name, i.customer_name) = ?1 AND COALESCE(i.currency_code, '') = COALESCE(?2, '')
           AND i.is_voided = 0 AND i.status IN ('UNPAID', 'PARTIAL') AND i.total_cents > i.amount_paid_cents
         ORDER BY i.due_date, i.invoice_number",
    )
    .bind(customer_name.trim())
    .bind(currency_code.filter(|c| !c.is_empty()))
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(in_order(&open, amount_cents))
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PayBillsResult {
    /// Bills that got a payment.
    pub payments: usize,
    /// Credit applied to the bills, in all.
    pub credits_cents: i64,
}

/// The vendor's open credits dated on or before `date`, oldest first: `(credit, remaining)`.
async fn open_vendor_credits(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>, bill_id: i64, date: NaiveDate) -> Result<Vec<(i64, i64)>, String> {
    sqlx::query_as(
        "SELECT c.id, b.remaining_cents FROM vendor_credits c
         JOIN v_vendor_credit_balances b ON b.vendor_credit_id = c.id
         WHERE c.vendor_id = (SELECT vendor_id FROM bills WHERE id = ?1) AND c.is_voided = 0 AND b.remaining_cents > 0 AND c.credit_date <= ?2
         ORDER BY c.credit_date, c.id",
    )
    .bind(bill_id)
    .bind(date)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)
}

/// Pays bills. With [`AUTO_APPLY_VENDOR_CREDITS`] on, each bill first takes the vendor's open
/// credits (oldest first, on the payment date), and the payment covers only what is left; a bill
/// the credits settle gets no payment. Everything is one transaction.
pub async fn pay_bills(pool: &SqlitePool, inputs: Vec<BillPaymentInput>) -> Result<PayBillsResult, String> {
    if inputs.is_empty() {
        return Err("BILL_NOTHING_TO_PAY: select at least one bill".into());
    }
    let use_credits = crate::preferences::flag(pool, AUTO_APPLY_VENDOR_CREDITS).await?;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let mut result = PayBillsResult::default();
    for input in &inputs {
        let mut payment = input.clone();
        if use_credits {
            let credits = match open_vendor_credits(&mut tx, input.bill_id, input.payment_date).await {
                Ok(c) => c,
                Err(err) => return Err(rollback(tx, err).await),
            };
            for credit in in_order(&credits, payment.amount_cents) {
                let apply = crate::credits::ApplyCreditInput { credit_id: credit.document_id, document_id: input.bill_id, amount_cents: credit.amount_cents, applied_date: input.payment_date };
                if let Err(err) = crate::credits::apply_vendor_credit_in_tx(&mut tx, &apply).await {
                    return Err(rollback(tx, err).await);
                }
                payment.amount_cents -= credit.amount_cents;
                result.credits_cents += credit.amount_cents;
            }
        }
        if payment.amount_cents > 0 {
            if let Err(err) = crate::expenses::write_bill_payment(&mut tx, &payment, "STANDARD").await {
                return Err(rollback(tx, err).await);
            }
            result.payments += 1;
        }
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(result)
}

#[cfg(test)]
mod tests;
