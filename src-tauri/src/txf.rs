//! Tax software export in TXF (Tax Exchange Format), version 042.
//!
//! TXF is the plain-text format personal tax programs import. The public specification is kept at
//! <https://taxdataexchange.org/docs/txf/v042/index.html>, with its reference numbers listed by form
//! at <https://taxdataexchange.org/docs/txf/v042/reference-numbers-by-form.html>. In our own words:
//!
//! - The file is ASCII text, one field per line, each line ending in CR LF. A field is one letter
//!   saying what it is, followed at once by its value.
//! - It starts with a header record: `V042` (the version), `A` and the exporting program's name, and
//!   `D` and the export date as mm/dd/yyyy. Every record, the header included, ends with a line
//!   holding only `^`.
//! - Each data record is `T` (`S` for a summary record), `N` and a three-digit reference number
//!   that names the form line, `C` (copy number, which form when there are several) and `L` (line
//!   number within that copy), then the value. Record format 1 is one `$` amount; format 3 adds a
//!   `P` description after it.
//! - Amounts have two decimals and no thousands separators. Income is positive; expenses and
//!   anything that reduces income are negative. That is credits minus debits, so the amount of every
//!   line is the accounts' credits less their debits for the year.
//!
//! The specification defines reference numbers for a business's own return only on Schedule C (the
//! sole proprietor's schedule): partnership, S and C corporation and nonprofit returns have none, so
//! the export is offered for companies that file Schedule C. Two Schedule C lines LocalBooks maps
//! (depreciation, which goes through Form 4562, and business use of home, Form 8829) have no plain
//! Schedule C reference number; their totals are listed for the user to enter by hand.

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::accountant_reports::{tax_lines, TaxLineKind};
use crate::ledger::db_err;

/// The tax form whose lines TXF carries.
pub const TXF_FORM: &str = "SCHEDULE_C";

/// LocalBooks Schedule C line → TXF reference number and record format.
const SCHEDULE_C_REFS: &[(&str, u16, u8)] = &[
    ("1", 293, 1),
    ("2", 296, 1),
    ("4", 295, 1),
    ("6", 303, 1),
    ("8", 304, 1),
    ("9", 306, 1),
    ("10", 307, 1),
    ("11", 685, 1),
    ("12", 309, 1),
    ("14", 308, 1),
    ("15", 310, 1),
    ("16a", 311, 1),
    ("16b", 312, 1),
    ("17", 298, 1),
    ("18", 313, 1),
    ("19", 314, 1),
    ("20a", 299, 1),
    ("20b", 300, 1),
    ("21", 315, 1),
    ("22", 301, 1),
    ("23", 316, 1),
    ("24a", 317, 1),
    ("24b", 294, 1),
    ("25", 318, 1),
    ("26", 297, 1),
    // Other expenses: one record per account, described by the account's name.
    ("27a", 302, 3),
];

/// The TXF reference number and record format for a Schedule C line, if TXF has one.
pub fn reference_for(line: &str) -> Option<(u16, u8)> {
    SCHEDULE_C_REFS.iter().find(|(l, _, _)| *l == line).map(|(_, n, f)| (*n, *f))
}

/// One record written to the file.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TxfRecord {
    pub reference: u16,
    /// "8 · Advertising".
    pub tax_line: String,
    /// The `P` description (format 3 only): the account's name.
    pub description: Option<String>,
    /// As written: credits minus debits.
    pub amount_cents: i64,
}

/// Something with activity in the year that the file does not carry, and why.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TxfLeftOut {
    pub account_name: String,
    pub reason: String,
    /// Credits minus debits for the year.
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TxfExport {
    pub text: String,
    pub records: Vec<TxfRecord>,
    pub left_out: Vec<TxfLeftOut>,
}

#[derive(sqlx::FromRow)]
struct Row {
    account_name: String,
    tax_line: Option<String>,
    /// Credits minus debits in the period.
    net_cents: i64,
}

