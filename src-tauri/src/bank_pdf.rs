//! Bank and card statements in PDF: pulls the text out with its positions and finds the
//! transaction table in it.
//!
//! A statement PDF has no columns, only words placed on a page. This module rebuilds the table the
//! way a reader sees it: words are grouped into lines by their baseline, the line of column titles
//! (`Date`, `Description`, `Withdrawals`, `Balance`, ...) gives each column its horizontal span, and
//! every word below it goes to the column it sits under. A line that starts with a date begins a
//! transaction; a line of text right below it continues its description. The opening and closing
//! balances are read from lines such as `Beginning balance` and `New balance` anywhere in the file.
//!
//! The result is a plain table of text cells with the column titles as headers, which the bank
//! feed then reads with the same column mapping as a CSV file (see `bank::CsvTable`). Amounts stay
//! text here; they are parsed to cents there.
//!
//! Scanned statements are images with no text; they are refused with a clear message, since
//! LocalBooks does not do OCR.

#![allow(clippy::float_arithmetic, reason = "text positions on a statement page; amounts are parsed to cents as text")]

use chrono::{Datelike, NaiveDate};
use pdf_extract::{MediaBox, OutputDev, OutputError, Transform};

use crate::bank::{parse_amount_cents, parse_date_ordered, DateOrder};

/// The transaction table read from a statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfTable {
    /// Column titles, unique, in page order.
    pub headers: Vec<String>,
    /// One record per transaction: its 1-based number and one cell per header.
    pub records: Vec<(u64, Vec<String>)>,
    /// The column holding the running balance, if the table has one.
    pub balance_column: Option<String>,
    /// Balances printed on the statement (`Beginning balance`, `New balance`), as printed.
    pub opening_cents: Option<i64>,
    pub closing_cents: Option<i64>,
}

/// Fewer visible characters than this means there is no text layer worth reading.
const MIN_TEXT_CHARS: usize = 20;

/// Reads the transaction table of a statement PDF.
pub fn read_table(bytes: &[u8]) -> Result<PdfTable, String> {
    let words = extract_words(bytes)?;
    if words.iter().map(|w| w.text.chars().count()).sum::<usize>() < MIN_TEXT_CHARS {
        return Err("BANK_PDF_NO_TEXT: this PDF has no text; OCR isn't supported. It is probably a scan or a photo: \
                    download the statement from your bank's website as a PDF, CSV or OFX file instead."
            .into());
    }
    table_from_lines(&group_lines(words))
}

// ------------------------------------------------------------------------------------------------
// Text extraction
// ------------------------------------------------------------------------------------------------

/// A run of characters on one baseline with no gap, in points from the page's top-left corner.
#[derive(Debug, Clone, PartialEq)]
struct Word {
    page: u32,
    x0: f64,
    x1: f64,
    y: f64,
    size: f64,
    text: String,
}

/// Collects characters with their positions and joins them into words.
struct WordCollector {
    page: u32,
    page_height: f64,
    words: Vec<Word>,
    current: Option<Word>,
}

impl WordCollector {
    fn flush(&mut self) {
        if let Some(w) = self.current.take() {
            if !w.text.trim().is_empty() {
                self.words.push(w);
            }
        }
    }
}

impl OutputDev for WordCollector {
    fn begin_page(&mut self, page_num: u32, media_box: &MediaBox, _: Option<(f64, f64, f64, f64)>) -> Result<(), OutputError> {
        self.flush();
        self.page = page_num;
        self.page_height = media_box.ury - media_box.lly;
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        self.flush();
        Ok(())
    }

