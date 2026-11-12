//! Bank deposits and transfers.
//!
//! A deposit moves customer payments out of Undeposited Funds into a bank account and may add other
//! funds: Dr bank / Cr Undeposited Funds (per selected payment) and Cr each other line's account.
//! A transfer moves money between balance-sheet accounts: Dr to-account / Cr from-account.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{db_err, format_cents, post_entry_in_tx, rollback, EntrySource, JournalEntry, JournalLine, MAX_LINE_CENTS};
use crate::money::Cents;

fn clean(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|s| !s.is_empty())
}

/// A received payment sitting in Undeposited Funds, waiting to be deposited.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UndepositedItem {
    /// `PAYMENT` (invoice payment), `SALES_RECEIPT` or `CREDIT` (an unapplied payment)
    pub kind: String,
    pub id: i64,
    pub date: String,
    pub customer_name: Option<String>,
    pub payment_method: String,
    /// The Payment Method list entry's name, when the payment has one.
    pub payment_method_name: Option<String>,
    pub reference: Option<String>,
    pub amount_cents: i64,
    pub undeposited_account_id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositItemRef {
    /// `PAYMENT`, `SALES_RECEIPT` or `CREDIT` (an unapplied payment)
    pub kind: String,
    pub id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositOtherLine {
    pub account_id: i64,
    pub amount_cents: Cents,
    pub memo: Option<String>,
    #[serde(default)]
    pub class_id: Option<i64>,
    /// Received From: the customer the money came from (at most one of this and `vendor_id`).
    #[serde(default)]
    pub customer_id: Option<i64>,
    /// Received From: the vendor the money came from (a refund, say).
    #[serde(default)]
    pub vendor_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositInput {
    pub bank_account_id: i64,
    pub deposit_date: NaiveDate,
    pub memo: Option<String>,
    pub items: Vec<DepositItemRef>,
    pub other_lines: Vec<DepositOtherLine>,
    /// Part of the deposit kept as cash rather than banked.
    #[serde(default)]
    pub cash_back: Option<CashBack>,
    /// Amounts taken out before the money reaches the bank (a processor's fees, refunds and
    /// chargebacks on a payout): each a debit to its account, out of what the bank gets.
    #[serde(default)]
    pub deductions: Vec<DepositDeduction>,
}

/// A deduction from a deposit: `amount_cents` debited to `account_id`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositDeduction {
    pub account_id: i64,
    pub amount_cents: Cents,
    pub memo: Option<String>,
    /// `FEE`, `REFUND`, `CHARGEBACK` or `OTHER`.
    pub kind: String,
}

/// Cash back on a deposit: posted to `account_id` (petty cash, say), out of what the bank gets.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CashBack {
    pub account_id: i64,
    pub amount_cents: Cents,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferInput {
    pub from_account_id: i64,
    pub to_account_id: i64,
    pub transfer_date: NaiveDate,
    pub amount_cents: Cents,
    pub memo: Option<String>,
}

/// One row of the Deposits & Transfers register.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BankingActivity {
    /// `DEPOSIT` or `TRANSFER`
    pub kind: String,
    pub id: i64,
    pub date: String,
    pub account_id: i64,
    pub other_account_id: Option<i64>,
    pub memo: Option<String>,
    pub line_count: i64,
    pub amount_cents: i64,
    pub journal_entry_id: Option<i64>,
    pub is_voided: bool,
}

/// What a deposited payment line comes from.
#[derive(Debug, Clone, Copy)]
enum DepositSource {
    Payment(i64),
    Receipt(i64),
    /// An unapplied payment (a customer credit with a payment account).
    Credit(i64),
}