/// `-1234.5` dollars as TXF writes it: `-1234.50`.
pub fn amount_text(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

/// TXF is ASCII: anything else in a description becomes `?`, and the line stays one line.
fn ascii(text: &str) -> String {
    text.chars().map(|c| if c.is_ascii() && !c.is_ascii_control() { c } else { '?' }).collect()
}

/// The TXF file for income and expense activity between `start` and `end` (normally one tax year),
/// for a company that files Schedule C. `today` is the export date in the header.
pub async fn export(pool: &SqlitePool, start: NaiveDate, end: NaiveDate, today: NaiveDate) -> Result<TxfExport, String> {
    if start > end {
        return Err("RPT_BAD_RANGE: the start date is after the end date".into());
    }
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT a.full_name AS account_name, c.tax_line,
                COALESCE((SELECT SUM(l.credit_cents - l.debit_cents) FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
                          WHERE l.account_id = a.id AND e.status = 'POSTED' AND e.entry_date BETWEEN ?1 AND ?2), 0) AS net_cents
         FROM v_chart_of_accounts a JOIN chart_of_accounts c ON c.id = a.id
         WHERE a.classification IN ('INCOME', 'EXPENSE')
         ORDER BY a.type_sort_order, a.account_number, a.full_name COLLATE NOCASE",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let all = tax_lines();
    let overflow = || "TXF_OVERFLOW: the amounts are out of range".to_string();
    // Records in Schedule C order; format 1 lines add up their accounts.
    let mut records: Vec<TxfRecord> = Vec::new();
    let mut left_out = Vec::new();
    for r in rows.iter().filter(|r| r.net_cents != 0) {
        let line = r.tax_line.as_deref().and_then(|code| all.iter().find(|t| t.code == code));
        let Some(line) = line else {
            let reason = match &r.tax_line {
                None => "No tax line: assign one under Tax lines…".to_string(),
                Some(text) => format!("'{text}' is not a Tillhouse tax line"),
            };
            left_out.push(TxfLeftOut { account_name: r.account_name.clone(), reason, amount_cents: r.net_cents });
            continue;
        };
        if line.form != TXF_FORM || line.kind == TaxLineKind::BalanceSheet {
            left_out.push(TxfLeftOut {
                account_name: r.account_name.clone(),
                reason: format!("{} is not a Schedule C line", line.title()),
                amount_cents: r.net_cents,
            });
            continue;
        }
        let Some((reference, format)) = reference_for(line.line) else {
            left_out.push(TxfLeftOut {
                account_name: r.account_name.clone(),
                reason: format!("Schedule C {} has no TXF reference number; enter it in the tax program", line.title()),
                amount_cents: r.net_cents,
            });
            continue;
        };
        if format == 3 {
            records.push(TxfRecord { reference, tax_line: line.title(), description: Some(r.account_name.clone()), amount_cents: r.net_cents });
        } else if let Some(rec) = records.iter_mut().find(|x| x.reference == reference) {
            rec.amount_cents = rec.amount_cents.checked_add(r.net_cents).ok_or_else(overflow)?;
        } else {
            records.push(TxfRecord { reference, tax_line: line.title(), description: None, amount_cents: r.net_cents });
        }
    }
    let order = |n: u16| SCHEDULE_C_REFS.iter().position(|(_, r, _)| *r == n).unwrap_or(usize::MAX);
    records.sort_by_key(|r| order(r.reference));
    records.retain(|r| r.amount_cents != 0);

    let mut lines: Vec<String> = vec!["V042".into(), format!("ATillhouse {}", env!("CARGO_PKG_VERSION")), format!("D{}", today.format("%m/%d/%Y")), "^".into()];
    for r in &records {
        lines.extend(["TS".to_string(), format!("N{}", r.reference), "C1".into(), "L1".into(), format!("${}", amount_text(r.amount_cents))]);
        if let Some(d) = &r.description {
            lines.push(format!("P{}", ascii(d)));
        }
        lines.push("^".into());
    }
    let mut text = lines.join("\r\n");
    text.push_str("\r\n");
    Ok(TxfExport { text, records, left_out })
}

#[cfg(test)]
mod tests;
