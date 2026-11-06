//! Record Bounced Check: a customer's check (or other deposited funds) that the bank returned.
//!
//! One write transaction posts one journal entry:
//!
//! - Dr Accounts Receivable for the payment (or the account a deposit's "other funds" line
//!   credited), Cr the bank account the money went into;
//! - and, when the bank charged a fee, Dr the chosen expense account, Cr the bank.
//!
//! The invoice the payment paid opens again: while the bounce is live its payment no longer counts
//! toward the invoice's amount paid (the schema checks this, migration 0066). A fee charged to the
//! customer is a new invoice with one "Returned check charge" line, written in the same
//! transaction. Voiding the bounce (through `ledger::void_in_tx`) reverses its entry, counts the
//! payment again, and voids the fee invoice too.
//!
//! Only money that reached the bank can bounce: a payment still in Undeposited Funds is voided
//! instead. Foreign-currency payments are not handled yet.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::invoices::{InvoiceInput, InvoiceLineInput};
use crate::ledger::{db_err, format_cents, post_entry_in_tx, rollback, EntrySource, JournalEntry, JournalLine, MAX_LINE_CENTS, QUANTITY_SCALE};

/// A deposited payment or deposit line that can be recorded as bounced.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BounceCandidate {
    /// `PAYMENT` (an invoice payment) or `DEPOSIT_LINE` (other funds on a deposit).
    pub kind: String,
    pub id: i64,
    pub date: String,
    pub customer_name: Option<String>,
    pub invoice_number: Option<i64>,
    pub payment_method: Option<String>,
    pub reference: Option<String>,
    pub memo: Option<String>,
    pub amount_cents: i64,
    pub bank_account_id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BounceInput {
    /// `PAYMENT` or `DEPOSIT_LINE`, as listed by [`candidates`].
    pub kind: String,
    pub id: i64,
    pub bounce_date: NaiveDate,
    /// What the bank charged; 0 for none.
    #[serde(default)]
    pub bank_fee_cents: i64,
    pub bank_fee_account_id: Option<i64>,
    /// What to charge the customer; 0 for none.
    #[serde(default)]
    pub customer_fee_cents: i64,
    pub customer_fee_account_id: Option<i64>,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BouncedCheckView {
    pub id: i64,
    pub bounce_date: String,
    pub customer_name: Option<String>,
    pub invoice_number: Option<i64>,
    pub reference: Option<String>,
    pub bank_account_id: i64,
    pub amount_cents: i64,
    pub bank_fee_cents: i64,
    pub fee_invoice_number: Option<i64>,
    pub memo: Option<String>,
    pub journal_entry_id: i64,
    pub is_voided: bool,
}

fn clean(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|s| !s.is_empty())
}

