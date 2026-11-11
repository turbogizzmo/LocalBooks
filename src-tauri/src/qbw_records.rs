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

use std::collections::{HashMap, HashSet};

use chrono::{Duration, NaiveDate, NaiveDateTime};

use crate::qbw_pages::LogicalPage;

/// Pages of these kinds hold out-of-row value chunks, not table rows.
pub const CHUNK_KINDS: std::ops::RangeInclusive<u8> = 0x10..=0x14;
/// Older rows lack at most this many trailing columns.
const MAX_DROPPED_COLUMNS: usize = 16;

/// The domain of a column the catalog is missing: its id falls between columns the catalog does
/// define, but the row defining it was not read (a damaged catalog page). Rows still store it.
pub const MISSING_DOMAIN: u16 = 0;

/// What a missing column most often is, tried in this order after "not stored at all": a nullable
/// 4-byte id (or date, stored the same way), a nullable 2-byte code, a 1-byte flag, and a 4-byte id
/// that is never null. Only fixed widths: a text or amount guess takes as many bytes as its length
/// byte says, so it could make a damaged row read through to its end with every value shifted.
const MISSING_COLUMN_GUESSES: [(u16, bool, u32); 4] = [(2, true, 4), (1, true, 2), (24, false, 1), (2, false, 4)];

/// Up to this many missing columns, each row is tried with every guess (at most 125 readings).
const MAX_GUESSED_COLUMNS: u32 = 3;

