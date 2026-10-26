//! NACHA ACH files: a small writer for the credit files Pay Bills makes, and a validator that reads
//! any PPD/CCD file back and checks it the way a bank's front end would.
//!
//! The layout follows the NACHA Operating Rules as implemented by moov-io/ach (Apache-2.0), whose
//! test files are our test vectors (`nacha/testdata`). A file is 94-character records: one file
//! header (`1`), then per batch a batch header (`5`), its entries (`6`, each optionally followed by
//! one `05` addenda `7`) and a batch control (`8`), then the file control (`9`). The record count is
//! padded with all-`9` lines to a multiple of the blocking factor, 10.
//!
//! Only credits are written (paying vendors). The validator accepts debits too, so it can check
//! other programs' files and moov's vectors.

use chrono::{NaiveDate, NaiveDateTime};

pub const RECORD_LEN: usize = 94;
const BLOCKING_FACTOR: usize = 10;
/// The largest amount one entry can carry: ten digits of cents.
pub const MAX_ENTRY_CENTS: i64 = 9_999_999_999;
/// Totals are twelve digits.
const MAX_TOTAL_CENTS: i64 = 999_999_999_999;

/// Whether `routing` is a valid ABA routing transit number: nine digits whose weighted sum
/// (3, 7, 1, repeating) is a multiple of ten.
pub fn routing_number_ok(routing: &str) -> bool {
    routing.len() == 9 && routing.bytes().all(|b| b.is_ascii_digit()) && check_digit(&routing[..8]) == Some(routing.as_bytes()[8] - b'0')
}

/// The ABA check digit for the first eight digits of a routing number.
pub fn check_digit(first8: &str) -> Option<u8> {
    if first8.len() != 8 || !first8.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let sum: u32 = first8.bytes().zip([3, 7, 1, 3, 7, 1, 3, 7]).map(|(b, w)| u32::from(b - b'0') * w).sum();
    Some(((10 - sum % 10) % 10) as u8)
}

/// Standard Entry Class: CCD pays a business, PPD a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecCode {
    Ccd,
    Ppd,
}

impl SecCode {
    pub fn as_str(self) -> &'static str {
        match self {
            SecCode::Ccd => "CCD",
            SecCode::Ppd => "PPD",
        }
    }

    pub fn parse(text: &str) -> Option<SecCode> {
        match text {
            "CCD" => Some(SecCode::Ccd),
            "PPD" => Some(SecCode::Ppd),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountType {
    Checking,
    Savings,
}

impl AccountType {
    /// The transaction code for a credit (deposit) to this kind of account.
    pub fn credit_code(self) -> &'static str {
        match self {
            AccountType::Checking => "22",
            AccountType::Savings => "32",
        }
    }

    pub fn parse(text: &str) -> Option<AccountType> {
        match text {
            "CHECKING" => Some(AccountType::Checking),
            "SAVINGS" => Some(AccountType::Savings),
            _ => None,
        }
    }
}

/// Who sends the file and through which bank: the company's ACH originator details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Originator {
    /// The bank's routing number (nine digits), written with a leading space.
    pub immediate_destination: String,
    pub immediate_destination_name: String,
    /// Ten characters as the bank assigned it (often `1` and the EIN), or a nine-digit routing number.
    pub immediate_origin: String,
    pub immediate_origin_name: String,
    /// Up to 16 characters; shown on the vendor's bank statement.
    pub company_name: String,
    /// Ten characters as the bank assigned it (often `1` and the EIN).
    pub company_id: String,
    /// Up to 10 characters, such as `VENDOR PAY`; shown on the vendor's bank statement.
    pub entry_description: String,
}

impl Originator {
    /// The originating bank's identification: the first eight digits of its routing number.
    pub fn odfi(&self) -> &str {
        &self.immediate_destination[..8.min(self.immediate_destination.len())]
    }

