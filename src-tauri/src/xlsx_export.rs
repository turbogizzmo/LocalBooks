//! Excel (.xlsx) export: the same reports, registers and lists as the CSV export, as a workbook with
//! typed cells.
//!
//! Layout follows QuickBooks' "Export to Excel": company, report title and date range in the top
//! rows, a blank row, then a bold column-header row that stays frozen while scrolling. Money is a
//! real number with a currency format (negatives in red, with a minus sign as on screen), dates are
//! real date cells, account hierarchies keep their indent through the cell's indent level, and
//! section, subtotal and total rows are bold with rules like the on-screen statement.
//!
//! Screens describe a report as a [`Sheet`]; the financial statements the CSV export renders on the
//! Rust side ([`crate::export_engine::render`]) are converted here by [`render_report`].

use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};
use rust_xlsxwriter::{Color, ExcelDateTime, Format, FormatAlign, FormatBorder, Workbook, Worksheet};
use serde::Deserialize;
use serde_json::Value;
use sqlx::SqlitePool;

use crate::preferences;
use crate::statements::{self, ClassFilter, ClassReportLine, LineKind, ReportLine};

/// One report as a single worksheet.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Sheet {
    pub company: String,
    pub title: String,
    /// The date range ("January 1 - March 31, 2026") or as-of date.
    #[serde(default)]
    pub subtitle: String,
    /// Further header lines: basis, class filter and the like.
    #[serde(default)]
    pub notes: Vec<String>,
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
    /// The column a row's `depth` indents (the account name); the first column unless given.
    #[serde(default)]
    pub indent_column: usize,
    /// A line under the report, as on screen.
    #[serde(default)]
    pub footer: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub label: String,
    #[serde(default)]
    pub kind: ColumnKind,
}

/// How a column's cells are read and written.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ColumnKind {
    /// Strings as text; numbers stay numbers.
    #[default]
    Text,
    /// Integer cents, written as currency.
    Money,
    /// Any number (quantities, counts).
    Number,
    /// `YYYY-MM-DD`, written as a date cell.
    Date,
    /// `YYYY-MM-DD HH:MM:SS` (or with a `T`), written as a date-and-time cell.
    Datetime,
    /// Basis points (12.5% = 1250), as on the "% Change" and "% of Budget" columns.
    Percent,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    #[serde(default)]
    pub kind: RowKind,
    /// Indent of the sheet's indent column (account hierarchy level).
    #[serde(default)]
    pub depth: u8,
    /// One value per column; `null` or `""` leaves the cell empty.
    pub cells: Vec<Value>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RowKind {
    #[default]
    Detail,
    Section,
    Subtotal,
    Total,
    Net,
}

impl From<LineKind> for RowKind {
    fn from(kind: LineKind) -> Self {
        match kind {
            LineKind::Section => RowKind::Section,
            LineKind::Account => RowKind::Detail,
            LineKind::Subtotal => RowKind::Subtotal,
            LineKind::Total => RowKind::Total,
            LineKind::Net => RowKind::Net,
        }
    }
}

/// Excel's number format for money in the company's currency: `"$"#,##0.00;[Red]-"$"#,##0.00`.
pub fn money_format(symbol: &str) -> String {
    let symbol = symbol.trim();
    if symbol.is_empty() {
        return "#,##0.00;[Red]-#,##0.00".into();
    }
    let quoted = format!("\"{}\"", symbol.replace('"', ""));
    format!("{quoted}#,##0.00;[Red]-{quoted}#,##0.00")
}

/// A worksheet name Excel accepts: at most 31 characters, none of `[]:*?/\`, not blank.
fn sheet_name(title: &str) -> String {
    let cleaned: String = title.chars().map(|c| if "[]:*?/\\".contains(c) { '-' } else { c }).collect();
    let trimmed: String = cleaned.trim().trim_matches('\'').chars().take(31).collect();
    if trimmed.trim().is_empty() {
        "Report".into()
    } else {
        trimmed
    }
}

fn parse_date(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok()
}