pub async fn list_undeposited(pool: &SqlitePool) -> Result<Vec<UndepositedItem>, String> {
    sqlx::query_as::<_, UndepositedItem>(
        "SELECT 'PAYMENT' AS kind, p.id AS id, p.payment_date AS date, i.customer_name, p.payment_method,
                (SELECT name FROM payment_methods WHERE id = p.payment_method_id) AS payment_method_name, p.reference,
                COALESCE(p.home_amount_cents, p.amount_cents - p.discount_cents) AS amount_cents, p.deposit_account_id AS undeposited_account_id
         FROM invoice_payments p
         JOIN invoices i ON i.id = p.invoice_id
         JOIN chart_of_accounts a ON a.id = p.deposit_account_id
         WHERE a.is_system = 1 AND a.account_type = 'OTHER_CURRENT_ASSET'
           AND p.is_voided = 0 AND p.journal_entry_id IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM deposit_lines d WHERE d.invoice_payment_id = p.id AND d.is_voided = 0)
           AND NOT EXISTS (SELECT 1 FROM undeposited_fund_matches m WHERE m.invoice_payment_id = p.id AND m.is_voided = 0)
           AND NOT EXISTS (SELECT 1 FROM undeposited_transfers t WHERE t.invoice_payment_id = p.id AND t.is_voided = 0)
         UNION ALL
         SELECT 'SALES_RECEIPT', r.id, r.receipt_date, c.name, r.payment_method,
                (SELECT name FROM payment_methods WHERE id = r.payment_method_id), r.reference,
                r.total_cents, r.deposit_account_id
         FROM sales_receipts r
         LEFT JOIN customers c ON c.id = r.customer_id
         JOIN chart_of_accounts a ON a.id = r.deposit_account_id
         WHERE a.is_system = 1 AND a.account_type = 'OTHER_CURRENT_ASSET'
           AND r.is_voided = 0 AND r.journal_entry_id IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM deposit_lines d WHERE d.sales_receipt_id = r.id AND d.is_voided = 0)
           AND NOT EXISTS (SELECT 1 FROM undeposited_fund_matches m WHERE m.sales_receipt_id = r.id AND m.is_voided = 0)
           AND NOT EXISTS (SELECT 1 FROM undeposited_transfers t WHERE t.sales_receipt_id = r.id AND t.is_voided = 0)
         UNION ALL
         SELECT 'CREDIT', m.id, m.credit_date, c.name, m.payment_method,
                (SELECT name FROM payment_methods WHERE id = m.payment_method_id), m.payment_reference,
                m.total_cents, m.payment_account_id
         FROM credit_memos m
         JOIN customers c ON c.id = m.customer_id
         JOIN chart_of_accounts a ON a.id = m.payment_account_id
         WHERE a.is_system = 1 AND a.account_type = 'OTHER_CURRENT_ASSET'
           AND m.is_voided = 0 AND m.journal_entry_id IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM deposit_lines d WHERE d.credit_memo_id = m.id AND d.is_voided = 0)
         ORDER BY date, kind, id",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn post_bank_deposit(pool: &SqlitePool, input: DepositInput) -> Result<i64, String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    match write_deposit(&mut tx, &input).await {
        Ok((deposit_id, _)) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            Ok(deposit_id)
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// Writes and posts a deposit inside the caller's transaction; returns (deposit id, entry id).
/// Bank feed approvals use this so an approved inflow is a real Deposit document.
pub(crate) async fn write_deposit(tx: &mut Transaction<'_, Sqlite>, input: &DepositInput) -> Result<(i64, i64), String> {
    if input.items.is_empty() && input.other_lines.is_empty() {
        return Err("DEP_EMPTY: select payments to deposit or add other funds".into());
    }

    // Resolve the selected payments: (which document, undeposited account, amount, label).
    let mut resolved: Vec<(DepositSource, i64, i64, String)> = Vec::with_capacity(input.items.len());
    for item in &input.items {
        let row: Option<(i64, i64, Option<String>)> = match item.kind.as_str() {
            "PAYMENT" => sqlx::query_as(
                "SELECT p.deposit_account_id, COALESCE(p.home_amount_cents, p.amount_cents - p.discount_cents), i.customer_name
                 FROM invoice_payments p JOIN invoices i ON i.id = p.invoice_id
                 WHERE p.id = ?1 AND p.is_voided = 0 AND p.journal_entry_id IS NOT NULL",
            ),
            "SALES_RECEIPT" => sqlx::query_as(
                "SELECT r.deposit_account_id, r.total_cents, c.name
                 FROM sales_receipts r LEFT JOIN customers c ON c.id = r.customer_id
                 WHERE r.id = ?1 AND r.is_voided = 0 AND r.journal_entry_id IS NOT NULL",
            ),
            // An unapplied payment kept as the customer's credit (T24).
            "CREDIT" => sqlx::query_as(
                "SELECT m.payment_account_id, m.total_cents, c.name
                 FROM credit_memos m JOIN customers c ON c.id = m.customer_id
                 WHERE m.id = ?1 AND m.is_voided = 0 AND m.journal_entry_id IS NOT NULL AND m.payment_account_id IS NOT NULL",
            ),
            other => return Err(format!("DEP_BAD_ITEM: unknown deposit item kind '{other}'")),
        }
        .bind(item.id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        let (account, amount, customer) =
            row.ok_or_else(|| format!("DEP_ITEM_UNAVAILABLE: {} {} is not available to deposit", item.kind.to_lowercase(), item.id))?;
        let source = match item.kind.as_str() {
            "PAYMENT" => DepositSource::Payment(item.id),
            "SALES_RECEIPT" => DepositSource::Receipt(item.id),
            _ => DepositSource::Credit(item.id),
        };
        resolved.push((source, account, amount, customer.unwrap_or_else(|| "Customer payment".into())));
    }

    let mut total: i64 = 0;
    let amounts = resolved.iter().map(|r| r.2).chain(input.other_lines.iter().map(|l| l.amount_cents.get()));
    for (idx, amount) in amounts.enumerate() {
        if !(1..=MAX_LINE_CENTS).contains(&amount) {
            return Err(format!("DEP_BAD_AMOUNT: deposit line {} amount must be greater than zero", idx + 1));
        }
        total = total
            .checked_add(amount)
            .filter(|t| *t <= MAX_LINE_CENTS)
            .ok_or_else(|| format!("DEP_AMOUNT_TOO_LARGE: deposit exceeds {}", format_cents(MAX_LINE_CENTS)))?;
    }

    // Cash back comes out of what the bank gets, which must stay above zero.
    let cash_back = match &input.cash_back {
        Some(c) if !c.amount_cents.is_zero() => {
            if c.amount_cents.is_negative() || c.amount_cents.get() >= total {
                return Err(format!("DEP_BAD_CASH_BACK: cash back must be more than zero and less than the deposit total, {}", format_cents(total)));
            }
            if c.account_id == input.bank_account_id {
                return Err("DEP_CASH_BACK_ACCOUNT: choose an account for the cash back other than the bank it is deposited to".into());
            }
            Some(c)
        }
        _ => None,
    };
    let cash_back_cents = cash_back.map_or(0, |c| c.amount_cents.get());
    // Deductions also come out of what the bank gets, which must stay above zero.
    let mut deducted: i64 = 0;
    for (idx, d) in input.deductions.iter().enumerate() {
        if !(1..=MAX_LINE_CENTS).contains(&d.amount_cents.get()) {
            return Err(format!("DEP_BAD_DEDUCTION: deduction {} must be more than zero", idx + 1));
        }
        if d.account_id == input.bank_account_id {
            return Err("DEP_DEDUCTION_ACCOUNT: a deduction goes to an account other than the bank".into());
        }
        if !matches!(d.kind.as_str(), "FEE" | "REFUND" | "CHARGEBACK" | "OTHER") {
            return Err(format!("DEP_BAD_DEDUCTION: unknown deduction kind '{}'", d.kind));
        }
        deducted = deducted.checked_add(d.amount_cents.get()).ok_or_else(|| "DEP_AMOUNT_TOO_LARGE: the deductions are out of range".to_string())?;
    }
    let banked = total
        .checked_sub(cash_back_cents)
        .and_then(|b| b.checked_sub(deducted))
        .ok_or_else(|| "DEP_AMOUNT_TOO_LARGE: the deductions are out of range".to_string())?;
    if banked <= 0 {
        return Err(format!("DEP_NOTHING_BANKED: cash back and deductions must leave the bank more than zero of the {} deposited", format_cents(total)));
    }

    let deposit_id: i64 = sqlx::query_scalar(
        "INSERT INTO deposits (bank_account_id, deposit_date, memo, total_cents, cash_back_account_id, cash_back_cents, cash_back_memo)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
    )
    .bind(input.bank_account_id)
    .bind(input.deposit_date)
    .bind(clean(input.memo.as_deref()))
    .bind(total)
    .bind(cash_back.map(|c| c.account_id))
    .bind(cash_back_cents)
    .bind(cash_back.and_then(|c| clean(c.memo.as_deref())))
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    let mut credits: Vec<JournalLine> = Vec::new();
    let mut line_number = 0i64;
    for (source, account, amount, label) in &resolved {
        line_number += 1;
        let (payment, receipt, credit) = match *source {
            DepositSource::Payment(id) => (Some(id), None, None),
            DepositSource::Receipt(id) => (None, Some(id), None),
            DepositSource::Credit(id) => (None, None, Some(id)),
        };
        sqlx::query(
            "INSERT INTO deposit_lines (deposit_id, line_number, invoice_payment_id, sales_receipt_id, credit_memo_id, account_id, amount_cents, memo)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(deposit_id)
        .bind(line_number)
        .bind(payment)
        .bind(receipt)
        .bind(credit)
        .bind(account)
        .bind(amount)
        .bind(label)
        .execute(&mut **tx)
        .await
        .map_err(|e| {
            let msg = db_err(e);
            if msg.contains("UNIQUE") {
                "DEP_ALREADY_DEPOSITED: a selected payment is already in another deposit".into()
            } else {
                msg
            }
        })?;
        credits.push(JournalLine::credit(*account, *amount).with_memo(Some(label)));
    }
    for line in &input.other_lines {
        line_number += 1;
        if line.customer_id.is_some() && line.vendor_id.is_some() {
            return Err(format!("DEP_TWO_NAMES: deposit line {line_number} is received from a customer or a vendor, not both"));
        }
        sqlx::query(
            "INSERT INTO deposit_lines (deposit_id, line_number, account_id, amount_cents, memo, class_id, customer_id, vendor_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(deposit_id)
        .bind(line_number)
        .bind(line.account_id)
        .bind(line.amount_cents)
        .bind(clean(line.memo.as_deref()))
        .bind(line.class_id)
        .bind(line.customer_id)
        .bind(line.vendor_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
        credits.push(JournalLine::credit(line.account_id, line.amount_cents.get()).with_memo(line.memo.as_deref()).with_class(line.class_id));
    }

    let mut lines = vec![JournalLine::debit(input.bank_account_id, banked)];
    if let Some(c) = cash_back {
        lines.push(JournalLine::debit(c.account_id, c.amount_cents.get()).with_memo(Some(clean(c.memo.as_deref()).unwrap_or("Cash back"))));
    }
    for (idx, d) in input.deductions.iter().enumerate() {
        sqlx::query("INSERT INTO deposit_deductions (deposit_id, line_number, account_id, amount_cents, memo, kind) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
            .bind(deposit_id)
            .bind(idx as i64 + 1)
            .bind(d.account_id)
            .bind(d.amount_cents)
            .bind(clean(d.memo.as_deref()))
            .bind(&d.kind)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        lines.push(JournalLine::debit(d.account_id, d.amount_cents.get()).with_memo(clean(d.memo.as_deref())));
    }
    lines.extend(credits);
    let description = clean(input.memo.as_deref()).map_or_else(|| "Deposit".to_string(), |m| format!("Deposit: {m}"));
    let entry = JournalEntry { id: None, date: input.deposit_date, description, lines, currency: None };
    let entry_id = post_entry_in_tx(tx, &entry, EntrySource::Deposit(deposit_id)).await?;
    sqlx::query("UPDATE deposits SET journal_entry_id = ?1 WHERE id = ?2").bind(entry_id).bind(deposit_id).execute(&mut **tx).await.map_err(db_err)?;
    Ok((deposit_id, entry_id))
}

pub async fn post_transfer(pool: &SqlitePool, input: TransferInput) -> Result<i64, String> {
    if input.from_account_id == input.to_account_id {
        return Err("XFER_SAME_ACCOUNT: choose two different accounts".into());
    }
    if !(1..=MAX_LINE_CENTS).contains(&input.amount_cents.get()) {
        return Err("XFER_BAD_AMOUNT: transfer amount must be greater than zero".into());
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result = async {
        let transfer_id: i64 = sqlx::query_scalar(
            "INSERT INTO transfers (transfer_date, from_account_id, to_account_id, amount_cents, memo)
             VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id",
        )
        .bind(input.transfer_date)
        .bind(input.from_account_id)
        .bind(input.to_account_id)
        .bind(input.amount_cents)
        .bind(clean(input.memo.as_deref()))
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)?;
        let description = clean(input.memo.as_deref()).map_or_else(|| "Transfer".to_string(), |m| format!("Transfer: {m}"));
        let entry = JournalEntry {
            id: None,
            date: input.transfer_date,
            description,
            lines: vec![
                JournalLine::debit(input.to_account_id, input.amount_cents.get()),
                JournalLine::credit(input.from_account_id, input.amount_cents.get()),
            ],
            currency: None,
        };
        let entry_id = post_entry_in_tx(&mut tx, &entry, EntrySource::Transfer(transfer_id)).await?;
        sqlx::query("UPDATE transfers SET journal_entry_id = ?1 WHERE id = ?2").bind(entry_id).bind(transfer_id).execute(&mut *tx).await.map_err(db_err)?;
        Ok::<i64, String>(transfer_id)
    }
    .await;
    match result {
        Ok(id) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            Ok(id)
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

pub async fn list_banking_activity(pool: &SqlitePool) -> Result<Vec<BankingActivity>, String> {
    sqlx::query_as::<_, BankingActivity>(
        "SELECT 'DEPOSIT' AS kind, d.id, d.deposit_date AS date, d.bank_account_id AS account_id,
                NULL AS other_account_id, d.memo,
                (SELECT COUNT(*) FROM deposit_lines WHERE deposit_id = d.id) AS line_count,
                d.total_cents - d.cash_back_cents AS amount_cents, d.journal_entry_id, d.is_voided
         FROM deposits d
         UNION ALL
         SELECT 'TRANSFER', t.id, t.transfer_date, t.from_account_id, t.to_account_id, t.memo, 1,
                t.amount_cents, t.journal_entry_id, t.is_voided
         FROM transfers t
         ORDER BY date DESC, kind, id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests;
