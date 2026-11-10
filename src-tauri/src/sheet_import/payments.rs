//! Payments sheets (gap T62): customer payments against invoices, and bill payments against bills,
//! that are already in the books. A payments sheet is not staged: each row names the document it
//! pays, so it is checked against the books (the document exists, is the named customer's or
//! vendor's, is open in the home currency, and the rows paying it leave nothing negative) and then
//! posted through the same code as Receive Payments and Pay Bills, all rows in one transaction.

use std::collections::HashMap;

use chrono::NaiveDate;
use sqlx::SqlitePool;

use super::{amount_of, date_order, read_date, Check, Mapping, Problem, Rows, Sheet, SheetKind};
use crate::bank::DateOrder;
use crate::ledger::{db_err, format_cents};

/// One row of a payments sheet, as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentRow {
    pub row: usize,
    pub date: NaiveDate,
    pub party: String,
    /// The invoice or bill number the row pays.
    pub document: String,
    pub amount_cents: i64,
    /// Deposit to (customer payments; Undeposited Funds when empty) or Pay from (bill payments).
    pub account: Option<String>,
    pub method: Option<String>,
    pub reference: Option<String>,
}

pub fn is_payments(kind: SheetKind) -> bool {
    matches!(kind, SheetKind::CustomerPayments | SheetKind::BillPayments)
}

/// Reads the rows: what each must have, with format problems listed. The books are checked by
/// [`check_against_books`].
pub(super) fn read(kind: SheetKind, sheet: &Sheet, mapping: &Mapping, order: Option<DateOrder>, problems: &mut Vec<Problem>) -> Vec<PaymentRow> {
    let r = Rows { mapping };
    let order = date_order(sheet, mapping, order);
    let (party_label, document_label) = if kind == SheetKind::CustomerPayments { ("customer", "invoice number") } else { ("vendor", "bill number") };
    let mut out = Vec::new();
    for (row, cells) in &sheet.rows {
        let row = *row;
        let date = read_date(r.get(cells, "date"), order, "date", row, problems);
        let party = r.get(cells, "party");
        let document = r.get(cells, "document");
        if party.is_none() {
            problems.push(Problem { row, message: format!("The {party_label} is empty") });
        }
        if document.is_none() {
            problems.push(Problem { row, message: format!("The {document_label} is empty") });
        }
        let amount = match amount_of(&r, cells, "amount", problems, row, "amount") {
            Some(a) if a > 0 => Some(a),
            Some(_) => {
                problems.push(Problem { row, message: "Write the amount paid as more than zero".into() });
                None
            }
            None if r.get(cells, "amount").is_none() => {
                problems.push(Problem { row, message: "The amount is empty".into() });
                None
            }
            None => None,
        };
        let account = r.get(cells, "account");
        if kind == SheetKind::BillPayments && account.is_none() {
            problems.push(Problem { row, message: "Pay from is empty: name the bank or credit card account".into() });
        }
        if let (Some(date), Some(party), Some(document), Some(amount_cents)) = (date, party, document, amount) {
            out.push(PaymentRow {
                row,
                date,
                party: party.to_string(),
                document: document.to_string(),
                amount_cents,
                account: account.map(String::from),
                method: r.get(cells, "method").map(String::from),
                reference: r.get(cells, "reference").map(String::from),
            });
        }
    }
    out
}

/// What a checked row will post: the document it pays and the account the money goes to or from.
#[derive(Debug, Clone)]
struct Resolved {
    document_id: i64,
    account_id: i64,
}

/// Finds an account by its name or number among the kinds a row allows.
async fn account(pool: &SqlitePool, name: &str, allowed: &[&str]) -> Result<Option<(i64, String)>, String> {
    let rows: Vec<(i64, String, String, bool, bool)> = sqlx::query_as(
        "SELECT id, account_type, name, is_system, is_active FROM chart_of_accounts
         WHERE name = ?1 COLLATE NOCASE OR account_number = ?1",
    )
    .bind(name.trim())
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .find(|(_, kind, _, system, active)| *active && allowed.iter().any(|a| *a == kind.as_str() || (*a == "UNDEPOSITED" && kind == "OTHER_CURRENT_ASSET" && *system)))
        .map(|(id, _, name, _, _)| (id, name)))
}

