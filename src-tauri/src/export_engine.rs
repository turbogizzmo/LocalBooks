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
use crate::statements::{self, LineKind, ReportLine};

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

fn long_date(d: NaiveDate) -> String {
    d.format("%B %-d, %Y").to_string()
}

pub async fn profit_and_loss_csv(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<String, String> {
    let pl = statements::profit_and_loss(pool, start, end).await?;
    let company = preferences::get(pool, "company_name").await?;
    let period = if start == end { long_date(start) } else { format!("{} - {}", long_date(start), long_date(end)) };
    statement_to_csv(&company, "Profit and Loss", &period, &pl.lines)
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

/// Renders the named report (`profit_and_loss` or `balance_sheet`) as CSV.
pub async fn render(pool: &SqlitePool, report: &str, start: Option<NaiveDate>, end: Option<NaiveDate>, as_of: Option<NaiveDate>) -> Result<String, String> {
    match report {
        "profit_and_loss" => {
            let (s, e) = start.zip(end).ok_or_else(|| "EXPORT_BAD_RANGE: a Profit and Loss export needs a start and end date".to_string())?;
            profit_and_loss_csv(pool, s, e).await
        }
        "balance_sheet" => {
            let a = as_of.ok_or_else(|| "EXPORT_BAD_RANGE: a Balance Sheet export needs an as-of date".to_string())?;
            balance_sheet_csv(pool, a).await
        }
        other => Err(format!("EXPORT_UNKNOWN_REPORT: '{other}' cannot be exported")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};

    #[test]
    fn plain_amounts_are_spreadsheet_numbers() {
        assert_eq!(plain_amount(123_456), "1234.56");
        assert_eq!(plain_amount(-5), "-0.05");
        assert_eq!(plain_amount(0), "0.00");
    }

    #[tokio::test]
    async fn exports_statements_as_quoted_indented_csv() {
        let pool = test_pool().await;
        let id = |n: &'static str| {
            let pool = pool.clone();
            async move {
                sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(&pool).await.unwrap()
            }
        };
        let (checking, services) = (id("1010").await, id("4100").await);
        let date = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        post_journal_entry(&pool, JournalEntry { id: None, date, description: String::new(), lines: vec![JournalLine::debit(checking, 123_456), JournalLine::credit(services, 123_456)], currency: None })
            .await
            .unwrap();
        crate::preferences::set(&pool, "company_name", "Acme, \"Widgets\" & Co").await.unwrap();

        let csv = render(&pool, "profit_and_loss", Some(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()), Some(NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()), None).await.unwrap();
        assert!(csv.starts_with('\u{feff}'));
        let mut reader = csv::ReaderBuilder::new().has_headers(false).flexible(true).from_reader(csv.trim_start_matches('\u{feff}').as_bytes());
        let rows: Vec<Vec<String>> = reader.records().map(|r| r.unwrap().iter().map(String::from).collect()).collect();
        assert_eq!(rows[0], vec!["Acme, \"Widgets\" & Co"], "commas and quotes survive a round trip");
        assert_eq!(rows[2], vec!["January 1, 2026 - December 31, 2026"]);
        assert!(rows.contains(&vec!["  4100 Services".to_string(), "Account".to_string(), "1234.56".to_string()]));
        assert_eq!(rows.last().unwrap(), &vec!["Net Income".to_string(), "Net".to_string(), "1234.56".to_string()]);

        let bs = render(&pool, "balance_sheet", None, None, Some(date)).await.unwrap();
        assert!(bs.contains("TOTAL ASSETS,Net,1234.56"));
        assert!(render(&pool, "cash_flow", None, None, Some(date)).await.unwrap_err().starts_with("EXPORT_UNKNOWN_REPORT"));

        let dir = std::env::temp_dir().join(format!("localbooks-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("pl.csv");
        write_atomically(&target, csv.as_bytes()).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), csv);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
