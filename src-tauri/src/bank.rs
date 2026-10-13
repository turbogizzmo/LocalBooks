//! Local bank feed: CSV import, vendor-matching category suggestions, and approval into the ledger.
//!
//! Sign convention: negative amounts are money out of the account, positive amounts are money in.
//! Amounts are parsed from text straight to integer cents; no floating point is involved.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use chrono::{Datelike, NaiveDate};
use regex::{Regex, RegexBuilder};
use serde::Serialize;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::banking::{write_deposit, DepositInput, DepositOtherLine};
use crate::expenses::{write_expense, ExpenseInput, SplitLineInput};
use crate::ledger::{db_err, rollback, MAX_LINE_CENTS};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BankCsvRow {
    /// 1-based line number in the source file, for error messages and review.
    pub row: u64,
    pub date: NaiveDate,
    pub description: String,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategorySuggestion {
    pub account_id: i64,
    pub rule_id: Option<i64>,
    /// `LEARNED` (from past approvals), `BUILT_IN` (vendor list) or `DEFAULT` (uncategorized fallback).
    pub origin: String,
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

// Two-digit-year formats come first: `%Y` would otherwise read "3/7/26" as the year 26 AD.
const DATE_FORMATS: &[&str] =
    &["%Y-%m-%d", "%m/%d/%y", "%m/%d/%Y", "%m-%d-%y", "%m-%d-%Y", "%Y/%m/%d", "%b %d, %Y", "%d %b %Y"];

pub fn parse_date(raw: &str) -> Result<NaiveDate, String> {
    let s = raw.trim();
    // Some exports append a time ("2026-10-01 00:00:00"); the date part is all we need.
    let s = s.split_once(' ').filter(|(d, _)| d.contains(['-', '/'])).map_or(s, |(d, _)| d);
    DATE_FORMATS
        .iter()
        .find_map(|fmt| NaiveDate::parse_from_str(s, fmt).ok())
        .filter(|d| (1900..=2200).contains(&d.year()))
        .ok_or_else(|| format!("unrecognized date '{raw}'"))
}

enum AmountColumns {
    Single(usize),
    Split { debit: usize, credit: usize },
}

fn find_column(headers: &[String], candidates: &[&str]) -> Option<usize> {
    candidates
        .iter()
        .find_map(|c| headers.iter().position(|h| h == c))
        .or_else(|| candidates.iter().find_map(|c| headers.iter().position(|h| h.contains(c))))
}

/// Parses a bank CSV with a header row. Recognizes a `Date`, a `Description` (or Memo/Payee/Name)
/// and either a signed `Amount` column or separate `Debit`/`Credit` (`Withdrawal`/`Deposit`) columns.
pub fn parse_bank_csv_reader<R: Read>(reader: R) -> Result<Vec<BankCsvRow>, String> {
    let mut csv = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(reader);

    let headers: Vec<String> = csv
        .headers()
        .map_err(|e| format!("BANK_CSV_READ: {e}"))?
        .iter()
        .map(|h| h.trim_start_matches('\u{feff}').trim().to_lowercase())
        .collect();

    let missing = |what: &str| format!("BANK_CSV_COLUMNS: no {what} column found (headers: {})", headers.join(", "));
    let date_col = find_column(&headers, &["date", "transaction date", "posting date", "posted date"])
        .ok_or_else(|| missing("Date"))?;
    let desc_col = find_column(&headers, &["description", "memo", "payee", "name", "details"])
        .ok_or_else(|| missing("Description"))?;
    let amount_cols = match find_column(&headers, &["amount"]) {
        Some(i) => AmountColumns::Single(i),
        None => AmountColumns::Split {
            debit: find_column(&headers, &["debit", "withdrawal"]).ok_or_else(|| missing("Amount or Debit"))?,
            credit: find_column(&headers, &["credit", "deposit"]).ok_or_else(|| missing("Amount or Credit"))?,
        },
    };

    let mut rows = Vec::new();
    for record in csv.records() {
        let record = record.map_err(|e| format!("BANK_CSV_READ: {e}"))?;
        let line = record.position().map_or(0, |p| p.line());
        if record.iter().all(str::is_empty) {
            continue;
        }
        let field = |i: usize| record.get(i).unwrap_or("");
        let err = |e: String| format!("BANK_CSV_ROW: line {line}: {e}");

        let date = parse_date(field(date_col)).map_err(err)?;
        let amount_cents = match amount_cols {
            AmountColumns::Single(i) => parse_amount_cents(field(i)).map_err(err)?,
            AmountColumns::Split { debit, credit } => {
                let side = |i: usize| -> Result<i64, String> {
                    if field(i).is_empty() { Ok(0) } else { parse_amount_cents(field(i)).map(i64::abs) }
                };
                side(credit).map_err(err)? - side(debit).map_err(err)?
            }
        };
        let description = match field(desc_col) {
            "" => "(no description)".to_string(),
            d => d.split_whitespace().collect::<Vec<_>>().join(" "),
        };
        rows.push(BankCsvRow { row: line, date, description, amount_cents });
    }
    Ok(rows)
}

pub fn parse_bank_csv(path: &Path) -> Result<Vec<BankCsvRow>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("BANK_CSV_OPEN: {}: {e}", path.display()))?;
    parse_bank_csv_reader(file)
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

    pub fn suggest(&self, description: &str, amount_cents: i64) -> Option<CategorySuggestion> {
        let direction = if amount_cents < 0 { "OUTFLOW" } else { "INFLOW" };
        self.rules
            .iter()
            .find(|r| (r.direction == "ANY" || r.direction == direction) && r.regex.is_match(description))
            .map(|r| CategorySuggestion { account_id: r.account_id, rule_id: Some(r.id), origin: r.origin.clone() })
            .or_else(|| {
                let fallback = if amount_cents < 0 { self.uncategorized_expense } else { self.uncategorized_income };
                fallback.map(|account_id| CategorySuggestion { account_id, rule_id: None, origin: "DEFAULT".into() })
            })
    }
}