/// Checks every row against the books: the document, whose it is, that it is open and in the home
/// currency, the account, and that the rows paying one document don't pay more than is open.
async fn resolve(pool: &SqlitePool, kind: SheetKind, rows: &[PaymentRow], problems: &mut Vec<Problem>) -> Result<Vec<Resolved>, String> {
    let undeposited: Option<i64> =
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'OTHER_CURRENT_ASSET' AND is_system = 1 ORDER BY id LIMIT 1")
            .fetch_optional(pool)
            .await
            .map_err(db_err)?;
    let mut open_left: HashMap<i64, i64> = HashMap::new();
    let mut out = Vec::new();
    for p in rows {
        let row = p.row;
        let found: Vec<(i64, String, i64, String, bool, Option<String>)> = if kind == SheetKind::CustomerPayments {
            let Ok(number) = p.document.trim().trim_start_matches('#').parse::<i64>() else {
                problems.push(Problem { row, message: format!("Invoice number \"{}\" isn't a number", p.document) });
                continue;
            };
            sqlx::query_as(
                "SELECT i.id, COALESCE(c.name, i.customer_name), i.total_cents - i.amount_paid_cents, i.status, i.is_voided, i.currency_code
                 FROM invoices i LEFT JOIN customers c ON c.id = i.customer_id WHERE i.invoice_number = ?1",
            )
            .bind(number)
            .fetch_all(pool)
            .await
            .map_err(db_err)?
        } else {
            sqlx::query_as(
                "SELECT b.id, v.name, b.total_cents - b.amount_paid_cents, b.status, b.is_voided, NULL
                 FROM bills b JOIN vendors v ON v.id = b.vendor_id
                 WHERE b.bill_number = ?1 COLLATE NOCASE AND v.name = ?2 COLLATE NOCASE",
            )
            .bind(p.document.trim())
            .bind(p.party.trim())
            .fetch_all(pool)
            .await
            .map_err(db_err)?
        };
        let what = if kind == SheetKind::CustomerPayments { format!("Invoice {}", p.document) } else { format!("Bill {} from {}", p.document, p.party) };
        let (id, owner, open, status, voided, currency) = match found.as_slice() {
            [] => {
                problems.push(Problem { row, message: format!("{what} isn't in the books") });
                continue;
            }
            [one] => one.clone(),
            _ => {
                problems.push(Problem { row, message: format!("{what} matches more than one bill; pay it in Pay Bills") });
                continue;
            }
        };
        if kind == SheetKind::CustomerPayments && !owner.eq_ignore_ascii_case(p.party.trim()) {
            problems.push(Problem { row, message: format!("{what} is {owner}'s, not {}'s", p.party) });
            continue;
        }
        if voided || !(status == "UNPAID" || status == "PARTIAL") {
            problems.push(Problem { row, message: format!("{what} has nothing left to pay") });
            continue;
        }
        if let Some(code) = currency {
            problems.push(Problem { row, message: format!("{what} is in {code}; record its payment in Receive Payments, with the exchange rate") });
            continue;
        }
        let left = open_left.entry(id).or_insert(open);
        if p.amount_cents > *left {
            problems.push(Problem { row, message: format!("{what} has {} left to pay, less than {}", format_cents(*left), format_cents(p.amount_cents)) });
            continue;
        }
        *left -= p.amount_cents;
        let account_id = match (kind, p.account.as_deref()) {
            (SheetKind::CustomerPayments, None) => match undeposited {
                Some(id) => id,
                None => {
                    problems.push(Problem { row, message: "There is no Undeposited Funds account; name the Deposit to account".into() });
                    continue;
                }
            },
            (SheetKind::CustomerPayments, Some(name)) => match account(pool, name, &["BANK", "UNDEPOSITED"]).await? {
                Some((id, _)) => id,
                None => {
                    problems.push(Problem { row, message: format!("Deposit to \"{name}\" isn't a bank account or Undeposited Funds") });
                    continue;
                }
            },
            (_, Some(name)) => match account(pool, name, &["BANK", "CREDIT_CARD"]).await? {
                Some((id, _)) => id,
                None => {
                    problems.push(Problem { row, message: format!("Pay from \"{name}\" isn't a bank or credit card account") });
                    continue;
                }
            },
            (_, None) => continue,
        };
        out.push(Resolved { document_id: id, account_id });
    }
    Ok(out)
}

