//! Local bank feed: CSV import, vendor-matching category suggestions, and approval into the ledger.
//!
//! Sign convention: negative amounts are money out of the account, positive amounts are money in.
//! Amounts are parsed from text straight to integer cents; no floating point is involved.

use std::collections::HashMap;
use std::path::Path;

use chrono::{Datelike, NaiveDate};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

pub mod suggest;

use suggest::{Example, Query, SimilarTo, Suggester};

use crate::banking::{write_deposit, DepositInput, DepositOtherLine};
use crate::expenses::{write_expense, ExpenseInput, SplitLineInput};
use crate::ledger::{db_err, rollback, MAX_LINE_CENTS};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategorySuggestion {
    pub account_id: i64,
    pub rule_id: Option<i64>,
    /// `LEARNED` (a rule from past approvals), `BUILT_IN` (vendor list), `MODEL` (similar past
    /// transactions, see [`suggest`]) or `DEFAULT` (uncategorized fallback).
    pub origin: String,
    /// For `MODEL` suggestions: 0-100, and the past transaction this line is most like.
    pub confidence: Option<u8>,
    pub confidence_level: Option<&'static str>,
    pub similar_to: Option<SimilarTo>,
}

impl CategorySuggestion {
    fn plain(account_id: i64, rule_id: Option<i64>, origin: &str) -> Self {
        CategorySuggestion { account_id, rule_id, origin: origin.into(), confidence: None, confidence_level: None, similar_to: None }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankTransactionView {
    pub id: i64,
    pub bank_account_id: i64,
    pub date: String,
    pub description: String,
    pub amount_cents: i64,
    pub suggestion: Option<CategorySuggestion>,
    /// A payee learned from similar past expenses (money out only).
    pub payee: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub imported: usize,
    pub duplicates: usize,
    pub skipped_zero: usize,
}

// ------------------------------------------------------------------------------------------------
// Parsing
// ------------------------------------------------------------------------------------------------

/// Parses a money string to cents: `1234.5`, `$1,234.56`, `-12.00`, `(12.00)`, `12.00-`, `+3`.
pub fn parse_amount_cents(raw: &str) -> Result<i64, String> {
    let mut s: String = raw.trim().chars().filter(|c| !matches!(c, '$' | ',' | ' ' | '\u{a0}')).collect();
    if s.is_empty() {
        return Err("empty amount".into());
    }

    let mut negative = false;
    if s.starts_with('(') && s.ends_with(')') {
        negative = true;
        s = s[1..s.len() - 1].to_string();
    }
    if let Some(rest) = s.strip_prefix('-') {
        negative = !negative;
        s = rest.to_string();
    } else if let Some(rest) = s.strip_suffix('-') {
        negative = !negative;
        s = rest.to_string();
    } else if let Some(rest) = s.strip_prefix('+') {
        s = rest.to_string();
    }

    let (whole, frac) = match s.split_once('.') {
        Some((w, f)) => (w, f),
        None => (s.as_str(), ""),
    };
    if whole.is_empty() && frac.is_empty() {
        return Err(format!("invalid amount '{raw}'"));
    }
    if !whole.chars().all(|c| c.is_ascii_digit()) || !frac.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("invalid amount '{raw}'"));
    }
    if frac.len() > 2 && frac[2..].chars().any(|c| c != '0') {
        return Err(format!("amount '{raw}' has fractional cents"));
    }

    let dollars: i64 = if whole.is_empty() { 0 } else { whole.parse().map_err(|_| format!("amount '{raw}' is too large"))? };
    let cents: i64 = match frac.len() {
        0 => 0,
        1 => frac.parse::<i64>().map_err(|_| format!("invalid amount '{raw}'"))? * 10,
        _ => frac[..2].parse().map_err(|_| format!("invalid amount '{raw}'"))?,
    };
    let total = dollars
        .checked_mul(100)
        .and_then(|d| d.checked_add(cents))
        .filter(|t| *t <= MAX_LINE_CENTS)
        .ok_or_else(|| format!("amount '{raw}' is too large"))?;
    Ok(if negative { -total } else { total })
}

/// Day/month order for numeric dates such as `03/07/2026`. A four-digit first field (`2026-03-07`,
/// `2026/03/07`) is always year-month-day, and OFX's `20260307` is unambiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DateOrder {
    /// Month first (`03/07/2026` is March 7), as US banks export.
    #[default]
    Mdy,
    /// Day first (`03/07/2026` is 3 July), as UK, EU and many other banks export.
    Dmy,
}

impl DateOrder {
    fn code(self) -> &'static str {
        match self {
            DateOrder::Mdy => "MDY",
            DateOrder::Dmy => "DMY",
        }
    }
}

const MONTH_NAMES: [&[u8; 3]; 12] = [b"jan", b"feb", b"mar", b"apr", b"may", b"jun", b"jul", b"aug", b"sep", b"oct", b"nov", b"dec"];