    pub fn check(&self) -> Result<(), String> {
        if !routing_number_ok(&self.immediate_destination) {
            return Err("ACH_BAD_DESTINATION: the bank's routing number (immediate destination) is not a valid routing number".into());
        }
        let origin = self.immediate_origin.trim();
        if !(origin.len() == 10 || (origin.len() == 9 && origin.bytes().all(|b| b.is_ascii_digit()))) || !origin.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err("ACH_BAD_ORIGIN: the immediate origin is ten letters or digits (or a nine-digit routing number), as the bank gave it".into());
        }
        let id = self.company_id.trim();
        if id.is_empty() || id.len() > 10 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err("ACH_BAD_COMPANY_ID: the company ID is up to ten letters or digits, as the bank gave it".into());
        }
        if self.company_name.trim().is_empty() {
            return Err("ACH_NO_COMPANY_NAME: enter the company name to show on vendors' statements".into());
        }
        if self.entry_description.trim().is_empty() {
            return Err("ACH_NO_DESCRIPTION: enter an entry description, such as VENDOR PAY".into());
        }
        Ok(())
    }
}

/// One credit to a receiver's account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The receiver's routing number, nine digits.
    pub routing: String,
    pub account: String,
    pub account_type: AccountType,
    pub amount_cents: i64,
    /// Our reference for the receiver (up to 15 characters), such as the vendor number.
    pub individual_id: String,
    pub name: String,
    /// Fifteen digits: the originating bank's eight, then a sequence number.
    pub trace_number: String,
    /// Payment details for the receiver (up to 80 characters), sent as one `05` addenda.
    pub addenda: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Batch {
    pub sec_code: SecCode,
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub originator: Originator,
    pub created: NaiveDateTime,
    /// `A`–`Z` then `0`–`9`: tells apart files made on the same day.
    pub file_id_modifier: char,
    pub effective_date: NaiveDate,
    pub batches: Vec<Batch>,
}

/// Text for an alphanumeric field: upper case, characters banks accept, cut or padded to `len`.
fn alpha(text: &str, len: usize) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| c.to_ascii_uppercase())
        .map(|c| if c.is_ascii_alphanumeric() || " &'.,/-#()+*:;=?!$%@".contains(c) { c } else { ' ' })
        .collect();
    format!("{:<len$}", cleaned.trim(), len = len).chars().take(len).collect()
}

fn digits(n: i64, len: usize) -> String {
    format!("{n:0len$}", len = len)
}

/// The account number as written: letters, digits and dashes only.
pub fn clean_account(account: &str) -> String {
    account.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_uppercase()
}

pub fn account_ok(account: &str) -> bool {
    (4..=17).contains(&account.len()) && account.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') && account.bytes().any(|b| b.is_ascii_digit())
}

