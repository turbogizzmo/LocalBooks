//! Typing a transaction straight into a bank or credit card register.
//!
//! The register's entry row is only another way to fill in a form: a payment from a bank account
//! is recorded as a check, a deposit as a deposit, a charge on a card as a card expense and a
//! payment or refund on a card as a card credit. Each goes through the same code as its own form
//! (`expenses::post_expense`, `banking::post_bank_deposit`), so the ledger rules stay there.
//!
//! Two rows stand for other forms: one line to another bank account is a transfer between the two
//! (`banking::post_transfer`), and a deposit from Undeposited Funds deposits the payments waiting
//! there that make up its amount, as Make Deposits would.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::banking::{DepositInput, DepositItemRef, DepositOtherLine, TransferInput};
use crate::expenses::{ExpenseInput, SplitLineInput};
use crate::ledger::db_err;
use crate::money::Cents;

/// Which column of the register the amount was typed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RegisterSide {
    /// Bank register: money out (recorded as a check).
    Payment,
    /// Bank register: money in (recorded as a deposit).
    Deposit,
    /// Credit card register: a purchase (recorded as a card expense).
    Charge,
    /// Credit card register: a payment or refund onto the card (recorded as a card credit).
    Credit,
}

/// One account the entry is split to; a plain one-account row sends one of these.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterSplit {
    pub account_id: i64,
    pub amount_cents: i64,
    pub memo: Option<String>,
    #[serde(default)]
    pub class_id: Option<i64>,
    /// The customer or job a payment or charge is for (job costing); a deposit's name is its payee.
    #[serde(default)]
    pub customer_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterEntryInput {
    /// The register's account: a Bank or Credit Card account.
    pub account_id: i64,
    pub entry_date: NaiveDate,
    /// Check number (bank payments only; other kinds have nowhere to keep it).
    pub number: Option<String>,
    pub payee_name: Option<String>,
    pub memo: Option<String>,
    pub side: RegisterSide,
    pub lines: Vec<RegisterSplit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterEntryPosted {
    /// `CHECK`, `DEPOSIT`, `EXPENSE`, `CARD_CREDIT` or `TRANSFER`, as journal entries show it.
    pub source_type: String,
    /// The check, expense, deposit or transfer's id in its own table.
    pub source_id: i64,
    pub journal_entry_id: i64,
}

fn clean(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|t| !t.is_empty())
}

/// The customer, else the vendor, with this name (case aside): a deposit's Received From.
async fn received_from(pool: &SqlitePool, name: &str) -> Result<(Option<i64>, Option<i64>), String> {
    let customer: Option<i64> = sqlx::query_scalar("SELECT id FROM customers WHERE name = ?1 COLLATE NOCASE ORDER BY id LIMIT 1")
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    if customer.is_some() {
        return Ok((customer, None));
    }
    let vendor: Option<i64> = sqlx::query_scalar("SELECT id FROM vendors WHERE name = ?1 COLLATE NOCASE ORDER BY id LIMIT 1")
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    Ok((None, vendor))
}