/// Reads ASCII digits as a number; the caller guarantees `digits` is 1-4 ASCII digits.
fn digits_value(digits: &[u8]) -> u32 {
    digits.iter().fold(0, |n, d| n * 10 + u32::from(d - b'0'))
}

/// `Mar`, `march`, `SEPT` → 1-12. Anything that does not start with a month's first three letters is `None`.
fn month_from_name(word: &[u8]) -> Option<u32> {
    let head = word.get(..3)?;
    MONTH_NAMES.iter().position(|m| m.eq_ignore_ascii_case(head)).map(|i| i as u32 + 1)
}

/// Two-digit years follow chrono's `%y` rule: 00-69 are 2000-2069, 70-99 are 1970-1999.
fn full_year(value: u32, width: usize) -> Option<i32> {
    match width {
        4 => Some(value as i32),
        2 if value < 70 => Some(2000 + value as i32),
        2 => Some(1900 + value as i32),
        _ => None,
    }
}

fn checked_date(year: Option<i32>, month: u32, day: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(year?, month, day).filter(|d| (1900..=2200).contains(&d.year()))
}

/// Parses a bank date in one pass over its bytes, without allocating or trying formats in turn.
///
/// Accepts `2026-03-07`, `2026/03/07`, `03/07/2026`, `3/7/26`, `03-07-2026`, `07.03.2026`,
/// `Mar 7, 2026`, `7 Mar 2026`, `07-Mar-2026` and OFX's `20260307[HHMMSS[.xxx][[-5:EST]]]`.
/// A trailing time (`2026-10-01 00:00:00`, `2026-10-01T09:30`) is ignored.
pub fn parse_date_ordered(raw: &str, order: DateOrder) -> Result<NaiveDate, String> {
    let bytes = raw.trim().as_bytes();
    let fail = || format!("unrecognized date '{raw}'");

    // OFX: eight digits, then optionally a time and time zone.
    if bytes.len() >= 8 && bytes[..8].iter().all(u8::is_ascii_digit) && bytes.get(8).is_none_or(|b| !matches!(b, b'-' | b'/' | b'.')) {
        let date = checked_date(Some(digits_value(&bytes[..4]) as i32), digits_value(&bytes[4..6]), digits_value(&bytes[6..8]));
        return date.ok_or_else(fail);
    }

    let mut fields = [(0u32, 0usize); 3]; // (value, digit count)
    let mut count = 0;
    let mut month_name = None;
    let mut i = 0;
    while i < bytes.len() && count < 3 && !(count == 2 && month_name.is_some()) {
        let b = bytes[i];
        if b.is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i - start > 4 {
                return Err(fail());
            }
            fields[count] = (digits_value(&bytes[start..i]), i - start);
            count += 1;
        } else if b.is_ascii_alphabetic() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            if month_name.is_some() {
                return Err(fail());
            }
            month_name = Some(month_from_name(&bytes[start..i]).ok_or_else(fail)?);
        } else if matches!(b, b'-' | b'/' | b'.' | b' ' | b',') {
            i += 1;
        } else {
            return Err(fail());
        }
    }

    let date = match (month_name, count) {
        // "Mar 7, 2026" and "7 Mar 2026" both leave day then year.
        (Some(month), 2) => checked_date(full_year(fields[1].0, fields[1].1), month, fields[0].0),
        (None, 3) if fields[0].1 == 4 => checked_date(Some(fields[0].0 as i32), fields[1].0, fields[2].0),
        (None, 3) => {
            let ((first, _), (second, _), (year, width)) = (fields[0], fields[1], fields[2]);
            let (month, day) = match order {
                DateOrder::Mdy => (first, second),
                DateOrder::Dmy => (second, first),
            };
            checked_date(full_year(year, width), month, day)
        }
        _ => None,
    };
    date.ok_or_else(fail)
}

/// Parses a date written month-first, the US convention.
pub fn parse_date(raw: &str) -> Result<NaiveDate, String> {
    parse_date_ordered(raw, DateOrder::Mdy)
}

/// Picks the day/month order for a column of dates. Day-first is chosen only when some date can only
/// be read that way (a first number above 12); `certain` is false when every date reads both ways.
pub fn detect_date_order<'a>(values: impl IntoIterator<Item = &'a str>) -> (DateOrder, bool) {
    let (mut mdy, mut dmy, mut differs) = (true, true, false);
    for v in values {
        let a = parse_date_ordered(v, DateOrder::Mdy);
        let b = parse_date_ordered(v, DateOrder::Dmy);
        mdy &= a.is_ok();
        dmy &= b.is_ok();
        differs |= matches!((&a, &b), (Ok(x), Ok(y)) if x != y);
    }
    match (mdy, dmy) {
        (false, true) => (DateOrder::Dmy, true),
        (true, _) => (DateOrder::Mdy, !differs || !dmy),
        (false, false) => (DateOrder::Mdy, true),
    }
}