/// Writes `file` as NACHA text, each record ending in a newline. Checks what it can before writing:
/// routing numbers, account numbers, amounts, trace numbers in ascending order.
pub fn write(file: &File) -> Result<String, String> {
    let o = &file.originator;
    o.check()?;
    if !(file.file_id_modifier.is_ascii_uppercase() || file.file_id_modifier.is_ascii_digit()) {
        return Err("ACH_BAD_MODIFIER: the file ID modifier is A–Z or 0–9".into());
    }
    if file.batches.iter().all(|b| b.entries.is_empty()) {
        return Err("ACH_EMPTY: there is nothing to pay in this file".into());
    }
    let mut lines: Vec<String> = Vec::new();
    let origin = o.immediate_origin.trim();
    let origin = if origin.len() == 9 { format!(" {origin}") } else { origin.to_string() };
    lines.push(format!(
        "101 {}{}{}{}{}094101{}{}{}",
        o.immediate_destination,
        origin,
        file.created.format("%y%m%d"),
        file.created.format("%H%M"),
        file.file_id_modifier,
        alpha(&o.immediate_destination_name, 23),
        alpha(&o.immediate_origin_name, 23),
        alpha("", 8),
    ));
    let (mut file_count, mut file_hash, mut file_credit, mut batch_count) = (0i64, 0i64, 0i64, 0i64);
    let mut last_trace: Option<&str> = None;
    for batch in file.batches.iter().filter(|b| !b.entries.is_empty()) {
        batch_count += 1;
        lines.push(format!(
            "5220{}{}{}{}{}{}{}   1{}{}",
            alpha(&o.company_name, 16),
            alpha("", 20),
            alpha(&o.company_id, 10),
            batch.sec_code.as_str(),
            alpha(&o.entry_description, 10),
            alpha("", 6),
            file.effective_date.format("%y%m%d"),
            o.odfi(),
            digits(batch_count, 7),
        ));
        let (mut count, mut hash, mut credit) = (0i64, 0i64, 0i64);
        for e in &batch.entries {
            if !routing_number_ok(&e.routing) {
                return Err(format!("ACH_BAD_ROUTING: {} has a routing number that fails its check digit", e.name));
            }
            let account = clean_account(&e.account);
            if !account_ok(&account) {
                return Err(format!("ACH_BAD_ACCOUNT: {} has an account number that is not 4 to 17 letters, digits or dashes", e.name));
            }
            if !(1..=MAX_ENTRY_CENTS).contains(&e.amount_cents) {
                return Err(format!("ACH_BAD_AMOUNT: a payment to {} must be between $0.01 and $99,999,999.99", e.name));
            }
            if e.trace_number.len() != 15 || !e.trace_number.bytes().all(|b| b.is_ascii_digit()) || !e.trace_number.starts_with(o.odfi()) {
                return Err(format!("ACH_BAD_TRACE: trace number {} is not the bank's eight digits and a sequence number", e.trace_number));
            }
            if last_trace.is_some_and(|t| t >= e.trace_number.as_str()) {
                return Err("ACH_BAD_TRACE: trace numbers must ascend through the file".into());
            }
            last_trace = Some(&e.trace_number);
            let addenda = e.addenda.as_deref().map(str::trim).filter(|a| !a.is_empty());
            lines.push(format!(
                "6{}{}{:<17}{}{}{}  {}{}",
                e.account_type.credit_code(),
                e.routing,
                account,
                digits(e.amount_cents, 10),
                alpha(&e.individual_id, 15),
                alpha(&e.name, 22),
                if addenda.is_some() { '1' } else { '0' },
                e.trace_number,
            ));
            count += 1;
            hash += e.routing[..8].parse::<i64>().expect("checked digits");
            credit += e.amount_cents;
            if let Some(text) = addenda {
                lines.push(format!("705{}0001{}", alpha(text, 80), &e.trace_number[8..]));
                count += 1;
            }
        }
        if credit > MAX_TOTAL_CENTS {
            return Err("ACH_TOO_LARGE: a batch total cannot exceed $9,999,999,999.99".into());
        }
        hash %= 10_000_000_000;
        lines.push(format!(
            "8220{}{}{}{}{}{}{}{}{}",
            digits(count, 6),
            digits(hash, 10),
            digits(0, 12),
            digits(credit, 12),
            alpha(&o.company_id, 10),
            alpha("", 19),
            alpha("", 6),
            o.odfi(),
            digits(batch_count, 7),
        ));
        file_count += count;
        file_hash += hash;
        file_credit += credit;
    }
    if file_credit > MAX_TOTAL_CENTS {
        return Err("ACH_TOO_LARGE: a file total cannot exceed $9,999,999,999.99".into());
    }
    let blocks = (lines.len() + 1).div_ceil(BLOCKING_FACTOR);
    lines.push(format!(
        "9{}{}{}{}{}{}{}",
        digits(batch_count, 6),
        digits(blocks as i64, 6),
        digits(file_count, 8),
        digits(file_hash % 10_000_000_000, 10),
        digits(0, 12),
        digits(file_credit, 12),
        alpha("", 39),
    ));
    while lines.len() % BLOCKING_FACTOR != 0 {
        lines.push("9".repeat(RECORD_LEN));
    }
    debug_assert!(lines.iter().all(|l| l.len() == RECORD_LEN), "every record is 94 characters");
    let mut text = lines.join("\n");
    text.push('\n');
    Ok(text)
}

