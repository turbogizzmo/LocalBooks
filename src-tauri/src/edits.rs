//! Editing posted transactions, the way desktop accounting software appears to: the form opens with
//! the transaction's details, and saving replaces it. Posted entries never change, so a replacement
//! voids the original (a reversing entry on its date) and posts the edited version, in one database
//! transaction; both stay in the journal and the audit trail, linked in `transaction_edits`.
//!
//! The same rules as voiding apply: an invoice or bill with payments or credits applied, anything
//! reconciled, and anything in a closed period cannot be replaced until that is undone.

use serde::Serialize;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::expenses::{BillInput, ExpenseInput};
use crate::invoices::InvoiceInput;
use crate::ledger::{db_err, post_entry_in_tx_flagged, rollback, void_in_tx, EntrySource, JournalEntry};

async fn run<T>(pool: &SqlitePool, work: impl for<'t> AsyncFnOnce(&mut Transaction<'t, Sqlite>) -> Result<T, String>) -> Result<T, String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    match work(&mut tx).await {
        Ok(value) => crate::ledger::commit_write(tx).await.map(|()| value).map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// The live journal entry of a document, or why it cannot be edited.
async fn live_entry(tx: &mut Transaction<'_, Sqlite>, table: &str, id: i64, noun: &str) -> Result<i64, String> {
    let row: Option<(Option<i64>, bool)> = sqlx::query_as(&format!("SELECT journal_entry_id, is_voided FROM {table} WHERE id = ?1"))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    match row {
        None => Err(format!("EDIT_NOT_FOUND: that {noun} does not exist")),
        Some((_, true)) => Err(format!("EDIT_VOIDED: a voided {noun} cannot be edited")),
        Some((None, _)) => Err(format!("EDIT_NOT_POSTED: that {noun} is not posted")),
        Some((Some(entry), false)) => Ok(entry),
    }
}

async fn record(tx: &mut Transaction<'_, Sqlite>, old_entry: i64, new_entry: i64) -> Result<(), String> {
    sqlx::query("INSERT INTO transaction_edits (old_entry_id, new_entry_id) VALUES (?1, ?2)")
        .bind(old_entry)
        .bind(new_entry)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    // Attached files (a vendor's invoice, a receipt) belong to the transaction, so they follow the edit.
    sqlx::query("UPDATE attachments SET journal_entry_id = ?2 WHERE journal_entry_id = ?1")
        .bind(old_entry)
        .bind(new_entry)
        .execute(&mut **tx)
        .await
        .map(|_| ())
        .map_err(db_err)
}

async fn entry_of(tx: &mut Transaction<'_, Sqlite>, table: &str, id: i64) -> Result<i64, String> {
    sqlx::query_scalar(&format!("SELECT journal_entry_id FROM {table} WHERE id = ?1")).bind(id).fetch_one(&mut **tx).await.map_err(db_err)
}

/// Replaces an invoice with an edited one; returns the new invoice's id.
pub async fn replace_invoice(pool: &SqlitePool, invoice_id: i64, input: InvoiceInput) -> Result<i64, String> {
    run(pool, async |tx| {
        let old = live_entry(tx, "invoices", invoice_id, "invoice").await?;
        // The edited invoice keeps the original's rep, shipping and printed fields if left as they were.
        let carried = crate::sales_lists::details_in_tx(tx, crate::sales_lists::SalesDoc::Invoice, invoice_id).await?;
        let reimbursed = crate::billable_costs::reimbursed_accounts(tx, invoice_id).await?;
        void_in_tx(tx, old).await?;
        let new_id = crate::invoices::write_invoice_reimbursing(tx, &input, &carried, &reimbursed).await?;
        let new_entry = entry_of(tx, "invoices", new_id).await?;
        record(tx, old, new_entry).await?;
        crate::print_queue::carry_over(tx, crate::print_queue::Form::Invoice, invoice_id, new_id).await?;
        crate::mileage::follow_replaced_invoice(tx, invoice_id, new_id).await?;
        crate::billable_costs::follow_replaced_invoice(tx, invoice_id, new_id).await?;
        Ok(new_id)
    })
    .await
}

pub async fn replace_bill(pool: &SqlitePool, bill_id: i64, input: BillInput) -> Result<i64, String> {
    run(pool, async |tx| {
        let old = live_entry(tx, "bills", bill_id, "bill").await?;
        let carried_terms: Option<i64> = sqlx::query_scalar("SELECT terms_id FROM bills WHERE id = ?1").bind(bill_id).fetch_one(&mut **tx).await.map_err(db_err)?;
        void_in_tx(tx, old).await?;
        let new_id = crate::expenses::write_bill_carrying(tx, &input, carried_terms).await?;
        let new_entry = entry_of(tx, "bills", new_id).await?;
        record(tx, old, new_entry).await?;
        crate::billable_costs::follow_replaced_lines(tx, "bills", bill_id, new_id).await?;
        Ok(new_id)
    })
    .await
}

pub async fn replace_expense(pool: &SqlitePool, expense_id: i64, input: ExpenseInput) -> Result<i64, String> {
    run(pool, async |tx| replace_expense_in_tx(tx, expense_id, &input).await).await
}

/// [`replace_expense`] inside a transaction the caller commits.
pub(crate) async fn replace_expense_in_tx(tx: &mut Transaction<'_, Sqlite>, expense_id: i64, input: &ExpenseInput) -> Result<i64, String> {
    let old = live_entry(tx, "expenses", expense_id, "expense").await?;
    void_in_tx(tx, old).await?;
    let (new_id, new_entry) = crate::expenses::write_expense(tx, input).await?;
    record(tx, old, new_entry).await?;
    crate::print_queue::carry_over(tx, crate::print_queue::Form::Check, expense_id, new_id).await?;
    crate::billable_costs::follow_replaced_lines(tx, "expenses", expense_id, new_id).await?;
    Ok(new_id)
}

/// A saved check or expense as its form would send it again, dated `date` (home currency only).
pub(crate) async fn expense_input_in_tx(tx: &mut Transaction<'_, Sqlite>, expense_id: i64, date: chrono::NaiveDate) -> Result<Option<ExpenseInput>, String> {
    type Row = (String, Option<String>, i64, Option<String>, Option<String>, Option<String>, bool, Option<i64>);
    let row: Option<Row> = sqlx::query_as(
        "SELECT x.kind, v.name, x.payment_account_id, x.check_number, x.payment_method, x.memo, x.currency_code IS NOT NULL OR x.is_credit = 1, x.payment_method_id
         FROM expenses x LEFT JOIN vendors v ON v.id = x.vendor_id WHERE x.id = ?1",
    )
    .bind(expense_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let Some((kind, payee_name, payment_account_id, check_number, payment_method, memo, skip, payment_method_id)) = row else { return Ok(None) };
    if skip {
        return Ok(None);
    }
    let lines: Vec<(i64, i64, Option<String>, Option<i64>, Option<i64>, bool)> =
        sqlx::query_as("SELECT account_id, amount_cents, memo, class_id, customer_id, is_billable FROM expense_lines WHERE expense_id = ?1 ORDER BY line_number")
            .bind(expense_id)
            .fetch_all(&mut **tx)
            .await
            .map_err(db_err)?;
    Ok(Some(ExpenseInput {
        kind,
        payee_name,
        payment_account_id,
        expense_date: date,
        check_number,
        payment_method,
        memo,
        lines: lines
            .into_iter()
            .map(|(account_id, amount_cents, memo, class_id, customer_id, billable)| crate::expenses::SplitLineInput { account_id, amount_cents, memo, class_id, customer_id, billable: Some(billable) })
            .collect(),
        currency: None,
        payment_method_id,
    }))
}

/// Replaces a manual journal entry. Entries made by another screen (documents, reclassifications,
/// refunds, tax payments) are edited from that screen. The replacement keeps the original's
/// adjusting-entry mark.
pub async fn replace_journal_entry(pool: &SqlitePool, journal_entry_id: i64, entry: JournalEntry) -> Result<i64, String> {
    replace_journal_entry_flagged(pool, journal_entry_id, entry, None).await
}

/// [`replace_journal_entry`], marking the replacement as an adjusting entry or not; `None` keeps
/// the original's mark.
pub async fn replace_journal_entry_flagged(pool: &SqlitePool, journal_entry_id: i64, entry: JournalEntry, adjusting: Option<bool>) -> Result<i64, String> {
    run(pool, async |tx| {
        let manual: Option<(bool, bool)> = sqlx::query_as(
            "SELECT e.source_type = 'JOURNAL' AND e.reverses_entry_id IS NULL AND e.is_voided = 0
                AND NOT EXISTS (SELECT 1 FROM class_reclassifications WHERE journal_entry_id = e.id)
                AND NOT EXISTS (SELECT 1 FROM credit_refunds WHERE journal_entry_id = e.id)
                AND NOT EXISTS (SELECT 1 FROM vendor_credit_refunds WHERE journal_entry_id = e.id)
                AND NOT EXISTS (SELECT 1 FROM sales_tax_payments WHERE journal_entry_id = e.id)
                AND NOT EXISTS (SELECT 1 FROM bounced_checks WHERE journal_entry_id = e.id)
                AND NOT EXISTS (SELECT 1 FROM fx_revaluations WHERE journal_entry_id = e.id OR reversal_entry_id = e.id)
             , e.is_adjusting FROM journal_entries e WHERE e.id = ?1",
        )
        .bind(journal_entry_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        let was_adjusting = match manual {
            None => return Err("EDIT_NOT_FOUND: that journal entry does not exist".into()),
            Some((false, _)) => return Err("EDIT_NOT_MANUAL: only manual journal entries are edited here; edit the others from the screen that made them".into()),
            Some((true, adjusting)) => adjusting,
        };
        void_in_tx(tx, journal_entry_id).await?;
        let new_entry = post_entry_in_tx_flagged(tx, &entry, EntrySource::Journal, adjusting.unwrap_or(was_adjusting)).await?;
        record(tx, journal_entry_id, new_entry).await?;
        Ok(new_entry)
    })
    .await
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DocumentLineView {
    pub account_id: i64,
    pub amount_cents: i64,
    pub memo: Option<String>,
    pub quantity_e4: Option<i64>,
    pub rate_cents: Option<i64>,
    pub product_service_id: Option<i64>,
    pub class_id: Option<i64>,
    pub customer_id: Option<i64>,
    /// Marked to invoice to the customer (Sales › Bill Costs).
    pub billable: bool,
}

/// A bill's or expense's lines, for loading it into its form.
pub async fn document_lines(pool: &SqlitePool, kind: &str, id: i64) -> Result<Vec<DocumentLineView>, String> {
    let sql = match kind {
        "bill" => "SELECT account_id, amount_cents, NULLIF(memo, '') AS memo, quantity_e4, rate_cents, product_service_id, class_id, customer_id, is_billable AS billable FROM bill_lines WHERE bill_id = ?1 ORDER BY line_number",
        "expense" => "SELECT account_id, amount_cents, memo, NULL AS quantity_e4, NULL AS rate_cents, NULL AS product_service_id, class_id, customer_id, is_billable AS billable FROM expense_lines WHERE expense_id = ?1 ORDER BY line_number",
        other => return Err(format!("EDIT_BAD_KIND: '{other}' has no lines to load")),
    };
    sqlx::query_as::<_, DocumentLineView>(sql).bind(id).fetch_all(pool).await.map_err(db_err)
}

/// A vendor's most recent bill, check or expense, to pre-fill a new one (My Preferences ›
/// General › "Automatically recall information"): its accounts, amounts and memos.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecalledTransaction {
    /// `bill` or `expense` (a check or an expense).
    pub kind: String,
    pub id: i64,
    pub date: String,
    pub memo: Option<String>,
    pub total_cents: i64,
    pub lines: Vec<DocumentLineView>,
}

/// The vendor's latest live, home-currency bill or check/expense (`kind` picks one, or `None` for
/// whichever is newer), or `None` when there is none.
pub async fn last_for_vendor(pool: &SqlitePool, vendor_name: &str, kind: Option<&str>) -> Result<Option<RecalledTransaction>, String> {
    let latest: Option<(String, i64, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT * FROM (
            SELECT 'bill' AS kind, b.id, b.bill_date AS date, b.memo, b.total_cents
            FROM bills b JOIN vendors v ON v.id = b.vendor_id JOIN journal_entries e ON e.id = b.journal_entry_id
            WHERE v.name = ?1 COLLATE NOCASE AND e.is_voided = 0
            UNION ALL
            SELECT 'expense', x.id, x.expense_date, x.memo, x.total_cents
            FROM expenses x JOIN vendors v ON v.id = x.vendor_id JOIN journal_entries e ON e.id = x.journal_entry_id
            WHERE v.name = ?1 COLLATE NOCASE AND e.is_voided = 0 AND x.is_credit = 0 AND x.currency_code IS NULL
         ) WHERE ?2 IS NULL OR kind = ?2 ORDER BY date DESC, id DESC LIMIT 1",
    )
    .bind(vendor_name.trim())
    .bind(kind)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let Some((kind, id, date, memo, total_cents)) = latest else { return Ok(None) };
    let lines = document_lines(pool, &kind, id).await?;
    Ok(Some(RecalledTransaction { kind, id, date, memo, total_cents, lines }))
}

#[cfg(test)]
mod tests;