// ------------------------------------------------------------------------------------------------
// Files: CSV with a column mapping, and OFX/QFX
// ------------------------------------------------------------------------------------------------

/// One transaction read from a bank file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankRow {
    /// 1-based line number in the source file, for error messages and review.
    pub row: u64,
    pub date: NaiveDate,
    pub description: String,
    pub amount_cents: i64,
    /// The bank's own transaction id (OFX `FITID`), which identifies a transaction across downloads.
    pub fitid: Option<String>,
}

/// Which CSV columns hold what. Columns are named by their header text; either `amount` (signed)
/// or both `money_in` and `money_out` must be set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMapping {
    pub date: String,
    pub description: String,
    #[serde(default)]
    pub amount: Option<String>,
    #[serde(default)]
    pub money_in: Option<String>,
    #[serde(default)]
    pub money_out: Option<String>,
    #[serde(default)]
    pub date_order: DateOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BankFileFormat {
    Csv,
    Ofx,
}

/// Bank files are usually UTF-8, but OFX 1.x and older CSV exports are often Windows-1252.
fn decode_text(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    const CP1252: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“',
        '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    bytes.iter().map(|&b| if (0x80..0xA0).contains(&b) { CP1252[usize::from(b - 0x80)] } else { char::from(b) }).collect()
}

fn normalize_header(h: &str) -> String {
    h.trim_start_matches('\u{feff}').trim().to_lowercase()
}

/// Identifies a CSV layout by its headers, so a saved mapping is reused for the same bank's files.
pub fn header_signature(headers: &[String]) -> String {
    headers.iter().map(|h| normalize_header(h)).collect::<Vec<_>>().join("|")
}

/// Recognizes common layouts: a date, a description, and either one signed amount column or
/// separate money-in/money-out columns. An exact `Amount` header wins; otherwise a pair of distinct
/// debit/withdrawal and credit/deposit columns (including "Debit Amount"/"Credit Amount") is a split
/// layout, and only then does a header merely containing "amount" count as the signed amount.
pub fn detect_mapping(headers: &[String]) -> Option<ColumnMapping> {
    let norm: Vec<String> = headers.iter().map(|h| normalize_header(h)).collect();
    let find = |candidates: &[&str]| -> Option<usize> {
        candidates
            .iter()
            .find_map(|c| norm.iter().position(|h| h == c))
            .or_else(|| candidates.iter().find_map(|c| norm.iter().position(|h| h.contains(c))))
    };
    let date = find(&["date", "transaction date", "posting date", "posted date"])?;
    let description = find(&["description", "memo", "payee", "name", "details"])?;
    let name = |i: usize| headers[i].trim().to_string();
    let exact_amount = norm.iter().position(|h| h == "amount");
    let out = find(&["debit", "withdrawal", "money out", "paid out"]);
    let into = find(&["credit", "deposit", "money in", "paid in"]);
    let (amount, money_in, money_out) = match (exact_amount, into, out) {
        (Some(a), _, _) => (Some(name(a)), None, None),
        (None, Some(i), Some(o)) if i != o => (None, Some(name(i)), Some(name(o))),
        _ => (Some(name(norm.iter().position(|h| h.contains("amount"))?)), None, None),
    };
    Some(ColumnMapping { date: name(date), description: name(description), amount, money_in, money_out, date_order: DateOrder::Mdy })
}

/// A CSV file read into memory: its headers and its non-empty records with their line numbers.
pub struct CsvTable {
    pub headers: Vec<String>,
    records: Vec<(u64, csv::StringRecord)>,
}

impl CsvTable {
    pub fn read(text: &str) -> Result<Self, String> {
        let mut csv = csv::ReaderBuilder::new().has_headers(true).flexible(true).trim(csv::Trim::All).from_reader(text.as_bytes());
        let headers = csv.headers().map_err(|e| format!("BANK_CSV_READ: {e}"))?.iter().map(|h| h.trim_start_matches('\u{feff}').trim().to_string()).collect();
        let mut records = Vec::new();
        for record in csv.records() {
            let record = record.map_err(|e| format!("BANK_CSV_READ: {e}"))?;
            if !record.iter().all(str::is_empty) {
                records.push((record.position().map_or(0, |p| p.line()), record));
            }
        }
        Ok(CsvTable { headers, records })
    }

    fn column(&self, name: &str) -> Result<usize, String> {
        let wanted = normalize_header(name);
        self.headers
            .iter()
            .position(|h| normalize_header(h) == wanted)
            .ok_or_else(|| format!("BANK_CSV_COLUMNS: this file has no '{name}' column"))
    }

