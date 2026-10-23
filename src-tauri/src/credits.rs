//! Customer credits (credit memos, refund receipts) and vendor credits, how they are used against
//! open invoices and bills, and refunds of credit left over.
//!
//! * Credit memo: Dr each income line / Cr Accounts Receivable. With a refund account it is a refund
//!   receipt instead (the money went straight back): Dr income / Cr that account.
//! * Applying a credit to an invoice moves nothing in the ledger (both sides are in A/R); it counts
//!   toward the invoice's amount paid, exactly like a payment. Removing the application undoes it.
//! * Refunding credit left over: Dr A/R / Cr the bank or card account, its own journal entry.
//! * Vendor credit: Dr Accounts Payable / Cr each expense line, applied to bills the same way, or
//!   refunded to us (Dr the bank account / Cr A/P).
//!
//! Credits are in the home currency and apply to home-currency invoices and bills.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::invoices::{line_amount, validate_line};
use crate::ledger::{db_err, format_cents, post_entry_in_tx, recompute_paid, rollback, EntrySource, JournalEntry, JournalLine, MAX_LINE_CENTS};
use crate::sales::{find_or_create_named, Party, SalesLineInput};

fn clean(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|s| !s.is_empty())
}

async fn system_account(tx: &mut Transaction<'_, Sqlite>, account_type: &str) -> Result<i64, String> {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = ?1 AND is_system = 1 ORDER BY id LIMIT 1")
        .bind(account_type)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("CREDIT_NO_SYSTEM_ACCOUNT: the {account_type} system account is missing"))
}

