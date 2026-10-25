//! Helpers shared by the plain-text importers (IIF and QIF): decoding the file's bytes, and
//! building the source chart of accounts and name lists that [`crate::qbw_company::Ledger`] holds.

use std::collections::HashMap;

use crate::qbw_company::{SourceAccount, SourceParty};

/// Windows-1252 characters for bytes 0x80-0x9F (the rest of the high half matches Latin-1).
const CP1252_HIGH: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š',
    '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

/// Decodes an export file. QuickBooks and Quicken write Windows-1252 ("ANSI"); newer tools write
/// UTF-8, sometimes with a byte-order mark, and a few write UTF-16.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        return String::from_utf8_lossy(rest).into_owned();
    }
    for (bom, little) in [(b"\xFF\xFE", true), (b"\xFE\xFF", false)] {
        if let Some(rest) = bytes.strip_prefix(bom) {
            let units = rest.chunks_exact(2).map(|c| if little { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) });
            return char::decode_utf16(units).map(|c| c.unwrap_or('\u{fffd}')).collect();
        }
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&b| if (0x80..0xA0).contains(&b) { CP1252_HIGH[(b - 0x80) as usize] } else { b as char }).collect(),
    }
}

/// Parses a quantity such as `1.5`, `-2` or `0.0001` to an integer scaled by 10,000.
pub fn quantity_e4(raw: &str) -> Option<i64> {
    let s = raw.trim().replace(',', "");
    let (negative, s) = match s.strip_prefix('-') {
        Some(rest) => (true, rest.to_string()),
        None => (false, s),
    };
    let (whole, frac) = s.split_once('.').unwrap_or((&s, ""));
    if whole.is_empty() && frac.is_empty() || !whole.chars().chain(frac.chars()).all(|c| c.is_ascii_digit()) {
        return None;
    }
    if frac.len() > 4 && frac[4..].chars().any(|c| c != '0') {
        return None;
    }
    let whole: i64 = if whole.is_empty() { 0 } else { whole.parse().ok()? };
    let frac: i64 = format!("{:0<4}", &frac[..frac.len().min(4)]).parse().ok()?;
    let value = whole.checked_mul(10_000)?.checked_add(frac)?;
    Some(if negative { -value } else { value })
}

/// The chart of accounts as a file names it, with `Parent:Child` paths and their parents.
#[derive(Default)]
pub struct Accounts {
    pub list: Vec<SourceAccount>,
    by_name: HashMap<String, i64>,
}

impl Accounts {
    pub fn id(&self, full_name: &str) -> Option<i64> {
        self.by_name.get(&full_name.to_lowercase()).copied()
    }

    pub fn kind(&self, id: i64) -> &'static str {
        self.list.iter().find(|a| a.id == id).map_or("", |a| a.account_type)
    }

    /// The account with this `Parent:Child` path, created (with any missing parents, of the same
    /// type) when the file uses it without declaring it.
    pub fn ensure(&mut self, full_name: &str, account_type: &'static str) -> i64 {
        let full_name = full_name.trim().trim_matches(':');
        if let Some(id) = self.id(full_name) {
            return id;
        }
        let (parent_id, name) = match full_name.rsplit_once(':') {
            Some((parent, name)) => (Some(self.ensure(parent, account_type)), name.trim()),
            None => (None, full_name),
        };
        let id = self.list.len() as i64 + 1;
        self.list.push(SourceAccount {
            id,
            name: name.to_string(),
            full_name: full_name.to_string(),
            account_type,
            parent_id,
            active: true,
            stored_balance_cents: None,
        });
        self.by_name.insert(full_name.to_lowercase(), id);
        id
    }

    /// Declares an account; a later declaration of the same path updates its type.
    pub fn declare(&mut self, full_name: &str, account_type: &'static str, active: bool) -> i64 {
        let id = self.ensure(full_name, account_type);
        if let Some(a) = self.list.iter_mut().find(|a| a.id == id) {
            a.account_type = account_type;
            a.active = active;
        }
        id
    }
}

/// Customers and vendors as a file names them. Both share one id space, as in a company file.
#[derive(Default)]
pub struct Names {
    pub customers: Vec<SourceParty>,
    pub vendors: Vec<SourceParty>,
    next_id: i64,
}

pub fn blank_party(id: i64, name: &str) -> SourceParty {
    SourceParty {
        id,
        name: name.to_string(),
        parent_id: None,
        company_name: None,
        email: None,
        phone: None,
        address: None,
        terms_days: None,
        tax_id: None,
        is_1099: false,
        active: true,
        balance_cents: None,
        notes: None,
        sales_rep: None,
        custom_fields: Vec::new(),
    }
}

impl Names {
    pub fn customer(&self, name: &str) -> Option<i64> {
        self.customers.iter().find(|p| p.name.eq_ignore_ascii_case(name.trim())).map(|p| p.id)
    }

    pub fn vendor(&self, name: &str) -> Option<i64> {
        self.vendors.iter().find(|p| p.name.eq_ignore_ascii_case(name.trim())).map(|p| p.id)
    }

    /// The customer with this `Customer:Job` path, created (with its parent customer) if needed.
    pub fn ensure_customer(&mut self, name: &str) -> i64 {
        let name = name.trim().trim_matches(':');
        if let Some(id) = self.customer(name) {
            return id;
        }
        let parent_id = name.rsplit_once(':').map(|(parent, _)| self.ensure_customer(parent));
        self.next_id += 1;
        self.customers.push(SourceParty { parent_id, ..blank_party(self.next_id, name) });
        self.next_id
    }

    pub fn ensure_vendor(&mut self, name: &str) -> i64 {
        let name = name.trim();
        if let Some(id) = self.vendor(name) {
            return id;
        }
        self.next_id += 1;
        self.vendors.push(blank_party(self.next_id, name));
        self.next_id
    }

    pub fn sort(&mut self) {
        self.customers.sort_by_key(|p| p.name.to_lowercase());
        self.vendors.sort_by_key(|p| p.name.to_lowercase());
    }
}

/// Days in payment terms such as `Net 30` or `2% 10 Net 30` (the last number).
pub fn terms_days(terms: &str) -> Option<i64> {
    let lower = terms.to_lowercase();
    if lower.contains("receipt") {
        return Some(0);
    }
    lower.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).last()?.parse().ok()
}

#[cfg(test)]
mod tests;