    fn output_character(&mut self, trm: &Transform, width: f64, _spacing: f64, font_size: f64, ch: &str) -> Result<(), OutputError> {
        // The text matrix maps glyph space to the page, whose origin is the bottom-left corner.
        let (x, y) = (trm.m31, self.page_height - trm.m32);
        let size = (font_size * (trm.m11 + trm.m21) * font_size * (trm.m12 + trm.m22)).abs().sqrt().max(1.0);
        let x1 = x + width * size;
        if ch.chars().all(char::is_whitespace) {
            self.flush();
            return Ok(());
        }
        // The same word continues when the next character starts where the last one ended, on the
        // same baseline. A gap of a sixth of the font size reads as a space even when the PDF draws
        // none (many statements position each word on its own).
        let continues = self.current.as_ref().is_some_and(|w| (w.y - y).abs() < size * 0.3 && x >= w.x0 && x - w.x1 < size * 0.17);
        match &mut self.current {
            Some(w) if continues => {
                w.text.push_str(ch);
                w.x1 = x1.max(w.x1);
            }
            _ => {
                self.flush();
                self.current = Some(Word { page: self.page, x0: x, x1, y, size, text: ch.to_string() });
            }
        }
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
}

fn extract_words(bytes: &[u8]) -> Result<Vec<Word>, String> {
    let mut doc = pdf_extract::Document::load_mem(bytes).map_err(|e| format!("BANK_PDF_READ: this PDF cannot be read: {e}"))?;
    if doc.is_encrypted() && doc.decrypt("").is_err() {
        return Err("BANK_PDF_ENCRYPTED: this PDF is password-protected. Open it, save an unprotected copy, and import that.".into());
    }
    let mut out = WordCollector { page: 0, page_height: 0.0, words: Vec::new(), current: None };
    // The text extractor has panicked on malformed fonts in the past; a bad file must not take the
    // app down with it.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pdf_extract::output_doc(&doc, &mut out)));
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return Err(format!("BANK_PDF_READ: the text of this PDF cannot be read: {e}")),
        Err(_) => return Err("BANK_PDF_READ: the text of this PDF cannot be read (its fonts are damaged or unsupported)".into()),
    }
    out.flush();
    Ok(out.words)
}

// ------------------------------------------------------------------------------------------------
// Lines
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Line {
    page: u32,
    y: f64,
    size: f64,
    /// Left to right.
    words: Vec<Word>,
}

impl Line {
    fn text(&self) -> String {
        self.words.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" ")
    }
}

/// Groups words into lines: same page, baselines within half a font size of each other.
fn group_lines(mut words: Vec<Word>) -> Vec<Line> {
    words.sort_by(|a, b| a.page.cmp(&b.page).then(a.y.total_cmp(&b.y)).then(a.x0.total_cmp(&b.x0)));
    let mut lines: Vec<Line> = Vec::new();
    for w in words {
        match lines.last_mut() {
            Some(line) if line.page == w.page && (w.y - line.y).abs() <= line.size.max(w.size) * 0.5 => {
                line.size = line.size.max(w.size);
                line.words.push(w);
            }
            _ => lines.push(Line { page: w.page, y: w.y, size: w.size, words: vec![w] }),
        }
    }
    for line in &mut lines {
        line.words.sort_by(|a, b| a.x0.total_cmp(&b.x0));
    }
    lines
}

/// Words closer than this (in font sizes) belong to one phrase, such as the title `Posting Date`.
const PHRASE_GAP: f64 = 0.8;

