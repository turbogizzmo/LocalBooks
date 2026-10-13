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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalLine {
    pub account_id: i64,
    pub debit: i64,
    pub credit: i64,
    #[serde(default)]
    pub memo: Option<String>,
}

impl JournalLine {
    pub fn debit(account_id: i64, cents: i64) -> Self {
        Self { account_id, debit: cents, credit: 0, memo: None }
    }

    pub fn credit(account_id: i64, cents: i64) -> Self {
        Self { account_id, debit: 0, credit: cents, memo: None }
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
    /// The reversing entry that voids the given journal entry.
    Reversal(i64),
}

impl EntrySource {
    fn type_code(self) -> &'static str {
        match self {
            EntrySource::Journal | EntrySource::Reversal(_) => "JOURNAL",
            EntrySource::Invoice(_) => "INVOICE",
            EntrySource::ReceivePayment(_) => "RECEIVE_PAYMENT",
            EntrySource::Expense(_) => "EXPENSE",
            EntrySource::Deposit(_) => "DEPOSIT",
            EntrySource::SalesReceipt(_) => "SALES_RECEIPT",
            EntrySource::Bill(_) => "BILL",
            EntrySource::BillPayment(_) => "BILL_PAYMENT",
            EntrySource::Check(_) => "CHECK",
            EntrySource::Transfer(_) => "TRANSFER",
        }
    }

    fn source_id(self) -> Option<i64> {
        match self {
            EntrySource::Journal | EntrySource::Reversal(_) => None,
            EntrySource::Invoice(id)
            | EntrySource::ReceivePayment(id)
            | EntrySource::Expense(id)
            | EntrySource::Deposit(id)
            | EntrySource::SalesReceipt(id)
            | EntrySource::Bill(id)
            | EntrySource::BillPayment(id)
            | EntrySource::Check(id)
            | EntrySource::Transfer(id) => Some(id),
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
    let mut tx = pool.begin().await.map_err(db_err)?;

    match post_entry_in_tx(&mut tx, &entry, EntrySource::Journal).await {
        Ok(id) => tx.commit().await.map(|()| id).map_err(db_err),
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
    write_entry(tx, entry, source).await
}

async fn write_entry(
    tx: &mut Transaction<'_, Sqlite>,
    entry: &JournalEntry,
    source: EntrySource,
) -> Result<i64, String> {
    let memo = Some(entry.description.trim()).filter(|s| !s.is_empty());

    // For standalone posts the header insert is the transaction's first statement, so the write
    // lock is taken up front instead of upgrading from a read lock later (SQLITE_BUSY under WAL).
    let entry_id: i64 = sqlx::query_scalar(
        "INSERT INTO journal_entries (id, entry_number, entry_date, memo, source_type, source_id, reverses_entry_id, status)
         VALUES (?1, (SELECT COALESCE(MAX(entry_number), 0) + 1 FROM journal_entries), ?2, ?3, ?4, ?5, ?6, 'DRAFT')
         RETURNING id",
    )
    .bind(entry.id)
    .bind(entry.date)
    .bind(memo)
    .bind(source.type_code())
    .bind(source.source_id())
    .bind(source.reverses())
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    let mut checked: HashSet<i64> = HashSet::with_capacity(entry.lines.len());
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

        sqlx::query(
            "INSERT INTO journal_lines (journal_entry_id, line_number, account_id, debit_cents, credit_cents, memo)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(entry_id)
        .bind(n as i64)
        .bind(line.account_id)
        .bind(line.debit)
        .bind(line.credit)
        .bind(line.memo.as_deref().map(str::trim).filter(|m| !m.is_empty()))
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
    let mut tx = pool.begin().await.map_err(db_err)?;
    match void_in_tx(&mut tx, journal_entry_id).await {
        Ok(()) => tx.commit().await.map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// Paid amount and status implied by an invoice's or bill's surviving (non-voided) payments.
async fn recompute_paid(
    tx: &mut Transaction<'_, Sqlite>,
    document_table: &str,
    payment_table: &str,
    fk: &str,
    document_id: i64,
) -> Result<(), String> {
    let (total, paid): (i64, i64) = sqlx::query_as(&format!(
        "SELECT d.total_cents,
                COALESCE((SELECT SUM(amount_cents) FROM {payment_table} WHERE {fk} = d.id AND is_voided = 0), 0)
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

async fn void_in_tx(tx: &mut Transaction<'_, Sqlite>, journal_entry_id: i64) -> Result<(), String> {
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
            "SELECT 'VOID_HAS_PAYMENTS: invoice #' || invoice_number || ' has payments applied; void those payments first'
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
        _ => None,
    };
    if let Some(reason) = blocked {
        return Err(reason);
    }

    let lines: Vec<(i64, i64, i64, Option<String>)> = sqlx::query_as(
        "SELECT account_id, debit_cents, credit_cents, memo FROM journal_lines WHERE journal_entry_id = ?1 ORDER BY line_number",
    )
    .bind(journal_entry_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    let reversal = JournalEntry {
        id: None,
        date,
        description: format!("VOID of entry #{number}{}", memo.map(|m| format!(": {m}")).unwrap_or_default()),
        lines: lines
            .into_iter()
            .map(|(account_id, debit, credit, memo)| JournalLine { account_id, debit: credit, credit: debit, memo })
            .collect(),
    };
    post_entry_in_tx(tx, &reversal, EntrySource::Reversal(journal_entry_id)).await?;

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
         SET status = 'PENDING', journal_entry_id = NULL, category_account_id = NULL, reviewed_at = NULL
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
        "SELECT l.line_number, l.account_id, c.full_name AS account_name, l.debit_cents, l.credit_cents, l.memo
         FROM journal_lines l JOIN v_chart_of_accounts c ON c.id = l.account_id
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
                JournalLine { account_id: fx.checking, debit: 10_000, credit: 10_000, memo: None },
                JournalLine::debit(fx.sales, 5_000),
                JournalLine::credit(fx.sales, 5_000),
            ],
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
                lines: vec![InvoiceLineInput { description: "Work".into(), quantity: 1, rate_cents: 50_000, income_account_id: services }],
            },
        )
        .await
        .unwrap();
        let paid = record_payment(
            &fx.pool,
            PaymentInput { invoice_id: invoice.id, deposit_account_id: fx.checking, payment_date: date(), amount_cents: 50_000, payment_method: "ACH".into(), reference: None },
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
            JournalEntry { id: None, date: date(), description: "Sale".into(), lines: vec![JournalLine::debit(fx.checking, 10_000), JournalLine::credit(fx.sales, 10_000)] },
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
}
