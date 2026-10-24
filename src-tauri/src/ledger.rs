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
    let advance = crate::audit_chain::continues_record(&mut tx, &head).await.map_err(sqlx::Error::Protocol)?;
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
    let mut tx = begin_write(&pool).await.map_err(db_err)?;

    match post_entry_in_tx(&mut tx, &entry, EntrySource::Journal).await {
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
    write_entry(tx, entry, source).await
}

async fn write_entry(
    tx: &mut Transaction<'_, Sqlite>,
    entry: &JournalEntry,
    source: EntrySource,
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
            (id, entry_number, entry_date, memo, source_type, source_id, reverses_entry_id, foreign_currency_code, exchange_rate_e6, status)
         VALUES (?1, (SELECT COALESCE(MAX(entry_number), 0) + 1 FROM journal_entries), ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'DRAFT')
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
    sqlx::query_as::<_, JournalEntryView>(
        "SELECT e.id, e.entry_number, e.entry_date, e.memo, e.source_type, e.source_id,
                COALESCE((SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = e.id), 0) AS total_cents,
                e.is_voided, e.reverses_entry_id, o.entry_number AS reverses_entry_number,
                r.entry_number AS reversed_by_entry_number, e.posted_at
         FROM journal_entries e
         LEFT JOIN journal_entries o ON o.id = e.reverses_entry_id
         LEFT JOIN journal_entries r ON r.reverses_entry_id = e.id
         WHERE e.status = 'POSTED'
         ORDER BY e.entry_date DESC, e.entry_number DESC
         LIMIT ?1",
    )
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
    /// Whether this computer holds a record of the chain to compare the file against.
    pub audit_trail_anchored: bool,
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
        audit_trail_anchored: chain.anchored,
        findings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    struct Fixture {
        pool: SqlitePool,
        checking: i64,
        sales: i64,
    }

    async fn setup() -> Fixture {
        let pool = test_pool().await;
        let checking = account_id(&pool, "1010").await;
        let sales = account_id(&pool, "4000").await;
        Fixture { pool, checking, sales }
    }

    async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1")
            .bind(number)
            .fetch_one(pool)
            .await
            .expect("seeded account exists")
    }

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 13).expect("valid date")
    }

    async fn row_counts(pool: &SqlitePool) -> (i64, i64) {
        let entries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries")
            .fetch_one(pool)
            .await
            .expect("count entries");
        let lines: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_lines")
            .fetch_one(pool)
            .await
            .expect("count lines");
        (entries, lines)
    }

    #[tokio::test]
    async fn posts_balanced_100_dollar_entry() {
        let fx = setup().await;
        let entry = JournalEntry {
            id: None,
            date: date(),
            description: "Cash sale".into(),
            lines: vec![JournalLine::debit(fx.checking, 10_000), JournalLine::credit(fx.sales, 10_000)],
            currency: None,
        };

        post_transaction(&fx.pool, entry).await.expect("balanced entry posts");

        let (status, number, memo, entry_date): (String, i64, Option<String>, String) = sqlx::query_as(
            "SELECT status, entry_number, memo, entry_date FROM journal_entries",
        )
        .fetch_one(&fx.pool)
        .await
        .expect("read header");
        assert_eq!(status, "POSTED");
        assert_eq!(number, 1);
        assert_eq!(memo.as_deref(), Some("Cash sale"));
        assert_eq!(entry_date, "2026-10-13");

        let (debits, credits): (i64, i64) =
            sqlx::query_as("SELECT SUM(debit_cents), SUM(credit_cents) FROM journal_lines")
                .fetch_one(&fx.pool)
                .await
                .expect("sum lines");
        assert_eq!((debits, credits), (10_000, 10_000));

        let checking_balance: i64 =
            sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1")
                .bind(fx.checking)
                .fetch_one(&fx.pool)
                .await
                .expect("checking balance");
        let sales_balance: i64 =
            sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1")
                .bind(fx.sales)
                .fetch_one(&fx.pool)
                .await
                .expect("sales balance");
        assert_eq!(checking_balance, 10_000);
        assert_eq!(sales_balance, 10_000);
    }

    #[tokio::test]
    async fn rejects_unbalanced_entry() {
        let fx = setup().await;
        let entry = JournalEntry {
            id: None,
            date: date(),
            description: "Unbalanced".into(),
            lines: vec![JournalLine::debit(fx.checking, 10_000), JournalLine::credit(fx.sales, 9_900)],
            currency: None,
        };

        let err = post_transaction(&fx.pool, entry).await.expect_err("unbalanced entry must be rejected");

        assert!(err.starts_with("JE_UNBALANCED"), "unexpected error: {err}");
        assert!(err.contains("$100.00") && err.contains("$99.00") && err.contains("$1.00"), "{err}");
        assert_eq!(row_counts(&fx.pool).await, (0, 0));
    }

    #[tokio::test]
    async fn rejects_line_with_both_debit_and_credit() {
        let fx = setup().await;
        let entry = JournalEntry {
            id: None,
            date: date(),
            description: "Two-sided line".into(),
            lines: vec![
                JournalLine { account_id: fx.checking, debit: 10_000, credit: 10_000, memo: None, class_id: None },
                JournalLine::debit(fx.sales, 5_000),
                JournalLine::credit(fx.sales, 5_000),
            ],
            currency: None,
        };

        let err = post_transaction(&fx.pool, entry).await.expect_err("two-sided line must be rejected");

        assert!(err.starts_with("JE_LINE_BOTH_SIDES: line 1"), "unexpected error: {err}");
        assert_eq!(row_counts(&fx.pool).await, (0, 0));
    }

    #[tokio::test]
    async fn rolls_back_header_when_a_later_step_fails() {
        let fx = setup().await;
        sqlx::query("UPDATE chart_of_accounts SET is_active = 0 WHERE id = ?1")
            .bind(fx.sales)
            .execute(&fx.pool)
            .await
            .expect("deactivate sales");
        let entry = JournalEntry {
            id: None,
            date: date(),
            description: "Posts to inactive account".into(),
            lines: vec![JournalLine::debit(fx.checking, 10_000), JournalLine::credit(fx.sales, 10_000)],
            currency: None,
        };

        let err = post_transaction(&fx.pool, entry).await.expect_err("inactive account must be rejected");

        assert!(err.starts_with("JE_INACTIVE_ACCOUNT: line 2"), "unexpected error: {err}");
        // The DRAFT header and line 1 were written before the failure; rollback must remove both.
        assert_eq!(row_counts(&fx.pool).await, (0, 0));
    }

    #[test]
    fn formats_cents() {
        assert_eq!(format_cents(0), "$0.00");
        assert_eq!(format_cents(1_050), "$10.50");
        assert_eq!(format_cents(-100), "-$1.00");
        assert_eq!(format_cents(123_456_789), "$1,234,567.89");
    }

    #[tokio::test]
    async fn voiding_reverses_unwinds_documents_and_audits() {
        use crate::invoices::{create_and_post_invoice, list_invoices, record_payment, InvoiceInput, InvoiceLineInput, PaymentInput};
        let fx = setup().await;
        let services = account_id(&fx.pool, "4100").await;
        let ar = account_id(&fx.pool, "1200").await;
        let bal = |account: i64| {
            let pool = fx.pool.clone();
            async move {
                sqlx::query_scalar::<_, i64>("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1")
                    .bind(account)
                    .fetch_one(&pool)
                    .await
                    .unwrap()
            }
        };

        let invoice = create_and_post_invoice(
            &fx.pool,
            InvoiceInput {
                customer_name: "Acme".into(),
                issue_date: date(),
                due_date: date(),
                lines: vec![InvoiceLineInput{ description: "Work".into(), quantity_e4: 10000, rate_cents: 50_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
                currency: None,
                tax_rate_id: None,
                discount_cents: 0,
            },
        )
        .await
        .unwrap();
        let paid = record_payment(
            &fx.pool,
            PaymentInput { invoice_id: invoice.id, deposit_account_id: fx.checking, payment_date: date(), amount_cents: 50_000, payment_method: "ACH".into(), reference: None, exchange_rate_e6: None },
        )
        .await
        .unwrap();
        assert_eq!(paid.status, "PAID");

        // An invoice with payments cannot be voided until the payment is.
        let err = void_transaction(&fx.pool, invoice.journal_entry_id.unwrap()).await.expect_err("has payments");
        assert!(err.starts_with("VOID_HAS_PAYMENTS"), "{err}");

        let payment_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM invoice_payments").fetch_one(&fx.pool).await.unwrap();
        void_transaction(&fx.pool, payment_entry).await.expect("payment voids");
        let reopened = list_invoices(&fx.pool).await.unwrap().remove(0);
        assert_eq!((reopened.status.as_str(), reopened.balance_cents), ("UNPAID", 50_000));
        assert_eq!((bal(fx.checking).await, bal(ar).await), (0, 50_000));

        void_transaction(&fx.pool, invoice.journal_entry_id.unwrap()).await.expect("invoice voids");
        assert_eq!((bal(ar).await, bal(services).await), (0, 0));
        assert!(list_invoices(&fx.pool).await.unwrap()[0].is_voided);

        // Nothing was deleted: originals and reversals are all still posted.
        let (posted, voided, reversals): (i64, i64, i64) = sqlx::query_as(
            "SELECT COUNT(*), SUM(is_voided), COUNT(reverses_entry_id) FROM journal_entries WHERE status = 'POSTED'",
        )
        .fetch_one(&fx.pool)
        .await
        .unwrap();
        assert_eq!((posted, voided, reversals), (4, 2, 2));

        let err = void_transaction(&fx.pool, payment_entry).await.expect_err("double void");
        assert!(err.starts_with("VOID_ALREADY"), "{err}");
        let reversal: i64 = sqlx::query_scalar("SELECT id FROM journal_entries WHERE reverses_entry_id = ?1")
            .bind(payment_entry)
            .fetch_one(&fx.pool)
            .await
            .unwrap();
        assert!(void_transaction(&fx.pool, reversal).await.unwrap_err().starts_with("VOID_REVERSAL"));

        let log = list_audit_log(&fx.pool, 50).await.unwrap();
        assert_eq!(log.iter().filter(|a| a.action == "VOID").count(), 2);
        assert_eq!(log.iter().filter(|a| a.action == "POST").count(), 4);
        assert!(sqlx::query("DELETE FROM audit_log").execute(&fx.pool).await.unwrap_err().to_string().contains("AUDIT_IMMUTABLE"));

        let register = list_journal_entries(&fx.pool, 10).await.unwrap();
        let original = register.iter().find(|e| e.id == payment_entry).unwrap();
        assert!(original.is_voided && original.reversed_by_entry_number.is_some());
        assert_eq!(journal_entry_lines(&fx.pool, reversal).await.unwrap()[0].credit_cents, 50_000);
    }

    #[tokio::test]
    async fn reconciled_entries_cannot_be_voided() {
        let fx = setup().await;
        let entry = post_journal_entry(
            &fx.pool,
            JournalEntry { id: None, date: date(), description: "Sale".into(), lines: vec![JournalLine::debit(fx.checking, 10_000), JournalLine::credit(fx.sales, 10_000)], currency: None },
        )
        .await
        .unwrap();
        let rec = crate::reconcile::start(&fx.pool, fx.checking, date(), 10_000).await.unwrap();
        let lines = crate::reconcile::unreconciled_lines(&fx.pool, fx.checking).await.unwrap();
        crate::reconcile::set_lines_cleared(&fx.pool, rec.id, &[lines[0].line_id], true).await.unwrap();
        assert!(void_transaction(&fx.pool, entry).await.unwrap_err().starts_with("VOID_CLEARED"));

        crate::reconcile::lock(&fx.pool, rec.id).await.unwrap();
        assert!(void_transaction(&fx.pool, entry).await.unwrap_err().starts_with("VOID_RECONCILED"));
        let still_posted: bool = sqlx::query_scalar("SELECT is_voided FROM journal_entries WHERE id = ?1").bind(entry).fetch_one(&fx.pool).await.unwrap();
        assert!(!still_posted);
    }

    #[test]
    fn conversion_balances_rounding_on_the_short_side() {
        // 3 × 33.33 EUR = 99.99 EUR at 1.085: each line rounds separately (36.16 × 3 = 108.48),
        // the total rounds to 108.49; the 0.01 lands on the largest line of the short side.
        let lines = vec![
            JournalLine::debit(1, 3_333),
            JournalLine::debit(2, 3_333),
            JournalLine::debit(3, 3_333),
            JournalLine::credit(4, 9_999),
        ];
        let home = convert_lines(&lines, 1_085_000).unwrap();
        let debits: i64 = home.iter().map(|l| l.0).sum();
        let credits: i64 = home.iter().map(|l| l.1).sum();
        assert_eq!(debits, credits);
        assert_eq!(home[3].1, 10_849);
        assert_eq!(home.iter().map(|l| l.0).collect::<Vec<_>>(), vec![3_617, 3_616, 3_616, 0]);
        assert!(convert_lines(&[JournalLine::debit(1, 1), JournalLine::credit(2, 1)], 100_000).unwrap_err().starts_with("FX_ROUNDS_TO_ZERO"));
    }

    #[tokio::test]
    async fn foreign_journal_entry_posts_home_amounts_and_keeps_foreign_detail() {
        let fx = setup().await;
        let eur = |rate| Some(ForeignCurrency { code: "EUR".into(), exchange_rate_e6: rate });
        let entry = |currency| JournalEntry {
            id: None,
            date: date(),
            description: "Euro sale".into(),
            lines: vec![JournalLine::debit(fx.checking, 10_000), JournalLine::credit(fx.sales, 10_000)],
            currency,
        };
        let id = post_journal_entry(&fx.pool, entry(eur(1_087_500))).await.expect("posts");
        let rows: Vec<(i64, i64, Option<i64>, Option<i64>)> = sqlx::query_as(
            "SELECT debit_cents, credit_cents, foreign_debit_cents, foreign_credit_cents FROM journal_lines WHERE journal_entry_id = ?1 ORDER BY line_number",
        )
        .bind(id)
        .fetch_all(&fx.pool)
        .await
        .unwrap();
        assert_eq!(rows, vec![(10_875, 0, Some(10_000), Some(0)), (0, 10_875, Some(0), Some(10_000))]);
        let (code, rate): (Option<String>, Option<i64>) =
            sqlx::query_as("SELECT foreign_currency_code, exchange_rate_e6 FROM journal_entries WHERE id = ?1").bind(id).fetch_one(&fx.pool).await.unwrap();
        assert_eq!((code.as_deref(), rate), (Some("EUR"), Some(1_087_500)));

        let usd = Some(ForeignCurrency { code: "USD".into(), exchange_rate_e6: 1_000_000 });
        assert!(post_journal_entry(&fx.pool, entry(usd)).await.unwrap_err().starts_with("FX_IS_HOME"));
        assert!(post_journal_entry(&fx.pool, entry(Some(ForeignCurrency { code: "eur".into(), exchange_rate_e6: 1 }))).await.unwrap_err().starts_with("FX_BAD_CODE"));
        assert!(post_journal_entry(&fx.pool, entry(eur(0))).await.unwrap_err().starts_with("FX_BAD_RATE"));

        // The home currency is locked once foreign transactions exist.
        assert!(crate::preferences::set(&fx.pool, "home_currency_code", "CAD").await.unwrap_err().starts_with("PREF_HOME_CURRENCY_LOCKED"));
        assert_eq!(crate::preferences::set(&fx.pool, "home_currency_code", "usd").await.unwrap(), "USD");
    }

    async fn post_sale(fx: &Fixture, cents: i64, memo: &str) -> i64 {
        post_journal_entry(
            &fx.pool,
            JournalEntry {
                id: None,
                date: date(),
                description: memo.into(),
                lines: vec![JournalLine::debit(fx.checking, cents).with_memo(Some("deposit")), JournalLine::credit(fx.sales, cents)],
                currency: None,
            },
        )
        .await
        .unwrap()
    }

    async fn sql_err(pool: &SqlitePool, sql: &str) -> String {
        sqlx::query(sql).execute(pool).await.map(|_| String::new()).unwrap_or_else(db_err)
    }

    #[tokio::test]
    async fn posting_and_voiding_write_immutable_snapshots() {
        let fx = setup().await;
        let kept = post_sale(&fx, 12_345, "Cash sale").await;
        let voided = post_sale(&fx, 500, "Wrong amount").await;
        void_transaction(&fx.pool, voided).await.unwrap();

        let snapshot: String = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE action_type = 'POST' AND target_entity_id = ?1")
            .bind(kept)
            .fetch_one(&fx.pool)
            .await
            .unwrap();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(snapshot["memo"], "Cash sale");
        assert_eq!(snapshot["totalDebitCents"], 12_345);
        assert_eq!(snapshot["lines"][0]["accountId"], fx.checking);
        assert_eq!(snapshot["lines"][0]["memo"], "deposit");
        assert_eq!(snapshot["lines"][1]["creditCents"], 12_345);

        let void: String = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE action_type = 'VOID' AND target_entity_id = ?1")
            .bind(voided)
            .fetch_one(&fx.pool)
            .await
            .unwrap();
        let void: serde_json::Value = serde_json::from_str(&void).unwrap();
        let reversal: i64 = sqlx::query_scalar("SELECT id FROM journal_entries WHERE reverses_entry_id = ?1").bind(voided).fetch_one(&fx.pool).await.unwrap();
        assert_eq!(void["reversalEntryId"], reversal, "the void links to the correcting entry");
        assert_eq!(void["amountCents"], 500);

        let verification = verify_ledger_audit(&fx.pool).await.unwrap();
        assert!(verification.intact, "{:?}", verification.findings);
        assert_eq!((verification.entries_checked, verification.voids_checked), (3, 1));

        assert!(sql_err(&fx.pool, "UPDATE ledger_audit_log SET payload_snapshot = '{}'").await.starts_with("AUDIT_IMMUTABLE"));
        assert!(sql_err(&fx.pool, "DELETE FROM ledger_audit_log").await.starts_with("AUDIT_IMMUTABLE"));
    }

    #[tokio::test]
    async fn posted_memos_and_account_types_with_history_are_locked() {
        let fx = setup().await;
        let entry = post_sale(&fx, 1_000, "Original memo").await;
        let err = sql_err(&fx.pool, &format!("UPDATE journal_entries SET memo = 'Edited' WHERE id = {entry}")).await;
        assert!(err.starts_with("JE_IMMUTABLE"), "{err}");

        let err = sql_err(&fx.pool, &format!("UPDATE chart_of_accounts SET account_type = 'EXPENSE' WHERE id = {}", fx.checking)).await;
        assert!(err.starts_with("COA_TYPE_LOCKED"), "{err}");

        // Renaming is allowed, and recorded with before/after values.
        sqlx::query("UPDATE chart_of_accounts SET name = 'Operating Checking' WHERE id = ?1").bind(fx.checking).execute(&fx.pool).await.unwrap();
        let change: String = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE action_type = 'ACCOUNT_CHANGE' AND target_entity_id = ?1")
            .bind(fx.checking)
            .fetch_one(&fx.pool)
            .await
            .unwrap();
        let change: serde_json::Value = serde_json::from_str(&change).unwrap();
        assert_eq!(change["before"]["name"], "Checking");
        assert_eq!(change["after"]["name"], "Operating Checking");

        // An account with no posted history can still change type.
        let unused = account_id(&fx.pool, "4100").await;
        sqlx::query("UPDATE chart_of_accounts SET account_type = 'OTHER_INCOME' WHERE id = ?1").bind(unused).execute(&fx.pool).await.unwrap();
    }

    #[tokio::test]
    async fn verification_detects_tampering_that_bypasses_the_triggers() {
        let fx = setup().await;
        let altered = post_sale(&fx, 1_000, "Altered").await;
        let deleted = post_sale(&fx, 2_000, "Deleted").await;
        let untouched = post_sale(&fx, 3_000, "Untouched").await;
        assert!(verify_ledger_audit(&fx.pool).await.unwrap().intact);

        // Simulate someone editing the file with an outside tool (the sqlite3 shell runs with
        // foreign keys off) after removing the protecting triggers.
        for sql in [
            "PRAGMA foreign_keys = OFF",
            "DROP TRIGGER trg_jl_update_draft_only",
            "DROP TRIGGER trg_jl_delete_draft_only",
            "DROP TRIGGER trg_je_no_delete_posted",
            "DROP TRIGGER trg_ledger_audit_no_delete",
        ] {
            sqlx::query(sql).execute(&fx.pool).await.unwrap();
        }
        sqlx::query("UPDATE journal_lines SET debit_cents = 9000 WHERE journal_entry_id = ?1 AND debit_cents > 0").bind(altered).execute(&fx.pool).await.unwrap();
        sqlx::query("DELETE FROM journal_lines WHERE journal_entry_id = ?1").bind(deleted).execute(&fx.pool).await.unwrap();
        sqlx::query("DELETE FROM journal_entries WHERE id = ?1").bind(deleted).execute(&fx.pool).await.unwrap();

        let verification = verify_ledger_audit(&fx.pool).await.unwrap();
        let problems: Vec<(i64, &str)> = verification.findings.iter().map(|f| (f.entry_id, f.problem.as_str())).collect();
        assert!(problems.contains(&(altered, "Differs from its posting snapshot (changed after posting)")), "{problems:?}");
        assert!(problems.contains(&(altered, "Debits do not equal credits")), "{problems:?}");
        assert!(problems.contains(&(deleted, "Snapshotted at posting but no longer in the ledger (deleted)")), "{problems:?}");
        assert!(!problems.iter().any(|(id, _)| *id == untouched));
        let deleted_number = verification.findings.iter().find(|f| f.entry_id == deleted).unwrap().entry_number;
        assert_eq!(deleted_number, Some(2), "the snapshot still knows the deleted entry's number");

        // Erasing the snapshot itself is caught as an entry posted without one.
        sqlx::query("DELETE FROM ledger_audit_log WHERE action_type = 'POST' AND target_entity_id = ?1").bind(untouched).execute(&fx.pool).await.unwrap();
        let verification = verify_ledger_audit(&fx.pool).await.unwrap();
        assert!(verification.findings.iter().any(|f| f.entry_id == untouched && f.problem == "Posted without a posting snapshot"));
    }

    #[tokio::test]
    async fn posted_entries_cannot_leave_the_ledger_by_status_change() {
        let fx = setup().await;
        let id = post_sale(&fx, 50_000, "Sale").await;

        let err = sql_err(&fx.pool, &format!("UPDATE journal_entries SET status = 'VOID', voided_at = 'x' WHERE id = {id}")).await;
        assert!(err.starts_with("JE_INVALID_TRANSITION"), "{err}");
        let err = sql_err(&fx.pool, &format!("UPDATE journal_entries SET status = 'DRAFT', posted_at = NULL WHERE id = {id}")).await;
        assert!(err.starts_with("JE_INVALID_TRANSITION"), "{err}");
        let err = sql_err(&fx.pool, &format!("INSERT INTO legacy_status_voids (entry_id) VALUES ({id})")).await;
        assert!(err.starts_with("AUDIT_IMMUTABLE"), "{err}");

        // Forced through with the trigger removed, the verifier now catches it.
        sqlx::query("DROP TRIGGER trg_je_status_transition").execute(&fx.pool).await.unwrap();
        sqlx::query("UPDATE journal_entries SET status = 'VOID', voided_at = 'x' WHERE id = ?1").bind(id).execute(&fx.pool).await.unwrap();
        let verification = verify_ledger_audit(&fx.pool).await.unwrap();
        assert!(!verification.intact);
        assert!(verification
            .findings
            .iter()
            .any(|f| f.entry_id == id && f.problem == "Removed from balances by a status change (no reversing entry)"));
    }

    #[tokio::test]
    async fn schema_rejects_unbalanced_or_missing_foreign_amounts() {
        let fx = setup().await;
        let draft = |n: i64, currency: bool| {
            format!(
                "INSERT INTO journal_entries (entry_number, entry_date, foreign_currency_code, exchange_rate_e6) VALUES ({n}, '2026-10-13', {}, {}) RETURNING id",
                if currency { "'EUR'" } else { "NULL" },
                if currency { "1100000" } else { "NULL" }
            )
        };
        let lines = |id: i64, fd: &str, fc: &str| {
            format!(
                "INSERT INTO journal_lines (journal_entry_id, line_number, account_id, debit_cents, foreign_debit_cents, foreign_credit_cents) VALUES ({id}, 1, {}, 1100, {fd}, 0);
                 INSERT INTO journal_lines (journal_entry_id, line_number, account_id, credit_cents, foreign_debit_cents, foreign_credit_cents) VALUES ({id}, 2, {}, 1100, 0, {fc});",
                fx.checking, fx.sales
            )
        };
        let post = |id: i64| format!("UPDATE journal_entries SET status = 'POSTED', posted_at = 'now' WHERE id = {id}");

        let unbalanced: i64 = sqlx::query_scalar(&draft(901, true)).fetch_one(&fx.pool).await.unwrap();
        sqlx::raw_sql(&lines(unbalanced, "1000", "999")).execute(&fx.pool).await.unwrap();
        assert!(sql_err(&fx.pool, &post(unbalanced)).await.starts_with("JE_FOREIGN_UNBALANCED"));

        let missing: i64 = sqlx::query_scalar(&draft(902, true)).fetch_one(&fx.pool).await.unwrap();
        sqlx::raw_sql(&lines(missing, "NULL", "1000")).execute(&fx.pool).await.unwrap();
        assert!(sql_err(&fx.pool, &post(missing)).await.starts_with("JE_FOREIGN_MISSING"));

        let home: i64 = sqlx::query_scalar(&draft(903, false)).fetch_one(&fx.pool).await.unwrap();
        sqlx::raw_sql(&lines(home, "1000", "1000")).execute(&fx.pool).await.unwrap();
        assert!(sql_err(&fx.pool, &post(home)).await.starts_with("JE_FOREIGN_ON_HOME"));

        let good: i64 = sqlx::query_scalar(&draft(904, true)).fetch_one(&fx.pool).await.unwrap();
        sqlx::raw_sql(&lines(good, "1000", "1000")).execute(&fx.pool).await.unwrap();
        assert_eq!(sql_err(&fx.pool, &post(good)).await, "");
    }

    /// The snapshot format from before classes existed (migration 0013), kept to prove that entries
    /// without a class still snapshot byte-for-byte as they did when they were posted.
    const PRE_CLASS_LINES_SQL: &str = "SELECT json_group_array(json_object(
            'lineNumber', l.line_number, 'accountId', l.account_id, 'debitCents', l.debit_cents,
            'creditCents', l.credit_cents, 'memo', l.memo, 'foreignDebitCents', l.foreign_debit_cents,
            'foreignCreditCents', l.foreign_credit_cents) ORDER BY l.line_number)
        FROM journal_lines l WHERE l.journal_entry_id = ?1";

    #[tokio::test]
    async fn classes_join_the_snapshot_without_changing_unclassified_ones() {
        let fx = setup().await;
        let rent = account_id(&fx.pool, "6300").await;
        let east = crate::classes::create_class(&fx.pool, crate::classes::ClassInput { name: "East".into(), parent_id: None }).await.unwrap().id;
        let post = |lines: Vec<JournalLine>| {
            post_journal_entry(&fx.pool, JournalEntry { id: None, date: date(), description: "Caf\u{e9} \"rent\" \u{1F680}".into(), lines, currency: None })
        };

        let plain = post(vec![JournalLine::debit(rent, 1_250).with_memo(Some("tab\there")), JournalLine::credit(fx.checking, 1_250)]).await.unwrap();
        let (live, old): (String, String) = sqlx::query_as(&format!(
            "SELECT json_extract(snapshot, '$.lines'), ({PRE_CLASS_LINES_SQL}) FROM v_entry_snapshot WHERE entry_id = ?1"
        ))
        .bind(plain)
        .fetch_one(&fx.pool)
        .await
        .unwrap();
        assert_eq!(live, old);

        let classed = post(vec![JournalLine::debit(rent, 700).with_class(Some(east)), JournalLine::credit(fx.checking, 700)]).await.unwrap();
        let snapshot: String = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE action_type = 'POST' AND target_entity_id = ?1")
            .bind(classed)
            .fetch_one(&fx.pool)
            .await
            .unwrap();
        let snapshot: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(snapshot["lines"][0]["classId"], east);
        assert!(snapshot["lines"][1].get("classId").is_none());

        // The void's reversing entry carries the class, so the class nets to zero.
        void_transaction(&fx.pool, classed).await.unwrap();
        let east_net: i64 = sqlx::query_scalar("SELECT SUM(debit_cents) - SUM(credit_cents) FROM v_posted_lines WHERE class_id = ?1")
            .bind(east)
            .fetch_one(&fx.pool)
            .await
            .unwrap();
        assert_eq!(east_net, 0);
        assert!(verify_ledger_audit(&fx.pool).await.unwrap().intact);

        // Moving a posted line to another class (bypassing the triggers) is detected.
        let west = crate::classes::create_class(&fx.pool, crate::classes::ClassInput { name: "West".into(), parent_id: None }).await.unwrap().id;
        sqlx::query("DROP TRIGGER trg_jl_update_draft_only").execute(&fx.pool).await.unwrap();
        sqlx::query("UPDATE journal_lines SET class_id = ?1 WHERE journal_entry_id = ?2 AND class_id IS NOT NULL").bind(west).bind(classed).execute(&fx.pool).await.unwrap();
        let verification = verify_ledger_audit(&fx.pool).await.unwrap();
        assert!(verification.findings.iter().any(|f| f.entry_id == classed && f.problem.starts_with("Differs from its posting snapshot")));
    }
}
