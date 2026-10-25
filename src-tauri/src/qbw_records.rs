//! Record layer for decoded company-file pages: the catalog, typed values and the row walk.
//!
//! Layout as described in OpenQBW issue #22 and checked on a real file:
//!
//! * Pages owned by table 1 hold `ISYSTAB` rows (table id at +4, name length at +54, name at +55)
//!   and pages owned by table 2 hold `ISYSTABCOL` rows (table id +4, column id +8, domain +12,
//!   nullable flag `01 'Y'|'N'` at +14, width +16, scale +20, object id +22, name at +38).
//!   Offsets are taken after removing the optional extra byte that follows the flags byte when
//!   `flags & 0x40`.
//! * A table row is `[u16 length][u8 flags][extra?][null bitmap][values]`: the bitmap has one bit
//!   per nullable column, most significant first, set when the value is stored; values follow
//!   in column-id order with no padding. A walk is accepted only when it ends exactly on the
//!   row's length. Rows written before trailing columns were added are shorter; those columns
//!   are dropped one at a time until the walk fits.

use std::collections::HashMap;

use chrono::{Duration, NaiveDate, NaiveDateTime};

use crate::qbw_pages::LogicalPage;

/// Pages of these kinds hold out-of-row value chunks, not table rows.
pub const CHUNK_KINDS: std::ops::RangeInclusive<u8> = 0x10..=0x14;
/// Older rows lack at most this many trailing columns.
const MAX_DROPPED_COLUMNS: usize = 16;

#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub id: u32,
    pub name: String,
    pub domain: u16,
    pub nullable: bool,
    pub width: u32,
}

#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub names: HashMap<u32, String>,
    pub columns: HashMap<u32, Vec<Column>>,
}

impl Catalog {
    pub fn table_id(&self, name: &str) -> Option<u32> {
        self.names.iter().find(|(_, n)| n.as_str() == name).map(|(id, _)| *id)
    }
}

/// A NUMERIC value: `coefficient * 100^exponent`, negated when `negative`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Numeric {
    pub coefficient: u128,
    pub exponent: i32,
    pub negative: bool,
}

impl Numeric {
    /// Exact value in cents, or `None` when it has finer precision or does not fit.
    pub fn to_cents(self) -> Option<i64> {
        if self.coefficient == 0 {
            return Some(0);
        }
        // cents = coefficient * 100^(exponent + 1)
        let shift = self.exponent + 1;
        let magnitude = if shift >= 0 {
            self.coefficient.checked_mul(100u128.checked_pow(shift as u32)?)?
        } else {
            let div = 100u128.checked_pow((-shift) as u32)?;
            if self.coefficient % div != 0 {
                return None;
            }
            self.coefficient / div
        };
        let cents = i64::try_from(magnitude).ok()?;
        Some(if self.negative { -cents } else { cents })
    }

}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Float(f64),
    Date(NaiveDate),
    Timestamp(NaiveDateTime),
    Numeric(Numeric),
    Bytes(Vec<u8>),
    /// A long value stored out of row: the inline prefix and where its chunks start.
    Long { prefix: Vec<u8>, total_len: u32, page: u32, slot: u16 },
}

impl Value {
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<String> {
        match self {
            Value::Bytes(b) => Some(b.iter().map(|&c| c as char).collect()),
            Value::Long { prefix, .. } => Some(prefix.iter().map(|&c| c as char).collect()),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Numeric(n) => Some(n.coefficient as f64 * 100f64.powi(n.exponent) * if n.negative { -1.0 } else { 1.0 }),
            Value::Float(f) => Some(*f),
            Value::Int(v) => Some(*v as f64),
            _ => None,
        }
    }

    pub fn as_date(&self) -> Option<NaiveDate> {
        match self {
            Value::Date(d) => Some(*d),
            Value::Timestamp(t) => Some(t.date()),
            _ => None,
        }
    }

    pub fn as_cents(&self) -> Option<i64> {
        match self {
            Value::Numeric(n) => n.to_cents(),
            Value::Int(v) => v.checked_mul(100),
            // Some tables store amounts as doubles; accept them when they are whole cents.
            Value::Float(f) => {
                let cents = (f * 100.0).round();
                ((f * 100.0 - cents).abs() < 1e-6 && cents.abs() < 9e15).then_some(cents as i64)
            }
            _ => None,
        }
    }

}

/// One decoded table row, by column name.
#[derive(Debug, Clone)]
pub struct Row {
    pub values: HashMap<String, Value>,
}

impl Row {
    pub fn get(&self, column: &str) -> &Value {
        self.values.get(column).unwrap_or(&Value::Null)
    }
}

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Removes the extra byte that follows the flags byte when `flags & 0x40`.
fn strip_extra(row: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if row.len() > 3 && row[2] & 0x40 != 0 {
        std::borrow::Cow::Owned([&row[..3], &row[4..]].concat())
    } else {
        std::borrow::Cow::Borrowed(row)
    }
}