    /// Values of one column, for detecting the date order.
    pub fn values<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a str> + 'a {
        let col = self.column(name).ok();
        self.records.iter().filter_map(move |(_, r)| col.and_then(|c| r.get(c)))
    }

    pub fn rows(&self, mapping: &ColumnMapping) -> Result<Vec<BankRow>, String> {
        enum Amounts {
            Signed(usize),
            Split { money_in: usize, money_out: usize },
        }
        let date_col = self.column(&mapping.date)?;
        let desc_col = self.column(&mapping.description)?;
        let amounts = match (&mapping.amount, &mapping.money_in, &mapping.money_out) {
            (Some(a), None, None) => Amounts::Signed(self.column(a)?),
            (None, Some(i), Some(o)) if i != o => Amounts::Split { money_in: self.column(i)?, money_out: self.column(o)? },
            _ => return Err("BANK_CSV_MAPPING: choose one amount column, or two different money-in and money-out columns".into()),
        };

        let mut rows = Vec::with_capacity(self.records.len());
        for (line, record) in &self.records {
            let field = |i: usize| record.get(i).unwrap_or("");
            let err = |e: String| format!("BANK_CSV_ROW: line {line}: {e}");
            let date = parse_date_ordered(field(date_col), mapping.date_order).map_err(err)?;
            let amount_cents = match amounts {
                Amounts::Signed(i) => parse_amount_cents(field(i)).map_err(err)?,
                // Money in minus money out; the sign each side is written with does not matter.
                Amounts::Split { money_in, money_out } => {
                    let side = |i: usize| if field(i).is_empty() { Ok(0) } else { parse_amount_cents(field(i)).map(i64::abs) };
                    side(money_in).map_err(err)? - side(money_out).map_err(err)?
                }
            };
            rows.push(BankRow { row: *line, date, description: clean_description(field(desc_col)), amount_cents, fitid: None });
        }
        Ok(rows)
    }
}

pub(crate) fn clean_description(raw: &str) -> String {
    match raw.split_whitespace().collect::<Vec<_>>().join(" ") {
        d if d.is_empty() => "(no description)".to_string(),
        d => d,
    }
}

/// Parses a bank CSV whose columns are recognized automatically (see [`detect_mapping`]).
#[cfg(test)]
pub fn parse_bank_csv_reader<R: std::io::Read>(mut reader: R) -> Result<Vec<BankRow>, String> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(|e| format!("BANK_CSV_READ: {e}"))?;
    let table = CsvTable::read(&decode_text(&bytes))?;
    let mut mapping = detect_mapping(&table.headers)
        .ok_or_else(|| format!("BANK_CSV_COLUMNS: could not tell which columns hold the date, description and amount (headers: {})", table.headers.join(", ")))?;
    mapping.date_order = detect_date_order(table.values(&mapping.date)).0;
    table.rows(&mapping)
}

/// True for OFX/QFX/QBO statements (SGML OFX 1.x or XML OFX 2.x).
pub fn is_ofx(text: &str) -> bool {
    let head = text.get(..text.len().min(4096)).unwrap_or(text).to_ascii_uppercase();
    head.contains("OFXHEADER") || head.contains("<OFX>")
}

fn decode_entities(value: &str) -> String {
    if !value.contains('&') {
        return value.to_string();
    }
    value.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&#39;", "'").replace("&nbsp;", " ").replace("&amp;", "&")
}

/// The text after `<TAG>` up to the next tag or line break, in `block` (whose uppercase copy is
/// `upper`, same byte offsets). Works for SGML (no closing tags) and XML.
fn tag_value<'a>(block: &'a str, upper: &str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let start = upper.find(&open)? + open.len();
    let rest = &block[start..];
    let end = rest.find(['<', '\r', '\n']).unwrap_or(rest.len());
    Some(rest[..end].trim()).filter(|v| !v.is_empty())
}

/// Reads every `<STMTTRN>` transaction: `DTPOSTED`, `TRNAMT` (signed: negative is money out),
/// `NAME`/`MEMO` as the description, and `FITID`.
pub fn parse_ofx(text: &str) -> Result<Vec<BankRow>, String> {
    let upper = text.to_ascii_uppercase(); // ASCII-only change, so byte offsets match `text`
    let mut rows = Vec::new();
    let mut pos = 0;
    while let Some(found) = upper[pos..].find("<STMTTRN>") {
        let start = pos + found;
        let body = start + "<STMTTRN>".len();
        let end = ["</STMTTRN>", "<STMTTRN>", "</BANKTRANLIST>"]
            .iter()
            .filter_map(|t| upper[body..].find(t).map(|i| body + i))
            .min()
            .unwrap_or(text.len());
        let (block, block_upper) = (&text[body..end], &upper[body..end]);
        let line = text[..start].bytes().filter(|&b| b == b'\n').count() as u64 + 1;
        let err = |e: String| format!("BANK_OFX_ROW: transaction at line {line}: {e}");

        let date = parse_date(tag_value(block, block_upper, "DTPOSTED").ok_or_else(|| err("no DTPOSTED".into()))?).map_err(err)?;
        let amount_cents = parse_amount_cents(tag_value(block, block_upper, "TRNAMT").ok_or_else(|| err("no TRNAMT".into()))?).map_err(err)?;
        let name = tag_value(block, block_upper, "NAME").map(decode_entities);
        let memo = tag_value(block, block_upper, "MEMO").map(decode_entities);
        let description = match (name, memo) {
            (Some(n), Some(m)) if !n.to_uppercase().contains(&m.to_uppercase()) => format!("{n} {m}"),
            (Some(n), _) => n,
            (None, Some(m)) => m,
            (None, None) => String::new(),
        };
        let fitid = tag_value(block, block_upper, "FITID").map(decode_entities);
        rows.push(BankRow { row: line, date, description: clean_description(&description), amount_cents, fitid });
        pos = end;
    }
    if rows.is_empty() {
        return Err("BANK_OFX_EMPTY: this OFX file has no transactions".into());
    }
    Ok(rows)
}

