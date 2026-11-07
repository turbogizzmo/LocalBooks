//! Print later: checks and invoices marked "To be printed", printed together from File › Print Forms.
//!
//! A check waiting to be printed has no number. When the batch prints, the user gives the number
//! on the first sheet of check stock and the checks take numbers in order; the numbers are saved
//! only once the user confirms which checks printed correctly, so a jammed sheet never burns a
//! number in the books. Misprinted checks stay in the queue. Invoices just leave the queue.
//!
//! The flag is not posting: it lives on the document row, and the schema (migration 0066) allows a
//! posted check's missing number to be filled in only while the check is queued.

use serde::Deserialize;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::expenses::{ExpenseInput, ExpenseView};
use crate::invoices::{InvoiceInput, InvoiceSummary};
use crate::ledger::{db_err, rollback};

/// Preference keys holding each form's "Print later" default (the last choice saved).
pub const PREF_CHECKS: &str = "print_checks_later";
pub const PREF_INVOICES: &str = "print_invoices_later";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    Check,
    Invoice,
}

impl Form {
    pub fn parse(kind: &str) -> Result<Self, String> {
        match kind {
            "CHECK" => Ok(Form::Check),
            "INVOICE" => Ok(Form::Invoice),
            other => Err(format!("PRINT_BAD_FORM: '{other}' cannot be printed later; use CHECK or INVOICE")),
        }
    }
}

/// One printed check and the number it printed with.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrintedCheck {
    pub expense_id: i64,
    pub check_number: String,
}