async fn run<T>(pool: &SqlitePool, work: impl for<'t> AsyncFnOnce(&mut Transaction<'t, Sqlite>) -> Result<T, String>) -> Result<T, String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    match work(&mut tx).await {
        Ok(value) => crate::ledger::commit_write(tx).await.map(|()| value).map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// Every deposited invoice payment (straight to a bank account, or banked through a deposit) and
/// every "other funds" deposit line that hasn't bounced yet, newest first. The bank account is
/// where the money is now.
const CANDIDATES: &str = "
    SELECT 'PAYMENT' AS kind, p.id, p.payment_date AS date, COALESCE(c.name, i.customer_name) AS customer_name,
           i.invoice_number, p.payment_method, p.reference, NULL AS memo, p.amount_cents,
           COALESCE(d.bank_account_id, p.deposit_account_id) AS bank_account_id
    FROM invoice_payments p
    JOIN invoices i ON i.id = p.invoice_id
    LEFT JOIN customers c ON c.id = i.customer_id
    JOIN chart_of_accounts pa ON pa.id = p.deposit_account_id
    LEFT JOIN deposit_lines dl ON dl.invoice_payment_id = p.id AND dl.is_voided = 0
    LEFT JOIN deposits d ON d.id = dl.deposit_id
    WHERE p.is_voided = 0 AND p.journal_entry_id IS NOT NULL AND i.is_voided = 0 AND i.currency_code IS NULL
      AND (pa.account_type = 'BANK' OR d.id IS NOT NULL)
      AND NOT EXISTS (SELECT 1 FROM bounced_checks b WHERE b.invoice_payment_id = p.id AND b.is_voided = 0)
    UNION ALL
    SELECT 'DEPOSIT_LINE', dl.id, d.deposit_date, NULL, NULL, NULL, NULL, COALESCE(dl.memo, d.memo), dl.amount_cents, d.bank_account_id
    FROM deposit_lines dl JOIN deposits d ON d.id = dl.deposit_id
    WHERE dl.is_voided = 0 AND d.is_voided = 0 AND d.journal_entry_id IS NOT NULL
      AND dl.invoice_payment_id IS NULL AND dl.sales_receipt_id IS NULL
      AND NOT EXISTS (SELECT 1 FROM bounced_checks b WHERE b.deposit_line_id = dl.id AND b.is_voided = 0)";

pub async fn candidates(pool: &SqlitePool) -> Result<Vec<BounceCandidate>, String> {
    sqlx::query_as::<_, BounceCandidate>(&format!("SELECT * FROM ({CANDIDATES}) ORDER BY date DESC, kind, id DESC"))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

const VIEW_SELECT: &str = "
    SELECT b.id, b.bounce_date, c.name AS customer_name, i.invoice_number, p.reference, b.bank_account_id, b.amount_cents,
           b.bank_fee_cents, fi.invoice_number AS fee_invoice_number, b.memo, b.journal_entry_id, b.is_voided
    FROM bounced_checks b
    LEFT JOIN customers c ON c.id = b.customer_id
    LEFT JOIN invoices i ON i.id = b.invoice_id
    LEFT JOIN invoice_payments p ON p.id = b.invoice_payment_id
    LEFT JOIN invoices fi ON fi.id = b.fee_invoice_id";

pub async fn list(pool: &SqlitePool) -> Result<Vec<BouncedCheckView>, String> {
    sqlx::query_as::<_, BouncedCheckView>(&format!("{VIEW_SELECT} ORDER BY b.is_voided, b.bounce_date DESC, b.id DESC"))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// What bounced, resolved inside the write transaction.
struct Source {
    date: NaiveDate,
    amount: i64,
    bank: i64,
    /// The account the bounce debits: A/R for a payment, the line's own account for other funds.
    debit_account: i64,
    payment_id: Option<i64>,
    deposit_line_id: Option<i64>,
    invoice: Option<(i64, i64)>,
    customer: Option<(i64, String)>,
    label: String,
}

async fn resolve(tx: &mut Transaction<'_, Sqlite>, kind: &str, id: i64) -> Result<Source, String> {
    let row: Option<(String, NaiveDate, i64, i64)> = sqlx::query_as(&format!("SELECT kind, date, amount_cents, bank_account_id FROM ({CANDIDATES}) WHERE kind = ?1 AND id = ?2"))
        .bind(kind)
        .bind(id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    let Some((_, date, amount, bank)) = row else {
        return Err(match kind {
            "PAYMENT" | "DEPOSIT_LINE" => {
                "BOUNCE_UNAVAILABLE: that payment can't be recorded as bounced: it is voided, already bounced, still in Undeposited Funds (void it instead) or in a foreign currency".into()
            }
            other => format!("BOUNCE_BAD_KIND: unknown kind '{other}'"),
        });
    };
    if kind == "PAYMENT" {
        let (invoice_id, number, customer_id, customer, reference): (i64, i64, Option<i64>, String, Option<String>) = sqlx::query_as(
            "SELECT i.id, i.invoice_number, i.customer_id, COALESCE(c.name, i.customer_name), p.reference
             FROM invoice_payments p JOIN invoices i ON i.id = p.invoice_id LEFT JOIN customers c ON c.id = i.customer_id
             WHERE p.id = ?1",
        )
        .bind(id)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)?;
        let ar = system_account(tx, "ACCOUNTS_RECEIVABLE").await?;
        let check = clean(reference.as_deref()).map(|r| format!(" #{r}")).unwrap_or_default();
        Ok(Source {
            date,
            amount,
            bank,
            debit_account: ar,
            payment_id: Some(id),
            deposit_line_id: None,
            invoice: Some((invoice_id, number)),
            label: format!("Bounced check{check} - {customer} (invoice #{number})"),
            customer: customer_id.map(|c| (c, customer)),
        })
    } else {
        let (account, memo): (i64, Option<String>) = sqlx::query_as("SELECT account_id, memo FROM deposit_lines WHERE id = ?1")
            .bind(id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_err)?;
        Ok(Source {
            date,
            amount,
            bank,
            debit_account: account,
            payment_id: None,
            deposit_line_id: Some(id),
            invoice: None,
            customer: None,
            label: clean(memo.as_deref()).map_or_else(|| "Bounced deposit".to_string(), |m| format!("Bounced deposit - {m}")),
        })
    }
}

async fn system_account(tx: &mut Transaction<'_, Sqlite>, account_type: &str) -> Result<i64, String> {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = ?1 AND is_system = 1 ORDER BY id LIMIT 1")
        .bind(account_type)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("COA_SYSTEM_ACCOUNT_MISSING: no system {account_type} account"))
}

/// The account's classification when it exists and is active.
async fn classification(tx: &mut Transaction<'_, Sqlite>, account_id: i64) -> Result<Option<String>, String> {
    sqlx::query_scalar(
        "SELECT t.classification FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1 AND a.is_active = 1",
    )
    .bind(account_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)
}

/// Records a bounced check; returns it.
pub async fn record(pool: &SqlitePool, input: BounceInput) -> Result<BouncedCheckView, String> {
    let id = run(pool, async |tx| write(tx, &input).await).await?;
    sqlx::query_as::<_, BouncedCheckView>(&format!("{VIEW_SELECT} WHERE b.id = ?1")).bind(id).fetch_one(pool).await.map_err(db_err)
}

async fn write(tx: &mut Transaction<'_, Sqlite>, input: &BounceInput) -> Result<i64, String> {
    if !(0..=MAX_LINE_CENTS).contains(&input.bank_fee_cents) || !(0..=MAX_LINE_CENTS).contains(&input.customer_fee_cents) {
        return Err("BOUNCE_BAD_FEE: fees can't be negative".into());
    }
    let source = resolve(tx, &input.kind, input.id).await?;
    if input.bounce_date < source.date {
        return Err(format!("BOUNCE_BEFORE_DEPOSIT: the check bounced on or after {}, when it was deposited", source.date));
    }

    let mut lines = vec![JournalLine::debit(source.debit_account, source.amount).with_memo(Some(&source.label))];
    let bank_fee_account = if input.bank_fee_cents > 0 {
        let account = input.bank_fee_account_id.ok_or("BOUNCE_FEE_ACCOUNT: choose the expense account for the bank's fee")?;
        if classification(tx, account).await?.as_deref() != Some("EXPENSE") {
            return Err("BOUNCE_FEE_ACCOUNT: the bank's fee goes to an active expense account".into());
        }
        lines.push(JournalLine::debit(account, input.bank_fee_cents).with_memo(Some("Bank fee for returned check")));
        Some(account)
    } else {
        None
    };
    let total = source
        .amount
        .checked_add(input.bank_fee_cents)
        .filter(|t| *t <= MAX_LINE_CENTS)
        .ok_or_else(|| format!("BOUNCE_TOO_LARGE: the amount and fee exceed {}", format_cents(MAX_LINE_CENTS)))?;
    lines.push(JournalLine::credit(source.bank, total));

    let memo = clean(input.memo.as_deref());
    let description = match memo {
        Some(m) => format!("{}: {m}", source.label),
        None => source.label.clone(),
    };
    let entry = JournalEntry { id: None, date: input.bounce_date, description, lines, currency: None };
    let entry_id = post_entry_in_tx(tx, &entry, EntrySource::Journal).await?;

    let fee_invoice = if input.customer_fee_cents > 0 {
        let (_, customer) = source.customer.as_ref().ok_or("BOUNCE_FEE_NO_CUSTOMER: a fee can only be charged when the check came from a customer")?;
        let account = input.customer_fee_account_id.ok_or("BOUNCE_FEE_ACCOUNT: choose the income account for the customer's fee")?;
        let invoice = InvoiceInput {
            customer_name: customer.clone(),
            issue_date: input.bounce_date,
            due_date: input.bounce_date,
            lines: vec![InvoiceLineInput {
                description: "Returned check charge".into(),
                quantity_e4: QUANTITY_SCALE,
                rate_cents: input.customer_fee_cents,
                income_account_id: account,
                product_service_id: None,
                class_id: None,
                is_taxable: false,
            }],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        };
        Some(crate::invoices::write_invoice(tx, &invoice).await?)
    } else {
        None
    };

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO bounced_checks
            (invoice_payment_id, deposit_line_id, invoice_id, customer_id, bank_account_id, bounce_date, amount_cents,
             bank_fee_cents, bank_fee_account_id, fee_invoice_id, memo, journal_entry_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) RETURNING id",
    )
    .bind(source.payment_id)
    .bind(source.deposit_line_id)
    .bind(source.invoice.map(|i| i.0))
    .bind(source.customer.as_ref().map(|c| c.0))
    .bind(source.bank)
    .bind(input.bounce_date)
    .bind(source.amount)
    .bind(input.bank_fee_cents)
    .bind(bank_fee_account)
    .bind(fee_invoice)
    .bind(memo)
    .bind(entry_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(|e| {
        let msg = db_err(e);
        if msg.contains("UNIQUE") { "BOUNCE_ALREADY: that check is already recorded as bounced".into() } else { msg }
    })?;

    if let Some((invoice_id, _)) = source.invoice {
        crate::ledger::recompute_paid(tx, "invoices", "invoice_payments", "invoice_id", invoice_id).await?;
    }
    Ok(id)
}

/// Voids a bounced check: its entry is reversed, the payment counts toward the invoice again, and
/// the fee invoice (if any) is voided too. Refused if the fee invoice has been paid.
pub async fn void(pool: &SqlitePool, id: i64) -> Result<(), String> {
    run(pool, async |tx| {
        let row: Option<(i64, bool, Option<i64>)> = sqlx::query_as("SELECT journal_entry_id, is_voided, fee_invoice_id FROM bounced_checks WHERE id = ?1")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
        let (entry, voided, fee_invoice) = row.ok_or("BOUNCE_NOT_FOUND: that bounced check does not exist")?;
        if voided {
            return Err("BOUNCE_ALREADY_VOIDED: that bounced check is already voided".into());
        }
        crate::ledger::void_in_tx(tx, entry).await?;
        if let Some(invoice) = fee_invoice {
            let live: Option<i64> = sqlx::query_scalar("SELECT journal_entry_id FROM invoices WHERE id = ?1 AND is_voided = 0")
                .bind(invoice)
                .fetch_optional(&mut **tx)
                .await
                .map_err(db_err)?
                .flatten();
            if let Some(fee_entry) = live {
                crate::ledger::void_in_tx(tx, fee_entry).await?;
            }
        }
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests;