/// Records the register row through the form it stands for.
pub async fn post_register_entry(pool: &SqlitePool, input: RegisterEntryInput) -> Result<RegisterEntryPosted, String> {
    let account_type: Option<String> = sqlx::query_scalar("SELECT account_type FROM chart_of_accounts WHERE id = ?1")
        .bind(input.account_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    let account_type = account_type.ok_or_else(|| format!("REG_NOT_FOUND: account {} does not exist", input.account_id))?;
    let kind = match (account_type.as_str(), input.side) {
        ("BANK", RegisterSide::Payment) => "CHECK",
        ("BANK", RegisterSide::Deposit) => "DEPOSIT",
        ("CREDIT_CARD", RegisterSide::Charge) => "EXPENSE",
        ("CREDIT_CARD", RegisterSide::Credit) => "CARD_CREDIT",
        ("BANK" | "CREDIT_CARD", _) => return Err("REG_ENTRY_BAD_SIDE: that column doesn't belong to this register".into()),
        _ => return Err("REG_ENTRY_UNSUPPORTED: transactions can be typed into bank and credit card registers; use a form or a journal entry for this account".into()),
    };
    if input.lines.is_empty() {
        return Err("REG_ENTRY_NO_ACCOUNT: choose the account this goes to".into());
    }
    if input.lines.iter().any(|l| l.account_id == input.account_id) {
        return Err("REG_ENTRY_SAME_ACCOUNT: the other account can't be the register's own account".into());
    }
    let memo = clean(input.memo.as_deref());

    // One line to another bank account: a transfer, out of this register on its Payment or Charge
    // side and into it on its Deposit or Credit side.
    let single: Option<(i64, String, bool)> = match input.lines.as_slice() {
        [line] => sqlx::query_as("SELECT id, account_type, is_system FROM chart_of_accounts WHERE id = ?1")
            .bind(line.account_id)
            .fetch_optional(pool)
            .await
            .map_err(db_err)?,
        _ => None,
    };
    if let Some((other, _, _)) = single.as_ref().filter(|(_, t, _)| t == "BANK") {
        let outflow = matches!(input.side, RegisterSide::Payment | RegisterSide::Charge);
        let (from, to) = if outflow { (input.account_id, *other) } else { (*other, input.account_id) };
        let transfer = TransferInput { from_account_id: from, to_account_id: to, transfer_date: input.entry_date, amount_cents: Cents(input.lines[0].amount_cents), memo: memo.map(str::to_string) };
        let id = crate::banking::post_transfer(pool, transfer).await?;
        let entry: Option<i64> = sqlx::query_scalar("SELECT journal_entry_id FROM transfers WHERE id = ?1").bind(id).fetch_one(pool).await.map_err(db_err)?;
        return Ok(RegisterEntryPosted {
            source_type: "TRANSFER".into(),
            source_id: id,
            journal_entry_id: entry.ok_or("REG_ENTRY_NOT_POSTED: the transfer was saved but not posted")?,
        });
    }

    // A deposit from Undeposited Funds banks the payments waiting there: the one that is the
    // amount, or else all of them up to the row's date when together they are.
    if kind == "DEPOSIT" {
        if let Some((undeposited, _, true)) = single.as_ref().filter(|(_, t, _)| t == "OTHER_CURRENT_ASSET") {
            let amount = input.lines[0].amount_cents;
            let on_or_before = |date: &str| date.parse::<NaiveDate>().is_ok_and(|d| d <= input.entry_date);
            let waiting: Vec<_> = crate::banking::list_undeposited(pool)
                .await?
                .into_iter()
                .filter(|w| w.undeposited_account_id == *undeposited && on_or_before(&w.date))
                .collect();
            let together = waiting.iter().try_fold(0i64, |sum, w| sum.checked_add(w.amount_cents));
            let chosen: Vec<_> = match waiting.iter().find(|w| w.amount_cents == amount) {
                Some(one) => vec![one],
                None if together == Some(amount) => waiting.iter().collect(),
                None => {
                    return Err(format!(
                        "REG_ENTRY_UNDEPOSITED_NO_MATCH: no payment waiting in Undeposited Funds is {}, and together they aren't either; choose them in Make Deposits",
                        crate::ledger::format_cents(amount)
                    ))
                }
            };
            let items = chosen.into_iter().map(|w| DepositItemRef { kind: w.kind.clone(), id: w.id }).collect();
            let deposit = DepositInput { bank_account_id: input.account_id, deposit_date: input.entry_date, memo: memo.map(str::to_string), items, other_lines: Vec::new(), cash_back: None, deductions: Vec::new() };
            let deposit_id = crate::banking::post_bank_deposit(pool, deposit).await?;
            let entry: Option<i64> = sqlx::query_scalar("SELECT journal_entry_id FROM deposits WHERE id = ?1").bind(deposit_id).fetch_one(pool).await.map_err(db_err)?;
            return Ok(RegisterEntryPosted {
                source_type: kind.into(),
                source_id: deposit_id,
                journal_entry_id: entry.ok_or("REG_ENTRY_NOT_POSTED: the deposit was saved but not posted")?,
            });
        }
    }

    if kind == "DEPOSIT" {
        // The payee is the deposit's Received From: a customer or vendor of that name. A name that
        // is on neither list is kept in the memo, so it still shows in the register.
        let payee = clean(input.payee_name.as_deref());
        let (customer_id, vendor_id) = match payee {
            Some(name) => received_from(pool, name).await?,
            None => (None, None),
        };
        let memo = match (payee.filter(|_| customer_id.is_none() && vendor_id.is_none()), memo) {
            (Some(payee), Some(memo)) => Some(format!("{payee} · {memo}")),
            (Some(payee), None) => Some(payee.to_string()),
            (None, memo) => memo.map(str::to_string),
        };
        let other_lines = input
            .lines
            .iter()
            .map(|l| DepositOtherLine { account_id: l.account_id, amount_cents: Cents(l.amount_cents), memo: l.memo.clone(), class_id: l.class_id, customer_id, vendor_id })
            .collect();
        let deposit = DepositInput { bank_account_id: input.account_id, deposit_date: input.entry_date, memo, items: Vec::new(), other_lines, cash_back: None, deductions: Vec::new() };
        let deposit_id = crate::banking::post_bank_deposit(pool, deposit).await?;
        let entry: Option<i64> = sqlx::query_scalar("SELECT journal_entry_id FROM deposits WHERE id = ?1").bind(deposit_id).fetch_one(pool).await.map_err(db_err)?;
        return Ok(RegisterEntryPosted {
            source_type: kind.into(),
            source_id: deposit_id,
            journal_entry_id: entry.ok_or("REG_ENTRY_NOT_POSTED: the deposit was saved but not posted")?,
        });
    }

    // A check or charge pays a vendor or other name already on the list: a typed name the person
    // hasn't added is refused, not added as a new vendor.
    crate::sales::require_listed_payee(pool, input.payee_name.as_deref()).await?;
    let expense = ExpenseInput { form_lines: Vec::new(),
        payment_method_id: None,
        kind: kind.into(),
        payee_name: input.payee_name.clone(),
        payment_account_id: input.account_id,
        expense_date: input.entry_date,
        check_number: if kind == "CHECK" { input.number.clone() } else { None },
        payment_method: None,
        memo: memo.map(str::to_string),
        lines: input
            .lines
            .iter()
            .map(|l| SplitLineInput { account_id: l.account_id, amount_cents: Cents(l.amount_cents), memo: l.memo.clone(), class_id: l.class_id, customer_id: l.customer_id, billable: None })
            .collect(),
        currency: None,
    };
    let posted = crate::expenses::post_expense(pool, expense).await?;
    Ok(RegisterEntryPosted {
        source_type: kind.into(),
        source_id: posted.id,
        journal_entry_id: posted.journal_entry_id.ok_or("REG_ENTRY_NOT_POSTED: the expense was saved but not posted")?,
    })
}

#[cfg(test)]
mod tests;