fn is_identifier(b: &[u8]) -> bool {
    !b.is_empty() && b.iter().all(|&c| (33..127).contains(&c))
}

/// Day 0 of DATE values and minute 0 of TIMESTAMP values.
fn epoch() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(1600, 2, 29).expect("valid epoch").and_hms_opt(0, 0, 0).expect("valid time")
}

/// Reads the catalog from the decoded pages of tables 1 and 2. A column can be defined more than
/// once (upgrades append newer definitions); the most frequent definition wins, then the newest.
pub fn read_catalog(pages: &[LogicalPage]) -> Catalog {
    let mut names = HashMap::new();
    let mut defs: HashMap<(u32, u32), HashMap<(String, u16, bool, u32), (u32, u64)>> = HashMap::new();
    for page in pages.iter().filter(|p| p.owner == 1 || p.owner == 2) {
        for span in page.rows.iter().filter(|r| r.chained) {
            let row = strip_extra(page.row(span));
            if page.owner == 1 {
                if row.len() < 56 {
                    continue;
                }
                let len = row[54] as usize;
                if let Some(name) = row.get(55..55 + len).filter(|n| is_identifier(n)) {
                    names.insert(u32le(&row, 4), String::from_utf8_lossy(name).into_owned());
                }
            } else {
                if row.len() < 40 || row[14] != 1 || !matches!(row[15], b'Y' | b'N') {
                    continue;
                }
                let len = row[38] as usize;
                let Some(name) = row.get(39..39 + len).filter(|n| is_identifier(n)) else { continue };
                let key = (String::from_utf8_lossy(name).into_owned(), u16le(&row, 12), row[15] == b'Y', u32le(&row, 16));
                let object_id = u64::from_le_bytes(row[22..30].try_into().expect("8 bytes"));
                let entry = defs.entry((u32le(&row, 4), u32le(&row, 8))).or_default().entry(key).or_default();
                entry.0 += 1;
                entry.1 = entry.1.max(object_id);
            }
        }
    }
    let mut columns: HashMap<u32, Vec<Column>> = HashMap::new();
    for ((table, id), candidates) in defs {
        let ((name, domain, nullable, width), _) =
            candidates.into_iter().max_by_key(|(_, (count, object_id))| (*count, *object_id)).expect("non-empty");
        columns.entry(table).or_default().push(Column { id, name, domain, nullable, width });
    }
    for cols in columns.values_mut() {
        cols.sort_by_key(|c| c.id);
    }
    Catalog { names, columns }
}

/// Reads one value at `o`; returns it and the offset after it, or `None` when the bytes cannot
/// be this type (which makes a misaligned walk fail early).
fn read_value(r: &[u8], o: usize, col: &Column) -> Option<(Value, usize)> {
    let fixed = |n: usize| r.get(o..o + n);
    Some(match col.domain {
        1 => (Value::Int(i16::from_le_bytes(fixed(2)?.try_into().ok()?) as i64), o + 2),
        22 => (Value::Int(u16le(fixed(2)?, 0) as i64), o + 2),
        2 => (Value::Int(i32::from_le_bytes(fixed(4)?.try_into().ok()?) as i64), o + 4),
        19 | 21 => (Value::Int(u32le(fixed(4)?, 0) as i64), o + 4),
        20 => (Value::Int(i64::from_le_bytes(fixed(8)?.try_into().ok()?)), o + 8),
        23 => (Value::Int(u64::from_le_bytes(fixed(8)?.try_into().ok()?) as i64), o + 8),
        4 => (Value::Float(f32::from_le_bytes(fixed(4)?.try_into().ok()?) as f64), o + 4),
        5 => (Value::Float(f64::from_le_bytes(fixed(8)?.try_into().ok()?)), o + 8),
        24 => (Value::Int(*r.get(o)? as i64), o + 1),
        6 => {
            let minutes = u32le(fixed(4)?, 0) as i64;
            (Value::Date((epoch() + Duration::minutes(minutes)).date()), o + 4)
        }
        13 | 14 => {
            let b = fixed(8)?;
            let t = epoch() + Duration::minutes(u32le(b, 0) as i64) + Duration::microseconds(u32le(b, 4) as i64);
            (Value::Timestamp(t), o + 8)
        }
        3 | 27 => {
            let k = *r.get(o)? as usize;
            let ctrl = *r.get(o + 1)?;
            let digits = r.get(o + 2..o + 2 + k)?;
            let mut coefficient = 0u128;
            for &d in digits.iter().rev() {
                if d > 99 {
                    return None;
                }
                coefficient = coefficient.checked_mul(100)?.checked_add(d as u128)?;
            }
            let n = Numeric { coefficient, exponent: (ctrl & 0x7F) as i32 - 64, negative: ctrl & 0x80 != 0 };
            (Value::Numeric(n), o + 2 + k)
        }
        7..=12 | 28 => {
            let b = *r.get(o)?;
            if col.width <= 255 || b < 0x80 {
                let n = b as usize;
                (Value::Bytes(r.get(o + 1..o + 1 + n)?.to_vec()), o + 1 + n)
            } else if b == 0xFF {
                let n = u16::from_be_bytes([*r.get(o + 1)?, *r.get(o + 2)?]) as usize;
                (Value::Bytes(r.get(o + 3..o + 3 + n)?.to_vec()), o + 3 + n)
            } else {
                let k = (b & 0x7F) as usize;
                let prefix = r.get(o + 1..o + 1 + k)?.to_vec();
                let d = r.get(o + 1 + k..o + 1 + k + 13)?;
                let value = Value::Long { prefix, total_len: u32le(d, 1), page: u32le(d, 5), slot: u16le(d, 11) };
                (value, o + 1 + k + 13)
            }
        }
        _ => return None,
    })
}