async fn run<T>(pool: &SqlitePool, work: impl for<'t> AsyncFnOnce(&mut Transaction<'t, Sqlite>) -> Result<T, String>) -> Result<T, String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    match work(&mut tx).await {
        Ok(value) => crate::ledger::commit_write(tx).await.map(|()| value).map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// Checks waiting to print, oldest first; only those written from `account_id` when given.
pub async fn queued_checks(pool: &SqlitePool, account_id: Option<i64>) -> Result<Vec<ExpenseView>, String> {
    crate::expenses::list_to_print(pool, account_id).await
}

/// Invoices waiting to print, lowest number first.
pub async fn queued_invoices(pool: &SqlitePool) -> Result<Vec<InvoiceSummary>, String> {
    crate::invoices::list_to_print(pool).await
}

/// Remembers the form's "Print later" choice as its default for the next new check or invoice.
/// Anyone who can write checks or invoices may change it (it is not a company-wide setting).
pub async fn remember_default(pool: &SqlitePool, form: Form, on: bool) -> Result<(), String> {
    let key = match form {
        Form::Check => PREF_CHECKS,
        Form::Invoice => PREF_INVOICES,
    };
    crate::preferences::set(pool, key, if on { "true" } else { "false" }).await.map(|_| ())
}

/// Puts a document in the print queue or takes it out.
pub async fn set_to_be_printed(pool: &SqlitePool, form: Form, id: i64, queued: bool) -> Result<(), String> {
    run(pool, async |tx| set_in_tx(tx, form, id, queued).await).await
}

async fn set_in_tx(tx: &mut Transaction<'_, Sqlite>, form: Form, id: i64, queued: bool) -> Result<(), String> {
    let (table, noun) = match form {
        Form::Check => ("expenses", "check"),
        Form::Invoice => ("invoices", "invoice"),
    };
    let row: Option<(bool, bool)> = sqlx::query_as(&format!("SELECT is_voided, journal_entry_id IS NOT NULL FROM {table} WHERE id = ?1"))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    match row {
        None => return Err(format!("PRINT_NOT_FOUND: that {noun} does not exist")),
        Some((true, _)) if queued => return Err(format!("PRINT_VOIDED: a voided {noun} cannot be printed")),
        Some((_, false)) => return Err(format!("PRINT_NOT_POSTED: that {noun} is not saved yet")),
        _ => {}
    }
    sqlx::query(&format!("UPDATE {table} SET to_be_printed = ?1 WHERE id = ?2"))
        .bind(queued)
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// Writes a check or expense, queued for printing when `to_be_printed` (a queued check is saved
/// without a number). One transaction.
pub async fn post_expense(pool: &SqlitePool, mut input: ExpenseInput, to_be_printed: bool) -> Result<ExpenseView, String> {
    if to_be_printed {
        if input.kind != "CHECK" {
            return Err("PRINT_NOT_A_CHECK: only checks can be printed later".into());
        }
        input.check_number = None;
    }
    let id = run(pool, async |tx| {
        let (id, _) = crate::expenses::write_expense(tx, &input).await?;
        if to_be_printed {
            set_in_tx(tx, Form::Check, id, true).await?;
        }
        Ok(id)
    })
    .await?;
    let mut all = crate::expenses::list_expenses(pool).await?;
    all.retain(|x| x.id == id);
    all.pop().ok_or_else(|| "EXP_NOT_FOUND: the saved check could not be read back".into())
}

/// Creates and posts an invoice, queued for printing when `to_be_printed`. One transaction.
pub async fn create_invoice(pool: &SqlitePool, input: InvoiceInput, to_be_printed: bool) -> Result<InvoiceSummary, String> {
    let id = run(pool, async |tx| {
        let id = crate::invoices::write_invoice(tx, &input).await?;
        if to_be_printed {
            set_in_tx(tx, Form::Invoice, id, true).await?;
        }
        Ok(id)
    })
    .await?;
    crate::invoices::get_summary(pool, id).await
}

/// After an edit replaced a queued document: the replacement stays queued (a check only while it
/// still has no number).
pub(crate) async fn carry_over(tx: &mut Transaction<'_, Sqlite>, form: Form, old_id: i64, new_id: i64) -> Result<(), String> {
    let sql = match form {
        Form::Check => {
            "UPDATE expenses SET to_be_printed = 1
             WHERE id = ?2 AND kind = 'CHECK' AND check_number IS NULL AND (SELECT to_be_printed FROM expenses WHERE id = ?1) = 1"
        }
        Form::Invoice => "UPDATE invoices SET to_be_printed = 1 WHERE id = ?2 AND (SELECT to_be_printed FROM invoices WHERE id = ?1) = 1",
    };
    sqlx::query(sql).bind(old_id).bind(new_id).execute(&mut **tx).await.map_err(db_err)?;
    Ok(())
}

/// Saves the numbers of the checks that printed correctly and takes them out of the queue.
/// Every number must be new for its bank account (counting voided checks and check bill payments,
/// so a number is never used twice).
///
/// With Preferences › Checking › "Change check date when a check is printed", a check dated other
/// than `today` is re-dated first. Posted entries never change, so this is an edit: the queued check
/// is voided and posted again on the print date, both kept in the audit log, and the number goes on
/// the replacement. Foreign-currency checks keep their date (their rate belongs to it).
pub async fn mark_checks_printed(pool: &SqlitePool, printed: Vec<PrintedCheck>) -> Result<usize, String> {
    mark_checks_printed_on(pool, printed, chrono::Local::now().date_naive()).await
}

pub async fn mark_checks_printed_on(pool: &SqlitePool, mut printed: Vec<PrintedCheck>, today: chrono::NaiveDate) -> Result<usize, String> {
    if printed.is_empty() {
        return Ok(0);
    }
    let redate = crate::preferences::flag(pool, crate::preferences::tax_time::DATE_CHECKS_PRINTED).await?;
    run(pool, async |tx| {
        if redate {
            for check in &mut printed {
                let date: Option<(chrono::NaiveDate, bool)> = sqlx::query_as("SELECT expense_date, to_be_printed FROM expenses WHERE id = ?1 AND kind = 'CHECK' AND is_voided = 0")
                    .bind(check.expense_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(db_err)?;
                if !matches!(date, Some((d, true)) if d != today) {
                    continue;
                }
                if let Some(input) = crate::edits::expense_input_in_tx(tx, check.expense_id, today).await? {
                    check.expense_id = crate::edits::replace_expense_in_tx(tx, check.expense_id, &input).await?;
                }
            }
        }
        let mut seen: Vec<(i64, String)> = Vec::with_capacity(printed.len());
        for check in &printed {
            let number = check.check_number.trim();
            if number.is_empty() || number.chars().count() > 20 {
                return Err("PRINT_BAD_NUMBER: give each printed check a number of 1 to 20 characters".into());
            }
            let row: Option<(String, bool, bool, i64)> =
                sqlx::query_as("SELECT kind, to_be_printed, is_voided, payment_account_id FROM expenses WHERE id = ?1")
                    .bind(check.expense_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(db_err)?;
            let account = match row {
                Some((kind, true, false, account)) if kind == "CHECK" => account,
                Some(_) => return Err(format!("PRINT_NOT_QUEUED: check {} is no longer waiting to print", check.expense_id)),
                None => return Err(format!("PRINT_NOT_FOUND: check {} does not exist", check.expense_id)),
            };
            if seen.iter().any(|(a, n)| *a == account && n == number) {
                return Err(format!("PRINT_DUPLICATE_NUMBER: check number {number} is given to two checks"));
            }
            let used: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM expenses WHERE payment_account_id = ?1 AND trim(check_number) = ?2)
                     OR EXISTS (SELECT 1 FROM bill_payments WHERE payment_account_id = ?1 AND kind = 'STANDARD' AND trim(reference) = ?2)",
            )
            .bind(account)
            .bind(number)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_err)?;
            if used {
                return Err(format!("PRINT_NUMBER_USED: check number {number} is already used on this account; start from a later number"));
            }
            sqlx::query("UPDATE expenses SET check_number = ?1, to_be_printed = 0 WHERE id = ?2")
                .bind(number)
                .bind(check.expense_id)
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
            seen.push((account, number.to_string()));
        }
        Ok(printed.len())
    })
    .await
}

/// Takes printed invoices out of the queue; returns how many were waiting in it.
pub async fn mark_invoices_printed(pool: &SqlitePool, invoice_ids: Vec<i64>) -> Result<usize, String> {
    let ids = serde_json::to_string(&invoice_ids).map_err(|e| e.to_string())?;
    run(pool, async |tx| {
        let done = sqlx::query("UPDATE invoices SET to_be_printed = 0 WHERE to_be_printed = 1 AND id IN (SELECT value FROM json_each(?1))")
            .bind(&ids)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        Ok(done.rows_affected() as usize)
    })
    .await
}

#[cfg(test)]
mod tests;
