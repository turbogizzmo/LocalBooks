//! Double-entry posting engine.
//!
//! All money is `i64` cents. An entry is validated in memory first (balance, one-sided lines,
//! overflow), then written inside a single SQLite transaction: header inserted as DRAFT, lines
//! attached, header flipped to POSTED. The schema's triggers re-check the balance at the flip, so
//! a bug here cannot persist an unbalanced entry. Any failure rolls the whole transaction back.
//!
//! Error strings start with a stable code (`JE_UNBALANCED: ...`) shared with the schema triggers,
//! so the UI can match on the prefix and show the remainder to the user.

use std::collections::HashSet;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

/// Per-line ceiling ($1 trillion), mirrored by the `journal_lines` CHECK constraints.
pub const MAX_LINE_CENTS: i64 = 100_000_000_000_000;
/// Line quantities are integers scaled by 10,000 (1.5 → 15_000).
pub const QUANTITY_SCALE: i64 = 10_000;
/// Exchange rates are integers scaled by 1,000,000 (home units per 1 foreign unit; 1.0875 → 1_087_500).
pub const RATE_SCALE: i64 = 1_000_000;

/// `a × b ÷ divisor`, rounded half away from zero, computed in 128 bits so it cannot overflow.
/// Returns `None` if the result does not fit in an `i64`.
pub fn mul_div_round(a: i64, b: i64, divisor: i64) -> Option<i64> {
    let product = a as i128 * b as i128;
    let d = divisor as i128;
    let half = d / 2;
    let rounded = if product >= 0 { (product + half) / d } else { (product - half) / d };
    i64::try_from(rounded).ok()
}

/// Line amount for a scaled quantity: round_half_up(quantity_e4 × rate_cents / 10,000).
pub fn quantity_amount(quantity_e4: i64, rate_cents: i64) -> Option<i64> {
    mul_div_round(quantity_e4, rate_cents, QUANTITY_SCALE)
}

/// Converts a foreign-currency amount to home currency at `rate_e6`, rounding half away from zero.
pub fn to_home_cents(foreign_cents: i64, rate_e6: i64) -> Option<i64> {
    mul_div_round(foreign_cents, rate_e6, RATE_SCALE)
}

/// The currency a document or journal entry was written in, when it is not the home currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignCurrency {
    /// ISO 4217 code, e.g. "EUR".
    pub code: String,
    /// Home-currency units per one foreign unit, scaled by 1,000,000.
    pub exchange_rate_e6: i64,
}

impl ForeignCurrency {
    pub fn validate(&self) -> Result<(), String> {
        if self.code.len() != 3 || !self.code.chars().all(|c| c.is_ascii_uppercase()) {
            return Err(format!("FX_BAD_CODE: '{}' is not a three-letter currency code", self.code));
        }
        if !(1..=1_000_000_000_000).contains(&self.exchange_rate_e6) {
            return Err("FX_BAD_RATE: the exchange rate must be greater than zero".into());
        }
        Ok(())
    }
}

