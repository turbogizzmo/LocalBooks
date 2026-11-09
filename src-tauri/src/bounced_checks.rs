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
//! A foreign-currency payment bounces at the rates it was booked at: the bank gives back the home
//! amount it received, A/R gets back what the payment relieved, and the realized exchange gain or
//! loss the payment posted is reversed.
//!
//! A sales receipt has no invoice to reopen, so its bounce bills the customer: the entry takes the
//! money out of the bank against the receipt's income account, and a charge-back invoice (written
//! in the same transaction) puts that income back and the amount in the customer's A/R, with the
//! returned-check charge as a second line. Voiding the bounce voids the invoice too.
//!
//! Only money that reached the bank can bounce: a payment still in Undeposited Funds is voided
//! instead.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::invoices::{InvoiceInput, InvoiceLineInput};
use crate::ledger::{db_err, format_cents, post_entry_in_tx, rollback, EntrySource, JournalEntry, JournalLine, MAX_LINE_CENTS, QUANTITY_SCALE};

/// A deposited payment or deposit line that can be recorded as bounced.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BounceCandidate {
    /// `PAYMENT` (an invoice payment), `SALES_RECEIPT` or `DEPOSIT_LINE` (other funds on a deposit).
    pub kind: String,
    pub id: i64,
    pub date: String,
    pub customer_name: Option<String>,
    pub invoice_number: Option<i64>,
    pub payment_method: Option<String>,
    /// The Payment Method list entry's name, when the payment has one.
    pub payment_method_name: Option<String>,
    pub reference: Option<String>,
    pub memo: Option<String>,
    /// In the invoice's currency for a foreign-currency payment (see `currency_code`).
    pub amount_cents: i64,
    pub bank_account_id: i64,
    /// `None` for home currency.
    pub currency_code: Option<String>,
    /// What left the bank, in home currency.
    pub home_amount_cents: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BounceInput {
    /// `PAYMENT`, `SALES_RECEIPT` or `DEPOSIT_LINE`, as listed by [`candidates`].
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
    /// `CHECK` (a payment or deposit line) or `SALES_RECEIPT`; ids are per kind.
    pub kind: String,
    pub id: i64,
    pub bounce_date: String,
    pub customer_name: Option<String>,
    pub invoice_number: Option<i64>,
    pub reference: Option<String>,
    pub bank_account_id: i64,
    pub amount_cents: i64,
    pub bank_fee_cents: i64,
    /// The fee invoice, or for a sales receipt the invoice that charged it back.
    pub fee_invoice_number: Option<i64>,
    pub receipt_number: Option<i64>,
    /// `None` for home currency; `amount_cents` is then home currency.
    pub currency_code: Option<String>,
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
/// where the money is now, and the date is when it got there (a check can't bounce before that).
const CANDIDATES: &str = "
    SELECT 'PAYMENT' AS kind, p.id, COALESCE(d.deposit_date, p.payment_date) AS date, COALESCE(c.name, i.customer_name) AS customer_name,
           i.invoice_number, p.payment_method, (SELECT name FROM payment_methods WHERE id = p.payment_method_id) AS payment_method_name,
           p.reference, NULL AS memo, p.amount_cents,
           COALESCE(d.bank_account_id, p.deposit_account_id) AS bank_account_id,
           i.currency_code, COALESCE(p.home_amount_cents, p.amount_cents - p.discount_cents) AS home_amount_cents
    FROM invoice_payments p
    JOIN invoices i ON i.id = p.invoice_id
    LEFT JOIN customers c ON c.id = i.customer_id
    JOIN chart_of_accounts pa ON pa.id = p.deposit_account_id
    LEFT JOIN deposit_lines dl ON dl.invoice_payment_id = p.id AND dl.is_voided = 0
    LEFT JOIN deposits d ON d.id = dl.deposit_id
    WHERE p.is_voided = 0 AND p.journal_entry_id IS NOT NULL AND i.is_voided = 0
      AND (pa.account_type = 'BANK' OR d.id IS NOT NULL)
      AND NOT EXISTS (SELECT 1 FROM bounced_checks b WHERE b.invoice_payment_id = p.id AND b.is_voided = 0)
    UNION ALL
    SELECT 'SALES_RECEIPT', r.id, COALESCE(d.deposit_date, r.receipt_date), c.name, NULL, r.payment_method,
           (SELECT name FROM payment_methods WHERE id = r.payment_method_id), r.reference, r.memo, r.total_cents,
           COALESCE(d.bank_account_id, r.deposit_account_id), NULL, r.total_cents
    FROM sales_receipts r
    JOIN customers c ON c.id = r.customer_id
    JOIN chart_of_accounts ra ON ra.id = r.deposit_account_id
    LEFT JOIN deposit_lines dl ON dl.sales_receipt_id = r.id AND dl.is_voided = 0
    LEFT JOIN deposits d ON d.id = dl.deposit_id
    WHERE r.is_voided = 0 AND r.journal_entry_id IS NOT NULL
      AND (ra.account_type = 'BANK' OR d.id IS NOT NULL)
      AND NOT EXISTS (SELECT 1 FROM bounced_sales_receipts b WHERE b.sales_receipt_id = r.id AND b.is_voided = 0)
    UNION ALL
    SELECT 'DEPOSIT_LINE', dl.id, d.deposit_date, NULL, NULL, NULL, NULL, NULL, COALESCE(dl.memo, d.memo), dl.amount_cents, d.bank_account_id,
           NULL, dl.amount_cents
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
    SELECT * FROM (
    SELECT 'CHECK' AS kind, b.id, b.bounce_date, c.name AS customer_name, i.invoice_number, p.reference, b.bank_account_id, b.amount_cents,
           b.bank_fee_cents, fi.invoice_number AS fee_invoice_number, NULL AS receipt_number, i.currency_code, b.memo, b.journal_entry_id, b.is_voided
    FROM bounced_checks b
    LEFT JOIN customers c ON c.id = b.customer_id
    LEFT JOIN invoices i ON i.id = b.invoice_id
    LEFT JOIN invoice_payments p ON p.id = b.invoice_payment_id
    LEFT JOIN invoices fi ON fi.id = b.fee_invoice_id
    UNION ALL
    SELECT 'SALES_RECEIPT', b.id, b.bounce_date, c.name, NULL, r.reference, b.bank_account_id, b.amount_cents,
           b.bank_fee_cents, ci.invoice_number, r.receipt_number, NULL, b.memo, b.journal_entry_id, b.is_voided
    FROM bounced_sales_receipts b
    JOIN customers c ON c.id = b.customer_id
    JOIN sales_receipts r ON r.id = b.sales_receipt_id
    JOIN invoices ci ON ci.id = b.charge_invoice_id)";

pub async fn list(pool: &SqlitePool) -> Result<Vec<BouncedCheckView>, String> {
    sqlx::query_as::<_, BouncedCheckView>(&format!("{VIEW_SELECT} ORDER BY is_voided, bounce_date DESC, id DESC"))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// What bounced, resolved inside the write transaction.
struct Source {
    date: NaiveDate,
    /// The bounce's own amount: the payment's (in its invoice's currency) or the receipt's.
    amount: i64,
    /// What left the bank, in home currency.
    bank_cents: i64,
    bank: i64,
    /// Every line of the bounce but the bank's: what it debits (A/R for a payment, the receipt's
    /// income account, a deposit line's own account), a discount taken back, any exchange
    /// difference. They net to `bank_cents`, which the bank is credited.
    lines: Vec<JournalLine>,
    payment_id: Option<i64>,
    receipt_id: Option<i64>,
    deposit_line_id: Option<i64>,
    invoice: Option<(i64, i64)>,
    customer: Option<(i64, String)>,
    /// For a sales receipt: (receipt number, income account the charge-back invoice credits).
    receipt: Option<(i64, i64)>,
    label: String,
}

async fn resolve(tx: &mut Transaction<'_, Sqlite>, kind: &str, id: i64) -> Result<Source, String> {
    let row: Option<(NaiveDate, i64, i64, i64)> =
        sqlx::query_as(&format!("SELECT date, amount_cents, bank_account_id, home_amount_cents FROM ({CANDIDATES}) WHERE kind = ?1 AND id = ?2"))
            .bind(kind)
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
    let Some((date, amount, bank, bank_cents)) = row else {
        return Err(match kind {
            "PAYMENT" | "DEPOSIT_LINE" | "SALES_RECEIPT" => {
                "BOUNCE_UNAVAILABLE: that payment can't be recorded as bounced: it is voided, already bounced, still in Undeposited Funds (void it instead), or is a sales receipt with no customer".into()
            }
            other => format!("BOUNCE_BAD_KIND: unknown kind '{other}'"),
        });
    };
    match kind {
        "PAYMENT" => {
            type PaymentRow = (i64, i64, Option<i64>, String, Option<String>, Option<i64>, Option<String>, i64, Option<i64>);
            let (invoice_id, number, customer_id, customer, reference, ar_relief, currency, discount, discount_account): PaymentRow =
                sqlx::query_as(
                    "SELECT i.id, i.invoice_number, i.customer_id, COALESCE(c.name, i.customer_name), p.reference, p.ar_relief_cents, i.currency_code,
                            p.discount_cents,
                            (SELECT l.account_id FROM journal_lines l WHERE l.journal_entry_id = p.journal_entry_id AND p.discount_cents > 0
                               AND l.debit_cents = p.discount_cents AND l.account_id <> p.deposit_account_id ORDER BY l.line_number LIMIT 1)
                     FROM invoice_payments p JOIN invoices i ON i.id = p.invoice_id LEFT JOIN customers c ON c.id = i.customer_id
                     WHERE p.id = ?1",
                )
                .bind(id)
                .fetch_one(&mut **tx)
                .await
                .map_err(db_err)?;
            let ar = system_account(tx, "ACCOUNTS_RECEIVABLE").await?;
            let check = clean(reference.as_deref()).map(|r| format!(" #{r}")).unwrap_or_default();
            let currency_note = currency.map(|c| format!(" ({c})")).unwrap_or_default();
            let label = format!("Bounced check{check} - {customer} (invoice #{number}){currency_note}");
            // A/R gets back what the payment relieved; the payment's realized exchange difference
            // (the bank's amount less that) is reversed.
            let ar_cents = ar_relief.unwrap_or(amount);
            let mut lines = Vec::with_capacity(3);
            if ar_cents > 0 {
                lines.push(JournalLine::debit(ar, ar_cents).with_memo(Some(&label)));
            }
            // An early-payment discount the payment took is reversed: the invoice is open again for
            // all of it, and the customer has to pay in full.
            if discount > 0 {
                let account = discount_account.ok_or("BOUNCE_NO_DISCOUNT_LINE: the payment's discount line could not be found")?;
                lines.push(JournalLine::credit(account, discount).with_memo(Some("Early-payment discount reversed")));
            }
            let difference = bank_cents + discount - ar_cents;
            if difference != 0 {
                let fx = system_account(tx, "OTHER_EXPENSE").await?;
                let memo = Some(if difference > 0 { "Realized exchange gain reversed" } else { "Realized exchange loss reversed" });
                lines.push(if difference > 0 { JournalLine::debit(fx, difference).with_memo(memo) } else { JournalLine::credit(fx, -difference).with_memo(memo) });
            }
            Ok(Source {
                date,
                amount,
                bank_cents,
                bank,
                lines,
                payment_id: Some(id),
                receipt_id: None,
                deposit_line_id: None,
                invoice: Some((invoice_id, number)),
                customer: customer_id.map(|c| (c, customer)),
                receipt: None,
                label,
            })
        }
        "SALES_RECEIPT" => {
            let (number, customer_id, customer, reference, deposit_line): (i64, i64, String, Option<String>, Option<i64>) = sqlx::query_as(
                "SELECT r.receipt_number, r.customer_id, c.name, r.reference,
                        (SELECT dl.id FROM deposit_lines dl WHERE dl.sales_receipt_id = r.id AND dl.is_voided = 0)
                 FROM sales_receipts r JOIN customers c ON c.id = r.customer_id WHERE r.id = ?1",
            )
            .bind(id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_err)?;
            let income: Option<i64> = sqlx::query_scalar(
                "SELECT l.income_account_id FROM sales_receipt_lines l
                 JOIN chart_of_accounts a ON a.id = l.income_account_id JOIN account_types t ON t.code = a.account_type
                 WHERE l.sales_receipt_id = ?1 AND t.classification = 'INCOME' AND a.is_active = 1
                 ORDER BY l.line_number LIMIT 1",
            )
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
            let income = income.ok_or("BOUNCE_NO_INCOME_ACCOUNT: this sales receipt has no line with an active income account to charge back")?;
            let check = clean(reference.as_deref()).map(|r| format!(" #{r}")).unwrap_or_default();
            let label = format!("Bounced check{check} - {customer} (sales receipt #{number})");
            Ok(Source {
                date,
                amount,
                bank_cents,
                bank,
                lines: vec![JournalLine::debit(income, amount).with_memo(Some(&label))],
                payment_id: None,
                receipt_id: Some(id),
                deposit_line_id: deposit_line,
                invoice: None,
                customer: Some((customer_id, customer)),
                receipt: Some((number, income)),
                label,
            })
        }
        _ => {
            let (account, memo): (i64, Option<String>) = sqlx::query_as("SELECT account_id, memo FROM deposit_lines WHERE id = ?1")
                .bind(id)
                .fetch_one(&mut **tx)
                .await
                .map_err(db_err)?;
            let label = clean(memo.as_deref()).map_or_else(|| "Bounced deposit".to_string(), |m| format!("Bounced deposit - {m}"));
            Ok(Source {
                date,
                amount,
                bank_cents,
                bank,
                lines: vec![JournalLine::debit(account, amount).with_memo(Some(&label))],
                payment_id: None,
                receipt_id: None,
                deposit_line_id: Some(id),
                invoice: None,
                customer: None,
                receipt: None,
                label,
            })
        }
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
    let (kind, id) = run(pool, async |tx| write(tx, &input).await).await?;
    sqlx::query_as::<_, BouncedCheckView>(&format!("{VIEW_SELECT} WHERE kind = ?1 AND id = ?2")).bind(kind).bind(id).fetch_one(pool).await.map_err(db_err)
}

async fn write(tx: &mut Transaction<'_, Sqlite>, input: &BounceInput) -> Result<(&'static str, i64), String> {
    if !(0..=MAX_LINE_CENTS).contains(&input.bank_fee_cents) || !(0..=MAX_LINE_CENTS).contains(&input.customer_fee_cents) {
        return Err("BOUNCE_BAD_FEE: fees can't be negative".into());
    }
    let source = resolve(tx, &input.kind, input.id).await?;
    if input.bounce_date < source.date {
        return Err(format!("BOUNCE_BEFORE_DEPOSIT: the check bounced on or after {}, when it was deposited", source.date));
    }

    let mut lines = source.lines.clone();
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
        .bank_cents
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

    let fee_line = |account: i64| InvoiceLineInput {
        description: "Returned check charge".into(),
        quantity_e4: QUANTITY_SCALE,
        rate_cents: input.customer_fee_cents,
        income_account_id: account,
        product_service_id: None,
        class_id: None,
        is_taxable: false,
    };
    let invoice_for = |customer: &str, lines: Vec<InvoiceLineInput>| InvoiceInput {
        customer_name: customer.to_string(),
        issue_date: input.bounce_date,
        due_date: input.bounce_date,
        lines,
        currency: None,
        tax_rate_id: None,
        discount_cents: 0,
        details: Default::default(),
    };

    if let (Some(receipt_id), Some((number, income)), Some((customer_id, customer))) = (source.receipt_id, source.receipt, source.customer.as_ref()) {
        // The charge-back: the receipt's amount back in the customer's A/R, and any fee.
        let mut charge = vec![InvoiceLineInput {
            description: format!("Returned check: sales receipt #{number}"),
            quantity_e4: QUANTITY_SCALE,
            rate_cents: source.amount,
            income_account_id: income,
            product_service_id: None,
            class_id: None,
            is_taxable: false,
        }];
        if input.customer_fee_cents > 0 {
            charge.push(fee_line(input.customer_fee_account_id.ok_or("BOUNCE_FEE_ACCOUNT: choose the income account for the customer's fee")?));
        }
        let invoice = crate::invoices::write_invoice(tx, &invoice_for(customer, charge)).await?;
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO bounced_sales_receipts
                (sales_receipt_id, deposit_line_id, customer_id, bank_account_id, bounce_date, amount_cents,
                 bank_fee_cents, bank_fee_account_id, charge_invoice_id, memo, journal_entry_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) RETURNING id",
        )
        .bind(receipt_id)
        .bind(source.deposit_line_id)
        .bind(customer_id)
        .bind(source.bank)
        .bind(input.bounce_date)
        .bind(source.amount)
        .bind(input.bank_fee_cents)
        .bind(bank_fee_account)
        .bind(invoice)
        .bind(memo)
        .bind(entry_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(|e| {
            let msg = db_err(e);
            if msg.contains("UNIQUE") { "BOUNCE_ALREADY: that check is already recorded as bounced".into() } else { msg }
        })?;
        return Ok(("SALES_RECEIPT", id));
    }

    let fee_invoice = if input.customer_fee_cents > 0 {
        let (_, customer) = source.customer.as_ref().ok_or("BOUNCE_FEE_NO_CUSTOMER: a fee can only be charged when the check came from a customer")?;
        let account = input.customer_fee_account_id.ok_or("BOUNCE_FEE_ACCOUNT: choose the income account for the customer's fee")?;
        Some(crate::invoices::write_invoice(tx, &invoice_for(customer, vec![fee_line(account)])).await?)
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
    Ok(("CHECK", id))
}

/// Voids a bounced check: its entry is reversed, the payment counts toward the invoice again, and
/// the fee invoice (if any) is voided too. A bounced sales receipt's charge-back invoice is voided
/// first. Refused if that invoice has been paid. `kind` is the view's (`CHECK` when `None`).
pub async fn void(pool: &SqlitePool, id: i64, kind: Option<&str>) -> Result<(), String> {
    let receipt = match kind.unwrap_or("CHECK") {
        "CHECK" => false,
        "SALES_RECEIPT" => true,
        other => return Err(format!("BOUNCE_BAD_KIND: unknown kind '{other}'")),
    };
    let table = if receipt { "bounced_sales_receipts" } else { "bounced_checks" };
    let invoice_column = if receipt { "charge_invoice_id" } else { "fee_invoice_id" };
    run(pool, async |tx| {
        let row: Option<(i64, bool, Option<i64>)> = sqlx::query_as(&format!("SELECT journal_entry_id, is_voided, {invoice_column} FROM {table} WHERE id = ?1"))
            .bind(id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
        let (entry, voided, invoice) = row.ok_or("BOUNCE_NOT_FOUND: that bounced check does not exist")?;
        if voided {
            return Err("BOUNCE_ALREADY_VOIDED: that bounced check is already voided".into());
        }
        let invoice_entry: Option<i64> = match invoice {
            Some(invoice) => sqlx::query_scalar("SELECT journal_entry_id FROM invoices WHERE id = ?1 AND is_voided = 0")
                .bind(invoice)
                .fetch_optional(&mut **tx)
                .await
                .map_err(db_err)?
                .flatten(),
            None => None,
        };
        // The charge-back goes first: voiding the bounce's entry while it is live is refused.
        if receipt {
            if let Some(invoice_entry) = invoice_entry {
                crate::ledger::void_in_tx(tx, invoice_entry).await?;
            }
        }
        crate::ledger::void_in_tx(tx, entry).await?;
        if !receipt {
            if let Some(invoice_entry) = invoice_entry {
                crate::ledger::void_in_tx(tx, invoice_entry).await?;
            }
        }
        Ok(())
    })
    .await
}

#[cfg(test)]
mod tests;