/// Refunds are paid from, or received into, a bank or credit card account.
async fn check_money_account(tx: &mut Transaction<'_, Sqlite>, account_id: i64, code: &str) -> Result<(), String> {
    let row: Option<(String, bool)> = sqlx::query_as("SELECT account_type, is_active FROM chart_of_accounts WHERE id = ?1")
        .bind(account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    match row {
        Some((kind, true)) if kind == "BANK" || kind == "CREDIT_CARD" => Ok(()),
        Some((_, false)) => Err(format!("{code}_INACTIVE_ACCOUNT: that account is inactive")),
        _ => Err(format!("{code}_BAD_ACCOUNT: refunds go through a Bank or Credit Card account")),
    }
}

async fn run<T>(pool: &SqlitePool, work: impl for<'t> AsyncFnOnce(&mut Transaction<'t, Sqlite>) -> Result<T, String>) -> Result<T, String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    match work(&mut tx).await {
        Ok(value) => tx.commit().await.map(|()| value).map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

// ------------------------------------------------------------------------------------------------
// Customer credits
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditMemoInput {
    pub customer_name: String,
    pub credit_date: NaiveDate,
    pub memo: Option<String>,
    /// Set for a refund receipt: the bank or card account the money was paid back from.
    #[serde(default)]
    pub refund_account_id: Option<i64>,
    pub lines: Vec<SalesLineInput>,
    /// Sales tax given back on the taxable lines.
    #[serde(default)]
    pub tax_rate_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CreditMemoSummary {
    pub id: i64,
    pub credit_number: i64,
    pub customer_id: i64,
    pub customer_name: String,
    pub credit_date: String,
    pub memo: Option<String>,
    pub refund_account_id: Option<i64>,
    pub total_cents: i64,
    pub remaining_cents: i64,
    pub is_voided: bool,
    pub journal_entry_id: Option<i64>,
}

const CREDIT_MEMO_SELECT: &str = "SELECT m.id, m.credit_number, m.customer_id, c.name AS customer_name, m.credit_date, m.memo,
        m.refund_account_id, m.total_cents, b.remaining_cents, m.is_voided, m.journal_entry_id
 FROM credit_memos m JOIN customers c ON c.id = m.customer_id JOIN v_credit_memo_balances b ON b.credit_memo_id = m.id";

pub async fn list_credit_memos(pool: &SqlitePool) -> Result<Vec<CreditMemoSummary>, String> {
    sqlx::query_as::<_, CreditMemoSummary>(&format!("{CREDIT_MEMO_SELECT} ORDER BY m.credit_date DESC, m.credit_number DESC"))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

async fn credit_memo(pool: &SqlitePool, id: i64) -> Result<CreditMemoSummary, String> {
    sqlx::query_as::<_, CreditMemoSummary>(&format!("{CREDIT_MEMO_SELECT} WHERE m.id = ?1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("CM_NOT_FOUND: credit memo {id} does not exist"))
}

pub async fn create_credit_memo(pool: &SqlitePool, input: CreditMemoInput) -> Result<CreditMemoSummary, String> {
    let id = run(pool, async |tx| write_credit_memo(tx, &input).await).await?;
    credit_memo(pool, id).await
}

async fn write_credit_memo(tx: &mut Transaction<'_, Sqlite>, input: &CreditMemoInput) -> Result<i64, String> {
    if input.lines.is_empty() {
        return Err("CM_NO_ITEMS: add at least one line".into());
    }
    let mut total: i64 = 0;
    for (idx, line) in input.lines.iter().enumerate() {
        let amount = validate_line(line.quantity_e4, line.rate_cents, idx + 1, "CM")?;
        total = total.checked_add(amount).filter(|t| *t <= MAX_LINE_CENTS).ok_or_else(|| format!("CM_TOO_LARGE: total exceeds {}", format_cents(MAX_LINE_CENTS)))?;
    }
    let mut taxable = 0;
    for line in input.lines.iter().filter(|l| l.is_taxable) {
        taxable += line_amount(line.quantity_e4, line.rate_cents)?;
    }
    let tax = crate::sales_tax::document_tax(tx, input.tax_rate_id, taxable, "CM").await?;
    let tax_cents = tax.map_or(0, |t| t.cents);
    let total = total.checked_add(tax_cents).filter(|t| *t <= MAX_LINE_CENTS).ok_or("CM_TOO_LARGE: the credit is too large")?;
    if let Some(account) = input.refund_account_id {
        check_money_account(tx, account, "CM").await?;
    }
    let customer = input.customer_name.trim();
    let customer_id = find_or_create_named(tx, Party::Customer, customer).await?;

    let (memo_id, number): (i64, i64) = sqlx::query_as(
        "INSERT INTO credit_memos (credit_number, customer_id, credit_date, memo, refund_account_id, total_cents, tax_rate_id, tax_cents)
         VALUES ((SELECT COALESCE(MAX(credit_number), 0) + 1 FROM credit_memos), ?1, ?2, ?3, ?4, ?5, ?6, ?7)
         RETURNING id, credit_number",
    )
    .bind(customer_id)
    .bind(input.credit_date)
    .bind(clean(input.memo.as_deref()))
    .bind(input.refund_account_id)
    .bind(total)
    .bind(tax.and_then(|t| t.rate_id))
    .bind(tax_cents)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    // Debits per income account and class, in first-appearance order.
    let mut debits: Vec<((i64, Option<i64>), i64)> = Vec::new();
    for (idx, line) in input.lines.iter().enumerate() {
        let n = idx + 1;
        let row: Option<(String, bool)> = sqlx::query_as(
            "SELECT t.classification, a.is_active FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1",
        )
        .bind(line.income_account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        match row {
            Some((class, true)) if class == "INCOME" => {}
            Some((_, false)) => return Err(format!("CM_INACTIVE_ACCOUNT: line {n} uses an inactive account")),
            _ => return Err(format!("CM_NOT_INCOME_ACCOUNT: line {n} must use an Income account")),
        }
        if let Some(product) = line.product_service_id {
            let tracked: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM inventory_items WHERE product_id = ?1)")
                .bind(product)
                .fetch_one(&mut **tx)
                .await
                .map_err(db_err)?;
            if tracked {
                return Err(format!("CM_STOCK_RETURN: line {n} is a tracked stock item; record returned stock with Products › Adjust stock and credit it as a non-stock line"));
            }
        }
        if let Some(class) = line.class_id {
            crate::classes::check_usable(tx, class, &format!("CM: line {n}")).await?;
        }
        let amount = line_amount(line.quantity_e4, line.rate_cents)?;
        sqlx::query(
            "INSERT INTO credit_memo_lines
                (credit_memo_id, line_number, product_service_id, description, quantity_e4, rate_cents, amount_cents, income_account_id, class_id, is_taxable)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .bind(memo_id)
        .bind(n as i64)
        .bind(line.product_service_id)
        .bind(line.description.trim())
        .bind(line.quantity_e4)
        .bind(line.rate_cents)
        .bind(amount)
        .bind(line.income_account_id)
        .bind(line.class_id)
        .bind(line.is_taxable && tax.is_some())
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
        let key = (line.income_account_id, line.class_id);
        match debits.iter_mut().find(|(k, _)| *k == key) {
            Some((_, sum)) => *sum += amount,
            None => debits.push((key, amount)),
        }
    }

    let credit_account = match input.refund_account_id {
        Some(account) => account,
        None => system_account(tx, "ACCOUNTS_RECEIVABLE").await?,
    };
    let mut lines: Vec<JournalLine> = debits.into_iter().map(|((a, class), c)| JournalLine::debit(a, c).with_class(class)).collect();
    lines.extend(tax.and_then(|t| t.line(true)));
    lines.push(JournalLine::credit(credit_account, total));
    let kind = if input.refund_account_id.is_some() { "Refund receipt" } else { "Credit memo" };
    let entry = JournalEntry { id: None, date: input.credit_date, description: format!("{kind} #{number} - {customer}"), lines, currency: None };
    let entry_id = post_entry_in_tx(tx, &entry, EntrySource::CreditMemo(memo_id)).await?;
    sqlx::query("UPDATE credit_memos SET journal_entry_id = ?1 WHERE id = ?2").bind(entry_id).bind(memo_id).execute(&mut **tx).await.map_err(db_err)?;
    Ok(memo_id)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyCreditInput {
    /// A credit memo (customer credit) or a vendor credit, depending on the call.
    pub credit_id: i64,
    /// An invoice (customer credit) or a bill (vendor credit).
    pub document_id: i64,
    pub amount_cents: i64,
    pub applied_date: NaiveDate,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CreditApplicationView {
    pub id: i64,
    pub credit_id: i64,
    pub document_id: i64,
    /// Invoice number, or the bill's reference.
    pub document_label: String,
    pub applied_date: String,
    pub amount_cents: i64,
}

/// Uses `amount_cents` of a credit memo against one of the same customer's open invoices.
pub async fn apply_customer_credit(pool: &SqlitePool, input: ApplyCreditInput) -> Result<CreditMemoSummary, String> {
    run(pool, async |tx| {
        let memo: Option<(i64, i64, bool, Option<i64>)> = sqlx::query_as(
            "SELECT m.customer_id, b.remaining_cents, m.is_voided, m.refund_account_id
             FROM credit_memos m JOIN v_credit_memo_balances b ON b.credit_memo_id = m.id WHERE m.id = ?1",
        )
        .bind(input.credit_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        let (customer, remaining, voided, refund) = memo.ok_or("CM_NOT_FOUND: that credit memo does not exist")?;
        if voided || refund.is_some() {
            return Err("CM_NOT_AVAILABLE: a voided credit memo or a refund receipt has no credit to apply".into());
        }
        let invoice: Option<(Option<i64>, i64, i64, bool, String, Option<String>)> = sqlx::query_as(
            "SELECT customer_id, total_cents, amount_paid_cents, is_voided, status, currency_code FROM invoices WHERE id = ?1",
        )
        .bind(input.document_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        let (inv_customer, total, paid, inv_voided, status, currency) = invoice.ok_or("CM_INVOICE_NOT_FOUND: that invoice does not exist")?;
        if inv_customer != Some(customer) {
            return Err("CM_OTHER_CUSTOMER: a credit can only be applied to the same customer's invoices".into());
        }
        if inv_voided || !(status == "UNPAID" || status == "PARTIAL") {
            return Err("CM_INVOICE_NOT_OPEN: that invoice has nothing left to pay".into());
        }
        if currency.is_some() {
            return Err("CM_FOREIGN_INVOICE: credits apply to home-currency invoices only".into());
        }
        if input.amount_cents < 1 || input.amount_cents > remaining || input.amount_cents > total - paid {
            return Err(format!(
                "CM_BAD_AMOUNT: apply between {} and {} (the smaller of the credit left and the invoice balance)",
                format_cents(1),
                format_cents(remaining.min(total - paid))
            ));
        }
        sqlx::query("INSERT INTO credit_applications (credit_memo_id, invoice_id, applied_date, amount_cents) VALUES (?1, ?2, ?3, ?4)")
            .bind(input.credit_id)
            .bind(input.document_id)
            .bind(input.applied_date)
            .bind(input.amount_cents)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        recompute_paid(tx, "invoices", "invoice_payments", "invoice_id", input.document_id).await
    })
    .await?;
    credit_memo(pool, input.credit_id).await
}

pub async fn list_customer_credit_applications(pool: &SqlitePool, credit_memo_id: i64) -> Result<Vec<CreditApplicationView>, String> {
    sqlx::query_as::<_, CreditApplicationView>(
        "SELECT a.id, a.credit_memo_id AS credit_id, a.invoice_id AS document_id, 'Invoice #' || i.invoice_number AS document_label,
                a.applied_date, a.amount_cents
         FROM credit_applications a JOIN invoices i ON i.id = a.invoice_id
         WHERE a.credit_memo_id = ?1 AND a.is_voided = 0 ORDER BY a.id",
    )
    .bind(credit_memo_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Takes an applied credit back off its invoice.
pub async fn remove_customer_credit(pool: &SqlitePool, application_id: i64) -> Result<(), String> {
    run(pool, async |tx| {
        let invoice: Option<i64> = sqlx::query_scalar("SELECT invoice_id FROM credit_applications WHERE id = ?1 AND is_voided = 0")
            .bind(application_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
        let invoice = invoice.ok_or("CM_APPLICATION_NOT_FOUND: that credit is not applied")?;
        sqlx::query("UPDATE credit_applications SET is_voided = 1 WHERE id = ?1").bind(application_id).execute(&mut **tx).await.map_err(db_err)?;
        recompute_paid(tx, "invoices", "invoice_payments", "invoice_id", invoice).await
    })
    .await
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefundCreditInput {
    pub credit_id: i64,
    /// Bank or credit card account the refund is paid from (customer) or received into (vendor).
    pub account_id: i64,
    pub refund_date: NaiveDate,
    pub amount_cents: i64,
    pub reference: Option<String>,
}

/// Pays credit left on a credit memo back to the customer: Dr A/R / Cr the bank or card account.
pub async fn refund_customer_credit(pool: &SqlitePool, input: RefundCreditInput) -> Result<CreditMemoSummary, String> {
    run(pool, async |tx| {
        let memo: Option<(i64, String, i64, bool, Option<i64>)> = sqlx::query_as(
            "SELECT m.credit_number, c.name, b.remaining_cents, m.is_voided, m.refund_account_id
             FROM credit_memos m JOIN customers c ON c.id = m.customer_id JOIN v_credit_memo_balances b ON b.credit_memo_id = m.id
             WHERE m.id = ?1",
        )
        .bind(input.credit_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        let (number, customer, remaining, voided, refund) = memo.ok_or("CM_NOT_FOUND: that credit memo does not exist")?;
        if voided || refund.is_some() {
            return Err("CM_NOT_AVAILABLE: a voided credit memo or a refund receipt has no credit to refund".into());
        }
        if input.amount_cents < 1 || input.amount_cents > remaining {
            return Err(format!("CM_BAD_AMOUNT: refund up to the {} of credit left", format_cents(remaining)));
        }
        check_money_account(tx, input.account_id, "CM").await?;
        let ar = system_account(tx, "ACCOUNTS_RECEIVABLE").await?;
        let reference = clean(input.reference.as_deref()).map(|r| format!(" ({r})")).unwrap_or_default();
        let entry = JournalEntry {
            id: None,
            date: input.refund_date,
            description: format!("Refund of credit memo #{number} - {customer}{reference}"),
            lines: vec![JournalLine::debit(ar, input.amount_cents), JournalLine::credit(input.account_id, input.amount_cents)],
            currency: None,
        };
        let entry_id = post_entry_in_tx(tx, &entry, EntrySource::Journal).await?;
        sqlx::query(
            "INSERT INTO credit_refunds (credit_memo_id, account_id, refund_date, amount_cents, reference, journal_entry_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(input.credit_id)
        .bind(input.account_id)
        .bind(input.refund_date)
        .bind(input.amount_cents)
        .bind(clean(input.reference.as_deref()))
        .bind(entry_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
        Ok(())
    })
    .await?;
    credit_memo(pool, input.credit_id).await
}

// ------------------------------------------------------------------------------------------------
// Vendor credits
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VendorCreditLineInput {
    pub account_id: i64,
    pub amount_cents: i64,
    pub memo: Option<String>,
    #[serde(default)]
    pub class_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VendorCreditInput {
    pub vendor_name: String,
    pub reference: Option<String>,
    pub credit_date: NaiveDate,
    pub memo: Option<String>,
    pub lines: Vec<VendorCreditLineInput>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct VendorCreditSummary {
    pub id: i64,
    pub vendor_id: i64,
    pub vendor_name: String,
    pub reference: Option<String>,
    pub credit_date: String,
    pub memo: Option<String>,
    pub total_cents: i64,
    pub remaining_cents: i64,
    pub is_voided: bool,
    pub journal_entry_id: Option<i64>,
}

const VENDOR_CREDIT_SELECT: &str = "SELECT c.id, c.vendor_id, v.name AS vendor_name, c.reference, c.credit_date, c.memo, c.total_cents,
        b.remaining_cents, c.is_voided, c.journal_entry_id
 FROM vendor_credits c JOIN vendors v ON v.id = c.vendor_id JOIN v_vendor_credit_balances b ON b.vendor_credit_id = c.id";

pub async fn list_vendor_credits(pool: &SqlitePool) -> Result<Vec<VendorCreditSummary>, String> {
    sqlx::query_as::<_, VendorCreditSummary>(&format!("{VENDOR_CREDIT_SELECT} ORDER BY c.credit_date DESC, c.id DESC"))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

async fn vendor_credit(pool: &SqlitePool, id: i64) -> Result<VendorCreditSummary, String> {
    sqlx::query_as::<_, VendorCreditSummary>(&format!("{VENDOR_CREDIT_SELECT} WHERE c.id = ?1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("VC_NOT_FOUND: vendor credit {id} does not exist"))
}

pub async fn create_vendor_credit(pool: &SqlitePool, input: VendorCreditInput) -> Result<VendorCreditSummary, String> {
    let id = run(pool, async |tx| write_vendor_credit(tx, &input).await).await?;
    vendor_credit(pool, id).await
}

async fn write_vendor_credit(tx: &mut Transaction<'_, Sqlite>, input: &VendorCreditInput) -> Result<i64, String> {
    if input.lines.is_empty() {
        return Err("VC_NO_ITEMS: add at least one line".into());
    }
    let mut total: i64 = 0;
    for (idx, line) in input.lines.iter().enumerate() {
        if !(1..=MAX_LINE_CENTS).contains(&line.amount_cents) {
            return Err(format!("VC_BAD_AMOUNT: line {} amount must be greater than zero", idx + 1));
        }
        total = total.checked_add(line.amount_cents).filter(|t| *t <= MAX_LINE_CENTS).ok_or("VC_TOO_LARGE: the credit is too large")?;
    }
    let vendor = input.vendor_name.trim();
    let vendor_id = find_or_create_named(tx, Party::Vendor, vendor).await?;
    let credit_id: i64 = sqlx::query_scalar(
        "INSERT INTO vendor_credits (vendor_id, reference, credit_date, memo, total_cents) VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id",
    )
    .bind(vendor_id)
    .bind(clean(input.reference.as_deref()))
    .bind(input.credit_date)
    .bind(clean(input.memo.as_deref()))
    .bind(total)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    let mut lines = vec![JournalLine::debit(system_account(tx, "ACCOUNTS_PAYABLE").await?, total)];
    for (idx, line) in input.lines.iter().enumerate() {
        let n = idx + 1;
        let row: Option<(String, String, bool, bool)> = sqlx::query_as(
            "SELECT t.classification, a.account_type, a.is_active, a.is_system FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1",
        )
        .bind(line.account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        match row {
            None => return Err(format!("VC_UNKNOWN_ACCOUNT: line {n} references an account that does not exist")),
            Some((_, _, false, _)) => return Err(format!("VC_INACTIVE_ACCOUNT: line {n} uses an inactive account")),
            Some((class, kind, _, is_system))
                if is_system || !(class == "EXPENSE" || matches!(kind.as_str(), "FIXED_ASSET" | "OTHER_ASSET" | "OTHER_CURRENT_ASSET")) =>
            {
                return Err(format!("VC_BAD_CATEGORY: line {n} must use an expense or asset category"))
            }
            Some(_) => {}
        }
        if let Some(class) = line.class_id {
            crate::classes::check_usable(tx, class, &format!("VC: line {n}")).await?;
        }
        sqlx::query("INSERT INTO vendor_credit_lines (vendor_credit_id, line_number, account_id, amount_cents, memo, class_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)")
            .bind(credit_id)
            .bind(n as i64)
            .bind(line.account_id)
            .bind(line.amount_cents)
            .bind(clean(line.memo.as_deref()))
            .bind(line.class_id)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        lines.push(JournalLine::credit(line.account_id, line.amount_cents).with_memo(line.memo.as_deref()).with_class(line.class_id));
    }
    let reference = clean(input.reference.as_deref()).map(|r| format!(" #{r}")).unwrap_or_default();
    let entry = JournalEntry { id: None, date: input.credit_date, description: format!("Vendor credit{reference} - {vendor}"), lines, currency: None };
    let entry_id = post_entry_in_tx(tx, &entry, EntrySource::VendorCredit(credit_id)).await?;
    sqlx::query("UPDATE vendor_credits SET journal_entry_id = ?1 WHERE id = ?2").bind(entry_id).bind(credit_id).execute(&mut **tx).await.map_err(db_err)?;
    Ok(credit_id)
}

/// Uses `amount_cents` of a vendor credit against one of the same vendor's open bills.
pub async fn apply_vendor_credit(pool: &SqlitePool, input: ApplyCreditInput) -> Result<VendorCreditSummary, String> {
    run(pool, async |tx| {
        let credit: Option<(i64, i64, bool)> = sqlx::query_as(
            "SELECT c.vendor_id, b.remaining_cents, c.is_voided FROM vendor_credits c JOIN v_vendor_credit_balances b ON b.vendor_credit_id = c.id WHERE c.id = ?1",
        )
        .bind(input.credit_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        let (vendor, remaining, voided) = credit.ok_or("VC_NOT_FOUND: that vendor credit does not exist")?;
        if voided {
            return Err("VC_NOT_AVAILABLE: that vendor credit is voided".into());
        }
        let bill: Option<(i64, i64, i64, bool, String)> =
            sqlx::query_as("SELECT vendor_id, total_cents, amount_paid_cents, is_voided, status FROM bills WHERE id = ?1")
                .bind(input.document_id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(db_err)?;
        let (bill_vendor, total, paid, bill_voided, status) = bill.ok_or("VC_BILL_NOT_FOUND: that bill does not exist")?;
        if bill_vendor != vendor {
            return Err("VC_OTHER_VENDOR: a vendor credit can only be applied to the same vendor's bills".into());
        }
        if bill_voided || !(status == "UNPAID" || status == "PARTIAL") {
            return Err("VC_BILL_NOT_OPEN: that bill has nothing left to pay".into());
        }
        if input.amount_cents < 1 || input.amount_cents > remaining || input.amount_cents > total - paid {
            return Err(format!("VC_BAD_AMOUNT: apply up to {} (the smaller of the credit left and the bill balance)", format_cents(remaining.min(total - paid))));
        }
        sqlx::query("INSERT INTO vendor_credit_applications (vendor_credit_id, bill_id, applied_date, amount_cents) VALUES (?1, ?2, ?3, ?4)")
            .bind(input.credit_id)
            .bind(input.document_id)
            .bind(input.applied_date)
            .bind(input.amount_cents)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        recompute_paid(tx, "bills", "bill_payments", "bill_id", input.document_id).await
    })
    .await?;
    vendor_credit(pool, input.credit_id).await
}

pub async fn list_vendor_credit_applications(pool: &SqlitePool, vendor_credit_id: i64) -> Result<Vec<CreditApplicationView>, String> {
    sqlx::query_as::<_, CreditApplicationView>(
        "SELECT a.id, a.vendor_credit_id AS credit_id, a.bill_id AS document_id,
                'Bill' || COALESCE(' #' || b.bill_number, ' of ' || b.bill_date) AS document_label, a.applied_date, a.amount_cents
         FROM vendor_credit_applications a JOIN bills b ON b.id = a.bill_id
         WHERE a.vendor_credit_id = ?1 AND a.is_voided = 0 ORDER BY a.id",
    )
    .bind(vendor_credit_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn remove_vendor_credit(pool: &SqlitePool, application_id: i64) -> Result<(), String> {
    run(pool, async |tx| {
        let bill: Option<i64> = sqlx::query_scalar("SELECT bill_id FROM vendor_credit_applications WHERE id = ?1 AND is_voided = 0")
            .bind(application_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
        let bill = bill.ok_or("VC_APPLICATION_NOT_FOUND: that credit is not applied")?;
        sqlx::query("UPDATE vendor_credit_applications SET is_voided = 1 WHERE id = ?1").bind(application_id).execute(&mut **tx).await.map_err(db_err)?;
        recompute_paid(tx, "bills", "bill_payments", "bill_id", bill).await
    })
    .await
}

/// Records a vendor paying credit back to us: Dr the bank or card account / Cr A/P.
pub async fn refund_vendor_credit(pool: &SqlitePool, input: RefundCreditInput) -> Result<VendorCreditSummary, String> {
    run(pool, async |tx| {
        let credit: Option<(String, i64, bool)> = sqlx::query_as(
            "SELECT v.name, b.remaining_cents, c.is_voided FROM vendor_credits c JOIN vendors v ON v.id = c.vendor_id
             JOIN v_vendor_credit_balances b ON b.vendor_credit_id = c.id WHERE c.id = ?1",
        )
        .bind(input.credit_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        let (vendor, remaining, voided) = credit.ok_or("VC_NOT_FOUND: that vendor credit does not exist")?;
        if voided {
            return Err("VC_NOT_AVAILABLE: that vendor credit is voided".into());
        }
        if input.amount_cents < 1 || input.amount_cents > remaining {
            return Err(format!("VC_BAD_AMOUNT: a refund can be up to the {} of credit left", format_cents(remaining)));
        }
        check_money_account(tx, input.account_id, "VC").await?;
        let ap = system_account(tx, "ACCOUNTS_PAYABLE").await?;
        let reference = clean(input.reference.as_deref()).map(|r| format!(" ({r})")).unwrap_or_default();
        let entry = JournalEntry {
            id: None,
            date: input.refund_date,
            description: format!("Vendor refund - {vendor}{reference}"),
            lines: vec![JournalLine::debit(input.account_id, input.amount_cents), JournalLine::credit(ap, input.amount_cents)],
            currency: None,
        };
        let entry_id = post_entry_in_tx(tx, &entry, EntrySource::Journal).await?;
        sqlx::query(
            "INSERT INTO vendor_credit_refunds (vendor_credit_id, account_id, refund_date, amount_cents, reference, journal_entry_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(input.credit_id)
        .bind(input.account_id)
        .bind(input.refund_date)
        .bind(input.amount_cents)
        .bind(clean(input.reference.as_deref()))
        .bind(entry_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
        Ok(())
    })
    .await?;
    vendor_credit(pool, input.credit_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;

    async fn account(pool: &SqlitePool, number: &str) -> i64 {
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
    }

    async fn balance(pool: &SqlitePool, account: i64) -> i64 {
        sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(account).fetch_one(pool).await.unwrap()
    }

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 6, day).unwrap()
    }

    fn line(account: i64, cents: i64) -> SalesLineInput {
        SalesLineInput { product_service_id: None, description: "Returned work".into(), quantity_e4: 10_000, rate_cents: cents, income_account_id: account, class_id: None, is_taxable: false }
    }

    async fn invoice(pool: &SqlitePool, customer: &str, services: i64, cents: i64) -> i64 {
        let input: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
            "customerName": customer, "issueDate": "2026-06-01", "dueDate": "2026-07-01",
            "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }]
        }))
        .unwrap();
        crate::invoices::create_and_post_invoice(pool, input).await.unwrap().id
    }

    async fn invoice_state(pool: &SqlitePool, id: i64) -> (i64, String, i64) {
        sqlx::query_as("SELECT i.amount_paid_cents, i.status, b.home_open_cents FROM invoices i JOIN v_invoice_balances b ON b.invoice_id = i.id WHERE i.id = ?1")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn credit_memos_reduce_receivables_apply_to_invoices_and_refund() {
        let pool = test_pool().await;
        let (checking, ar, services) = (account(&pool, "1010").await, account(&pool, "1200").await, account(&pool, "4100").await);
        let inv = invoice(&pool, "Acme", services, 50_000).await;
        let other = invoice(&pool, "Other Co", services, 10_000).await;

        let memo = create_credit_memo(&pool, CreditMemoInput { customer_name: "acme".into(), credit_date: d(5), memo: None, refund_account_id: None, lines: vec![line(services, 20_000)], tax_rate_id: None })
            .await
            .unwrap();
        assert_eq!((memo.customer_name.as_str(), memo.remaining_cents), ("Acme", 20_000));
        assert_eq!(balance(&pool, ar).await, 40_000, "A/R: 50,000 + 10,000 invoiced less 20,000 credit");
        assert_eq!(balance(&pool, services).await, -40_000);

        let apply = |doc, cents| apply_customer_credit(&pool, ApplyCreditInput { credit_id: memo.id, document_id: doc, amount_cents: cents, applied_date: d(6) });
        assert!(apply(other, 1_000).await.unwrap_err().starts_with("CM_OTHER_CUSTOMER"));
        assert!(apply(inv, 25_000).await.unwrap_err().starts_with("CM_BAD_AMOUNT"));
        assert_eq!(apply(inv, 15_000).await.unwrap().remaining_cents, 5_000);
        assert_eq!(invoice_state(&pool, inv).await, (15_000, "PARTIAL".into(), 35_000));
        assert_eq!(balance(&pool, ar).await, 40_000, "applying a credit posts nothing");

        // The invoice can still be paid; the credit counts toward what is paid.
        let pay = crate::invoices::PaymentInput {
            invoice_id: inv,
            deposit_account_id: checking,
            payment_date: d(7),
            amount_cents: 35_000,
            payment_method: "CHECK".into(),
            reference: None,
            exchange_rate_e6: None,
        };
        crate::invoices::record_payment(&pool, pay).await.unwrap();
        assert_eq!(invoice_state(&pool, inv).await, (50_000, "PAID".into(), 0));

        // An invoice with a credit applied cannot be voided; nor can a credit memo in use.
        let memo_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM credit_memos WHERE id = ?1").bind(memo.id).fetch_one(&pool).await.unwrap();
        assert!(crate::ledger::void_transaction(&pool, memo_entry).await.unwrap_err().starts_with("VOID_CREDIT_USED"));

        // Refund the 5,000 left, then undo it by voiding the refund's entry.
        let refunded = refund_customer_credit(&pool, RefundCreditInput { credit_id: memo.id, account_id: checking, refund_date: d(8), amount_cents: 5_000, reference: Some("Chk 101".into()) })
            .await
            .unwrap();
        assert_eq!(refunded.remaining_cents, 0);
        assert_eq!(balance(&pool, ar).await, 10_000 + 0, "only the other customer's invoice is still owed");
        let refund_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM credit_refunds").fetch_one(&pool).await.unwrap();
        crate::ledger::void_transaction(&pool, refund_entry).await.unwrap();
        assert_eq!(credit_memo(&pool, memo.id).await.unwrap().remaining_cents, 5_000);

        let applications = list_customer_credit_applications(&pool, memo.id).await.unwrap();
        assert_eq!(applications.len(), 1);
        assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
        let party: Option<String> = sqlx::query_scalar("SELECT party_name FROM v_entry_party WHERE journal_entry_id = ?1").bind(memo_entry).fetch_one(&pool).await.unwrap();
        assert_eq!(party.as_deref(), Some("Acme"));
    }

    #[tokio::test]
    async fn removing_a_credit_reopens_the_invoice_and_refund_receipts_credit_the_bank() {
        let pool = test_pool().await;
        let (checking, ar, services) = (account(&pool, "1010").await, account(&pool, "1200").await, account(&pool, "4100").await);
        let inv = invoice(&pool, "Acme", services, 30_000).await;
        let memo = create_credit_memo(&pool, CreditMemoInput { customer_name: "Acme".into(), credit_date: d(5), memo: None, refund_account_id: None, lines: vec![line(services, 30_000)], tax_rate_id: None })
            .await
            .unwrap();
        apply_customer_credit(&pool, ApplyCreditInput { credit_id: memo.id, document_id: inv, amount_cents: 30_000, applied_date: d(6) }).await.unwrap();
        assert_eq!(invoice_state(&pool, inv).await, (30_000, "PAID".into(), 0));
        let app = list_customer_credit_applications(&pool, memo.id).await.unwrap()[0].id;
        remove_customer_credit(&pool, app).await.unwrap();
        assert_eq!(invoice_state(&pool, inv).await, (0, "UNPAID".into(), 30_000));
        assert_eq!(credit_memo(&pool, memo.id).await.unwrap().remaining_cents, 30_000);

        // A refund receipt pays the customer back at once: no credit is left to apply.
        let before = balance(&pool, checking).await;
        let receipt = create_credit_memo(
            &pool,
            CreditMemoInput { customer_name: "Walk-in".into(), credit_date: d(9), memo: None, refund_account_id: Some(checking), lines: vec![line(services, 4_500)], tax_rate_id: None },
        )
        .await
        .unwrap();
        assert_eq!((receipt.remaining_cents, balance(&pool, checking).await - before), (0, -4_500));
        assert_eq!(balance(&pool, ar).await, 0, "invoice 30,000 less the 30,000 credit memo; the refund receipt never touches A/R");
        let err = apply_customer_credit(&pool, ApplyCreditInput { credit_id: receipt.id, document_id: inv, amount_cents: 100, applied_date: d(9) }).await.unwrap_err();
        assert!(err.starts_with("CM_NOT_AVAILABLE"), "{err}");
        let receipt_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM credit_memos WHERE id = ?1").bind(receipt.id).fetch_one(&pool).await.unwrap();
        crate::ledger::void_transaction(&pool, receipt_entry).await.unwrap();
        assert!(list_credit_memos(&pool).await.unwrap().iter().any(|m| m.id == receipt.id && m.is_voided));
    }

    #[tokio::test]
    async fn vendor_credits_reduce_payables_and_apply_to_bills() {
        let pool = test_pool().await;
        let (checking, ap, supplies) = (account(&pool, "1010").await, account(&pool, "2000").await, account(&pool, "6250").await);
        let bill: crate::expenses::BillInput = serde_json::from_value(serde_json::json!({
            "vendorName": "Staples", "billNumber": "B-1", "billDate": "2026-06-01", "dueDate": "2026-07-01", "memo": null, "totalCents": 40_000,
            "lines": [{ "accountId": supplies, "amountCents": 40_000, "memo": null }]
        }))
        .unwrap();
        let bill = crate::expenses::post_vendor_bill(&pool, bill).await.unwrap().id;
        let credit = create_vendor_credit(
            &pool,
            VendorCreditInput { vendor_name: "staples".into(), reference: Some("RMA-7".into()), credit_date: d(3), memo: None, lines: vec![VendorCreditLineInput { account_id: supplies, amount_cents: 12_000, memo: Some("Returned toner".into()), class_id: None }] },
        )
        .await
        .unwrap();
        assert_eq!(balance(&pool, ap).await, -28_000);
        assert_eq!(balance(&pool, supplies).await, 28_000);

        let applied = apply_vendor_credit(&pool, ApplyCreditInput { credit_id: credit.id, document_id: bill, amount_cents: 10_000, applied_date: d(4) }).await.unwrap();
        assert_eq!(applied.remaining_cents, 2_000);
        let (paid, status): (i64, String) = sqlx::query_as("SELECT amount_paid_cents, status FROM bills WHERE id = ?1").bind(bill).fetch_one(&pool).await.unwrap();
        assert_eq!((paid, status.as_str()), (10_000, "PARTIAL"));

        let refunded = refund_vendor_credit(&pool, RefundCreditInput { credit_id: credit.id, account_id: checking, refund_date: d(5), amount_cents: 2_000, reference: None }).await.unwrap();
        assert_eq!(refunded.remaining_cents, 0);
        assert_eq!(balance(&pool, ap).await, -30_000, "bill 40,000 less the credit's 10,000 applied; the 2,000 refunded came back to the bank");
        assert_eq!(balance(&pool, checking).await, 2_000);
        let app = list_vendor_credit_applications(&pool, credit.id).await.unwrap()[0].id;
        remove_vendor_credit(&pool, app).await.unwrap();
        let paid: i64 = sqlx::query_scalar("SELECT amount_paid_cents FROM bills WHERE id = ?1").bind(bill).fetch_one(&pool).await.unwrap();
        assert_eq!(paid, 0);
    }
}