/// The sheet's format problems and the books' problems together: nothing posts while there are any.
pub async fn check_against_books(pool: &SqlitePool, kind: SheetKind, sheet: &Sheet, mapping: &Mapping, order: Option<DateOrder>) -> Result<Check, String> {
    let mut check = super::check(kind, sheet, mapping, order);
    if !check.problems.is_empty() {
        return Ok(check);
    }
    let rows = read(kind, sheet, mapping, order, &mut Vec::new());
    resolve(pool, kind, &rows, &mut check.problems).await?;
    check.problems.sort_by_key(|p| p.row);
    Ok(check)
}

/// The Payment Method list's kind for what a row says ("Check", "Visa", "ACH"...).
fn method_kind(text: Option<&str>) -> &'static str {
    let t = text.unwrap_or("check").to_ascii_lowercase();
    if t.contains("cash") {
        "CASH"
    } else if t.contains("card") || t.contains("visa") || t.contains("master") || t.contains("amex") || t.contains("discover") {
        "CREDIT_CARD"
    } else if t.contains("ach") || t.contains("transfer") || t.contains("e-check") || t.contains("echeck") {
        "ACH"
    } else if t.contains("wire") {
        "WIRE"
    } else if t.contains("check") || t.contains("cheque") {
        "CHECK"
    } else {
        "OTHER"
    }
}

/// What posting a payments sheet did.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Posted {
    pub payments: usize,
    pub total_cents: i64,
}

/// Posts a payments sheet with no problems: every row, or none (`SHEET_PROBLEMS` while any row
/// would fail).
pub async fn post(pool: &SqlitePool, kind: SheetKind, sheet: &Sheet, mapping: &Mapping, order: Option<DateOrder>) -> Result<Posted, String> {
    let check = check_against_books(pool, kind, sheet, mapping, order).await?;
    if let Some(first) = check.problems.first() {
        let at = if first.row == 0 { first.message.clone() } else { format!("row {}: {}", first.row, first.message) };
        return Err(format!("SHEET_PROBLEMS: {} row problem(s) to fix before posting; the first is {at}", check.problems.len()));
    }
    let rows = read(kind, sheet, mapping, order, &mut Vec::new());
    if rows.is_empty() {
        return Err("SHEET_EMPTY: the spreadsheet has no rows to import".into());
    }
    // Resolved again for posting: a row that stopped resolving since the check (paid meanwhile) must
    // stop the post, or the rows and their documents would pair up wrongly.
    let mut late = Vec::new();
    let resolved = resolve(pool, kind, &rows, &mut late).await?;
    if let Some(first) = late.first() {
        return Err(format!("SHEET_PROBLEMS: the books changed since the check; row {}: {}", first.row, first.message));
    }
    if resolved.len() != rows.len() {
        return Err("SHEET_PROBLEMS: the books changed since the check; check the sheet again".into());
    }
    let total_cents = rows.iter().try_fold(0i64, |t, r| t.checked_add(r.amount_cents)).ok_or("SHEET_TOO_LARGE: the payments add up to more than LocalBooks can hold")?;
    if kind == SheetKind::CustomerPayments {
        let inputs: Vec<crate::invoices::PaymentInput> = rows
            .iter()
            .zip(&resolved)
            .map(|(r, x)| crate::invoices::PaymentInput {
                invoice_id: x.document_id,
                deposit_account_id: x.account_id,
                payment_date: r.date,
                amount_cents: r.amount_cents,
                payment_method: method_kind(r.method.as_deref()).into(),
                payment_method_id: None,
                reference: r.reference.clone(),
                exchange_rate_e6: None,
                discount_cents: 0,
            })
            .collect();
        crate::invoices::record_payments(pool, inputs).await?;
    } else {
        let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
        for (r, x) in rows.iter().zip(&resolved) {
            let input = crate::expenses::BillPaymentInput {
                bill_id: x.document_id,
                payment_account_id: x.account_id,
                payment_date: r.date,
                amount_cents: r.amount_cents,
                reference: r.reference.clone(),
                to_be_printed: false,
            };
            if let Err(err) = crate::expenses::write_bill_payment(&mut tx, &input, "STANDARD").await {
                return Err(crate::ledger::rollback(tx, format!("row {}: {err}", r.row)).await);
            }
        }
        crate::ledger::commit_write(tx).await.map_err(db_err)?;
    }
    Ok(Posted { payments: rows.len(), total_cents })
}