/// Up to this many missing columns (a placeholder is put in for each), a table's rows are read by
/// working out the layout once, on the first row that needs it, and reusing it for the others (all
/// rows of a table share one layout). More gaps than this is a catalog too damaged to guess.
const MAX_LEARNED_COLUMNS: u32 = 6;

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
        self.scaled(1).filter(|&(_, exact)| exact).map(|(cents, _)| cents)
    }

    /// Value × 100^`pow100` as an integer, rounded half away from zero, and whether it was exact;
    /// `None` when it does not fit. Pure integer arithmetic, so no value passes through a float.
    fn scaled(self, pow100: i32) -> Option<(i64, bool)> {
        if self.coefficient == 0 {
            return Some((0, true));
        }
        // scaled = coefficient * 100^(exponent + pow100)
        let shift = self.exponent.checked_add(pow100)?;
        let (magnitude, exact) = if shift >= 0 {
            (self.coefficient.checked_mul(100u128.checked_pow(shift as u32)?)?, true)
        } else {
            let div = 100u128.checked_pow((-shift) as u32)?;
            let (whole, rest) = (self.coefficient / div, self.coefficient % div);
            (if rest * 2 >= div { whole + 1 } else { whole }, rest == 0)
        };
        let value = i64::try_from(magnitude).ok()?;
        Some((if self.negative { -value } else { value }, exact))
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

    /// A quantity scaled by 10,000 (`ledger::QUANTITY_SCALE`), rounded half away from zero.
    #[allow(clippy::float_arithmetic, reason = "some tables store quantities as doubles")]
    pub fn as_quantity_e4(&self) -> Option<i64> {
        match self {
            Value::Numeric(n) => n.scaled(2).map(|(q, _)| q),
            Value::Int(v) => v.checked_mul(10_000),
            Value::Float(f) => {
                let q = (f * 10_000.0).round();
                (q.is_finite() && q.abs() < 9e15).then_some(q as i64)
            }
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

    /// A percentage (10 for 10%) in parts per million, ignoring its sign, when it is exact and
    /// between 0.0001% and 100%.
    pub fn as_percent_ppm(&self) -> Option<i64> {
        match self {
            Value::Numeric(n) => n.scaled(2).filter(|&(_, exact)| exact).map(|(ppm, _)| ppm.abs()).filter(|ppm| (1..=1_000_000).contains(ppm)),
            _ => None,
        }
    }

    /// A double that is not a whole number of cents, as the file stores it (`as_cents` refuses
    /// these; the import lists them rather than drop them silently).
    #[allow(clippy::float_arithmetic, reason = "only tells whether a stored double is whole cents; no amount is computed from it")]
    pub fn fractional_amount(&self) -> Option<String> {
        match self {
            Value::Float(f) if f.is_finite() && ((f * 100.0) - (f * 100.0).round()).abs() >= 1e-6 => Some(f.to_string()),
            _ => None,
        }
    }

    #[allow(clippy::float_arithmetic, reason = "some tables store amounts as doubles; only whole cents are accepted")]
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

/// One catalog row: a table's name, or one column's definition (name, domain, nullable, width)
/// with the object id that orders competing definitions.
enum CatalogEntry {
    Table(u32, String),
    Column { table: u32, id: u32, def: (String, u16, bool, u32), object_id: u64 },
}

/// Reads a row of a catalog page (owner 1: tables, owner 2: columns); `None` when the row is
/// not a well-formed catalog row, as when its page decoded with a few wrong bytes.
fn catalog_entry(owner: u32, row: &[u8]) -> Option<CatalogEntry> {
    let row = strip_extra(row);
    if owner == 1 {
        let len = *row.get(54)? as usize;
        let name = row.get(55..55 + len).filter(|n| is_identifier(n))?;
        Some(CatalogEntry::Table(u32le(&row, 4), String::from_utf8_lossy(name).into_owned()))
    } else {
        if row.len() < 40 || row[14] != 1 || !matches!(row[15], b'Y' | b'N') {
            return None;
        }
        let len = row[38] as usize;
        let name = row.get(39..39 + len).filter(|n| is_identifier(n))?;
        let def = (String::from_utf8_lossy(name).into_owned(), u16le(&row, 12), row[15] == b'Y', u32le(&row, 16));
        let object_id = u64::from_le_bytes(row[22..30].try_into().expect("8 bytes"));
        Some(CatalogEntry::Column { table: u32le(&row, 4), id: u32le(&row, 8), def, object_id })
    }
}

/// Catalog pages are those of tables 1 (table names) and 2 (column definitions).
pub fn is_catalog_page(page: &LogicalPage) -> bool {
    matches!(page.owner, 1 | 2)
}

/// Rows of a catalog page that chain and read as catalog rows: the score a re-solved page
/// must improve on.
pub fn catalog_rows_read(page: &LogicalPage) -> usize {
    page.rows.iter().filter(|r| catalog_row_reads(page, r)).count()
}

/// The row chains and reads as a catalog row.
pub fn catalog_row_reads(page: &LogicalPage, span: &crate::qbw_pages::RowSpan) -> bool {
    span.chained && catalog_entry(page.owner, page.row(span)).is_some()
}

/// Reads the catalog from the decoded pages of tables 1 and 2. A column can be defined more than
/// once (upgrades append newer definitions); the most frequent definition wins, then the newest.
pub fn read_catalog(pages: &[LogicalPage]) -> Catalog {
    let mut names = HashMap::new();
    type FieldKey = (String, u16, bool, u32);
    let mut defs: HashMap<(u32, u32), HashMap<FieldKey, (u32, u64)>> = HashMap::new();
    for page in pages.iter().filter(|p| is_catalog_page(p)) {
        for span in page.rows.iter().filter(|r| r.chained) {
            match catalog_entry(page.owner, page.row(span)) {
                Some(CatalogEntry::Table(id, name)) => {
                    names.insert(id, name);
                }
                Some(CatalogEntry::Column { table, id, def, object_id }) => {
                    let entry = defs.entry((table, id)).or_default().entry(def).or_default();
                    entry.0 += 1;
                    entry.1 = entry.1.max(object_id);
                }
                None => {}
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
        fill_missing_columns(cols);
    }
    Catalog { names, columns }
}

/// Sorts a table's columns by id and puts a placeholder (`MISSING_DOMAIN`) where an id is
/// missing: ids are numbered from 1 without gaps, so a gap is a column whose definition was lost.
/// A table with more gaps than the walk would guess (a damaged definition can carry a wild id) is
/// left as it is.
fn fill_missing_columns(cols: &mut Vec<Column>) {
    cols.sort_by_key(|c| c.id);
    let defined: HashSet<u32> = cols.iter().map(|c| c.id).collect();
    let last = cols.last().map_or(0, |c| c.id);
    let gaps = (last as usize).saturating_sub(1).saturating_sub(defined.iter().filter(|id| (1..last).contains(*id)).count());
    if gaps == 0 || gaps > MAX_LEARNED_COLUMNS as usize {
        return;
    }
    for id in (1..last).filter(|id| !defined.contains(id)) {
        cols.push(Column { id, name: format!("column_{id}"), domain: MISSING_DOMAIN, nullable: false, width: 0 });
    }
    cols.sort_by_key(|c| c.id);
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
    // A column the catalog lost is taken as not stored, as the first reading of `walk_row` does.
    for col in cols.iter().filter(|c| c.domain != MISSING_DOMAIN) {
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
/// Columns the catalog is missing are first taken as not stored, then as each likely type in
/// turn; the first reading that ends exactly on the row's length wins. With more missing columns
/// than `MAX_GUESSED_COLUMNS` they are only taken as not stored (see `table_rows`).
pub fn walk_row(row: &[u8], cols: &[Column]) -> Option<HashMap<String, Value>> {
    let row = strip_extra(row);
    let missing = cols.iter().filter(|c| c.domain == MISSING_DOMAIN).count() as u32;
    let limit = if missing > MAX_GUESSED_COLUMNS { 0 } else { missing };
    guess_layout(&row, cols, limit).map(|(values, _)| values)
}

/// Walks with trailing columns dropped one at a time, as `walk_row` does.
fn walk_dropping(row: &[u8], cols: &[Column]) -> Option<HashMap<String, Value>> {
    (0..=MAX_DROPPED_COLUMNS.min(cols.len())).find_map(|drop| walk_columns(row, &cols[..cols.len() - drop]))
}

/// Tries the missing columns (the first `limit` of them guessed, the rest taken as not stored)
/// as not stored, then as each likely type, and returns the first reading that fits with the
/// column list that gave it. `row` has had its extra byte removed.
fn guess_layout(row: &[u8], cols: &[Column], limit: u32) -> Option<(HashMap<String, Value>, Vec<Column>)> {
    let missing: Vec<usize> = cols.iter().enumerate().filter(|(_, c)| c.domain == MISSING_DOMAIN).map(|(i, _)| i).collect();
    if missing.is_empty() {
        return walk_dropping(row, cols).map(|values| (values, cols.to_vec()));
    }
    let options = MISSING_COLUMN_GUESSES.len() + 1;
    let tries = options.pow(limit.min(missing.len() as u32));
    (0..tries).find_map(|n| {
        let mut guess = n;
        let mut chosen = Vec::with_capacity(cols.len());
        for (i, col) in cols.iter().enumerate() {
            let Some(k) = missing.iter().position(|m| *m == i) else {
                chosen.push(col.clone());
                continue;
            };
            if k as u32 >= limit {
                continue;
            }
            let choice = guess % options;
            guess /= options;
            if let Some(&(domain, nullable, width)) = choice.checked_sub(1).and_then(|c| MISSING_COLUMN_GUESSES.get(c)) {
                chosen.push(Column { domain, nullable, width, ..col.clone() });
            }
        }
        walk_dropping(row, &chosen).map(|values| (values, chosen))
    })
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
    // A table with more lost columns than each row can afford to guess learns its layout from the
    // first row that reads with one, and reads the rest with it.
    let missing = cols.iter().filter(|c| c.domain == MISSING_DOMAIN).count() as u32;
    let mut learned: Option<Vec<Column>> = None;
    // A row that fits no guess is damaged; after a few, stop searching (each search is costly).
    let mut searches_left = 3;
    for page in pages.iter().filter(|p| p.owner == table && !CHUNK_KINDS.contains(&p.kind)) {
        for span in &page.rows {
            if !span.chained {
                out.unchained += 1;
                continue;
            }
            let values = match &learned {
                Some(layout) => walk_dropping(&strip_extra(page.row(span)), layout).or_else(|| walk_row(page.row(span), cols)),
                None if missing > MAX_GUESSED_COLUMNS && searches_left > 0 => {
                    searches_left -= 1;
                    guess_layout(&strip_extra(page.row(span)), cols, missing).map(|(values, layout)| {
                        learned = Some(layout);
                        values
                    })
                }
                None => walk_row(page.row(span), cols),
            };
            match values {
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