// ------------------------------------------------------------------------------------------------
// Persistence
// ------------------------------------------------------------------------------------------------

fn normalize_description(description: &str) -> String {
    description.split_whitespace().collect::<Vec<_>>().join(" ").to_uppercase()
}

/// Imports a CSV into the review queue of `bank_account_id`. Re-importing the same file is a no-op:
/// each line is fingerprinted by date, amount, description and its occurrence count within the file,
/// so two identical same-day purchases both import while a repeated export does not.
pub async fn import_csv(pool: &SqlitePool, path: &Path, bank_account_id: i64) -> Result<ImportSummary, String> {
    let rows = parse_bank_csv(path)?;

    let mut tx = pool.begin().await.map_err(db_err)?;
    match write_import(&mut tx, &rows, bank_account_id).await {
        Ok(summary) => {
            tx.commit().await.map_err(db_err)?;
            Ok(summary)
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

async fn write_import(
    tx: &mut Transaction<'_, Sqlite>,
    rows: &[BankCsvRow],
    bank_account_id: i64,
) -> Result<ImportSummary, String> {
    let mut summary = ImportSummary::default();
    let mut occurrences: HashMap<String, u32> = HashMap::new();

    for row in rows {
        if row.amount_cents == 0 {
            summary.skipped_zero += 1;
            continue;
        }
        let key = format!("{}|{}|{}", row.date, row.amount_cents, normalize_description(&row.description));
        let n = occurrences.entry(key.clone()).or_insert(0);
        *n += 1;
        let fingerprint = format!("{key}|{n}");

        let result = sqlx::query(
            "INSERT INTO bank_transactions (bank_account_id, txn_date, description, amount_cents, fingerprint)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT (bank_account_id, fingerprint) DO NOTHING",
        )
        .bind(bank_account_id)
        .bind(row.date)
        .bind(&row.description)
        .bind(row.amount_cents)
        .bind(&fingerprint)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;

        if result.rows_affected() == 1 {
            summary.imported += 1;
        } else {
            summary.duplicates += 1;
        }
    }
    Ok(summary)
}

pub async fn list_pending(pool: &SqlitePool) -> Result<Vec<BankTransactionView>, String> {
    let rows: Vec<(i64, i64, String, String, i64)> = sqlx::query_as(
        "SELECT id, bank_account_id, txn_date, description, amount_cents
         FROM bank_transactions WHERE status = 'PENDING'
         ORDER BY txn_date DESC, id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let rules = RuleSet::load(pool).await?;
    Ok(rows
        .into_iter()
        .map(|(id, bank_account_id, date, description, amount_cents)| BankTransactionView {
            suggestion: rules.suggest(&description, amount_cents),
            id,
            bank_account_id,
            date,
            description,
            amount_cents,
        })
        .collect())
}

/// Posts a pending bank line to the ledger against `category_account_id` and learns a matching rule.
pub async fn approve(pool: &SqlitePool, transaction_id: i64, category_account_id: i64) -> Result<i64, String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    match write_approval(&mut tx, transaction_id, category_account_id).await {
        Ok(entry_id) => {
            tx.commit().await.map_err(db_err)?;
            Ok(entry_id)
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

async fn write_approval(
    tx: &mut Transaction<'_, Sqlite>,
    transaction_id: i64,
    category_account_id: i64,
) -> Result<i64, String> {
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

    // Approval creates a real document, as in QuickBooks: money out becomes an Expense
    // (Dr category / Cr bank), money in becomes a Deposit (Dr bank / Cr category).
    let amount = amount_cents.abs();
    let (entry_id, direction) = if amount_cents < 0 {
        let expense = ExpenseInput {
            kind: "EXPENSE".into(),
            payee_name: None,
            payment_account_id: bank_account_id,
            expense_date: date,
            check_number: None,
            payment_method: None,
            memo: Some(description.clone()),
            lines: vec![SplitLineInput { account_id: category_account_id, amount_cents: amount, memo: Some(description.clone()) }],
            currency: None,
        };
        (write_expense(tx, &expense).await?.1, "OUTFLOW")
    } else {
        let deposit = DepositInput {
            bank_account_id,
            deposit_date: date,
            memo: Some(description.clone()),
            items: Vec::new(),
            other_lines: vec![DepositOtherLine { account_id: category_account_id, amount_cents: amount, memo: Some(description.clone()) }],
        };
        (write_deposit(tx, &deposit).await?.1, "INFLOW")
    };

    sqlx::query(
        "UPDATE bank_transactions
         SET status = 'APPROVED', category_account_id = ?1, journal_entry_id = ?2,
             reviewed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?3",
    )
    .bind(category_account_id)
    .bind(entry_id)
    .bind(transaction_id)
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

    Ok(entry_id)
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
mod tests {
    use super::*;
    use crate::db::test_pool;

    async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1")
            .bind(number)
            .fetch_one(pool)
            .await
            .expect("seeded account exists")
    }

    #[test]
    fn parses_amounts_to_exact_cents() {
        let cases = [
            ("12.34", 1_234),
            ("-12.34", -1_234),
            ("$1,234.56", 123_456),
            ("(45.00)", -4_500),
            ("45.00-", -4_500),
            ("+3", 300),
            ("0.1", 10),
            (".05", 5),
            ("7.500", 750),
            ("-$0.99", -99),
        ];
        for (raw, cents) in cases {
            assert_eq!(parse_amount_cents(raw), Ok(cents), "{raw}");
        }
        for bad in ["", "abc", "1.234", "1.2.3", "12e3", "--5", "."] {
            assert!(parse_amount_cents(bad).is_err(), "{bad} should fail");
        }
    }

    #[test]
    fn parses_common_date_formats() {
        let expected = NaiveDate::from_ymd_opt(2026, 3, 7).unwrap();
        for raw in ["2026-03-07", "03/07/2026", "3/7/2026", "3/7/26", "Mar 07, 2026", "2026-03-07 00:00:00"] {
            assert_eq!(parse_date(raw), Ok(expected), "{raw}");
        }
    }

    #[test]
    fn parses_signed_amount_csv() {
        let csv = "\u{feff}Date,Description,Amount\n10/01/2026,AWS EMEA  aws.amazon.com,-120.50\n10/02/2026,STRIPE TRANSFER,1500.00\n,,\n";
        let rows = parse_bank_csv_reader(csv.as_bytes()).expect("parses");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].description, "AWS EMEA aws.amazon.com");
        assert_eq!(rows[0].amount_cents, -12_050);
        assert_eq!(rows[1].amount_cents, 150_000);
        assert_eq!(rows[1].row, 3);
    }

    #[test]
    fn parses_split_debit_credit_csv() {
        let csv = "Posted Date,Payee,Withdrawal,Deposit\n2026-10-01,EXXONMOBIL 4421,52.10,\n2026-10-02,CLIENT ACH,,900\n";
        let rows = parse_bank_csv_reader(csv.as_bytes()).expect("parses");
        assert_eq!(rows.iter().map(|r| r.amount_cents).collect::<Vec<_>>(), vec![-5_210, 90_000]);
    }

    #[test]
    fn reports_bad_row_with_line_number() {
        let csv = "Date,Description,Amount\n2026-10-01,OK,1.00\n2026-10-02,BAD,twelve\n";
        let err = parse_bank_csv_reader(csv.as_bytes()).expect_err("bad amount");
        assert!(err.starts_with("BANK_CSV_ROW: line 3"), "{err}");
    }

    #[test]
    fn learns_patterns_that_ignore_noise() {
        let pattern = learned_pattern("SQ *BLUE BOTTLE #123 OAKLAND").expect("pattern");
        let regex = RegexBuilder::new(&pattern).case_insensitive(true).build().unwrap();
        assert!(regex.is_match("SQ *BLUE BOTTLE #998 SF"));
        assert!(regex.is_match("Blue-Bottle Coffee"));
        assert!(!regex.is_match("BLUE APRON"));
        assert_eq!(learned_pattern("POS DEBIT 0042 #"), None);
    }

    #[tokio::test]
    async fn built_in_rules_suggest_vendor_categories() {
        let pool = test_pool().await;
        let rules = RuleSet::load(&pool).await.expect("rules compile");
        let expect = |desc: &str, amount: i64| rules.suggest(desc, amount).map(|s| s.account_id);

        assert_eq!(expect("AWS EMEA aws.amazon.com", -12_050), Some(account_id(&pool, "6350").await));
        assert_eq!(expect("EXXONMOBIL 4421 HOUSTON", -5_210), Some(account_id(&pool, "6100").await));
        assert_eq!(expect("UBER EATS 8005928996", -2_399), Some(account_id(&pool, "6200").await));
        assert_eq!(expect("UBER *TRIP", -1_850), Some(account_id(&pool, "6450").await));
        assert_eq!(expect("STRIPE TRANSFER", 150_000), Some(account_id(&pool, "4000").await));
        assert_eq!(expect("RANDOM VENDOR", -100), Some(account_id(&pool, "6900").await));
        assert_eq!(expect("RANDOM PAYER", 100), Some(account_id(&pool, "4900").await));
    }

    #[tokio::test]
    async fn import_dedupes_and_approval_posts_and_learns() {
        let pool = test_pool().await;
        let checking = account_id(&pool, "1010").await;
        let meals = account_id(&pool, "6200").await;

        let dir = std::env::temp_dir().join(format!("localbooks-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("feed.csv");
        std::fs::write(
            &path,
            "Date,Description,Amount\n2026-10-01,JOES DINER 0042,-18.75\n2026-10-01,JOES DINER 0042,-18.75\n2026-10-02,TRANSFER,0.00\n",
        )
        .unwrap();

        let first = import_csv(&pool, &path, checking).await.expect("imports");
        assert_eq!(first, ImportSummary { imported: 2, duplicates: 0, skipped_zero: 1 });
        let again = import_csv(&pool, &path, checking).await.expect("re-imports");
        assert_eq!(again, ImportSummary { imported: 0, duplicates: 2, skipped_zero: 1 });
        std::fs::remove_dir_all(&dir).unwrap();

        let pending = list_pending(&pool).await.expect("pending");
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].suggestion.as_ref().map(|s| s.origin.as_str()), Some("DEFAULT"));

        approve(&pool, pending[0].id, meals).await.expect("approves");
        let (checking_balance, meals_balance): (i64, i64) = sqlx::query_as(
            "SELECT (SELECT balance_cents FROM v_account_balances WHERE account_id = ?1),
                    (SELECT balance_cents FROM v_account_balances WHERE account_id = ?2)",
        )
        .bind(checking)
        .bind(meals)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((checking_balance, meals_balance), (-1_875, 1_875));

        let remaining = list_pending(&pool).await.expect("pending");
        assert_eq!(remaining.len(), 1);
        let learned = remaining[0].suggestion.as_ref().expect("suggestion");
        assert_eq!((learned.account_id, learned.origin.as_str()), (meals, "LEARNED"));

        let err = approve(&pool, pending[0].id, meals).await.expect_err("already approved");
        assert!(err.starts_with("BANK_TXN_NOT_PENDING"), "{err}");

        exclude(&pool, remaining[0].id).await.expect("excludes");
        assert!(list_pending(&pool).await.unwrap().is_empty());

        // The approval is a real Expense document; voiding it sends the line back for review.
        let entry_id: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM expenses WHERE kind = 'EXPENSE'")
            .fetch_one(&pool)
            .await
            .expect("approval created an expense");
        crate::ledger::void_transaction(&pool, entry_id).await.expect("voids");
        let back = list_pending(&pool).await.unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].id, pending[0].id);
        let checking_after: i64 = sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1")
            .bind(checking)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(checking_after, 0);
    }
}