/// A line's words joined into phrases: (x0, x1, text).
fn phrases(line: &Line) -> Vec<(f64, f64, String)> {
    let mut out: Vec<(f64, f64, String)> = Vec::new();
    for w in &line.words {
        match out.last_mut() {
            Some(p) if w.x0 - p.1 < line.size * PHRASE_GAP => {
                p.1 = w.x1;
                p.2.push(' ');
                p.2.push_str(&w.text);
            }
            _ => out.push((w.x0, w.x1, w.text.clone())),
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------
// Columns
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Date,
    Text,
    /// Amounts: one signed column, money in or out, or the running balance.
    Money,
    Balance,
}

#[derive(Debug, Clone)]
struct Column {
    title: String,
    kind: Kind,
    x0: f64,
    x1: f64,
}

const MONEY_STEMS: &[&str] =
    &["amount", "debit", "credit", "withdrawal", "deposit", "payment", "charge", "purchase", "money", "paid", "fee", "interest", "subtraction", "addition"];
const MONEY_WORDS: &[&str] = &["in", "out", "dr", "cr"];

fn title_kind(title: &str) -> Option<Kind> {
    let t = title.to_lowercase();
    let words: Vec<&str> = t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let has = |stem: &str| words.iter().any(|w| w.starts_with(stem));
    if has("balance") {
        Some(Kind::Balance)
    } else if has("date") {
        Some(Kind::Date)
    } else if MONEY_STEMS.iter().any(|m| has(m)) || MONEY_WORDS.iter().any(|m| words.contains(m)) {
        Some(Kind::Money)
    } else if t.len() <= 40 {
        Some(Kind::Text)
    } else {
        None
    }
}

/// The columns of a line of column titles, or `None` if it is not one: it needs a date column, a
/// money column and at least three titles, all short.
fn header_columns(line: &Line) -> Option<Vec<Column>> {
    let phrases = phrases(line);
    if phrases.len() < 3 || phrases.len() > 10 {
        return None;
    }
    let mut columns = Vec::with_capacity(phrases.len());
    for (x0, x1, text) in phrases {
        // A title is words, not numbers: a line with an amount or date in it is data.
        if text.chars().any(|c| c.is_ascii_digit()) {
            return None;
        }
        columns.push(Column { kind: title_kind(&text)?, title: text, x0, x1 });
    }
    let has = |k: Kind| columns.iter().any(|c| c.kind == k);
    (has(Kind::Date) && (has(Kind::Money) || has(Kind::Balance))).then_some(columns)
}

/// Titles made unique (`Date`, `Date (2)`) so the column mapping can name each one.
fn unique_titles(columns: &[Column]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for c in columns {
        let mut title = c.title.clone();
        let mut n = 2;
        while seen.iter().any(|s| s.eq_ignore_ascii_case(&title)) {
            title = format!("{} ({n})", c.title);
            n += 1;
        }
        seen.push(title);
    }
    seen
}

/// `1,234.56`, `$1,234.56`, `-12.00`, `(12.00)`, `12.00-`: a number with cents.
fn is_amount(text: &str) -> bool {
    let t = text.trim_start_matches(['-', '+', '(', '$']).trim_start_matches('$').trim_end_matches([')', '-']);
    let Some((whole, cents)) = t.split_once('.') else { return false };
    cents.len() == 2
        && cents.bytes().all(|b| b.is_ascii_digit())
        && !whole.is_empty()
        && whole.bytes().all(|b| b.is_ascii_digit() || b == b',')
        && whole.bytes().next().is_some_and(|b| b.is_ascii_digit())
        && parse_amount_cents(text).is_ok()
}

/// `12.00CR` → (`12.00`, CR); anything else is returned as it is.
fn split_marker(text: &str) -> (String, Option<Marker>) {
    let upper = text.to_ascii_uppercase();
    match upper.strip_suffix("CR").or_else(|| upper.strip_suffix("DR")) {
        Some(head) if is_amount(head) => (text[..head.len()].to_string(), is_marker(&upper[head.len()..])),
        _ => (text.to_string(), None),
    }
}

fn is_marker(text: &str) -> Option<Marker> {
    match text.to_ascii_uppercase().as_str() {
        "CR" => Some(Marker::Cr),
        "DR" => Some(Marker::Dr),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marker {
    Cr,
    Dr,
}

/// One line's text split into the table's columns. Amounts carry their `CR`/`DR` marker, if any.
struct Cells {
    text: Vec<String>,
    markers: Vec<Option<Marker>>,
}

impl Cells {
    fn has_money(&self, columns: &[Column]) -> bool {
        columns.iter().zip(&self.text).any(|(c, t)| c.kind == Kind::Money && !t.is_empty())
    }
}

fn split_cells(line: &Line, columns: &[Column]) -> Cells {
    let n = columns.len();
    let mut text = vec![String::new(); n];
    let mut markers = vec![None; n];
    let first_money_x0 = columns.iter().filter(|c| matches!(c.kind, Kind::Money | Kind::Balance)).map(|c| c.x0).fold(f64::INFINITY, f64::min);
    let text_columns: Vec<usize> = (0..n).filter(|&i| matches!(columns[i].kind, Kind::Date | Kind::Text)).collect();
    let mut last_amount: Option<(usize, f64)> = None;

    let mut words = line.words.iter().peekable();
    while let Some(w) = words.next() {
        let mut token = w.text.clone();
        let mut right = w.x1;
        // `$ 12.00` printed as two words.
        if token == "$" {
            if let Some(next) = words.peek().filter(|n| is_amount(&n.text)) {
                token = next.text.clone();
                right = next.x1;
                words.next();
            }
        }
        if let (Some(marker), Some((col, end))) = (is_marker(&token), last_amount) {
            if w.x0 - end < line.size * 1.5 {
                markers[col] = Some(marker);
                last_amount = None;
                continue;
            }
        }
        let (token, marker) = split_marker(&token);

        let col = if is_amount(&token) && right >= first_money_x0 - line.size {
            // Numbers are usually right-aligned under their title: pick the money column it overlaps
            // most, else the one whose right edge is nearest.
            let overlap = |c: &Column| (right.min(c.x1) - w.x0.max(c.x0)).max(0.0);
            let candidates = (0..n).filter(|&i| matches!(columns[i].kind, Kind::Money | Kind::Balance));
            let best = candidates.clone().max_by(|&a, &b| overlap(&columns[a]).total_cmp(&overlap(&columns[b])));
            match best.filter(|&i| overlap(&columns[i]) > 0.0) {
                Some(i) => Some(i),
                None => candidates.min_by(|&a, &b| (columns[a].x1 - right).abs().total_cmp(&(columns[b].x1 - right).abs())),
            }
        } else {
            // Text is left-aligned: it belongs to the last text column starting at or before it.
            text_columns.iter().rev().copied().find(|&i| columns[i].x0 <= w.x0 + line.size * 0.5).or(text_columns.first().copied())
        };
        let Some(col) = col else { continue };
        if !text[col].is_empty() {
            text[col].push(' ');
        }
        text[col].push_str(&token);
        if matches!(columns[col].kind, Kind::Money | Kind::Balance) && is_amount(&token) {
            markers[col] = marker;
            last_amount = Some((col, right));
        } else {
            last_amount = None;
        }
    }
    Cells { text, markers }
}

// ------------------------------------------------------------------------------------------------
// Dates and balances
// ------------------------------------------------------------------------------------------------

fn parses_as_date(text: &str) -> bool {
    parse_date_ordered(text, DateOrder::Mdy).is_ok() || parse_date_ordered(text, DateOrder::Dmy).is_ok()
}

/// `01/15`, `15/01`, `Jan 15`, `15 Jan`: a date printed without its year.
fn yearless_month(text: &str) -> Option<u32> {
    let t = text.trim();
    let with_year = |suffix: &str| format!("{t}{suffix}");
    if t.contains('/') || t.contains('-') || t.contains('.') {
        let sep = if t.contains('/') { '/' } else if t.contains('-') { '-' } else { '.' };
        let parts: Vec<&str> = t.split(sep).collect();
        if parts.len() != 2 || !parts.iter().all(|p| (1..=2).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit())) {
            return None;
        }
        let candidate = with_year(&format!("{sep}2000"));
        let (a, b): (u32, u32) = (parts[0].parse().ok()?, parts[1].parse().ok()?);
        return parses_as_date(&candidate).then(|| if a <= 12 { a } else { b });
    }
    let date = parse_date_ordered(&with_year(" 2000"), DateOrder::Mdy).or_else(|_| parse_date_ordered(&with_year(", 2000"), DateOrder::Mdy)).ok()?;
    t.chars().any(|c| c.is_ascii_alphabetic()).then(|| date.month())
}

/// A date cell with its year: as printed if it has one, else completed from the statement period.
/// A period that runs from December into January puts the later months in the earlier year.
fn complete_date(text: &str, period: Option<(NaiveDate, NaiveDate)>) -> Option<String> {
    let t = text.trim();
    if parses_as_date(t) {
        return Some(t.to_string());
    }
    let month = yearless_month(t)?;
    let (start, end) = period?;
    let year = if start.year() < end.year() && month > end.month() { start.year() } else { end.year() };
    let sep = ['/', '-', '.'].into_iter().find(|c| t.contains(*c));
    Some(match sep {
        Some(sep) => format!("{t}{sep}{year}"),
        None if t.starts_with(|c: char| c.is_ascii_digit()) => format!("{t} {year}"),
        None => format!("{t}, {year}"),
    })
}

/// Whether a date cell looks like a date, with or without its year.
fn looks_like_date(text: &str) -> bool {
    let t = text.trim();
    !t.is_empty() && (parses_as_date(t) || yearless_month(t).is_some())
}

/// The statement period, from the full dates printed on it (in titles like `Statement period
/// January 1, 2026 to January 31, 2026`), to give yearless transaction dates their year. It ends at
/// the latest date and starts at the earliest one within a few months of it, so a date such as
/// `Member since 01/05/1998` does not stretch it.
fn statement_period(lines: &[Line]) -> Option<(NaiveDate, NaiveDate)> {
    let mut found: Vec<NaiveDate> = Vec::new();
    for line in lines {
        let words: Vec<&str> = line.words.iter().map(|w| w.text.as_str()).collect();
        for i in 0..words.len() {
            for len in 1..=3.min(words.len() - i) {
                let candidate = words[i..i + len].join(" ");
                let has_year = candidate.split(|c: char| !c.is_ascii_digit()).any(|d| d.len() == 4);
                if has_year {
                    if let Ok(d) = parse_date_ordered(candidate.trim_end_matches([',', '.', ':']), DateOrder::Mdy) {
                        found.push(d);
                    }
                }
            }
        }
    }
    let end = *found.iter().max()?;
    let start = found.into_iter().filter(|d| (end - *d).num_days() <= 100).min()?;
    Some((start, end))
}

const OPENING: &[&str] = &[
    "opening balance", "beginning balance", "previous balance", "starting balance", "balance forward", "balance brought forward",
    "brought forward", "balance last statement", "start balance",
];
const CLOSING: &[&str] = &["closing balance", "ending balance", "new balance", "balance this statement", "end balance", "statement balance"];

fn mentions(line_text: &str, phrases: &[&str]) -> bool {
    let t = line_text.to_lowercase();
    phrases.iter().any(|p| t.contains(p))
}

/// The amount printed on a balance line: its last amount, with a `CR`/`DR` marker or a minus sign.
fn line_amount(line: &Line) -> Option<i64> {
    let words = &line.words;
    let i = words.iter().rposition(|w| is_amount(&split_marker(&w.text).0))?;
    let (amount, marker) = split_marker(&words[i].text);
    let marker = marker.or_else(|| words.get(i + 1).and_then(|w| is_marker(&w.text)));
    let cents = parse_amount_cents(&amount).ok()?;
    // A balance marked CR on a card statement is money the bank owes you; on a bank statement a DR
    // balance is an overdraft. Either way the marked side is the unusual one.
    Some(if marker.is_some() { -cents } else { cents })
}

fn is_total(line_text: &str) -> bool {
    let t = line_text.trim_start().to_lowercase();
    t.starts_with("total") || t.starts_with("subtotal") || t.starts_with("sub-total")
}

// ------------------------------------------------------------------------------------------------
// The table
// ------------------------------------------------------------------------------------------------

/// A continuation line must sit within this many line heights of the line above it.
const CONTINUATION_GAP: f64 = 2.2;

struct Building {
    cells: Vec<String>,
    markers: Vec<Option<Marker>>,
    page: u32,
    bottom: f64,
}

fn table_from_lines(lines: &[Line]) -> Result<PdfTable, String> {
    let period = statement_period(lines);
    let opening_cents = lines.iter().find(|l| mentions(&l.text(), OPENING)).and_then(line_amount);
    let closing_cents = lines.iter().rev().find(|l| mentions(&l.text(), CLOSING)).and_then(line_amount);

    // The first header fixes the table's columns; later pages repeat it (same titles), and their
    // own header positions are used for their lines.
    let mut columns: Option<Vec<Column>> = None;
    let mut titles: Vec<String> = Vec::new();
    let mut page_columns: Option<(u32, Vec<Column>)> = None;
    let mut rows: Vec<Building> = Vec::new();

    for line in lines {
        if let Some(found) = header_columns(line) {
            match &columns {
                None => {
                    titles = unique_titles(&found);
                    columns = Some(found.clone());
                    page_columns = Some((line.page, found));
                }
                Some(first) if same_titles(first, &found) => page_columns = Some((line.page, found)),
                // A different table (a daily-balance summary, say): stop reading this page.
                Some(_) => page_columns = None,
            }
            continue;
        }
        let Some((_, cols)) = page_columns.as_ref() else { continue };
        // A page without its own header continues the previous page's table.
        let text = line.text();
        let cells = split_cells(line, cols);
        let date = cols.iter().position(|c| c.kind == Kind::Date).map(|i| cells.text[i].as_str()).unwrap_or("");
        let balance_line = mentions(&text, OPENING) || mentions(&text, CLOSING);
        if is_total(&text) || balance_line {
            continue;
        }
        if looks_like_date(date) {
            // Dated lines with no amount are daily balances or notes, not transactions.
            if cells.has_money(cols) {
                rows.push(Building { cells: cells.text, markers: cells.markers, page: line.page, bottom: line.y });
            }
            continue;
        }
        let Some(last) = rows.last_mut() else { continue };
        let close = last.page == line.page && line.y - last.bottom <= line.size * CONTINUATION_GAP;
        if cells.has_money(cols) && close {
            // Statements that print each date once: a line with an amount but no date is another
            // transaction on the same day.
            let date_col = cols.iter().position(|c| c.kind == Kind::Date);
            let mut text_cells = cells.text;
            if let Some(i) = date_col {
                text_cells[i] = last.cells[i].clone();
            }
            let page = last.page;
            rows.push(Building { cells: text_cells, markers: cells.markers, page, bottom: line.y });
        } else if close && !cells.has_money(cols) {
            // A wrapped description.
            let desc = description_column(cols);
            let extra: Vec<&str> = cells.text.iter().enumerate().filter(|(i, t)| !t.is_empty() && cols[*i].kind != Kind::Balance).map(|(_, t)| t.as_str()).collect();
            if let Some(d) = desc {
                let joined = extra.join(" ");
                if !joined.is_empty() {
                    if !last.cells[d].is_empty() {
                        last.cells[d].push(' ');
                    }
                    last.cells[d].push_str(&joined);
                }
                last.bottom = line.y;
            }
        }
    }

    let Some(columns) = columns else {
        return Err("BANK_PDF_NO_TABLE: LocalBooks found text in this PDF but no transaction table in it \
                    (a row of column titles such as Date, Description, Amount and Balance). \
                    If this is a bank or card statement, download it as CSV or OFX from your bank instead."
            .into());
    };
    if rows.is_empty() {
        return Err("BANK_PDF_NO_ROWS: this statement's transaction table has no transactions".into());
    }

    let date_col = columns.iter().position(|c| c.kind == Kind::Date);
    let missing_year = date_col.is_some_and(|i| rows.iter().any(|r| !parses_as_date(&r.cells[i])));
    if missing_year && period.is_none() {
        return Err("BANK_PDF_NO_YEAR: the dates in this statement have no year, and LocalBooks could not find the statement period".into());
    }

    let markers_used: Vec<(bool, bool)> = (0..columns.len())
        .map(|i| (rows.iter().any(|r| r.markers[i] == Some(Marker::Cr)), rows.iter().any(|r| r.markers[i] == Some(Marker::Dr))))
        .collect();
    let records = rows
        .into_iter()
        .enumerate()
        .map(|(n, mut r)| {
            for (i, c) in columns.iter().enumerate() {
                match c.kind {
                    Kind::Date => {
                        if let Some(full) = complete_date(&r.cells[i], period) {
                            r.cells[i] = full;
                        }
                    }
                    Kind::Money | Kind::Balance => {
                        // When a column marks both sides, DR is money out and CR money in. When it
                        // marks only one side, that side is the exception: card statements mark
                        // payments CR, some bank statements mark withdrawals DR.
                        let negate = match (r.markers[i], markers_used[i]) {
                            (Some(Marker::Dr), _) => true,
                            (Some(Marker::Cr), (_, false)) => true,
                            _ => false,
                        };
                        if negate && !r.cells[i].is_empty() {
                            r.cells[i] = format!("-{}", r.cells[i].trim_start_matches('+'));
                        }
                    }
                    Kind::Text => {}
                }
            }
            (n as u64 + 1, r.cells)
        })
        .collect();

    let balance_column = columns.iter().position(|c| c.kind == Kind::Balance).map(|i| titles[i].clone());
    Ok(PdfTable { headers: titles, records, balance_column, opening_cents, closing_cents })
}

fn same_titles(a: &[Column], b: &[Column]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.title.eq_ignore_ascii_case(&y.title))
}

/// The column a wrapped line's text belongs to: the one titled like a description, else the
/// widest-spaced text column after the date.
fn description_column(columns: &[Column]) -> Option<usize> {
    const NAMES: &[&str] = &["description", "details", "transaction", "particulars", "narrative", "memo", "payee", "merchant"];
    let lower: Vec<String> = columns.iter().map(|c| c.title.to_lowercase()).collect();
    NAMES
        .iter()
        .find_map(|n| lower.iter().position(|t| t.contains(n) && !t.contains("date")))
        .or_else(|| columns.iter().position(|c| c.kind == Kind::Text))
}

#[cfg(test)]
pub(crate) mod tests;