/// Walks a row against a column list; `None` unless it ends exactly on the row length.
fn walk_columns(r: &[u8], cols: &[Column]) -> Option<HashMap<String, Value>> {
    let nullable = cols.iter().filter(|c| c.nullable).count();
    let bitmap_len = nullable.div_ceil(8);
    let bitmap = r.get(3..3 + bitmap_len)?;
    let mut o = 3 + bitmap_len;
    let mut bit = 0;
    let mut out = HashMap::with_capacity(cols.len());
    for col in cols {
        if col.nullable {
            let present = bitmap[bit / 8] >> (7 - bit % 8) & 1 == 1;
            bit += 1;
            if !present {
                out.insert(col.name.clone(), Value::Null);
                continue;
            }
        }
        let (value, next) = read_value(r, o, col)?;
        out.insert(col.name.clone(), value);
        o = next;
    }
    (o == r.len()).then_some(out)
}

/// The values a walk reads before it fails, with the offset where it stopped. A row damaged near
/// its end still yields its leading columns (ids, account, amount), which callers can use when
/// another check confirms them.
pub fn walk_prefix(row: &[u8], cols: &[Column]) -> (Vec<(String, Value)>, usize) {
    let r = strip_extra(row);
    let nullable = cols.iter().filter(|c| c.nullable).count();
    let bitmap_len = nullable.div_ceil(8);
    let Some(bitmap) = r.get(3..3 + bitmap_len) else { return (Vec::new(), 0) };
    let (mut o, mut bit, mut out) = (3 + bitmap_len, 0, Vec::new());
    for col in cols {
        if col.nullable {
            let present = bitmap[bit / 8] >> (7 - bit % 8) & 1 == 1;
            bit += 1;
            if !present {
                out.push((col.name.clone(), Value::Null));
                continue;
            }
        }
        match read_value(&r, o, col) {
            Some((v, next)) => {
                out.push((col.name.clone(), v));
                o = next;
            }
            None => break,
        }
    }
    (out, o)
}

/// Decodes a row, dropping trailing columns for rows written under an older, shorter schema.
pub fn walk_row(row: &[u8], cols: &[Column]) -> Option<HashMap<String, Value>> {
    let row = strip_extra(row);
    (0..=MAX_DROPPED_COLUMNS.min(cols.len())).find_map(|drop| walk_columns(&row, &cols[..cols.len() - drop]))
}

/// Every row of a table that decodes, with counts of rows that do not.
pub struct TableRows {
    pub rows: Vec<Row>,
    pub unchained: usize,
    pub unwalkable: usize,
    /// Leading columns of the rows that chain but do not walk to the end.
    pub partial: Vec<Row>,
}

pub fn table_rows(pages: &[LogicalPage], catalog: &Catalog, table: u32) -> TableRows {
    let mut out = TableRows { rows: Vec::new(), unchained: 0, unwalkable: 0, partial: Vec::new() };
    let Some(cols) = catalog.columns.get(&table) else { return out };
    for page in pages.iter().filter(|p| p.owner == table && !CHUNK_KINDS.contains(&p.kind)) {
        for span in &page.rows {
            if !span.chained {
                out.unchained += 1;
                continue;
            }
            match walk_row(page.row(span), cols) {
                Some(values) => out.rows.push(Row { values }),
                None => {
                    out.unwalkable += 1;
                    let (prefix, _) = walk_prefix(page.row(span), cols);
                    out.partial.push(Row { values: prefix.into_iter().collect() });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