/// One entry as the validator read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntrySummary {
    pub sec_code: String,
    pub transaction_code: String,
    pub routing: String,
    pub amount_cents: i64,
    pub name: String,
    pub trace_number: String,
    pub addenda: Option<String>,
}

/// What a valid file holds, from its control records (which the validator checked against the entries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub batch_count: i64,
    pub block_count: i64,
    /// Entries and addenda.
    pub entry_addenda_count: i64,
    pub entry_hash: i64,
    pub total_debit_cents: i64,
    pub total_credit_cents: i64,
    pub entries: Vec<EntrySummary>,
}

fn num(line: &str, from: usize, to: usize, what: &str, n: usize) -> Result<i64, String> {
    let field = &line[from - 1..to];
    if !field.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("line {n}: {what} is not a number ({field:?})"));
    }
    Ok(field.parse().unwrap_or(0))
}

fn field(line: &str, from: usize, to: usize) -> &str {
    &line[from - 1..to]
}

const CREDIT_CODES: [&str; 6] = ["22", "23", "24", "32", "33", "34"];
const DEBIT_CODES: [&str; 6] = ["27", "28", "29", "37", "38", "39"];

/// Reads a PPD or CCD file and checks it: record lengths and order, field formats, routing check
/// digits, trace numbers, addenda, and every batch and file control total against the entries.
/// Returns every problem found, one per line.
pub fn validate(text: &str) -> Result<Summary, String> {
    let mut problems: Vec<String> = Vec::new();
    let lines: Vec<&str> = text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    let lines: &[&str] = match lines.split_last() {
        Some((last, rest)) if last.is_empty() => rest,
        _ => &lines,
    };
    for (i, line) in lines.iter().enumerate() {
        if !line.is_ascii() {
            problems.push(format!("line {}: has characters other than ASCII", i + 1));
        } else if line.len() != RECORD_LEN {
            problems.push(format!("line {}: is {} characters, not 94", i + 1, line.len()));
        }
    }
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    if lines.is_empty() {
        return Err("the file is empty".into());
    }
    if lines.len() % BLOCKING_FACTOR != 0 {
        problems.push(format!("the file has {} records, not a multiple of 10", lines.len()));
    }

    let mut summary = Summary { batch_count: 0, block_count: 0, entry_addenda_count: 0, entry_hash: 0, total_debit_cents: 0, total_credit_cents: 0, entries: vec![] };
    let mut pos = 0;
    let mut push = |p: Result<(), String>| {
        if let Err(e) = p {
            problems.push(e);
        }
    };

    // File header.
    let header = lines[0];
    if !header.starts_with('1') {
        return Err("line 1: the file does not start with a file header (record type 1)".into());
    }
    push((|| {
        if field(header, 2, 3) != "01" {
            return Err("line 1: priority code is not 01".into());
        }
        let dest = field(header, 4, 13);
        if !(dest.starts_with(' ') && routing_number_ok(&dest[1..])) {
            return Err(format!("line 1: immediate destination {dest:?} is not a space and a valid routing number"));
        }
        NaiveDate::parse_from_str(field(header, 24, 29), "%y%m%d").map_err(|_| "line 1: file creation date is not a date".to_string())?;
        let modifier = header.as_bytes()[33];
        if !(modifier.is_ascii_uppercase() || modifier.is_ascii_digit()) {
            return Err("line 1: file ID modifier is not A–Z or 0–9".into());
        }
        if field(header, 35, 37) != "094" || field(header, 38, 39) != "10" || field(header, 40, 40) != "1" {
            return Err("line 1: record size, blocking factor or format code is not 094, 10, 1".into());
        }
        Ok(())
    })());
    pos += 1;

    let mut traces: Vec<String> = Vec::new();
    while pos < lines.len() && lines[pos].starts_with('5') {
        let bh = lines[pos];
        let n = pos + 1;
        summary.batch_count += 1;
        let class = field(bh, 2, 4).to_string();
        let sec = field(bh, 51, 53).to_string();
        let company_id = field(bh, 41, 50).to_string();
        let odfi = field(bh, 80, 87).to_string();
        let batch_number = field(bh, 88, 94).to_string();
        if !["200", "220", "225"].contains(&class.as_str()) {
            push(Err(format!("line {n}: service class code {class} is not 200, 220 or 225")));
        }
        if !["PPD", "CCD"].contains(&sec.as_str()) {
            push(Err(format!("line {n}: SEC code {sec} is not PPD or CCD")));
        }
        if NaiveDate::parse_from_str(field(bh, 70, 75), "%y%m%d").is_err() {
            push(Err(format!("line {n}: effective entry date is not a date")));
        }
        if !odfi.bytes().all(|b| b.is_ascii_digit()) {
            push(Err(format!("line {n}: originating bank identification is not eight digits")));
        }
        if num(bh, 88, 94, "batch number", n).ok() != Some(summary.batch_count) {
            push(Err(format!("line {n}: batch number {batch_number} is out of sequence")));
        }
        pos += 1;

        let (mut count, mut hash, mut debit, mut credit) = (0i64, 0i64, 0i64, 0i64);
        while pos < lines.len() && lines[pos].starts_with('6') {
            let ed = lines[pos];
            let n = pos + 1;
            count += 1;
            let code = field(ed, 2, 3).to_string();
            let routing = field(ed, 4, 12).to_string();
            let trace = field(ed, 80, 94).to_string();
            let amount = num(ed, 30, 39, "amount", n);
            if let Err(e) = &amount {
                push(Err(e.clone()));
            }
            let amount = amount.unwrap_or(0);
            if CREDIT_CODES.contains(&code.as_str()) {
                credit += amount;
                if class == "225" {
                    push(Err(format!("line {n}: a credit in a debits-only batch (225)")));
                }
            } else if DEBIT_CODES.contains(&code.as_str()) {
                debit += amount;
                if class == "220" {
                    push(Err(format!("line {n}: a debit in a credits-only batch (220)")));
                }
            } else {
                push(Err(format!("line {n}: transaction code {code} is not a checking or savings credit or debit")));
            }
            if routing_number_ok(&routing) {
                hash += routing[..8].parse::<i64>().unwrap_or(0);
            } else {
                push(Err(format!("line {n}: routing number {routing} fails its check digit")));
            }
            if field(ed, 13, 29).trim().is_empty() {
                push(Err(format!("line {n}: no account number")));
            }
            if !trace.bytes().all(|b| b.is_ascii_digit()) {
                push(Err(format!("line {n}: trace number is not 15 digits")));
            } else if !trace.starts_with(&odfi) {
                push(Err(format!("line {n}: trace number does not start with the originating bank's {odfi}")));
            }
            if traces.last().is_some_and(|t| *t >= trace) {
                push(Err(format!("line {n}: trace number {trace} is not above the one before it")));
            }
            traces.push(trace.clone());
            let indicator = field(ed, 79, 79).to_string();
            pos += 1;
            let mut addenda = None;
            if pos < lines.len() && lines[pos].starts_with('7') {
                let ad = lines[pos];
                let n = pos + 1;
                count += 1;
                if field(ad, 2, 3) != "05" {
                    push(Err(format!("line {n}: addenda type {} is not 05", field(ad, 2, 3))));
                }
                if field(ad, 84, 87) != "0001" {
                    push(Err(format!("line {n}: addenda sequence number is not 0001")));
                }
                if field(ad, 88, 94) != &trace[8..] {
                    push(Err(format!("line {n}: addenda does not name its entry's sequence number")));
                }
                if indicator != "1" {
                    push(Err(format!("line {n}: an addenda after an entry whose addenda indicator is not 1")));
                }
                addenda = Some(field(ad, 4, 83).trim_end().to_string());
                pos += 1;
                if pos < lines.len() && lines[pos].starts_with('7') {
                    push(Err(format!("line {}: PPD and CCD entries have at most one addenda", pos + 1)));
                }
            } else if indicator != "0" {
                push(Err(format!("line {n}: addenda indicator is {indicator}, but no addenda follows")));
            }
            summary.entries.push(EntrySummary {
                sec_code: sec.clone(),
                transaction_code: code,
                routing,
                amount_cents: amount,
                name: field(ed, 55, 76).trim_end().to_string(),
                trace_number: trace,
                addenda,
            });
        }
        if count == 0 {
            push(Err(format!("line {n}: a batch with no entries")));
        }

        let Some(bc) = lines.get(pos).filter(|l| l.starts_with('8')) else {
            push(Err(format!("line {}: batch {} has no batch control (record type 8)", pos + 1, summary.batch_count)));
            break;
        };
        let n = pos + 1;
        hash %= 10_000_000_000;
        if field(bc, 2, 4) != class {
            push(Err(format!("line {n}: batch control service class does not match its header")));
        }
        if num(bc, 5, 10, "entry/addenda count", n).ok() != Some(count) {
            push(Err(format!("line {n}: batch control counts {} entries and addenda; the batch has {count}", field(bc, 5, 10))));
        }
        if num(bc, 11, 20, "entry hash", n).ok() != Some(hash) {
            push(Err(format!("line {n}: batch control entry hash {} is not {hash:010}", field(bc, 11, 20))));
        }
        if num(bc, 21, 32, "total debit", n).ok() != Some(debit) {
            push(Err(format!("line {n}: batch control debit total does not match the entries")));
        }
        if num(bc, 33, 44, "total credit", n).ok() != Some(credit) {
            push(Err(format!("line {n}: batch control credit total does not match the entries")));
        }
        if field(bc, 45, 54) != company_id {
            push(Err(format!("line {n}: batch control company ID does not match its header")));
        }
        if field(bc, 80, 87) != odfi || field(bc, 88, 94) != batch_number {
            push(Err(format!("line {n}: batch control bank or batch number does not match its header")));
        }
        summary.entry_addenda_count += count;
        summary.entry_hash += hash;
        summary.total_debit_cents += debit;
        summary.total_credit_cents += credit;
        pos += 1;
    }
    summary.entry_hash %= 10_000_000_000;

    let Some(fc) = lines.get(pos).filter(|l| l.starts_with('9') && !l.bytes().all(|b| b == b'9')) else {
        problems.push(format!("line {}: expected a batch header or the file control (record type 9)", pos + 1));
        return Err(problems.join("\n"));
    };
    let n = pos + 1;
    let records = pos + 1;
    summary.block_count = records.div_ceil(BLOCKING_FACTOR) as i64;
    for (from, to, what, want) in [
        (2, 7, "batch count", summary.batch_count),
        (8, 13, "block count", summary.block_count),
        (14, 21, "entry/addenda count", summary.entry_addenda_count),
        (22, 31, "entry hash", summary.entry_hash),
        (32, 43, "total debit", summary.total_debit_cents),
        (44, 55, "total credit", summary.total_credit_cents),
    ] {
        match num(fc, from, to, what, n) {
            Ok(got) if got == want => {}
            Ok(got) => problems.push(format!("line {n}: file control {what} is {got}; the file has {want}")),
            Err(e) => problems.push(e),
        }
    }
    for (i, line) in lines.iter().enumerate().skip(pos + 1) {
        if !line.bytes().all(|b| b == b'9') {
            problems.push(format!("line {}: after the file control, only filler lines of 9s may follow", i + 1));
            break;
        }
    }
    if problems.is_empty() { Ok(summary) } else { Err(problems.join("\n")) }
}

#[cfg(test)]
mod tests;