/// What a bank file contains and how it will be read, for the import dialog.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankFileInspection {
    pub format: BankFileFormat,
    /// CSV headers (empty for OFX).
    pub headers: Vec<String>,
    /// The mapping that will be used: given, saved for these headers, or detected. `None` when the
    /// columns could not be recognized and the user must choose them.
    pub mapping: Option<ColumnMapping>,
    /// `GIVEN`, `SAVED` or `DETECTED`.
    pub mapping_source: Option<&'static str>,
    /// False when every date reads correctly both month-first and day-first, so the order is a guess.
    pub date_order_certain: bool,
    pub row_count: usize,
    pub preview: Vec<BankRow>,
    /// Why the file cannot be read with this mapping (shown in the dialog), if it cannot.
    pub problem: Option<String>,
}

/// Saved column layouts for CSV files, keyed by their headers.
async fn saved_mapping(pool: &SqlitePool, signature: &str) -> Result<Option<ColumnMapping>, String> {
    let row: Option<(String, String, Option<String>, Option<String>, Option<String>, String)> = sqlx::query_as(
        "SELECT date_column, description_column, amount_column, money_in_column, money_out_column, date_order
         FROM bank_format_templates WHERE header_signature = ?1",
    )
    .bind(signature)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    Ok(row.map(|(date, description, amount, money_in, money_out, order)| ColumnMapping {
        date,
        description,
        amount,
        money_in,
        money_out,
        date_order: if order == "DMY" { DateOrder::Dmy } else { DateOrder::Mdy },
    }))
}

async fn save_mapping(pool: &SqlitePool, signature: &str, m: &ColumnMapping) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO bank_format_templates
            (header_signature, date_column, description_column, amount_column, money_in_column, money_out_column, date_order)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT (header_signature) DO UPDATE SET
            date_column = excluded.date_column, description_column = excluded.description_column,
            amount_column = excluded.amount_column, money_in_column = excluded.money_in_column,
            money_out_column = excluded.money_out_column, date_order = excluded.date_order,
            last_used_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(signature)
    .bind(&m.date)
    .bind(&m.description)
    .bind(&m.amount)
    .bind(&m.money_in)
    .bind(&m.money_out)
    .bind(m.date_order.code())
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(db_err)
}

fn read_text(path: &Path) -> Result<String, String> {
    std::fs::read(path).map(|b| decode_text(&b)).map_err(|e| format!("BANK_FILE_OPEN: {}: {e}", path.display()))
}

