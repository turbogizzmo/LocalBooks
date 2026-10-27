//! Bank deposits and transfers.
//!
//! A deposit moves customer payments out of Undeposited Funds into a bank account and may add other
//! funds: Dr bank / Cr Undeposited Funds (per selected payment) and Cr each other line's account.
//! A transfer moves money between balance-sheet accounts: Dr to-account / Cr from-account.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{db_err, format_cents, post_entry_in_tx, rollback, EntrySource, JournalEntry, JournalLine, MAX_LINE_CENTS};

fn clean(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|s| !s.is_empty())
}

/// A received payment sitting in Undeposited Funds, waiting to be deposited.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UndepositedItem {
    /// `PAYMENT` (invoice payment) or `SALES_RECEIPT`
    pub kind: String,
    pub id: i64,
    pub date: String,
    pub customer_name: Option<String>,
    pub payment_method: String,
    pub reference: Option<String>,
    pub amount_cents: i64,
    pub undeposited_account_id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositItemRef {
    /// `PAYMENT` or `SALES_RECEIPT`
    pub kind: String,
    pub id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositOtherLine {
    pub account_id: i64,
    pub amount_cents: i64,
    pub memo: Option<String>,
    #[serde(default)]
    pub class_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositInput {
    pub bank_account_id: i64,
    pub deposit_date: NaiveDate,
    pub memo: Option<String>,
    pub items: Vec<DepositItemRef>,
    pub other_lines: Vec<DepositOtherLine>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferInput {
    pub from_account_id: i64,
    pub to_account_id: i64,
    pub transfer_date: NaiveDate,
    pub amount_cents: i64,
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

pub async fn list_undeposited(pool: &SqlitePool) -> Result<Vec<UndepositedItem>, String> {
    sqlx::query_as::<_, UndepositedItem>(
        "SELECT 'PAYMENT' AS kind, p.id AS id, p.payment_date AS date, i.customer_name, p.payment_method, p.reference,
                COALESCE(p.home_amount_cents, p.amount_cents) AS amount_cents, p.deposit_account_id AS undeposited_account_id
         FROM invoice_payments p
         JOIN invoices i ON i.id = p.invoice_id
         JOIN chart_of_accounts a ON a.id = p.deposit_account_id
         WHERE a.is_system = 1 AND a.account_type = 'OTHER_CURRENT_ASSET'
           AND p.is_voided = 0 AND p.journal_entry_id IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM deposit_lines d WHERE d.invoice_payment_id = p.id AND d.is_voided = 0)
           AND NOT EXISTS (SELECT 1 FROM undeposited_fund_matches m WHERE m.invoice_payment_id = p.id AND m.is_voided = 0)
         UNION ALL
         SELECT 'SALES_RECEIPT', r.id, r.receipt_date, c.name, r.payment_method, r.reference,
                r.total_cents, r.deposit_account_id
         FROM sales_receipts r
         LEFT JOIN customers c ON c.id = r.customer_id
         JOIN chart_of_accounts a ON a.id = r.deposit_account_id
         WHERE a.is_system = 1 AND a.account_type = 'OTHER_CURRENT_ASSET'
           AND r.is_voided = 0 AND r.journal_entry_id IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM deposit_lines d WHERE d.sales_receipt_id = r.id AND d.is_voided = 0)
           AND NOT EXISTS (SELECT 1 FROM undeposited_fund_matches m WHERE m.sales_receipt_id = r.id AND m.is_voided = 0)
         ORDER BY date, kind, id",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn post_bank_deposit(pool: &SqlitePool, input: DepositInput) -> Result<i64, String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
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

    // Resolve the selected payments: (payment id, receipt id, undeposited account, amount, label).
    let mut resolved: Vec<(Option<i64>, Option<i64>, i64, i64, String)> = Vec::with_capacity(input.items.len());
    for item in &input.items {
        let row: Option<(i64, i64, Option<String>)> = match item.kind.as_str() {
            "PAYMENT" => sqlx::query_as(
                "SELECT p.deposit_account_id, COALESCE(p.home_amount_cents, p.amount_cents), i.customer_name
                 FROM invoice_payments p JOIN invoices i ON i.id = p.invoice_id
                 WHERE p.id = ?1 AND p.is_voided = 0 AND p.journal_entry_id IS NOT NULL",
            ),
            "SALES_RECEIPT" => sqlx::query_as(
                "SELECT r.deposit_account_id, r.total_cents, c.name
                 FROM sales_receipts r LEFT JOIN customers c ON c.id = r.customer_id
                 WHERE r.id = ?1 AND r.is_voided = 0 AND r.journal_entry_id IS NOT NULL",
            ),
            other => return Err(format!("DEP_BAD_ITEM: unknown deposit item kind '{other}'")),
        }
        .bind(item.id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        let (account, amount, customer) =
            row.ok_or_else(|| format!("DEP_ITEM_UNAVAILABLE: {} {} is not available to deposit", item.kind.to_lowercase(), item.id))?;
        let (payment, receipt) = if item.kind == "PAYMENT" { (Some(item.id), None) } else { (None, Some(item.id)) };
        resolved.push((payment, receipt, account, amount, customer.unwrap_or_else(|| "Customer payment".into())));
    }

    let mut total: i64 = 0;
    let amounts = resolved.iter().map(|r| r.3).chain(input.other_lines.iter().map(|l| l.amount_cents));
    for (idx, amount) in amounts.enumerate() {
        if !(1..=MAX_LINE_CENTS).contains(&amount) {
            return Err(format!("DEP_BAD_AMOUNT: deposit line {} amount must be greater than zero", idx + 1));
        }
        total = total
            .checked_add(amount)
            .filter(|t| *t <= MAX_LINE_CENTS)
            .ok_or_else(|| format!("DEP_AMOUNT_TOO_LARGE: deposit exceeds {}", format_cents(MAX_LINE_CENTS)))?;
    }

    let deposit_id: i64 = sqlx::query_scalar(
        "INSERT INTO deposits (bank_account_id, deposit_date, memo, total_cents) VALUES (?1, ?2, ?3, ?4) RETURNING id",
    )
    .bind(input.bank_account_id)
    .bind(input.deposit_date)
    .bind(clean(input.memo.as_deref()))
    .bind(total)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    let mut credits: Vec<JournalLine> = Vec::new();
    let mut line_number = 0i64;
    for (payment, receipt, account, amount, label) in &resolved {
        line_number += 1;
        sqlx::query(
            "INSERT INTO deposit_lines (deposit_id, line_number, invoice_payment_id, sales_receipt_id, account_id, amount_cents, memo)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(deposit_id)
        .bind(line_number)
        .bind(payment)
        .bind(receipt)
        .bind(account)
        .bind(amount)
        .bind(label)
        .execute(&mut **tx)
        .await
        .map_err(|e| {
            let msg = db_err(e);
            if msg.contains("UNIQUE") { "DEP_ALREADY_DEPOSITED: a selected payment is already in another deposit".into() } else { msg }
        })?;
        credits.push(JournalLine::credit(*account, *amount).with_memo(Some(label)));
    }
    for line in &input.other_lines {
        line_number += 1;
        sqlx::query(
            "INSERT INTO deposit_lines (deposit_id, line_number, account_id, amount_cents, memo, class_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(deposit_id)
        .bind(line_number)
        .bind(line.account_id)
        .bind(line.amount_cents)
        .bind(clean(line.memo.as_deref()))
        .bind(line.class_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
        credits.push(JournalLine::credit(line.account_id, line.amount_cents).with_memo(line.memo.as_deref()).with_class(line.class_id));
    }

    let mut lines = vec![JournalLine::debit(input.bank_account_id, total)];
    lines.extend(credits);
    let description = clean(input.memo.as_deref()).map_or_else(|| "Deposit".to_string(), |m| format!("Deposit: {m}"));
    let entry = JournalEntry { id: None, date: input.deposit_date, description, lines, currency: None };
    let entry_id = post_entry_in_tx(tx, &entry, EntrySource::Deposit(deposit_id)).await?;
    sqlx::query("UPDATE deposits SET journal_entry_id = ?1 WHERE id = ?2")
        .bind(entry_id)
        .bind(deposit_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok((deposit_id, entry_id))
}

pub async fn post_transfer(pool: &SqlitePool, input: TransferInput) -> Result<i64, String> {
    if input.from_account_id == input.to_account_id {
        return Err("XFER_SAME_ACCOUNT: choose two different accounts".into());
    }
    if !(1..=MAX_LINE_CENTS).contains(&input.amount_cents) {
        return Err("XFER_BAD_AMOUNT: transfer amount must be greater than zero".into());
    }
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
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
                JournalLine::debit(input.to_account_id, input.amount_cents),
                JournalLine::credit(input.from_account_id, input.amount_cents),
            ],
            currency: None,
        };
        let entry_id = post_entry_in_tx(&mut tx, &entry, EntrySource::Transfer(transfer_id)).await?;
        sqlx::query("UPDATE transfers SET journal_entry_id = ?1 WHERE id = ?2")
            .bind(entry_id)
            .bind(transfer_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
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
                d.total_cents AS amount_cents, d.journal_entry_id, d.is_voided
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
