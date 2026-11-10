//! The numbers of sales forms: invoices, estimates, sales receipts, credit memos and sales orders.
//!
//! Each form shows the number it will get before it is saved (the next one: one more than the
//! highest used, so a number typed out of sequence carries on from there), and the person may type
//! another. A number is used once per kind of form: the schema keeps each column `UNIQUE`, because
//! the number is how a person finds the form again (lists, payments, statements, reports, the
//! customer's copy). So a typed number that is already used is refused with
//! [`DOC_NUMBER_TAKEN`], naming what uses it and the next free number, and the form offers to take
//! that one instead.
//!
//! Credit memos and refunds of unapplied payments share one sequence (both are rows of
//! `credit_memos`).

use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;

use crate::ledger::db_err;

/// The error a save gets for a typed number already in use.
pub const DOC_NUMBER_TAKEN: &str = "DOC_NUMBER_TAKEN";

/// The highest number a form may have, so the next one is always in range.
pub const MAX_DOC_NUMBER: i64 = 999_999_999;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DocKind {
    Invoice,
    Estimate,
    SalesReceipt,
    CreditMemo,
    SalesOrder,
}

impl DocKind {
    fn table(self) -> &'static str {
        match self {
            DocKind::Invoice => "invoices",
            DocKind::Estimate => "estimates",
            DocKind::SalesReceipt => "sales_receipts",
            DocKind::CreditMemo => "credit_memos",
            DocKind::SalesOrder => "sales_orders",
        }
    }

    fn column(self) -> &'static str {
        match self {
            DocKind::Invoice => "invoice_number",
            DocKind::Estimate => "estimate_number",
            DocKind::SalesReceipt => "receipt_number",
            DocKind::CreditMemo => "credit_number",
            DocKind::SalesOrder => "order_number",
        }
    }

    /// The number before the first one: invoices and estimates start at 1001, the rest at 1.
    fn before_first(self) -> i64 {
        match self {
            DocKind::Invoice | DocKind::Estimate => 1000,
            DocKind::SalesReceipt | DocKind::CreditMemo | DocKind::SalesOrder => 0,
        }
    }

    fn noun(self) -> &'static str {
        match self {
            DocKind::Invoice => "Invoice",
            DocKind::Estimate => "Estimate",
            DocKind::SalesReceipt => "Sales receipt",
            DocKind::CreditMemo => "Credit memo",
            DocKind::SalesOrder => "Sales order",
        }
    }

    /// What a used number belongs to, for the message: the customer and date of that form.
    fn owner_sql(self) -> &'static str {
        match self {
            DocKind::Invoice => "SELECT COALESCE(c.name, d.customer_name), d.issue_date FROM invoices d LEFT JOIN customers c ON c.id = d.customer_id WHERE d.invoice_number = ?1",
            DocKind::Estimate => "SELECT c.name, d.estimate_date FROM estimates d LEFT JOIN customers c ON c.id = d.customer_id WHERE d.estimate_number = ?1",
            DocKind::SalesReceipt => "SELECT c.name, d.receipt_date FROM sales_receipts d LEFT JOIN customers c ON c.id = d.customer_id WHERE d.receipt_number = ?1",
            DocKind::CreditMemo => "SELECT c.name, d.credit_date FROM credit_memos d LEFT JOIN customers c ON c.id = d.customer_id WHERE d.credit_number = ?1",
            DocKind::SalesOrder => "SELECT c.name, d.order_date FROM sales_orders d LEFT JOIN customers c ON c.id = d.customer_id WHERE d.order_number = ?1",
        }
    }
}

/// The number the next form of `kind` gets: one more than the highest used.
pub async fn next_number(conn: &mut SqliteConnection, kind: DocKind) -> Result<i64, String> {
    let next: i64 = sqlx::query_scalar(&format!("SELECT COALESCE(MAX({}), ?1) + 1 FROM {}", kind.column(), kind.table()))
        .bind(kind.before_first())
        .fetch_one(conn)
        .await
        .map_err(db_err)?;
    if next > MAX_DOC_NUMBER {
        return Err(format!("DOC_NUMBER_RANGE: {} numbers have reached {MAX_DOC_NUMBER}; type a lower number that isn't used", kind.noun()));
    }
    Ok(next)
}

/// The number to save a new form of `kind` under: `wanted` when the person typed one (refused with
/// [`DOC_NUMBER_TAKEN`] when it is already used), else the next one.
pub async fn take_number(conn: &mut SqliteConnection, kind: DocKind, wanted: Option<i64>) -> Result<i64, String> {
    match wanted {
        None => next_number(conn, kind).await,
        Some(wanted) => check_number(conn, kind, wanted).await.map(|()| wanted),
    }
}

/// `Ok` when `wanted` is free for a new form of `kind`; else why not ([`DOC_NUMBER_TAKEN`] names
/// what uses it and the next free number). The form asks this before saving, so the question comes
/// before the save's other warnings; saving checks again.
pub async fn check_number(conn: &mut SqliteConnection, kind: DocKind, wanted: i64) -> Result<(), String> {
    if !(1..=MAX_DOC_NUMBER).contains(&wanted) {
        return Err(format!("DOC_NUMBER_RANGE: a {} number is a whole number from 1 to {MAX_DOC_NUMBER}", kind.noun().to_lowercase()));
    }
    let owner: Option<(Option<String>, String)> = sqlx::query_as(kind.owner_sql()).bind(wanted).fetch_optional(&mut *conn).await.map_err(db_err)?;
    let Some((customer, date)) = owner else { return Ok(()) };
    let next = next_number(conn, kind).await?;
    let date = date.parse::<chrono::NaiveDate>().map_or(date, |d| d.format("%m/%d/%Y").to_string());
    let whose = match customer {
        Some(name) => format!("{name}, dated {date}"),
        None => format!("dated {date}"),
    };
    Err(format!("{DOC_NUMBER_TAKEN}: {} #{wanted} is already used ({whose}). The next free number is #{next}.", kind.noun()))
}

#[cfg(test)]
mod tests;