/// Reads a bank file and reports how it would be imported, without importing anything.
pub async fn inspect_file(pool: &SqlitePool, path: &Path, given: Option<ColumnMapping>) -> Result<BankFileInspection, String> {
    let text = read_text(path)?;
    if is_ofx(&text) {
        let (rows, problem) = match parse_ofx(&text) {
            Ok(rows) => (rows, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        return Ok(BankFileInspection {
            format: BankFileFormat::Ofx,
            headers: Vec::new(),
            mapping: None,
            mapping_source: None,
            date_order_certain: true,
            row_count: rows.len(),
            preview: rows.into_iter().take(8).collect(),
            problem,
        });
    }

    let table = CsvTable::read(&text)?;
    let signature = header_signature(&table.headers);
    let (mapping, source, certain) = if let Some(m) = given {
        (Some(m), Some("GIVEN"), true)
    } else if let Some(m) = saved_mapping(pool, &signature).await? {
        (Some(m), Some("SAVED"), true)
    } else if let Some(mut m) = detect_mapping(&table.headers) {
        let (order, certain) = detect_date_order(table.values(&m.date));
        m.date_order = order;
        (Some(m), Some("DETECTED"), certain)
    } else {
        (None, None, false)
    };
    let (rows, problem) = match &mapping {
        None => (Vec::new(), Some("Choose which columns hold the date, description and amount.".to_string())),
        Some(m) => match table.rows(m) {
            Ok(rows) => (rows, None),
            Err(e) => (Vec::new(), Some(e)),
        },
    };
    Ok(BankFileInspection {
        format: BankFileFormat::Csv,
        headers: table.headers,
        mapping,
        mapping_source: source,
        date_order_certain: certain,
        row_count: rows.len(),
        preview: rows.into_iter().take(8).collect(),
        problem,
    })
}

// ------------------------------------------------------------------------------------------------
// Category suggestions
// ------------------------------------------------------------------------------------------------

const NOISE_WORDS: &[&str] = &[
    "POS", "DEBIT", "CREDIT", "PURCHASE", "CARD", "CHECKCARD", "ACH", "PAYMENT", "ONLINE", "RECURRING", "WEB",
    "PMT", "PPD", "CCD", "ID", "SQ", "TST", "VISA", "MC", "INC", "LLC", "CO", "THE", "WWW", "COM", "XX", "XXX", "XXXX",
];

/// Derives a reusable match pattern from a bank description, e.g. `SQ *BLUE BOTTLE #123 OAKLAND`
/// becomes a pattern matching `BLUE ... BOTTLE`. Returns `None` when nothing distinctive remains.
pub fn learned_pattern(description: &str) -> Option<String> {
    let upper = description.to_uppercase();
    let words: Vec<&str> = upper
        .split(|c: char| !(c.is_ascii_alphabetic() || c == '&' || c == '\''))
        .filter(|w| w.len() >= 2 && !NOISE_WORDS.contains(w))
        .take(2)
        .collect();
    if words.is_empty() {
        return None;
    }
    let body = words.iter().map(|w| regex::escape(w)).collect::<Vec<_>>().join("[^A-Za-z&']+");
    Some(format!(r"\b{body}\b"))
}

struct Rule {
    id: i64,
    regex: Regex,
    direction: String,
    account_id: i64,
    origin: String,
}

pub struct RuleSet {
    rules: Vec<Rule>,
    uncategorized_expense: Option<i64>,
    uncategorized_income: Option<i64>,
}

impl RuleSet {
    pub async fn load(pool: &SqlitePool) -> Result<Self, String> {
        let rows: Vec<(i64, String, String, i64, String)> = sqlx::query_as(
            "SELECT r.id, r.pattern, r.direction, r.account_id, r.origin
             FROM bank_rules r JOIN chart_of_accounts a ON a.id = r.account_id
             WHERE a.is_active = 1
             ORDER BY r.priority DESC, r.hit_count DESC, r.id",
        )
        .fetch_all(pool)
        .await
        .map_err(db_err)?;

        let rules = rows
            .into_iter()
            .map(|(id, pattern, direction, account_id, origin)| {
                RegexBuilder::new(&pattern)
                    .case_insensitive(true)
                    .build()
                    .map(|regex| Rule { id, regex, direction, account_id, origin })
                    .map_err(|e| format!("BANK_RULE_INVALID: rule {id} pattern is not a valid regex: {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let fallback = |number: &'static str| {
            sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1 AND is_active = 1")
                .bind(number)
                .fetch_optional(pool)
        };
        Ok(Self {
            rules,
            uncategorized_expense: fallback("6900").await.map_err(db_err)?,
            uncategorized_income: fallback("4900").await.map_err(db_err)?,
        })
    }

    /// The first rule matching the line, if any.
    pub fn matching(&self, description: &str, amount_cents: i64) -> Option<CategorySuggestion> {
        let direction = if amount_cents < 0 { "OUTFLOW" } else { "INFLOW" };
        self.rules
            .iter()
            .find(|r| (r.direction == "ANY" || r.direction == direction) && r.regex.is_match(description))
            .map(|r| CategorySuggestion::plain(r.account_id, Some(r.id), &r.origin))
    }

    /// Uncategorized Expense or Income, for a line nothing else suggests a category for.
    pub fn fallback(&self, amount_cents: i64) -> Option<CategorySuggestion> {
        let fallback = if amount_cents < 0 { self.uncategorized_expense } else { self.uncategorized_income };
        fallback.map(|account_id| CategorySuggestion::plain(account_id, None, "DEFAULT"))
    }

    /// Rules only: the first matching rule, else the uncategorized fallback.
    #[cfg(test)]
    pub fn suggest(&self, description: &str, amount_cents: i64) -> Option<CategorySuggestion> {
        self.matching(description, amount_cents).or_else(|| self.fallback(amount_cents))
    }
}

/// What a bank line is categorized as: an explicit rule first; otherwise the model's suggestion from
/// similar past transactions; otherwise uncategorized. The model's payee is offered either way.
pub fn combine(rules: &RuleSet, amount_cents: i64, ruled: Option<CategorySuggestion>, predicted: Option<suggest::Prediction>) -> (Option<CategorySuggestion>, Option<String>) {
    let payee = predicted.as_ref().and_then(|p| p.payee.clone()).filter(|_| amount_cents < 0);
    let suggestion = ruled
        .or_else(|| {
            predicted.map(|p| CategorySuggestion {
                account_id: p.account_id,
                rule_id: None,
                origin: "MODEL".into(),
                confidence: Some(p.confidence),
                confidence_level: Some(suggest::confidence_level(p.confidence)),
                similar_to: Some(p.similar_to),
            })
        })
        .or_else(|| rules.fallback(amount_cents));
    (suggestion, payee)
}

// ------------------------------------------------------------------------------------------------
// Persistence
// ------------------------------------------------------------------------------------------------

fn normalize_description(description: &str) -> String {
    description.split_whitespace().collect::<Vec<_>>().join(" ").to_uppercase()
}

/// Imports a bank file (OFX/QFX, or CSV read with `given`, the mapping saved for its headers, or the
/// detected one) into the review queue of `bank_account_id`. A mapping the user chose is saved for the
/// next file with the same headers.
///
/// Re-importing is safe: OFX transactions are identified by the bank's FITID, and CSV lines by date,
/// amount, description and their occurrence count within the file, so two identical same-day
/// purchases both import while an overlapping or repeated statement adds nothing twice.
pub async fn import_file(pool: &SqlitePool, path: &Path, bank_account_id: i64, given: Option<ColumnMapping>) -> Result<ImportSummary, String> {
    let text = read_text(path)?;
    let rows = if is_ofx(&text) {
        parse_ofx(&text)?
    } else {
        let table = CsvTable::read(&text)?;
        let signature = header_signature(&table.headers);
        let mapping = match &given {
            Some(m) => m.clone(),
            None => match saved_mapping(pool, &signature).await? {
                Some(m) => m,
                None => {
                    let mut m = detect_mapping(&table.headers).ok_or_else(|| {
                        format!("BANK_CSV_COLUMNS: could not tell which columns hold the date, description and amount (headers: {})", table.headers.join(", "))
                    })?;
                    m.date_order = detect_date_order(table.values(&m.date)).0;
                    m
                }
            },
        };
        let rows = table.rows(&mapping)?;
        if given.is_some() {
            save_mapping(pool, &signature, &mapping).await?;
        }
        rows
    };

    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    match write_import(&mut tx, &rows, bank_account_id).await {
        Ok(summary) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            Ok(summary)
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

async fn write_import(
    tx: &mut Transaction<'_, Sqlite>,
    rows: &[BankRow],
    bank_account_id: i64,
) -> Result<ImportSummary, String> {
    let mut summary = ImportSummary::default();
    let mut occurrences: HashMap<String, u32> = HashMap::new();

    for row in rows {
        if row.amount_cents == 0 {
            summary.skipped_zero += 1;
            continue;
        }
        let fingerprint = match &row.fitid {
            Some(fitid) => format!("FITID|{fitid}"),
            None => {
                let key = format!("{}|{}|{}", row.date, row.amount_cents, normalize_description(&row.description));
                let n = occurrences.entry(key.clone()).or_insert(0);
                *n += 1;
                format!("{key}|{n}")
            }
        };

        if insert_line(tx, bank_account_id, row.date, &row.description, row.amount_cents, &fingerprint).await? {
            summary.imported += 1;
        } else {
            summary.duplicates += 1;
        }
    }
    Ok(summary)
}

/// Adds one line to the review queue of `bank_account_id`, unless a line with the same fingerprint
/// is already there (in any status). Returns whether it was added.
pub(crate) async fn insert_line(
    tx: &mut Transaction<'_, Sqlite>,
    bank_account_id: i64,
    date: NaiveDate,
    description: &str,
    amount_cents: i64,
    fingerprint: &str,
) -> Result<bool, String> {
    let result = sqlx::query(
        "INSERT INTO bank_transactions (bank_account_id, txn_date, description, amount_cents, fingerprint)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (bank_account_id, fingerprint) DO NOTHING",
    )
    .bind(bank_account_id)
    .bind(date)
    .bind(description)
    .bind(amount_cents)
    .bind(fingerprint)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(result.rows_affected() == 1)
}

/// Pending lines with their suggestions. `file` names the open company file, so the cached model is
/// never used for another one.
pub async fn list_pending(pool: &SqlitePool, suggester: &Suggester, file: &str) -> Result<Vec<BankTransactionView>, String> {
    let rows: Vec<(i64, i64, NaiveDate, String, i64)> = sqlx::query_as(
        "SELECT id, bank_account_id, txn_date, description, amount_cents
         FROM bank_transactions WHERE status = 'PENDING'
         ORDER BY txn_date DESC, id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let rules = RuleSet::load(pool).await?;
    let ruled: Vec<Option<CategorySuggestion>> = rows.iter().map(|r| rules.matching(&r.3, r.4)).collect();
    let queries: Vec<Query> =
        rows.iter().map(|r| Query { bank_account_id: r.1, date: r.2, description: &r.3, amount_cents: r.4 }).collect();
    let only: Vec<Option<i64>> = ruled.iter().map(|s| s.as_ref().map(|s| s.account_id)).collect();
    let predicted = suggester.with_model(pool, file, |m| m.predict_batch(&queries, &only)).await?;
    drop(queries);
    Ok(rows
        .into_iter()
        .zip(ruled.into_iter().zip(predicted))
        .map(|((id, bank_account_id, date, description, amount_cents), (ruled, predicted))| {
            let (suggestion, payee) = combine(&rules, amount_cents, ruled, predicted);
            BankTransactionView { suggestion, payee, id, bank_account_id, date: date.to_string(), description, amount_cents }
        })
        .collect())
}

/// What the user chose for a pending line.
#[derive(Debug, Clone, Default)]
pub struct Approval {
    pub category_account_id: i64,
    pub class_id: Option<i64>,
    /// The vendor on the expense (money out only); a new name adds a vendor.
    pub payee_name: Option<String>,
}

/// Posts a pending bank line to the ledger against the chosen category (and optional class and
/// payee), learns a matching rule, and adds the line to the suggestion model.
pub async fn approve(pool: &SqlitePool, suggester: &Suggester, file: &str, transaction_id: i64, approval: Approval) -> Result<i64, String> {
    let before = Suggester::signature(pool).await?;
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    match write_approval(&mut tx, transaction_id, &approval).await {
        Ok((entry_id, example)) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            suggester.learn(pool, file, &before, example).await?;
            Ok(entry_id)
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

async fn write_approval(tx: &mut Transaction<'_, Sqlite>, transaction_id: i64, approval: &Approval) -> Result<(i64, Example), String> {
    let Approval { category_account_id, class_id, .. } = *approval;
    let payee_name = approval.payee_name.as_deref().map(str::trim).filter(|p| !p.is_empty());
    let row: Option<(i64, NaiveDate, String, i64, String)> = sqlx::query_as(
        "SELECT bank_account_id, txn_date, description, amount_cents, status FROM bank_transactions WHERE id = ?1",
    )
    .bind(transaction_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let (bank_account_id, date, description, amount_cents, status) =
        row.ok_or_else(|| format!("BANK_TXN_NOT_FOUND: bank line {transaction_id} does not exist"))?;
    if status != "PENDING" {
        return Err(format!("BANK_TXN_NOT_PENDING: bank line {transaction_id} is already {status}"));
    }
    if category_account_id == bank_account_id {
        return Err("BANK_SAME_ACCOUNT: choose a category other than the bank account itself".into());
    }

    // Approval creates a real document, as in classic desktop accounting: money out becomes an Expense
    // (Dr category / Cr bank), money in becomes a Deposit (Dr bank / Cr category).
    let amount = amount_cents.abs();
    let (entry_id, direction) = if amount_cents < 0 {
        let expense = ExpenseInput {
            kind: "EXPENSE".into(),
            payee_name: payee_name.map(str::to_string),
            payment_account_id: bank_account_id,
            expense_date: date,
            check_number: None,
            payment_method: None,
            memo: Some(description.clone()),
            lines: vec![SplitLineInput { account_id: category_account_id, amount_cents: amount, memo: Some(description.clone()), class_id }],
            currency: None,
        };
        (write_expense(tx, &expense).await?.1, "OUTFLOW")
    } else {
        let deposit = DepositInput {
            bank_account_id,
            deposit_date: date,
            memo: Some(description.clone()),
            items: Vec::new(),
            other_lines: vec![DepositOtherLine { account_id: category_account_id, amount_cents: amount, memo: Some(description.clone()), class_id }],
        };
        (write_deposit(tx, &deposit).await?.1, "INFLOW")
    };

    sqlx::query(
        "UPDATE bank_transactions
         SET status = 'APPROVED', category_account_id = ?1, journal_entry_id = ?2, class_id = ?4,
             reviewed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?3",
    )
    .bind(category_account_id)
    .bind(entry_id)
    .bind(transaction_id)
    .bind(class_id)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;

    if let Some(pattern) = learned_pattern(&description) {
        sqlx::query(
            "INSERT INTO bank_rules (pattern, direction, account_id, origin, priority, hit_count)
             VALUES (?1, ?2, ?3, 'LEARNED', 1000, 1)
             ON CONFLICT (pattern, direction) DO UPDATE SET
                 account_id = excluded.account_id,
                 origin     = 'LEARNED',
                 priority   = MAX(priority, 1000),
                 hit_count  = hit_count + 1,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .bind(pattern)
        .bind(direction)
        .bind(category_account_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }

    let payee = if amount_cents < 0 { payee_name.map(str::to_string) } else { None };
    Ok((entry_id, Example { bank_account_id, date, description, amount_cents, account_id: category_account_id, payee }))
}

/// Removes a pending line from the review queue without posting it (e.g. a transfer already recorded).
pub async fn exclude(pool: &SqlitePool, transaction_id: i64) -> Result<(), String> {
    let result = sqlx::query(
        "UPDATE bank_transactions SET status = 'EXCLUDED', reviewed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1 AND status = 'PENDING'",
    )
    .bind(transaction_id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if result.rows_affected() != 1 {
        return Err(format!("BANK_TXN_NOT_PENDING: bank line {transaction_id} is not pending"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