fn parse_datetime(text: &str) -> Option<NaiveDateTime> {
    let text = text.trim();
    ["%Y-%m-%d %H:%M:%S", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M"]
        .iter()
        .find_map(|f| NaiveDateTime::parse_from_str(text, f).ok())
        .or_else(|| parse_date(text).and_then(|d| d.and_hms_opt(0, 0, 0)))
}

/// What a cell becomes once its column kind is applied.
enum CellValue {
    Empty,
    Text(String),
    Number(f64),
    Date(NaiveDate),
    DateTime(NaiveDateTime),
}

fn cell_value(kind: ColumnKind, value: &Value) -> CellValue {
    match value {
        Value::Null => CellValue::Empty,
        Value::String(s) if s.trim().is_empty() => CellValue::Empty,
        Value::Bool(b) => CellValue::Text(if *b { "Yes" } else { "No" }.into()),
        Value::Number(n) => match kind {
            // Cents and basis points are integers; f64 holds every amount the ledger allows exactly
            // enough for a two-decimal display.
            ColumnKind::Money => n.as_i64().map(|c| CellValue::Number(c as f64 / 100.0)).unwrap_or_else(|| CellValue::Number(n.as_f64().unwrap_or(0.0) / 100.0)),
            ColumnKind::Percent => CellValue::Number(n.as_f64().unwrap_or(0.0) / 10_000.0),
            _ => CellValue::Number(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => match kind {
            ColumnKind::Date => parse_date(s).map(CellValue::Date).unwrap_or_else(|| CellValue::Text(s.clone())),
            ColumnKind::Datetime => parse_datetime(s).map(CellValue::DateTime).unwrap_or_else(|| CellValue::Text(s.clone())),
            _ => CellValue::Text(s.clone()),
        },
        other => CellValue::Text(other.to_string()),
    }
}

/// Characters the cell shows, for column widths.
fn display_width(kind: ColumnKind, value: &CellValue) -> usize {
    match value {
        CellValue::Empty => 0,
        CellValue::Text(s) => s.chars().count(),
        CellValue::Date(_) => 10,
        CellValue::DateTime(_) => 19,
        CellValue::Number(n) => match kind {
            // "$1,234,567.89": digits, separators, cents and symbol.
            ColumnKind::Money => {
                let digits = format!("{:.0}", n.abs().trunc()).len();
                digits + digits.saturating_sub(1) / 3 + 5
            }
            ColumnKind::Percent => 7,
            _ => format!("{n}").len(),
        },
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Rule {
    None,
    Top,
    TopAndDouble,
}

/// The sheet's money format; `cell` builds each cell's format from its column kind, boldness, rule and indent.
struct Formats {
    money: String,
}

impl Formats {
    fn cell(&self, kind: ColumnKind, bold: bool, rule: Rule, indent: u8) -> Format {
        let mut f = Format::new();
        f = match kind {
            ColumnKind::Money => f.set_num_format(&self.money),
            ColumnKind::Percent => f.set_num_format("0.0%;[Red]-0.0%"),
            ColumnKind::Number => f.set_num_format("General;[Red]-General"),
            ColumnKind::Date => f.set_num_format("mm/dd/yyyy").set_align(FormatAlign::Left),
            ColumnKind::Datetime => f.set_num_format("mm/dd/yyyy h:mm AM/PM").set_align(FormatAlign::Left),
            ColumnKind::Text => f,
        };
        if bold {
            f = f.set_bold();
        }
        if indent > 0 {
            f = f.set_indent(indent);
        }
        match rule {
            Rule::None => f,
            Rule::Top => f.set_border_top(FormatBorder::Thin),
            Rule::TopAndDouble => f.set_border_top(FormatBorder::Thin).set_border_bottom(FormatBorder::Double),
        }
    }
}

fn xlsx_err(e: impl std::fmt::Display) -> String {
    format!("EXPORT_XLSX: {e}")
}

fn write_cell(sheet: &mut Worksheet, row: u32, col: u16, value: &CellValue, format: &Format) -> Result<(), String> {
    match value {
        CellValue::Empty => sheet.write_blank(row, col, format).map(|_| ()),
        CellValue::Text(s) => sheet.write_string_with_format(row, col, s, format).map(|_| ()),
        CellValue::Number(n) => sheet.write_number_with_format(row, col, *n, format).map(|_| ()),
        CellValue::Date(d) => {
            let date = ExcelDateTime::from_ymd(d.year() as u16, d.month() as u8, d.day() as u8).map_err(xlsx_err)?;
            sheet.write_datetime_with_format(row, col, &date, format).map(|_| ())
        }
        CellValue::DateTime(t) => {
            let date = ExcelDateTime::from_ymd(t.year() as u16, t.month() as u8, t.day() as u8)
                .and_then(|d| d.and_hms(t.hour() as u16, t.minute() as u8, t.second() as f64))
                .map_err(xlsx_err)?;
            sheet.write_datetime_with_format(row, col, &date, format).map(|_| ())
        }
    }
    .map_err(xlsx_err)
}

/// Renders `sheet` as an .xlsx workbook; `currency_symbol` comes from the company's preferences.
pub fn to_xlsx(sheet: &Sheet, currency_symbol: &str) -> Result<Vec<u8>, String> {
    if sheet.columns.is_empty() {
        return Err("EXPORT_XLSX: the report has no columns".into());
    }
    let formats = Formats { money: money_format(currency_symbol) };
    let mut workbook = Workbook::new();
    let ws = workbook.add_worksheet();
    ws.set_name(sheet_name(&sheet.title)).map_err(xlsx_err)?;

    // Header block.
    let mut row: u32 = 0;
    ws.write_string_with_format(row, 0, &sheet.company, &Format::new().set_bold().set_font_size(12)).map_err(xlsx_err)?;
    row += 1;
    ws.write_string_with_format(row, 0, &sheet.title, &Format::new().set_bold().set_font_size(14)).map_err(xlsx_err)?;
    row += 1;
    for line in std::iter::once(&sheet.subtitle).chain(sheet.notes.iter()).filter(|l| !l.trim().is_empty()) {
        ws.write_string(row, 0, line).map_err(xlsx_err)?;
        row += 1;
    }
    row += 1;

    // Column headers, frozen.
    let header_row = row;
    let mut widths: Vec<usize> = sheet.columns.iter().map(|c| c.label.chars().count()).collect();
    for (c, column) in sheet.columns.iter().enumerate() {
        let mut f = Format::new().set_bold().set_border_bottom(FormatBorder::Thin).set_background_color(Color::RGB(0xF2F2F2));
        if matches!(column.kind, ColumnKind::Money | ColumnKind::Number | ColumnKind::Percent) {
            f = f.set_align(FormatAlign::Right);
        }
        ws.write_string_with_format(header_row, c as u16, &column.label, &f).map_err(xlsx_err)?;
    }
    ws.set_freeze_panes(header_row + 1, 0).map_err(xlsx_err)?;
    ws.set_repeat_rows(header_row, header_row).map_err(xlsx_err)?;
    row += 1;

    // Body.
    for line in &sheet.rows {
        let bold = line.kind != RowKind::Detail;
        let rule = match line.kind {
            RowKind::Subtotal | RowKind::Total => Rule::Top,
            RowKind::Net => Rule::TopAndDouble,
            _ => Rule::None,
        };
        for (c, column) in sheet.columns.iter().enumerate() {
            let value = line.cells.get(c).map(|v| cell_value(column.kind, v)).unwrap_or(CellValue::Empty);
            let indent = if c == sheet.indent_column { line.depth } else { 0 };
            // Rules go over amounts only, as on screen.
            let amount = matches!(column.kind, ColumnKind::Money | ColumnKind::Number | ColumnKind::Percent);
            let cell_rule = if amount && !matches!(value, CellValue::Empty) { rule } else { Rule::None };
            let format = formats.cell(column.kind, bold, cell_rule, indent);
            write_cell(ws, row, c as u16, &value, &format)?;
            // An indent level is about three characters wide.
            widths[c] = widths[c].max(display_width(column.kind, &value) + indent as usize * 3);
        }
        row += 1;
    }

    if let Some(footer) = sheet.footer.as_deref().filter(|f| !f.trim().is_empty()) {
        ws.write_string_with_format(row + 1, 0, footer, &Format::new().set_italic().set_font_color(Color::RGB(0x595959))).map_err(xlsx_err)?;
    }

    for (c, column) in sheet.columns.iter().enumerate() {
        let floor = match column.kind {
            ColumnKind::Money => 12,
            ColumnKind::Date => 11,
            ColumnKind::Datetime => 20,
            _ => 6,
        };
        ws.set_column_width(c as u16, (widths[c].max(floor) + 2).min(60) as f64).map_err(xlsx_err)?;
    }
    if sheet.columns.len() > 6 {
        ws.set_landscape();
    }
    ws.set_print_fit_to_pages(1, 0);

    workbook.save_to_buffer().map_err(xlsx_err)
}

/// A single-column financial statement (Profit and Loss, Balance Sheet) as a sheet.
pub fn statement_sheet(company: &str, title: &str, period: &str, notes: Vec<String>, lines: &[ReportLine]) -> Sheet {
    Sheet {
        company: company.into(),
        title: title.into(),
        subtitle: period.into(),
        notes,
        columns: vec![Column { label: String::new(), kind: ColumnKind::Text }, Column { label: "Total".into(), kind: ColumnKind::Money }],
        rows: lines
            .iter()
            .map(|l| Row { kind: l.kind.into(), depth: l.depth.min(15) as u8, cells: vec![Value::from(l.label.clone()), l.amount_cents.map(Value::from).unwrap_or(Value::Null)] })
            .collect(),
        indent_column: 0,
        footer: None,
    }
}

/// A multi-column (by class) statement as a sheet, one money column per class.
pub fn class_statement_sheet(company: &str, title: &str, period: &str, columns: &[String], lines: &[ClassReportLine]) -> Sheet {
    let mut cols = vec![Column { label: String::new(), kind: ColumnKind::Text }];
    cols.extend(columns.iter().map(|c| Column { label: c.clone(), kind: ColumnKind::Money }));
    Sheet {
        company: company.into(),
        title: title.into(),
        subtitle: period.into(),
        notes: vec!["Accrual basis".into()],
        columns: cols,
        rows: lines
            .iter()
            .map(|l| {
                let mut cells = vec![Value::from(l.label.clone())];
                cells.extend(l.amounts_cents.iter().map(|a| a.map(Value::from).unwrap_or(Value::Null)));
                Row { kind: l.kind.into(), depth: l.depth.min(15) as u8, cells }
            })
            .collect(),
        indent_column: 0,
        footer: None,
    }
}

fn long_date(d: NaiveDate) -> String {
    d.format("%B %-d, %Y").to_string()
}

fn range_text(start: NaiveDate, end: NaiveDate) -> String {
    if start == end {
        long_date(start)
    } else {
        format!("{} - {}", long_date(start), long_date(end))
    }
}

/// The named report (`profit_and_loss`, `profit_and_loss_by_class` or `balance_sheet`) as a sheet,
/// matching [`crate::export_engine::render`].
pub async fn report_sheet(
    pool: &SqlitePool,
    report: &str,
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
    as_of: Option<NaiveDate>,
    class_filter: Option<&ClassFilter>,
) -> Result<Sheet, String> {
    let company = preferences::get(pool, "company_name").await?;
    match report {
        "profit_and_loss" => {
            let (s, e) = start.zip(end).ok_or_else(|| "EXPORT_BAD_RANGE: a Profit and Loss export needs a start and end date".to_string())?;
            let pl = statements::profit_and_loss_filtered(pool, s, e, class_filter).await?;
            let mut notes = vec!["Accrual basis".to_string()];
            if let Some(filter) = class_filter {
                notes.push(statements::class_filter_label(pool, filter).await?);
            }
            Ok(statement_sheet(&company, "Profit and Loss", &range_text(s, e), notes, &pl.lines))
        }
        "profit_and_loss_by_class" => {
            let (s, e) = start.zip(end).ok_or_else(|| "EXPORT_BAD_RANGE: a Profit and Loss by Class export needs a start and end date".to_string())?;
            let report = statements::profit_and_loss_by_class(pool, s, e).await?;
            let columns: Vec<String> = report.columns.iter().map(|c| c.label.clone()).collect();
            Ok(class_statement_sheet(&company, "Profit and Loss by Class", &range_text(s, e), &columns, &report.lines))
        }
        "balance_sheet" => {
            let a = as_of.ok_or_else(|| "EXPORT_BAD_RANGE: a Balance Sheet export needs an as-of date".to_string())?;
            let bs = statements::balance_sheet(pool, a).await?;
            Ok(statement_sheet(&company, "Balance Sheet", &format!("As of {}", long_date(a)), vec!["Accrual basis".into()], &bs.lines))
        }
        other => Err(format!("EXPORT_UNKNOWN_REPORT: '{other}' cannot be exported")),
    }
}

/// [`report_sheet`] rendered as a workbook in the company's currency.
pub async fn render_report(
    pool: &SqlitePool,
    report: &str,
    start: Option<NaiveDate>,
    end: Option<NaiveDate>,
    as_of: Option<NaiveDate>,
    class_filter: Option<&ClassFilter>,
) -> Result<Vec<u8>, String> {
    let sheet = report_sheet(pool, report, start, end, as_of, class_filter).await?;
    to_xlsx(&sheet, &preferences::currency_symbol(pool).await?)
}

#[cfg(test)]
mod tests;
