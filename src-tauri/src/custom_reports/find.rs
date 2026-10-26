//! Find: posted transactions picked by the custom report filters, one row per transaction, like
//! classic desktop accounting software's Edit › Find. A transaction matches when any of its lines
//! passes the line filters; the document number and the amount bounds apply to the transaction as
//! a whole (the amount is its total, as the desktop Find compares).

use std::collections::HashMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use super::{ledger_lines, with_descendants, Filters, Line, LineFilter, NameKind};

/// Rows returned when the query names no limit; the count and total still cover every match.
pub const DEFAULT_LIMIT: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FindQuery {
    /// First day; `None` for all dates.
    pub start_date: Option<NaiveDate>,
    /// Last day; `None` for today.
    pub end_date: Option<NaiveDate>,
    /// The report filters. `amount_min_cents` and `amount_max_cents` bound the transaction's total
    /// rather than a line.
    pub filters: Filters,
    /// Text the name, number or a line's memo contains, ignoring case (what the ⌘K box passes on).
    pub text: Option<String>,
    /// The document's number (invoice, check, bill, reference; a journal entry's number), ignoring
    /// case and a leading `#`.
    pub number: Option<String>,
    /// At most this many rows, newest first; `None` for [`DEFAULT_LIMIT`].
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundTransaction {
    pub journal_entry_id: i64,
    pub date: NaiveDate,
    /// As journal entries show it (`INVOICE`, `CHECK`, `CARD_CREDIT`, ...).
    pub source_type: String,
    /// `Void` for the entry that voids another.
    pub type_label: String,
    pub num: Option<String>,
    pub name: Option<String>,
    pub name_kind: Option<NameKind>,
    /// The first matching line's account and memo, so an account or memo filter shows why it matched.
    pub account: String,
    pub memo: Option<String>,
    pub amount_cents: i64,
    pub is_voided: bool,
    pub is_reversal: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindResult {
    pub start_date: Option<NaiveDate>,
    pub end_date: NaiveDate,
    pub rows: Vec<FoundTransaction>,
    /// Every match, though `rows` stops at the limit.
    pub match_count: usize,
    /// The total of every match.
    pub total_cents: i64,
}

fn normalize_number(text: &str) -> String {
    text.trim().trim_start_matches('#').trim().to_lowercase()
}

fn found(l: &Line) -> FoundTransaction {
    FoundTransaction {
        journal_entry_id: l.journal_entry_id,
        date: l.date,
        source_type: l.source_type.clone(),
        type_label: l.type_text().to_string(),
        num: l.num.clone(),
        name: l.name.clone(),
        name_kind: l.party().map(|p| p.kind),
        account: l.account_name.clone(),
        memo: l.memo.clone(),
        amount_cents: l.entry_total_cents,
        is_voided: l.is_voided,
        is_reversal: l.is_reversal,
    }
}

/// Runs a search; `today` stands in for a missing end date.
pub async fn find(pool: &SqlitePool, query: &FindQuery, today: NaiveDate) -> Result<FindResult, String> {
    let end = query.end_date.unwrap_or(today);
    if query.start_date.is_some_and(|s| s > end) {
        return Err("RPT_BAD_RANGE: the end date is before the start date".into());
    }
    let (min, max) = (query.filters.amount_min_cents, query.filters.amount_max_cents);
    if let (Some(min), Some(max)) = (min, max) {
        if min > max {
            return Err("FIND_BAD_AMOUNT: the smallest amount is more than the largest".into());
        }
    }
    let f = Filters { amount_min_cents: None, amount_max_cents: None, ..query.filters.clone() };
    let filter = LineFilter {
        f: &f,
        accounts: with_descendants(pool, "v_account_ancestry", "account_id", &f.account_ids).await?,
        classes: with_descendants(pool, "v_class_ancestry", "class_id", &f.class_ids).await?,
        memo: f.memo.as_deref().map(str::trim).filter(|m| !m.is_empty()).map(str::to_lowercase),
    };
    let text = query.text.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_lowercase);
    let has_text = |l: &Line| {
        text.as_ref().is_none_or(|t| [&l.name, &l.num, &l.memo].iter().any(|v| v.as_deref().is_some_and(|v| v.to_lowercase().contains(t.as_str()))))
    };
    let number = query.number.as_deref().map(normalize_number).filter(|n| !n.is_empty());

    let lines = ledger_lines(pool, query.start_date, end).await?;
    // The first matching line of each transaction, oldest transaction first.
    let mut first: HashMap<i64, &Line> = HashMap::new();
    let mut order = Vec::new();
    for l in &lines {
        if !first.contains_key(&l.journal_entry_id) && filter.matches(l) && has_text(l) {
            first.insert(l.journal_entry_id, l);
            order.push(l.journal_entry_id);
        }
    }
    let matches: Vec<&Line> = order
        .into_iter()
        .rev()
        .map(|id| first[&id])
        .filter(|l| number.as_ref().is_none_or(|n| l.num.as_deref().is_some_and(|num| normalize_number(num) == *n)))
        .filter(|l| min.is_none_or(|m| l.entry_total_cents >= m) && max.is_none_or(|m| l.entry_total_cents <= m))
        .collect();
    Ok(FindResult {
        start_date: query.start_date,
        end_date: end,
        match_count: matches.len(),
        total_cents: matches.iter().map(|l| l.entry_total_cents).sum(),
        rows: matches.iter().take(query.limit.unwrap_or(DEFAULT_LIMIT)).map(|l| found(l)).collect(),
    })
}

#[cfg(test)]
mod tests;
