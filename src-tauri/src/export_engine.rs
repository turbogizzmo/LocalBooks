//! Financial statement export: CSV payloads for spreadsheets and atomic writes to disk.
//!
//! CSV layout: a header block (company, title, period, basis), a blank row, then one row per
//! statement line with the account label indented by two spaces per level, a line-type column, and
//! the amount as a plain number (no currency symbol or thousands separators) so spreadsheet cells
//! stay numeric. The file starts with a UTF-8 byte-order mark so Excel reads symbols like € correctly.

use std::io::Write;
use std::path::Path;

use chrono::NaiveDate;
use sqlx::SqlitePool;

use crate::preferences;
use crate::statements::{self, ClassFilter, ClassReportLine, LineKind, ReportLine};

/// Cents as a plain decimal string: 123456 → "1234.56", -5 → "-0.05".
pub fn plain_amount(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

fn kind_label(kind: LineKind) -> &'static str {
    match kind {
        LineKind::Section => "Section",
        LineKind::Account => "Account",
        LineKind::Subtotal => "Subtotal",
        LineKind::Total => "Total",
        LineKind::Net => "Net",
    }
}

/// Renders statement lines as a CSV document.
pub fn statement_to_csv(company: &str, title: &str, period: &str, lines: &[ReportLine]) -> Result<String, String> {
    let mut writer = csv::WriterBuilder::new().flexible(true).from_writer(Vec::new());
    let mut row = |fields: &[&str]| writer.write_record(fields).map_err(|e| format!("EXPORT_CSV: {e}"));
    row(&[company])?;
    row(&[title])?;
    row(&[period])?;
    row(&["Accrual basis"])?;
    row(&[""])?;
    row(&["Account", "Line type", "Total"])?;
    for line in lines {
        let label = format!("{}{}", "  ".repeat(line.depth as usize), line.label);
        let amount = line.amount_cents.map(plain_amount).unwrap_or_default();
        row(&[label.as_str(), kind_label(line.kind), amount.as_str()])?;
    }
    let bytes = writer.into_inner().map_err(|e| format!("EXPORT_CSV: {e}"))?;
    let body = String::from_utf8(bytes).map_err(|e| format!("EXPORT_CSV: {e}"))?;
    Ok(format!("\u{feff}{body}"))
}

/// Renders multi-column (by class) statement lines as a CSV document, one amount column per class.
pub fn class_statement_to_csv(company: &str, title: &str, period: &str, columns: &[String], lines: &[ClassReportLine]) -> Result<String, String> {
    let mut writer = csv::WriterBuilder::new().flexible(true).from_writer(Vec::new());
    let mut row = |fields: &[&str]| writer.write_record(fields).map_err(|e| format!("EXPORT_CSV: {e}"));
    row(&[company])?;
    row(&[title])?;
    row(&[period])?;
    row(&["Accrual basis"])?;
    row(&[""])?;
    let mut header = vec!["Account", "Line type"];
    header.extend(columns.iter().map(String::as_str));
    row(&header)?;
    for line in lines {
        let mut fields = vec![format!("{}{}", "  ".repeat(line.depth as usize), line.label), kind_label(line.kind).to_string()];
        fields.extend(line.amounts_cents.iter().map(|a| a.map(plain_amount).unwrap_or_default()));
        row(&fields.iter().map(String::as_str).collect::<Vec<_>>())?;
    }
    let bytes = writer.into_inner().map_err(|e| format!("EXPORT_CSV: {e}"))?;
    let body = String::from_utf8(bytes).map_err(|e| format!("EXPORT_CSV: {e}"))?;
    Ok(format!("\u{feff}{body}"))
}

fn long_date(d: NaiveDate) -> String {
    d.format("%B %-d, %Y").to_string()
}

pub async fn profit_and_loss_csv(pool: &SqlitePool, start: NaiveDate, end: NaiveDate, filter: Option<&ClassFilter>) -> Result<String, String> {
    let pl = statements::profit_and_loss_filtered(pool, start, end, filter).await?;
    let company = preferences::get(pool, "company_name").await?;
    let mut period = if start == end { long_date(start) } else { format!("{} - {}", long_date(start), long_date(end)) };
    if let Some(filter) = filter {
        period = format!("{period} · {}", statements::class_filter_label(pool, filter).await?);
    }
    statement_to_csv(&company, "Profit and Loss", &period, &pl.lines)
}

pub async fn profit_and_loss_by_class_csv(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<String, String> {
    let report = statements::profit_and_loss_by_class(pool, start, end).await?;
    let company = preferences::get(pool, "company_name").await?;
    let period = if start == end { long_date(start) } else { format!("{} - {}", long_date(start), long_date(end)) };
    let columns: Vec<String> = report.columns.iter().map(|c| c.label.clone()).collect();
    class_statement_to_csv(&company, "Profit and Loss by Class", &period, &columns, &report.lines)
}

pub async fn balance_sheet_csv(pool: &SqlitePool, as_of: NaiveDate) -> Result<String, String> {
    let bs = statements::balance_sheet(pool, as_of).await?;
    let company = preferences::get(pool, "company_name").await?;
    statement_to_csv(&company, "Balance Sheet", &format!("As of {}", long_date(as_of)), &bs.lines)
}

/// Writes `content` to `path` atomically: a sibling temp file is written, synced, then renamed.
pub fn write_atomically(path: &Path, content: &[u8]) -> Result<(), String> {
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = path.file_name().ok_or_else(|| "EXPORT_BAD_PATH: choose a file name".to_string())?;
    let tmp = dir.join(format!(".{}.partial-{}", name.to_string_lossy(), std::process::id()));
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(content)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("EXPORT_WRITE: could not write {}: {e}", path.display()));
    }
    Ok(())
}

/// Renders the named report (`profit_and_loss`, `profit_and_loss_by_class` or `balance_sheet`) as CSV.
pub async fn render(
    pool: &SqlitePool,
    report: &str,
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
    as_of: Option<NaiveDate>,
    class_filter: Option<&ClassFilter>,
) -> Result<String, String> {
    match report {
        "profit_and_loss" => {
            let (s, e) = start.zip(end).ok_or_else(|| "EXPORT_BAD_RANGE: a Profit and Loss export needs a start and end date".to_string())?;
            profit_and_loss_csv(pool, s, e, class_filter).await
        }
        "profit_and_loss_by_class" => {
            let (s, e) = start.zip(end).ok_or_else(|| "EXPORT_BAD_RANGE: a Profit and Loss by Class export needs a start and end date".to_string())?;
            profit_and_loss_by_class_csv(pool, s, e).await
        }
        "balance_sheet" => {
            let a = as_of.ok_or_else(|| "EXPORT_BAD_RANGE: a Balance Sheet export needs an as-of date".to_string())?;
            balance_sheet_csv(pool, a).await
        }
        other => Err(format!("EXPORT_UNKNOWN_REPORT: '{other}' cannot be exported")),
    }
}

#[cfg(test)]
mod tests;