/// Converts balanced foreign-currency lines to home currency. Each line is converted on its own;
/// any rounding difference between the two sides (a few cents at most) is absorbed by the largest
/// line on the short side, so the home-currency entry balances exactly. Deterministic, so callers
/// can predict the posted amounts.
pub fn convert_lines(lines: &[JournalLine], rate_e6: i64) -> Result<Vec<(i64, i64)>, String> {
    let overflow = || "FX_OVERFLOW: converted amount is out of range".to_string();
    let mut home: Vec<(i64, i64)> = Vec::with_capacity(lines.len());
    for line in lines {
        let d = to_home_cents(line.debit, rate_e6).ok_or_else(overflow)?;
        let c = to_home_cents(line.credit, rate_e6).ok_or_else(overflow)?;
        if (line.debit > 0 && d == 0) || (line.credit > 0 && c == 0) {
            return Err("FX_ROUNDS_TO_ZERO: a line converts to 0.00 in the home currency".into());
        }
        home.push((d, c));
    }
    let debits: i64 = home.iter().map(|l| l.0).sum();
    let credits: i64 = home.iter().map(|l| l.1).sum();
    let diff = debits - credits;
    if diff != 0 {
        let (side_is_credit, delta) = if diff > 0 { (true, diff) } else { (false, -diff) };
        let idx = home
            .iter()
            .enumerate()
            .filter(|(_, l)| if side_is_credit { l.1 > 0 } else { l.0 > 0 })
            .max_by_key(|(i, l)| (if side_is_credit { l.1 } else { l.0 }, std::cmp::Reverse(*i)))
            .map(|(i, _)| i)
            .ok_or_else(|| "FX_UNBALANCED: cannot balance the converted entry".to_string())?;
        if side_is_credit {
            home[idx].1 += delta;
        } else {
            home[idx].0 += delta;
        }
    }
    Ok(home)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalLine {
    pub account_id: i64,
    pub debit: i64,
    pub credit: i64,
    #[serde(default)]
    pub memo: Option<String>,
    /// Optional class (see `classes`), reported in Profit & Loss by Class.
    #[serde(default)]
    pub class_id: Option<i64>,
}

impl JournalLine {
    pub fn debit(account_id: i64, cents: i64) -> Self {
        Self { account_id, debit: cents, credit: 0, memo: None, class_id: None }
    }

    pub fn credit(account_id: i64, cents: i64) -> Self {
        Self { account_id, debit: 0, credit: cents, memo: None, class_id: None }
    }

    pub fn with_class(mut self, class_id: Option<i64>) -> Self {
        self.class_id = class_id;
        self
    }

    pub fn with_memo(mut self, memo: Option<&str>) -> Self {
        self.memo = memo.map(str::trim).filter(|m| !m.is_empty()).map(str::to_string);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    /// `None` lets SQLite assign the id; `Some` posts under a caller-chosen id.
    pub id: Option<i64>,
    pub date: NaiveDate,
    pub description: String,
    pub lines: Vec<JournalLine>,
    /// When set, `lines` are in this foreign currency and are converted to home currency on posting.
    #[serde(default)]
    pub currency: Option<ForeignCurrency>,
}

/// An entry's source type for display, given the entry as `e`: a card credit posts as an
/// `EXPENSE` entry but is listed as `CARD_CREDIT`.
pub(crate) const DISPLAY_SOURCE_TYPE: &str =
    "CASE WHEN e.source_type = 'EXPENSE' AND (SELECT is_credit FROM expenses WHERE id = e.source_id) = 1 THEN 'CARD_CREDIT' ELSE e.source_type END";

/// The business document that produced an entry. Manual journal entries have no source document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntrySource {
    Journal,
    Invoice(i64),
    ReceivePayment(i64),
    Expense(i64),
    Deposit(i64),
    SalesReceipt(i64),
    Bill(i64),
    BillPayment(i64),
    Check(i64),
    Transfer(i64),
    CreditMemo(i64),
    VendorCredit(i64),
    /// The reversing entry that voids the given journal entry.
    Reversal(i64),
    /// A journal entry that moves posted lines to another class (see `classes::reclassify`).
    Reclassification,
}

impl EntrySource {
    fn type_code(self) -> &'static str {
        match self {
            EntrySource::Journal | EntrySource::Reversal(_) | EntrySource::Reclassification => "JOURNAL",
            EntrySource::Invoice(_) => "INVOICE",
            EntrySource::ReceivePayment(_) => "RECEIVE_PAYMENT",
            EntrySource::Expense(_) => "EXPENSE",
            EntrySource::Deposit(_) => "DEPOSIT",
            EntrySource::SalesReceipt(_) => "SALES_RECEIPT",
            EntrySource::Bill(_) => "BILL",
            EntrySource::BillPayment(_) => "BILL_PAYMENT",
            EntrySource::Check(_) => "CHECK",
            EntrySource::Transfer(_) => "TRANSFER",
            EntrySource::CreditMemo(_) => "CREDIT_MEMO",
            EntrySource::VendorCredit(_) => "VENDOR_CREDIT",
        }
    }

    fn source_id(self) -> Option<i64> {
        match self {
            EntrySource::Journal | EntrySource::Reversal(_) | EntrySource::Reclassification => None,
            EntrySource::Invoice(id)
            | EntrySource::ReceivePayment(id)
            | EntrySource::Expense(id)
            | EntrySource::Deposit(id)
            | EntrySource::SalesReceipt(id)
            | EntrySource::Bill(id)
            | EntrySource::BillPayment(id)
            | EntrySource::Check(id)
            | EntrySource::Transfer(id)
            | EntrySource::CreditMemo(id)
            | EntrySource::VendorCredit(id) => Some(id),
        }
    }

    fn reverses(self) -> Option<i64> {
        match self {
            EntrySource::Reversal(id) => Some(id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryTotals {
    pub debits: i64,
    pub credits: i64,
}

/// Renders cents as `$1,234.56` / `-$1,234.56` for error messages.
pub fn format_cents(cents: i64) -> String {
    format_money(cents, "$")
}

/// Renders cents with a configurable currency symbol: `€1,234.56` / `-€1,234.56`.
pub fn format_money(cents: i64, symbol: &str) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let dollars = (abs / 100).to_string();
    let mut grouped = String::with_capacity(dollars.len() + dollars.len() / 3);
    for (i, ch) in dollars.chars().enumerate() {
        if i > 0 && (dollars.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(ch);
    }
    format!("{sign}{symbol}{grouped}.{:02}", abs % 100)
}

/// Pure, database-free validation. Returns the entry's totals when it is postable.
pub fn validate_entry(entry: &JournalEntry) -> Result<EntryTotals, String> {
    if entry.lines.len() < 2 {
        return Err(format!(
            "JE_TOO_FEW_LINES: a journal entry requires at least two lines (got {})",
            entry.lines.len()
        ));
    }

    let mut debits: i64 = 0;
    let mut credits: i64 = 0;

    for (idx, line) in entry.lines.iter().enumerate() {
        let n = idx + 1;
        if line.debit < 0 || line.credit < 0 {
            return Err(format!(
                "JE_NEGATIVE_AMOUNT: line {n} has a negative amount; enter it on the opposite side instead"
            ));
        }
        if line.debit != 0 && line.credit != 0 {
            return Err(format!(
                "JE_LINE_BOTH_SIDES: line {n} has both a debit ({}) and a credit ({}); one must be zero",
                format_cents(line.debit),
                format_cents(line.credit)
            ));
        }
        if line.debit == 0 && line.credit == 0 {
            return Err(format!("JE_LINE_EMPTY: line {n} has neither a debit nor a credit"));
        }
        if line.debit > MAX_LINE_CENTS || line.credit > MAX_LINE_CENTS {
            return Err(format!(
                "JE_AMOUNT_TOO_LARGE: line {n} exceeds the per-line maximum of {}",
                format_cents(MAX_LINE_CENTS)
            ));
        }
        debits = debits
            .checked_add(line.debit)
            .ok_or_else(|| "JE_OVERFLOW: total debits exceed the representable range".to_string())?;
        credits = credits
            .checked_add(line.credit)
            .ok_or_else(|| "JE_OVERFLOW: total credits exceed the representable range".to_string())?;
    }

    if debits != credits {
        return Err(format!(
            "JE_UNBALANCED: total debits {} do not equal total credits {} (out of balance by {})",
            format_cents(debits),
            format_cents(credits),
            format_cents(debits - credits)
        ));
    }

    Ok(EntryTotals { debits, credits })
}

/// Starts a write transaction with `BEGIN IMMEDIATE`, taking the write lock up front. Write paths read
/// before they write (closing date, balances, ...); under WAL a plain deferred transaction that reads
/// and then writes after another connection commits fails at once with SQLITE_BUSY_SNAPSHOT, which
/// `busy_timeout` does not retry. Taking the lock first makes concurrent writers wait their turn.
pub(crate) async fn begin_write(pool: &SqlitePool) -> Result<Transaction<'static, Sqlite>, sqlx::Error> {
    pool.begin_with("BEGIN IMMEDIATE").await
}

/// Commits a write transaction opened with [`begin_write`]: seals the audit rows it wrote into the
/// hash chain first (see `audit_chain`), then records the new chain head outside the company file.
pub(crate) async fn commit_write(mut tx: Transaction<'static, Sqlite>) -> Result<(), sqlx::Error> {
    let head = crate::audit_chain::seal(&mut tx).await.map_err(sqlx::Error::Protocol)?;
    // A chain that no longer holds the recorded head keeps the old record, as evidence for verify.
    // The record is best effort: if it can't be read, the write still commits and the record stays put.
    let advance = crate::audit_chain::continues_record(&mut tx, &head).await.unwrap_or_else(|err| {
        eprintln!("LocalBooks: could not read the audit trail record; leaving it unchanged: {err}");
        false
    });
    tx.commit().await?;
    if advance {
        crate::audit_chain::anchor(head).await;
    }
    Ok(())
}

pub(crate) fn db_err(err: sqlx::Error) -> String {
    match err {
        // Trigger and constraint messages already carry our JE_/COA_ codes; pass them through as-is.
        sqlx::Error::Database(db) => db.message().to_string(),
        other => format!("JE_DATABASE: {other}"),
    }
}

/// Validates `entry` and posts it atomically. Nothing is written unless every check passes.
/// The app's commands use [`post_journal_entry`] because they need the new id.
#[cfg_attr(not(test), allow(dead_code))]
pub async fn post_transaction(pool: &SqlitePool, entry: JournalEntry) -> Result<(), String> {
    post_journal_entry(pool, entry).await.map(|_| ())
}

/// Same as [`post_transaction`], returning the id of the posted entry.
pub async fn post_journal_entry(pool: &SqlitePool, entry: JournalEntry) -> Result<i64, String> {
    post_journal_entry_flagged(pool, entry, false).await
}

/// Posts a manual journal entry, marked as an adjusting entry when `adjusting` is set (the flag is
/// part of the posted header, so it is fixed once posted).
pub async fn post_journal_entry_flagged(pool: &SqlitePool, entry: JournalEntry, adjusting: bool) -> Result<i64, String> {
    let mut tx = begin_write(&pool).await.map_err(db_err)?;

    match post_entry_in_tx_flagged(&mut tx, &entry, EntrySource::Journal, adjusting).await {
        Ok(id) => commit_write(tx).await.map(|()| id).map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// Rolls `tx` back and returns `err`, noting a failed rollback rather than masking the original error.
pub(crate) async fn rollback(tx: Transaction<'_, Sqlite>, err: String) -> String {
    match tx.rollback().await {
        Ok(()) => err,
        Err(rollback_err) => format!("{err} (rollback also failed: {})", db_err(rollback_err)),
    }
}

/// Validates and posts `entry` inside the caller's transaction, so a source document (invoice,
/// bank line) and its ledger entry commit or roll back together. Returns the new entry id.
pub async fn post_entry_in_tx(
    tx: &mut Transaction<'_, Sqlite>,
    entry: &JournalEntry,
    source: EntrySource,
) -> Result<i64, String> {
    post_entry_in_tx_flagged(tx, entry, source, false).await
}

/// [`post_entry_in_tx`], with the entry marked as an adjusting entry when `adjusting` is set.
pub async fn post_entry_in_tx_flagged(
    tx: &mut Transaction<'_, Sqlite>,
    entry: &JournalEntry,
    source: EntrySource,
    adjusting: bool,
) -> Result<i64, String> {
    validate_entry(entry)?;
    crate::closing::check_open(tx, entry.date).await?;
    if let Some(currency) = &entry.currency {
        currency.validate()?;
        let home: String = sqlx::query_scalar("SELECT value FROM system_preferences WHERE key = 'home_currency_code'")
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?
            .unwrap_or_else(|| "USD".into());
        if currency.code == home {
            return Err(format!("FX_IS_HOME: {} is the home currency; record it without an exchange rate", currency.code));
        }
    }
    write_entry(tx, entry, source, adjusting).await
}

async fn write_entry(
    tx: &mut Transaction<'_, Sqlite>,
    entry: &JournalEntry,
    source: EntrySource,
    adjusting: bool,
) -> Result<i64, String> {
    let memo = Some(entry.description.trim()).filter(|s| !s.is_empty());

    // Callers open the transaction with `begin_write`, so the write lock is already held here.
    // Journal lines always hold home-currency amounts; foreign amounts are kept alongside.
    let home_amounts: Vec<(i64, i64)> = match &entry.currency {
        Some(currency) => convert_lines(&entry.lines, currency.exchange_rate_e6)?,
        None => entry.lines.iter().map(|l| (l.debit, l.credit)).collect(),
    };

    let entry_id: i64 = sqlx::query_scalar(
        "INSERT INTO journal_entries
            (id, entry_number, entry_date, memo, source_type, source_id, reverses_entry_id, foreign_currency_code, exchange_rate_e6, is_adjusting, status)
         VALUES (?1, (SELECT COALESCE(MAX(entry_number), 0) + 1 FROM journal_entries), ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'DRAFT')
         RETURNING id",
    )
    .bind(entry.id)
    .bind(entry.date)
    .bind(memo)
    .bind(source.type_code())
    .bind(source.source_id())
    .bind(source.reverses())
    .bind(entry.currency.as_ref().map(|c| c.code.as_str()))
    .bind(entry.currency.as_ref().map(|c| c.exchange_rate_e6))
    .bind(adjusting)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    let mut checked: HashSet<i64> = HashSet::with_capacity(entry.lines.len());
    let mut checked_classes: HashSet<i64> = HashSet::new();
    for (idx, line) in entry.lines.iter().enumerate() {
        let n = idx + 1;
        if checked.insert(line.account_id) {
            let is_active: Option<i64> =
                sqlx::query_scalar("SELECT is_active FROM chart_of_accounts WHERE id = ?1")
                    .bind(line.account_id)
                    .fetch_optional(&mut **tx)
                    .await
                    .map_err(db_err)?;
            match is_active {
                None => {
                    return Err(format!(
                        "JE_UNKNOWN_ACCOUNT: line {n} references account {} which does not exist",
                        line.account_id
                    ))
                }
                Some(0) => {
                    return Err(format!(
                        "JE_INACTIVE_ACCOUNT: line {n} references inactive account {}",
                        line.account_id
                    ))
                }
                Some(_) => {}
            }
        }
        // A reversal must carry the original's classes even if one has since been made inactive, and
        // a reclassification moves lines out of a class that may be inactive (it checks the target).
        let carries_existing = source.reverses().is_some() || source == EntrySource::Reclassification;
        if let Some(class_id) = line.class_id.filter(|c| !carries_existing && checked_classes.insert(*c)) {
            crate::classes::check_usable(tx, class_id, &format!("JE: line {n}")).await?;
        }

        let (home_debit, home_credit) = home_amounts[idx];
        let foreign = entry.currency.is_some();
        sqlx::query(
            "INSERT INTO journal_lines
                (journal_entry_id, line_number, account_id, debit_cents, credit_cents, memo, foreign_debit_cents, foreign_credit_cents, class_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(entry_id)
        .bind(n as i64)
        .bind(line.account_id)
        .bind(home_debit)
        .bind(home_credit)
        .bind(line.memo.as_deref().map(str::trim).filter(|m| !m.is_empty()))
        .bind(foreign.then_some(line.debit))
        .bind(foreign.then_some(line.credit))
        .bind(line.class_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }

    let posted = sqlx::query(
        "UPDATE journal_entries
         SET status = 'POSTED', posted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1 AND status = 'DRAFT'",
    )
    .bind(entry_id)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;

    if posted.rows_affected() != 1 {
        return Err(format!("JE_POST_FAILED: entry {entry_id} could not be moved to POSTED"));
    }

    Ok(entry_id)
}


// ------------------------------------------------------------------------------------------------
// Voiding
// ------------------------------------------------------------------------------------------------

/// Voids a posted transaction without deleting anything: a reversing entry (every debit and credit
/// swapped, same date) is posted, the original is flagged `is_voided`, and the source document is
/// marked void. Refused when any line is reconciled or ticked in a reconciliation, when an invoice
/// or bill still has payments, or when a payment has already been banked in a deposit.
pub async fn void_transaction(pool: &SqlitePool, journal_entry_id: i64) -> Result<(), String> {
    let mut tx = begin_write(&pool).await.map_err(db_err)?;
    match void_in_tx(&mut tx, journal_entry_id).await {
        Ok(()) => commit_write(tx).await.map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// Paid amount and status implied by an invoice's or bill's surviving (non-voided) payments and
/// applied credits.
pub(crate) async fn recompute_paid(
    tx: &mut Transaction<'_, Sqlite>,
    document_table: &str,
    payment_table: &str,
    fk: &str,
    document_id: i64,
) -> Result<(), String> {
    let credit_table = if document_table == "bills" { "vendor_credit_applications" } else { "credit_applications" };
    let (total, paid): (i64, i64) = sqlx::query_as(&format!(
        "SELECT d.total_cents,
                COALESCE((SELECT SUM(amount_cents) FROM {payment_table} WHERE {fk} = d.id AND is_voided = 0), 0)
              + COALESCE((SELECT SUM(amount_cents) FROM {credit_table} WHERE {fk} = d.id AND is_voided = 0), 0)
         FROM {document_table} d WHERE d.id = ?1"
    ))
    .bind(document_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    let status = if paid == 0 { "UNPAID" } else if paid < total { "PARTIAL" } else { "PAID" };
    sqlx::query(&format!("UPDATE {document_table} SET amount_paid_cents = ?1, status = ?2 WHERE id = ?3"))
        .bind(paid)
        .bind(status)
        .bind(document_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(())
}

pub(crate) async fn void_in_tx(tx: &mut Transaction<'_, Sqlite>, journal_entry_id: i64) -> Result<(), String> {
    let header: Option<(i64, NaiveDate, Option<String>, String, Option<i64>, String, bool, Option<i64>)> = sqlx::query_as(
        "SELECT entry_number, entry_date, memo, source_type, source_id, status, is_voided, reverses_entry_id
         FROM journal_entries WHERE id = ?1",
    )
    .bind(journal_entry_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let (number, date, memo, source_type, source_id, status, is_voided, reverses) =
        header.ok_or_else(|| format!("VOID_NOT_FOUND: journal entry {journal_entry_id} does not exist"))?;
    if status != "POSTED" {
        return Err(format!("VOID_NOT_POSTED: entry #{number} is {status}; only posted entries can be voided"));
    }
    if is_voided {
        return Err(format!("VOID_ALREADY: entry #{number} is already voided"));
    }
    if reverses.is_some() {
        return Err(format!("VOID_REVERSAL: entry #{number} is itself a void reversal and cannot be voided"));
    }

    let (reconciled, ticked): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*) FILTER (WHERE l.reconciled_at IS NOT NULL OR r.is_locked = 1),
                COUNT(*) FILTER (WHERE l.reconciled_at IS NULL AND r.is_locked = 0)
         FROM journal_lines l LEFT JOIN reconciliations r ON r.id = l.reconciliation_id
         WHERE l.journal_entry_id = ?1",
    )
    .bind(journal_entry_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    if reconciled > 0 {
        return Err(format!(
            "VOID_RECONCILED: entry #{number} is part of a completed reconciliation; voiding it would change a reconciled balance"
        ));
    }
    if ticked > 0 {
        return Err(format!(
            "VOID_CLEARED: entry #{number} is ticked in the reconciliation in progress; untick it there first"
        ));
    }

    let doc = source_id.unwrap_or_default();
    let blocked: Option<String> = match source_type.as_str() {
        "INVOICE" => sqlx::query_scalar(
            "SELECT 'VOID_HAS_PAYMENTS: invoice #' || invoice_number || ' has payments or credits applied; void or remove those first'
             FROM invoices WHERE id = ?1 AND amount_paid_cents <> 0",
        )
        .bind(doc)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?,
        "BILL" => sqlx::query_scalar(
            "SELECT 'VOID_HAS_PAYMENTS: this bill has payments; void those payments first'
             FROM bills WHERE id = ?1 AND amount_paid_cents <> 0",
        )
        .bind(doc)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?,
        "RECEIVE_PAYMENT" | "SALES_RECEIPT" => sqlx::query_scalar(
            "SELECT 'VOID_DEPOSITED: this payment is included in a bank deposit; void the deposit first'
             FROM deposit_lines
             WHERE is_voided = 0 AND (CASE ?2 WHEN 'RECEIVE_PAYMENT' THEN invoice_payment_id ELSE sales_receipt_id END) = ?1
             LIMIT 1",
        )
        .bind(doc)
        .bind(&source_type)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?,
        "CREDIT_MEMO" => sqlx::query_scalar(
            "SELECT 'VOID_CREDIT_USED: credit memo #' || m.credit_number || ' has been applied to invoices or refunded; remove those first'
             FROM credit_memos m WHERE m.id = ?1 AND (
                 EXISTS (SELECT 1 FROM credit_applications a WHERE a.credit_memo_id = m.id AND a.is_voided = 0)
                 OR EXISTS (SELECT 1 FROM credit_refunds r JOIN journal_entries e ON e.id = r.journal_entry_id
                            WHERE r.credit_memo_id = m.id AND e.is_voided = 0))",
        )
        .bind(doc)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?,
        "VENDOR_CREDIT" => sqlx::query_scalar(
            "SELECT 'VOID_CREDIT_USED: this vendor credit has been applied to bills or refunded; remove those first'
             FROM vendor_credits c WHERE c.id = ?1 AND (
                 EXISTS (SELECT 1 FROM vendor_credit_applications a WHERE a.vendor_credit_id = c.id AND a.is_voided = 0)
                 OR EXISTS (SELECT 1 FROM vendor_credit_refunds r JOIN journal_entries e ON e.id = r.journal_entry_id
                            WHERE r.vendor_credit_id = c.id AND e.is_voided = 0))",
        )
        .bind(doc)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?,
        _ => None,
    };
    if let Some(reason) = blocked {
        return Err(reason);
    }
    // Lines moved to another class: voiding this entry would reverse them into their original class
    // while the reclassifying entry still moves them, so that entry is voided first.
    let reclassified_by: Option<i64> = sqlx::query_scalar(
        "SELECT re.entry_number FROM class_reclassifications r
         JOIN journal_lines l ON l.id = r.line_id
         JOIN journal_entries re ON re.id = r.journal_entry_id
         WHERE l.journal_entry_id = ?1 AND re.is_voided = 0
         ORDER BY r.id DESC LIMIT 1",
    )
    .bind(journal_entry_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    if let Some(reclass_number) = reclassified_by {
        return Err(format!(
            "VOID_RECLASSIFIED: entry #{number} has lines moved to another class by entry #{reclass_number}; void that entry first"
        ));
    }
    // A reclassifying entry is undone newest first: a later move of the same lines builds on it.
    let later: Option<i64> = sqlx::query_scalar(
        "SELECT re.entry_number FROM class_reclassifications mine
         JOIN class_reclassifications r ON r.line_id = mine.line_id AND r.id > mine.id
         JOIN journal_entries re ON re.id = r.journal_entry_id
         WHERE mine.journal_entry_id = ?1 AND re.is_voided = 0
         LIMIT 1",
    )
    .bind(journal_entry_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    if let Some(later_number) = later {
        return Err(format!("VOID_RECLASS_ORDER: entry #{later_number} reclassifies the same lines again; void it first"));
    }

    let lines: Vec<(i64, i64, i64, Option<String>, Option<i64>)> = sqlx::query_as(
        "SELECT account_id, debit_cents, credit_cents, memo, class_id FROM journal_lines WHERE journal_entry_id = ?1 ORDER BY line_number",
    )
    .bind(journal_entry_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    // The reversal swaps the posted home-currency amounts exactly, so the void nets to zero even
    // for entries that were converted from a foreign currency.
    let reversal = JournalEntry {
        id: None,
        date,
        description: format!("VOID of entry #{number}{}", memo.map(|m| format!(": {m}")).unwrap_or_default()),
        lines: lines
            .into_iter()
            .map(|(account_id, debit, credit, memo, class_id)| JournalLine { account_id, debit: credit, credit: debit, memo, class_id })
            .collect(),
        currency: None,
    };
    let reversal_id = post_entry_in_tx(tx, &reversal, EntrySource::Reversal(journal_entry_id)).await?;
    // Stock that moved with this entry moves back (refused if later sales already used it).
    crate::inventory::reverse_for_entry(tx, journal_entry_id, reversal_id, date).await?;

    sqlx::query("UPDATE journal_entries SET is_voided = 1 WHERE id = ?1")
        .bind(journal_entry_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;

    let flag = |table: &'static str| format!("UPDATE {table} SET is_voided = 1 WHERE id = ?1");
    match source_type.as_str() {
        "INVOICE" => {
            sqlx::query(&flag("invoices")).bind(doc).execute(&mut **tx).await.map_err(db_err)?;
        }
        "SALES_RECEIPT" => {
            sqlx::query(&flag("sales_receipts")).bind(doc).execute(&mut **tx).await.map_err(db_err)?;
        }
        "BILL" => {
            sqlx::query(&flag("bills")).bind(doc).execute(&mut **tx).await.map_err(db_err)?;
        }
        "EXPENSE" | "CHECK" => {
            sqlx::query(&flag("expenses")).bind(doc).execute(&mut **tx).await.map_err(db_err)?;
        }
        "TRANSFER" => {
            sqlx::query(&flag("transfers")).bind(doc).execute(&mut **tx).await.map_err(db_err)?;
        }
        "CREDIT_MEMO" => {
            sqlx::query(&flag("credit_memos")).bind(doc).execute(&mut **tx).await.map_err(db_err)?;
        }
        "VENDOR_CREDIT" => {
            sqlx::query(&flag("vendor_credits")).bind(doc).execute(&mut **tx).await.map_err(db_err)?;
        }
        "DEPOSIT" => {
            sqlx::query(&flag("deposits")).bind(doc).execute(&mut **tx).await.map_err(db_err)?;
            sqlx::query("UPDATE deposit_lines SET is_voided = 1 WHERE deposit_id = ?1")
                .bind(doc)
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
        }
        "RECEIVE_PAYMENT" => {
            sqlx::query(&flag("invoice_payments")).bind(doc).execute(&mut **tx).await.map_err(db_err)?;
            let invoice_id: i64 = sqlx::query_scalar("SELECT invoice_id FROM invoice_payments WHERE id = ?1")
                .bind(doc)
                .fetch_one(&mut **tx)
                .await
                .map_err(db_err)?;
            recompute_paid(tx, "invoices", "invoice_payments", "invoice_id", invoice_id).await?;
        }
        "BILL_PAYMENT" => {
            sqlx::query(&flag("bill_payments")).bind(doc).execute(&mut **tx).await.map_err(db_err)?;
            let bill_id: i64 = sqlx::query_scalar("SELECT bill_id FROM bill_payments WHERE id = ?1")
                .bind(doc)
                .fetch_one(&mut **tx)
                .await
                .map_err(db_err)?;
            recompute_paid(tx, "bills", "bill_payments", "bill_id", bill_id).await?;
        }
        _ => {}
    }

    // A bank feed line whose document was voided goes back to "For review".
    sqlx::query(
        "UPDATE bank_transactions
         SET status = 'PENDING', journal_entry_id = NULL, category_account_id = NULL, class_id = NULL, reviewed_at = NULL
         WHERE journal_entry_id = ?1",
    )
    .bind(journal_entry_id)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;

    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Journal register and audit log
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntryView {
    pub id: i64,
    pub entry_number: i64,
    pub entry_date: String,
    pub memo: Option<String>,
    pub source_type: String,
    pub source_id: Option<i64>,
    pub total_cents: i64,
    pub is_voided: bool,
    pub reverses_entry_id: Option<i64>,
    pub reverses_entry_number: Option<i64>,
    pub reversed_by_entry_number: Option<i64>,
    pub posted_at: Option<String>,
    pub is_adjusting: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct JournalLineView {
    pub line_number: i64,
    pub account_id: i64,
    pub account_name: String,
    pub debit_cents: i64,
    pub credit_cents: i64,
    pub memo: Option<String>,
    pub class_id: Option<i64>,
    pub class_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AuditEntry {
    pub id: i64,
    pub occurred_at: String,
    pub action: String,
    pub journal_entry_id: Option<i64>,
    pub entry_number: Option<i64>,
    pub source_type: Option<String>,
    pub source_id: Option<i64>,
    pub amount_cents: Option<i64>,
    pub detail: String,
}

pub async fn list_journal_entries(pool: &SqlitePool, limit: i64) -> Result<Vec<JournalEntryView>, String> {
    sqlx::query_as::<_, JournalEntryView>(&format!(
        "SELECT e.id, e.entry_number, e.entry_date, e.memo, {DISPLAY_SOURCE_TYPE} AS source_type, e.source_id,
                COALESCE((SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = e.id), 0) AS total_cents,
                e.is_voided, e.reverses_entry_id, o.entry_number AS reverses_entry_number,
                r.entry_number AS reversed_by_entry_number, e.posted_at, e.is_adjusting
         FROM journal_entries e
         LEFT JOIN journal_entries o ON o.id = e.reverses_entry_id
         LEFT JOIN journal_entries r ON r.reverses_entry_id = e.id
         WHERE e.status = 'POSTED'
         ORDER BY e.entry_date DESC, e.entry_number DESC
         LIMIT ?1"
    ))
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn journal_entry_lines(pool: &SqlitePool, journal_entry_id: i64) -> Result<Vec<JournalLineView>, String> {
    sqlx::query_as::<_, JournalLineView>(
        "SELECT l.line_number, l.account_id, c.full_name AS account_name, l.debit_cents, l.credit_cents, l.memo,
                l.class_id, k.full_name AS class_name
         FROM journal_lines l JOIN v_chart_of_accounts c ON c.id = l.account_id
         LEFT JOIN v_classes k ON k.id = l.class_id
         WHERE l.journal_entry_id = ?1 ORDER BY l.line_number",
    )
    .bind(journal_entry_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn list_audit_log(pool: &SqlitePool, limit: i64) -> Result<Vec<AuditEntry>, String> {
    sqlx::query_as::<_, AuditEntry>(
        "SELECT a.id, a.occurred_at, a.action, a.journal_entry_id, e.entry_number, a.source_type, a.source_id,
                a.amount_cents, a.detail
         FROM audit_log a LEFT JOIN journal_entries e ON e.id = a.journal_entry_id
         ORDER BY a.id DESC LIMIT ?1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

// ------------------------------------------------------------------------------------------------
// Forensic verification against the ledger audit trail
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AuditFinding {
    pub entry_id: i64,
    pub entry_number: Option<i64>,
    pub problem: String,
    /// Set for problems with the audit trail itself (see `audit_chain`) rather than with an entry.
    pub audit_record_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerVerification {
    /// True when there are no findings.
    pub intact: bool,
    pub entries_checked: i64,
    pub voids_checked: i64,
    pub migrations_recorded: i64,
    /// Audit records verified through the hash chain.
    pub audit_records_sealed: i64,
    /// The day this computer started keeping a record of the audit trail (YYYY-MM-DD), if it has one.
    pub audit_trail_recorded_since: Option<String>,
    pub findings: Vec<AuditFinding>,
}

/// Rebuilds every posted entry from the live tables and compares it with the snapshot written when
/// it was posted. The ledger is intact when every posted entry has an identical snapshot, balances,
/// every void links to its reversal, and no snapshotted entry has disappeared.
pub async fn verify_ledger_audit(pool: &SqlitePool) -> Result<LedgerVerification, String> {
    let checks: [(&str, &str); 6] = [
        (
            "Posted without a posting snapshot",
            "SELECT e.id AS entry_id, e.entry_number FROM journal_entries e
             WHERE e.status <> 'DRAFT'
               AND NOT EXISTS (SELECT 1 FROM ledger_audit_log a WHERE a.action_type = 'POST' AND a.target_entity_id = e.id)",
        ),
        (
            "Differs from its posting snapshot (changed after posting)",
            "SELECT e.id AS entry_id, e.entry_number FROM journal_entries e
             JOIN ledger_audit_log a ON a.action_type = 'POST' AND a.target_entity_id = e.id
             JOIN v_entry_snapshot s ON s.entry_id = e.id
             WHERE json_remove(a.payload_snapshot, '$.backfilled') <> s.snapshot",
        ),
        (
            "Debits do not equal credits",
            "SELECT e.id AS entry_id, e.entry_number FROM journal_entries e
             WHERE e.status <> 'DRAFT'
               AND (SELECT COALESCE(SUM(debit_cents), 0) - COALESCE(SUM(credit_cents), 0)
                    FROM journal_lines WHERE journal_entry_id = e.id) <> 0",
        ),
        (
            "Voided without a recorded reversal",
            "SELECT e.id AS entry_id, e.entry_number FROM journal_entries e
             WHERE e.is_voided = 1
               AND NOT EXISTS (SELECT 1 FROM ledger_audit_log a WHERE a.action_type = 'VOID' AND a.target_entity_id = e.id)",
        ),
        (
            "Removed from balances by a status change (no reversing entry)",
            "SELECT e.id AS entry_id, e.entry_number FROM journal_entries e
             WHERE e.status = 'VOID' AND e.id NOT IN (SELECT entry_id FROM legacy_status_voids)",
        ),
        (
            "Snapshotted at posting but no longer in the ledger (deleted)",
            "SELECT a.target_entity_id AS entry_id, json_extract(a.payload_snapshot, '$.entryNumber') AS entry_number
             FROM ledger_audit_log a
             WHERE a.action_type = 'POST' AND NOT EXISTS (SELECT 1 FROM journal_entries e WHERE e.id = a.target_entity_id)",
        ),
    ];

    let mut findings = Vec::new();
    for (problem, sql) in checks {
        let rows: Vec<(i64, Option<i64>)> = sqlx::query_as(sql).fetch_all(pool).await.map_err(db_err)?;
        findings.extend(rows.into_iter().map(|(entry_id, entry_number)| AuditFinding { entry_id, entry_number, problem: problem.into(), audit_record_id: None }));
    }
    let chain = crate::audit_chain::verify(pool).await?;
    findings.extend(chain.problems.into_iter().map(|(record, problem)| AuditFinding { entry_id: 0, entry_number: None, problem, audit_record_id: Some(record) }));
    findings.sort_by_key(|f| (f.audit_record_id.is_some(), f.entry_id, f.audit_record_id, f.problem.clone()));

    let (entries_checked, voids_checked, migrations_recorded): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM journal_entries WHERE status <> 'DRAFT'),
                (SELECT COUNT(*) FROM journal_entries WHERE is_voided = 1),
                (SELECT COUNT(*) FROM ledger_audit_log WHERE action_type = 'MIGRATION')",
    )
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    Ok(LedgerVerification {
        intact: findings.is_empty(),
        entries_checked,
        voids_checked,
        migrations_recorded,
        audit_records_sealed: chain.records_sealed,
        audit_trail_recorded_since: chain.recorded_since,
        findings,
    })
}

#[cfg(test)]
mod tests;
