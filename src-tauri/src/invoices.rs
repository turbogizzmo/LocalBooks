//! Invoices: creation posts Dr Accounts Receivable / Cr income; each payment (full or partial) posts
//! Dr deposit / Cr A/R. Every document row and its journal entry are written in one transaction.
//!
//! Quantities are integers scaled by 10,000 (`quantity_e4`), so 1.5 hours is 15_000; a line's amount
//! is round_half_up(quantity_e4 × rate_cents / 10,000).
//!
//! Foreign-currency invoices keep their lines, total and payments in the invoice currency and book
//! A/R in home currency at the invoice's exchange rate. A payment converts at its own rate: the cash
//! received and the A/R relieved can differ, and the difference is a realized exchange gain or loss.
//! A/R is relieved cumulatively (in proportion to the amount paid so far), so the final payment
//! clears the home-currency A/R exactly.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{
    convert_lines, db_err, format_cents, mul_div_round, post_entry_in_tx, quantity_amount, rollback, to_home_cents,
    EntrySource, ForeignCurrency, JournalEntry, JournalLine, MAX_LINE_CENTS, QUANTITY_SCALE,
};

/// Largest line quantity (1,000,000,000 units), scaled by 10,000.
pub const MAX_QUANTITY_E4: i64 = 1_000_000_000 * QUANTITY_SCALE;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceLineInput {
    pub description: String,
    /// Quantity scaled by 10,000 (1.5 → 15_000).
    pub quantity_e4: i64,
    pub rate_cents: i64,
    pub income_account_id: i64,
    /// The product or service sold; a tracked inventory product also records its cost of goods sold.
    #[serde(default)]
    pub product_service_id: Option<i64>,
    #[serde(default)]
    pub class_id: Option<i64>,
    #[serde(default)]
    pub is_taxable: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceInput {
    pub customer_name: String,
    pub issue_date: NaiveDate,
    pub due_date: NaiveDate,
    pub lines: Vec<InvoiceLineInput>,
    /// Set for invoices issued in a foreign currency; rates and totals are then in that currency.
    #[serde(default)]
    pub currency: Option<ForeignCurrency>,
    /// Sales tax rate applied to the taxable lines.
    #[serde(default)]
    pub tax_rate_id: Option<i64>,
    /// Taken off the subtotal before tax.
    #[serde(default)]
    pub discount_cents: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceSummary {
    pub id: i64,
    pub invoice_number: i64,
    pub tax_rate_id: Option<i64>,
    pub customer_id: Option<i64>,
    /// The customer's current name (renaming a customer renames it on its invoices too).
    pub customer_name: String,
    pub issue_date: String,
    pub due_date: String,
    pub total_cents: i64,
    pub amount_paid_cents: i64,
    pub balance_cents: i64,
    pub status: String,
    pub journal_entry_id: Option<i64>,
    pub last_payment_date: Option<String>,
    pub item_count: i64,
    pub is_voided: bool,
    /// `None` for home-currency invoices.
    pub currency_code: Option<String>,
    pub exchange_rate_e6: Option<i64>,
    /// A/R booked in home currency, and the home-currency A/R still open.
    pub home_total_cents: i64,
    pub home_balance_cents: i64,
    pub tax_cents: i64,
    pub tax_rate_name: Option<String>,
    pub discount_cents: i64,
    /// Credit memos applied to this invoice (included in amount paid).
    pub credits_applied_cents: i64,
}

const SUMMARY_SELECT: &str = "
    SELECT i.id, i.invoice_number, i.tax_rate_id, i.customer_id, COALESCE(c.name, i.customer_name) AS customer_name, i.issue_date, i.due_date, i.total_cents,
           i.amount_paid_cents, i.total_cents - i.amount_paid_cents AS balance_cents, i.status,
           i.journal_entry_id,
           (SELECT MAX(payment_date) FROM invoice_payments WHERE invoice_id = i.id AND is_voided = 0) AS last_payment_date,
           (SELECT COUNT(*) FROM invoice_items WHERE invoice_id = i.id) AS item_count,
           i.is_voided, i.currency_code, i.exchange_rate_e6,
           b.home_total_cents, CASE WHEN i.is_voided = 1 THEN 0 ELSE b.home_open_cents END AS home_balance_cents,
           i.tax_cents, t.name AS tax_rate_name, i.discount_cents,
           COALESCE((SELECT SUM(amount_cents) FROM credit_applications WHERE invoice_id = i.id AND is_voided = 0), 0) AS credits_applied_cents
    FROM invoices i JOIN v_invoice_balances b ON b.invoice_id = i.id LEFT JOIN customers c ON c.id = i.customer_id
    LEFT JOIN tax_rates t ON t.id = i.tax_rate_id";

pub const PAYMENT_METHODS: &[&str] = &["CASH", "CHECK", "CREDIT_CARD", "ACH", "WIRE", "OTHER"];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentInput {
    pub invoice_id: i64,
    pub deposit_account_id: i64,
    pub payment_date: NaiveDate,
    /// In the invoice's currency.
    pub amount_cents: i64,
    pub payment_method: String,
    pub reference: Option<String>,
    /// Payment-date exchange rate for a foreign-currency invoice; defaults to the invoice's rate.
    #[serde(default)]
    pub exchange_rate_e6: Option<i64>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PaymentView {
    pub id: i64,
    pub payment_date: String,
    pub amount_cents: i64,
    pub payment_method: String,
    pub reference: Option<String>,
    pub deposit_account_id: i64,
    pub journal_entry_id: Option<i64>,
    pub is_voided: bool,
    pub exchange_rate_e6: Option<i64>,
    /// Home-currency cash received and A/R relieved (`None` for home-currency payments).
    pub home_amount_cents: Option<i64>,
    pub ar_relief_cents: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgingBucket {
    /// `current`, `days1To30`, `days31To60`, `days61Plus`
    pub key: &'static str,
    pub label: &'static str,
    pub invoice_count: i64,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArAging {
    pub as_of: NaiveDate,
    pub buckets: Vec<AgingBucket>,
    pub total_cents: i64,
}

/// round_half_up(quantity_e4 × rate / 10,000), rejecting zero, overflow and the per-line ceiling.
pub fn line_amount(quantity_e4: i64, rate_cents: i64) -> Result<i64, String> {
    let amount = quantity_amount(quantity_e4, rate_cents)
        .filter(|amount| *amount <= MAX_LINE_CENTS)
        .ok_or_else(|| format!("INV_AMOUNT_TOO_LARGE: line amount exceeds {}", format_cents(MAX_LINE_CENTS)))?;
    if amount < 1 {
        return Err("INV_AMOUNT_ROUNDS_TO_ZERO: quantity × rate rounds to 0.00".into());
    }
    Ok(amount)
}

/// Validates a scaled quantity and rate for a sales line (invoice, sales receipt, estimate).
pub fn validate_line(quantity_e4: i64, rate_cents: i64, n: usize, code: &str) -> Result<i64, String> {
    if !(1..=MAX_QUANTITY_E4).contains(&quantity_e4) {
        return Err(format!("{code}_BAD_QUANTITY: line {n} quantity must be between 0.0001 and 1,000,000,000"));
    }
    if rate_cents <= 0 {
        return Err(format!("{code}_BAD_RATE: line {n} rate must be greater than zero"));
    }
    line_amount(quantity_e4, rate_cents).map_err(|e| format!("{e} (line {n})"))
}

/// Pure validation; returns the invoice total in cents.
pub fn validate_invoice(input: &InvoiceInput) -> Result<i64, String> {
    if input.customer_name.trim().is_empty() {
        return Err("INV_NO_CUSTOMER: enter a customer name".into());
    }
    if input.due_date < input.issue_date {
        return Err("INV_DUE_BEFORE_ISSUE: due date cannot be earlier than the invoice date".into());
    }
    if input.lines.is_empty() {
        return Err("INV_NO_ITEMS: an invoice requires at least one line item".into());
    }

    if let Some(currency) = &input.currency {
        currency.validate()?;
    }
    let mut total: i64 = 0;
    for (idx, line) in input.lines.iter().enumerate() {
        let amount = validate_line(line.quantity_e4, line.rate_cents, idx + 1, "INV")?;
        total = total
            .checked_add(amount)
            .filter(|t| *t <= MAX_LINE_CENTS)
            .ok_or_else(|| format!("INV_AMOUNT_TOO_LARGE: invoice total exceeds {}", format_cents(MAX_LINE_CENTS)))?;
    }
    Ok(total)
}

pub async fn list_invoices(pool: &SqlitePool) -> Result<Vec<InvoiceSummary>, String> {
    sqlx::query_as::<_, InvoiceSummary>(&format!("{SUMMARY_SELECT} ORDER BY i.is_voided, i.invoice_number DESC"))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

pub(crate) async fn get_summary(pool: &SqlitePool, invoice_id: i64) -> Result<InvoiceSummary, String> {
    sqlx::query_as::<_, InvoiceSummary>(&format!("{SUMMARY_SELECT} WHERE i.id = ?1"))
        .bind(invoice_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

/// Creates the invoice and posts it to the ledger atomically; the invoice comes back UNPAID.
pub async fn create_and_post_invoice(pool: &SqlitePool, input: InvoiceInput) -> Result<InvoiceSummary, String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let invoice_id = match write_invoice(&mut tx, &input).await {
        Ok(id) => id,
        Err(err) => return Err(rollback(tx, err).await),
    };
    tx.commit().await.map_err(db_err)?;
    get_summary(pool, invoice_id).await
}

pub(crate) async fn write_invoice(tx: &mut Transaction<'_, Sqlite>, input: &InvoiceInput) -> Result<i64, String> {
    let subtotal = validate_invoice(input)?;
    let mut taxable = 0;
    for line in input.lines.iter().filter(|l| l.is_taxable) {
        taxable += line_amount(line.quantity_e4, line.rate_cents)?;
    }
    let discount = crate::sales_tax::document_discount(tx, input.discount_cents, subtotal, input.lines.iter().map(|l| l.class_id), "INV").await?;
    let taxable = crate::sales_tax::taxable_after_discount(taxable, input.discount_cents, subtotal);
    let tax = crate::sales_tax::document_tax(tx, input.tax_rate_id, taxable, "INV").await?;
    let tax_cents = tax.map_or(0, |t| t.cents);
    let total = (subtotal - input.discount_cents).checked_add(tax_cents).filter(|t| *t > 0 && *t <= MAX_LINE_CENTS).ok_or("INV_AMOUNT_TOO_LARGE: the invoice total must be above zero and in range")?;
    let customer = input.customer_name.trim();
    let customer_id = crate::sales::find_or_create_named(tx, crate::sales::Party::Customer, customer).await?;

    // Credits aggregated per income account and class, in first-appearance order (in the invoice currency).
    let mut credits: Vec<((i64, Option<i64>), i64)> = Vec::new();
    for line in &input.lines {
        let amount = line_amount(line.quantity_e4, line.rate_cents)?;
        let key = (line.income_account_id, line.class_id);
        match credits.iter_mut().find(|(k, _)| *k == key) {
            Some((_, sum)) => *sum += amount,
            None => credits.push((key, amount)),
        }
    }
    let ar_account = system_account(tx, "ACCOUNTS_RECEIVABLE").await?;
    let mut lines = vec![JournalLine::debit(ar_account, total)];
    lines.extend(credits.into_iter().map(|((account, class), cents)| JournalLine::credit(account, cents).with_class(class)));
    lines.extend(discount.and_then(|d| d.line()));
    lines.extend(tax.and_then(|t| t.line(false)));
    // Tracked products also move their cost from Inventory Asset to Cost of Goods Sold, in this entry.
    let sold: Vec<(Option<i64>, i64, Option<i64>)> = input.lines.iter().map(|l| (l.product_service_id, l.quantity_e4, l.class_id)).collect();
    let cost = crate::inventory::cost_of_sale(tx, &sold).await?;
    if !cost.is_empty() && input.currency.is_some() {
        return Err("INV_FX_INVENTORY: inventory items can only be sold on home-currency invoices".into());
    }
    // The ledger converts foreign lines deterministically; the A/R debit it will post is the home total.
    let home_total = match &input.currency {
        Some(currency) => Some(convert_lines(&lines, currency.exchange_rate_e6)?[0].0),
        None => None,
    };

    let (invoice_id, invoice_number): (i64, i64) = sqlx::query_as(
        "INSERT INTO invoices
            (invoice_number, customer_name, issue_date, due_date, total_cents, status, customer_id,
             currency_code, exchange_rate_e6, home_total_cents, tax_rate_id, tax_cents, discount_cents)
         VALUES ((SELECT COALESCE(MAX(invoice_number), 1000) + 1 FROM invoices), ?1, ?2, ?3, ?4, 'DRAFT', ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         RETURNING id, invoice_number",
    )
    .bind(customer)
    .bind(input.issue_date)
    .bind(input.due_date)
    .bind(total)
    .bind(customer_id)
    .bind(input.currency.as_ref().map(|c| c.code.as_str()))
    .bind(input.currency.as_ref().map(|c| c.exchange_rate_e6))
    .bind(home_total)
    .bind(tax.and_then(|t| t.rate_id))
    .bind(tax_cents)
    .bind(input.discount_cents)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    for (idx, line) in input.lines.iter().enumerate() {
        let n = idx + 1;
        let account: Option<(String, i64)> = sqlx::query_as(
            "SELECT t.classification, a.is_active
             FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type
             WHERE a.id = ?1",
        )
        .bind(line.income_account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        match account {
            None => return Err(format!("INV_UNKNOWN_ACCOUNT: line {n} references an account that does not exist")),
            Some((classification, _)) if classification != "INCOME" => {
                return Err(format!("INV_NOT_INCOME_ACCOUNT: line {n} must use an Income account"))
            }
            Some((_, 0)) => return Err(format!("INV_INACTIVE_ACCOUNT: line {n} uses an inactive account")),
            Some(_) => {}
        }
        if let Some(class) = line.class_id {
            crate::classes::check_usable(tx, class, &format!("INV: line {n}")).await?;
        }

        let amount = line_amount(line.quantity_e4, line.rate_cents)?;
        sqlx::query(
            "INSERT INTO invoice_items
                (invoice_id, line_number, description, quantity_e4, rate_cents, amount_cents, income_account_id, product_service_id, class_id, is_taxable)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .bind(invoice_id)
        .bind(n as i64)
        .bind(line.description.trim())
        .bind(line.quantity_e4)
        .bind(line.rate_cents)
        .bind(amount)
        .bind(line.income_account_id)
        .bind(line.product_service_id)
        .bind(line.class_id)
        .bind(line.is_taxable && tax.is_some())
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }

    lines.extend(cost.lines.iter().cloned());
    let entry = JournalEntry {
        id: None,
        date: input.issue_date,
        description: format!("Invoice #{invoice_number} - {customer}"),
        lines,
        currency: input.currency.clone(),
    };
    let entry_id = post_entry_in_tx(tx, &entry, EntrySource::Invoice(invoice_id)).await?;
    cost.record(tx, input.issue_date, entry_id).await?;

    sqlx::query("UPDATE invoices SET status = 'UNPAID', journal_entry_id = ?1 WHERE id = ?2")
        .bind(entry_id)
        .bind(invoice_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;

    Ok(invoice_id)
}

/// Applies a full or partial payment to an open invoice. The invoice becomes PARTIAL while a balance
/// remains and PAID once the payments sum to the total; overpayment is rejected.
pub async fn record_payment(pool: &SqlitePool, input: PaymentInput) -> Result<InvoiceSummary, String> {
    let invoice_id = input.invoice_id;
    record_payments(pool, vec![input]).await?;
    get_summary(pool, invoice_id).await
}

/// Receive Payment: applies one customer payment across several invoices. All post, or none do.
pub async fn record_payments(pool: &SqlitePool, inputs: Vec<PaymentInput>) -> Result<usize, String> {
    if inputs.is_empty() {
        return Err("INV_NOTHING_TO_APPLY: enter an amount for at least one invoice".into());
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    for input in &inputs {
        if let Err(err) = write_payment(&mut tx, input).await {
            return Err(rollback(tx, err).await);
        }
    }
    tx.commit().await.map_err(db_err)?;
    Ok(inputs.len())
}

async fn write_payment(tx: &mut Transaction<'_, Sqlite>, input: &PaymentInput) -> Result<(), String> {
    if !PAYMENT_METHODS.contains(&input.payment_method.as_str()) {
        return Err(format!("INV_BAD_METHOD: unknown payment method '{}'", input.payment_method));
    }
    if input.amount_cents <= 0 {
        return Err("INV_BAD_AMOUNT: payment amount must be greater than zero".into());
    }

    type InvoiceRow = (i64, String, i64, i64, String, String, bool, Option<String>, Option<i64>, i64, i64);
    let invoice: Option<InvoiceRow> = sqlx::query_as(
        "SELECT i.invoice_number, i.customer_name, i.total_cents, i.amount_paid_cents, i.status, i.issue_date, i.is_voided,
                i.currency_code, i.exchange_rate_e6, b.home_total_cents, b.home_total_cents - b.home_open_cents
         FROM invoices i JOIN v_invoice_balances b ON b.invoice_id = i.id WHERE i.id = ?1",
    )
    .bind(input.invoice_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let (number, customer, total, paid, status, issue_date, voided, currency_code, invoice_rate, home_total, relieved_before) =
        invoice.ok_or_else(|| format!("INV_NOT_FOUND: invoice {} does not exist", input.invoice_id))?;
    if voided {
        return Err(format!("INV_NOT_OPEN: invoice #{number} is voided"));
    }
    if status != "UNPAID" && status != "PARTIAL" {
        return Err(format!("INV_NOT_OPEN: invoice #{number} is {status}; only open invoices accept payments"));
    }
    let open_balance = total - paid;
    if input.amount_cents > open_balance {
        return Err(format!(
            "INV_OVERPAYMENT: payment of {} exceeds the open balance of {} on invoice #{number}",
            format_cents(input.amount_cents),
            format_cents(open_balance)
        ));
    }
    if input.payment_date.format("%Y-%m-%d").to_string() < issue_date {
        return Err(format!("INV_PAYMENT_BEFORE_ISSUE: payment date is earlier than invoice #{number}'s date"));
    }

    let deposit_type: Option<String> = sqlx::query_scalar("SELECT account_type FROM chart_of_accounts WHERE id = ?1")
        .bind(input.deposit_account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    match deposit_type.as_deref() {
        Some("BANK") | Some("OTHER_CURRENT_ASSET") => {}
        Some(_) => return Err("INV_BAD_DEPOSIT_ACCOUNT: deposit to a Bank or Undeposited Funds account".into()),
        None => return Err("INV_BAD_DEPOSIT_ACCOUNT: deposit account does not exist".into()),
    }

    // Home-currency effect: cash received at the payment rate, A/R relieved at the invoice's booked
    // rate (cumulatively, so the last payment clears A/R exactly), and the realized difference.
    let new_paid = paid + input.amount_cents;
    let fx = match (&currency_code, invoice_rate) {
        (Some(_), Some(invoice_rate)) => {
            let rate = input.exchange_rate_e6.unwrap_or(invoice_rate);
            if rate <= 0 {
                return Err("FX_BAD_RATE: the exchange rate must be greater than zero".into());
            }
            let home_amount = to_home_cents(input.amount_cents, rate)
                .filter(|h| *h > 0)
                .ok_or_else(|| "FX_ROUNDS_TO_ZERO: the payment converts to 0.00 in the home currency".to_string())?;
            let relieved_after = if new_paid == total {
                home_total
            } else {
                mul_div_round(home_total, new_paid, total).ok_or_else(|| "FX_OVERFLOW: amount out of range".to_string())?
            };
            Some((rate, home_amount, relieved_after - relieved_before))
        }
        _ if input.exchange_rate_e6.is_some() => {
            return Err(format!("FX_IS_HOME: invoice #{number} is in the home currency; record the payment without an exchange rate"))
        }
        _ => None,
    };

    let reference = input.reference.as_deref().map(str::trim).filter(|r| !r.is_empty());
    let payment_id: i64 = sqlx::query_scalar(
        "INSERT INTO invoice_payments
            (invoice_id, payment_date, amount_cents, payment_method, reference, deposit_account_id,
             exchange_rate_e6, home_amount_cents, ar_relief_cents)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         RETURNING id",
    )
    .bind(input.invoice_id)
    .bind(input.payment_date)
    .bind(input.amount_cents)
    .bind(&input.payment_method)
    .bind(reference)
    .bind(input.deposit_account_id)
    .bind(fx.map(|f| f.0))
    .bind(fx.map(|f| f.1))
    .bind(fx.map(|f| f.2))
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    let ar_account = system_account(tx, "ACCOUNTS_RECEIVABLE").await?;
    let mut lines = Vec::with_capacity(3);
    match fx {
        None => {
            lines.push(JournalLine::debit(input.deposit_account_id, input.amount_cents));
            lines.push(JournalLine::credit(ar_account, input.amount_cents));
        }
        Some((_, home_amount, ar_relief)) => {
            lines.push(JournalLine::debit(input.deposit_account_id, home_amount));
            if ar_relief > 0 {
                lines.push(JournalLine::credit(ar_account, ar_relief));
            }
            let difference = home_amount - ar_relief;
            if difference != 0 {
                let fx_account = system_account(tx, "OTHER_EXPENSE").await?;
                let memo = Some(if difference > 0 { "Realized exchange gain" } else { "Realized exchange loss" });
                lines.push(if difference > 0 {
                    JournalLine::credit(fx_account, difference).with_memo(memo)
                } else {
                    JournalLine::debit(fx_account, -difference).with_memo(memo)
                });
            }
        }
    }
    let currency_note = currency_code.as_deref().map(|c| format!(" ({c})")).unwrap_or_default();
    let entry = JournalEntry {
        id: None,
        date: input.payment_date,
        description: format!("Payment - Invoice #{number} - {customer}{currency_note}"),
        lines,
        currency: None,
    };
    let entry_id = post_entry_in_tx(tx, &entry, EntrySource::ReceivePayment(payment_id)).await?;

    sqlx::query("UPDATE invoice_payments SET journal_entry_id = ?1 WHERE id = ?2")
        .bind(entry_id)
        .bind(payment_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;

    let new_status = if new_paid == total { "PAID" } else { "PARTIAL" };
    sqlx::query("UPDATE invoices SET amount_paid_cents = ?1, status = ?2 WHERE id = ?3")
        .bind(new_paid)
        .bind(new_status)
        .bind(input.invoice_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;

    Ok(())
}

pub async fn list_payments(pool: &SqlitePool, invoice_id: i64) -> Result<Vec<PaymentView>, String> {
    sqlx::query_as::<_, PaymentView>(
        "SELECT id, payment_date, amount_cents, payment_method, reference, deposit_account_id, journal_entry_id, is_voided,
                exchange_rate_e6, home_amount_cents, ar_relief_cents
         FROM invoice_payments WHERE invoice_id = ?1 ORDER BY payment_date, id",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceItemView {
    pub line_number: i64,
    pub description: String,
    pub quantity_e4: i64,
    pub rate_cents: i64,
    pub amount_cents: i64,
    pub income_account_id: i64,
    pub product_service_id: Option<i64>,
    pub class_id: Option<i64>,
    pub is_taxable: bool,
}

/// Everything needed to print an invoice.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceDetail {
    pub invoice: InvoiceSummary,
    pub items: Vec<InvoiceItemView>,
    pub payments: Vec<PaymentView>,
}

pub async fn invoice_detail(pool: &SqlitePool, invoice_id: i64) -> Result<InvoiceDetail, String> {
    let invoice = get_summary(pool, invoice_id).await?;
    let items = sqlx::query_as::<_, InvoiceItemView>(
        "SELECT line_number, description, quantity_e4, rate_cents, amount_cents, income_account_id, product_service_id, class_id, is_taxable
         FROM invoice_items WHERE invoice_id = ?1 ORDER BY line_number",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let payments = list_payments(pool, invoice_id).await?.into_iter().filter(|p| !p.is_voided).collect();
    Ok(InvoiceDetail { invoice, items, payments })
}

/// Groups open invoice balances (home currency, i.e. the A/R still on the books) by days past due.
pub async fn ar_aging(pool: &SqlitePool, as_of: NaiveDate) -> Result<ArAging, String> {
    // Open invoices by due date, and credit not yet used (negative) by the credit memo's date.
    let open: Vec<(NaiveDate, i64)> = sqlx::query_as(
        "SELECT i.due_date, b.home_open_cents FROM invoices i JOIN v_invoice_balances b ON b.invoice_id = i.id
         WHERE i.status IN ('UNPAID', 'PARTIAL') AND i.is_voided = 0
         UNION ALL
         SELECT m.credit_date, -b.remaining_cents FROM credit_memos m JOIN v_credit_memo_balances b ON b.credit_memo_id = m.id
         WHERE b.remaining_cents > 0",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let mut buckets = vec![
        AgingBucket { key: "current", label: "Current", invoice_count: 0, amount_cents: 0 },
        AgingBucket { key: "days1To30", label: "1–30 days", invoice_count: 0, amount_cents: 0 },
        AgingBucket { key: "days31To60", label: "31–60 days", invoice_count: 0, amount_cents: 0 },
        AgingBucket { key: "days61Plus", label: "61+ days", invoice_count: 0, amount_cents: 0 },
    ];
    let mut total_cents = 0;
    for (due, balance) in open {
        let days_overdue = (as_of - due).num_days();
        let idx = match days_overdue {
            d if d <= 0 => 0,
            1..=30 => 1,
            31..=60 => 2,
            _ => 3,
        };
        buckets[idx].invoice_count += 1;
        buckets[idx].amount_cents += balance;
        total_cents += balance;
    }
    Ok(ArAging { as_of, buckets, total_cents })
}

async fn system_account(tx: &mut Transaction<'_, Sqlite>, account_type: &str) -> Result<i64, String> {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = ?1 AND is_system = 1 ORDER BY id LIMIT 1")
        .bind(account_type)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("COA_SYSTEM_ACCOUNT_MISSING: no system {account_type} account"))
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

    async fn balance(pool: &SqlitePool, account: i64) -> i64 {
        sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1")
            .bind(account)
            .fetch_one(pool)
            .await
            .expect("balance")
    }

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).expect("valid date")
    }

    fn payment(invoice_id: i64, account: i64, day: u32, cents: i64) -> PaymentInput {
        PaymentInput {
            invoice_id,
            deposit_account_id: account,
            payment_date: d(day),
            amount_cents: cents,
            payment_method: "CHECK".into(),
            reference: Some("1042".into()),
            exchange_rate_e6: None,
        }
    }

    async fn simple_invoice(pool: &SqlitePool, customer: &str, issue: NaiveDate, due: NaiveDate, cents: i64) -> InvoiceSummary {
        let services = account_id(pool, "4100").await;
        create_and_post_invoice(
            pool,
            InvoiceInput {
                customer_name: customer.into(),
                issue_date: issue,
                due_date: due,
                lines: vec![InvoiceLineInput{ description: "Work".into(), quantity_e4: 10000, rate_cents: cents, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
                currency: None,
                tax_rate_id: None,
                discount_cents: 0,
            },
        )
        .await
        .expect("invoice posts")
    }

    #[tokio::test]
    async fn invoice_posts_to_ar_and_income_then_payment_clears_ar() {
        let pool = test_pool().await;
        let (ar, checking, sales, services) = (
            account_id(&pool, "1200").await,
            account_id(&pool, "1010").await,
            account_id(&pool, "4000").await,
            account_id(&pool, "4100").await,
        );

        let input = InvoiceInput {
            customer_name: "Acme Corp".into(),
            issue_date: d(1),
            due_date: d(31),
            lines: vec![
                InvoiceLineInput{ description: "Consulting".into(), quantity_e4: 30000, rate_cents: 15_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false },
                InvoiceLineInput{ description: "Widget".into(), quantity_e4: 10000, rate_cents: 2_500, income_account_id: sales, product_service_id: None, class_id: None, is_taxable: false },
                InvoiceLineInput{ description: "Support".into(), quantity_e4: 20000, rate_cents: 1_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false },
            ],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
        };
        let invoice = create_and_post_invoice(&pool, input).await.expect("invoice posts");

        assert_eq!(invoice.status, "UNPAID");
        assert_eq!(invoice.invoice_number, 1001);
        assert_eq!(invoice.total_cents, 49_500);
        assert_eq!(invoice.item_count, 3);
        assert_eq!(balance(&pool, ar).await, 49_500);
        assert_eq!(balance(&pool, services).await, 47_000);
        assert_eq!(balance(&pool, sales).await, 2_500);

        let paid = record_payment(&pool, payment(invoice.id, checking, 15, 49_500)).await.expect("payment posts");
        assert_eq!(paid.status, "PAID");
        assert_eq!(paid.balance_cents, 0);
        assert_eq!(paid.last_payment_date.as_deref(), Some("2026-10-15"));
        assert_eq!(balance(&pool, ar).await, 0);
        assert_eq!(balance(&pool, checking).await, 49_500);

        let err = record_payment(&pool, payment(invoice.id, checking, 16, 100)).await.expect_err("cannot pay twice");
        assert!(err.starts_with("INV_NOT_OPEN"), "{err}");
    }

    #[tokio::test]
    async fn partial_payments_track_remaining_balance() {
        let pool = test_pool().await;
        let (ar, checking) = (account_id(&pool, "1200").await, account_id(&pool, "1010").await);
        let invoice = simple_invoice(&pool, "Globex", d(1), d(31), 100_000).await;

        let first = record_payment(&pool, payment(invoice.id, checking, 5, 30_000)).await.expect("first payment");
        assert_eq!((first.status.as_str(), first.amount_paid_cents, first.balance_cents), ("PARTIAL", 30_000, 70_000));

        let err = record_payment(&pool, payment(invoice.id, checking, 6, 70_001)).await.expect_err("overpayment");
        assert!(err.starts_with("INV_OVERPAYMENT"), "{err}");

        let second = record_payment(&pool, payment(invoice.id, checking, 7, 45_000)).await.expect("second payment");
        assert_eq!((second.status.as_str(), second.balance_cents), ("PARTIAL", 25_000));

        let last = record_payment(&pool, payment(invoice.id, checking, 9, 25_000)).await.expect("final payment");
        assert_eq!((last.status.as_str(), last.balance_cents), ("PAID", 0));

        let payments = list_payments(&pool, invoice.id).await.unwrap();
        assert_eq!(payments.iter().map(|p| p.amount_cents).collect::<Vec<_>>(), vec![30_000, 45_000, 25_000]);
        assert!(payments.iter().all(|p| p.journal_entry_id.is_some()));
        assert_eq!(balance(&pool, ar).await, 0);
        assert_eq!(balance(&pool, checking).await, 100_000);

        // The schema itself refuses to let amount_paid drift from the payment rows.
        let tamper = sqlx::query("UPDATE invoices SET amount_paid_cents = 1 WHERE id = ?1")
            .bind(invoice.id)
            .execute(&pool)
            .await
            .expect_err("trigger blocks tampering");
        assert!(tamper.to_string().contains("INV_PAID_MISMATCH"), "{tamper}");
    }

    #[tokio::test]
    async fn aging_buckets_open_balances_by_days_overdue() {
        let pool = test_pool().await;
        let checking = account_id(&pool, "1010").await;
        let as_of = d(31);
        let sept = |day| NaiveDate::from_ymd_opt(2026, 9, day).unwrap();
        let aug = |day| NaiveDate::from_ymd_opt(2026, 8, day).unwrap();

        simple_invoice(&pool, "Not due", d(20), NaiveDate::from_ymd_opt(2026, 11, 19).unwrap(), 10_000).await;
        simple_invoice(&pool, "Due today", d(1), d(31), 20_000).await;
        simple_invoice(&pool, "1 day late", d(1), d(30), 30_000).await;
        let partial = simple_invoice(&pool, "30 days late", sept(1), d(1), 40_000).await;
        record_payment(&pool, payment(partial.id, checking, 2, 15_000)).await.unwrap();
        simple_invoice(&pool, "31 days late", aug(30), sept(30), 50_000).await;
        simple_invoice(&pool, "61 days late", aug(1), aug(31), 60_000).await;
        let settled = simple_invoice(&pool, "Paid", aug(1), aug(2), 70_000).await;
        record_payment(&pool, payment(settled.id, checking, 1, 70_000)).await.unwrap();

        let aging = ar_aging(&pool, as_of).await.unwrap();
        let summary: Vec<(&str, i64, i64)> = aging.buckets.iter().map(|b| (b.key, b.invoice_count, b.amount_cents)).collect();
        assert_eq!(
            summary,
            vec![("current", 2, 30_000), ("days1To30", 2, 55_000), ("days31To60", 1, 50_000), ("days61Plus", 1, 60_000)]
        );
        assert_eq!(aging.total_cents, 195_000);
        assert_eq!(balance(&pool, account_id(&pool, "1200").await).await, aging.total_cents);
    }

    #[tokio::test]
    async fn non_income_account_rolls_back_everything() {
        let pool = test_pool().await;
        let services = account_id(&pool, "4100").await;
        let rent = account_id(&pool, "6300").await;
        let input = InvoiceInput {
            customer_name: "Acme Corp".into(),
            issue_date: d(1),
            due_date: d(31),
            lines: vec![
                InvoiceLineInput{ description: "Consulting".into(), quantity_e4: 10000, rate_cents: 10_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false },
                InvoiceLineInput{ description: "Oops".into(), quantity_e4: 10000, rate_cents: 10_000, income_account_id: rent, product_service_id: None, class_id: None, is_taxable: false },
            ],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
        };

        let err = create_and_post_invoice(&pool, input).await.expect_err("expense account rejected");
        assert!(err.starts_with("INV_NOT_INCOME_ACCOUNT: line 2"), "{err}");

        let (invoices, items, entries): (i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM invoices), (SELECT COUNT(*) FROM invoice_items), (SELECT COUNT(*) FROM journal_entries)",
        )
        .fetch_one(&pool)
        .await
        .expect("counts");
        assert_eq!((invoices, items, entries), (0, 0, 0));
    }

    #[tokio::test]
    async fn invoice_detail_has_items_and_live_payments() {
        let pool = test_pool().await;
        let checking = account_id(&pool, "1010").await;
        let invoice = simple_invoice(&pool, "Print Co", d(1), d(31), 25_000).await;
        record_payment(&pool, payment(invoice.id, checking, 2, 10_000)).await.unwrap();
        let detail = invoice_detail(&pool, invoice.id).await.unwrap();
        assert_eq!(detail.items.len(), 1);
        assert_eq!(detail.items[0].amount_cents, 25_000);
        assert_eq!(detail.payments.iter().map(|p| p.amount_cents).sum::<i64>(), 10_000);
        assert_eq!(detail.invoice.balance_cents, 15_000);
    }

    #[test]
    fn scaled_quantities_round_half_up_and_reject_overflow() {
        assert_eq!(line_amount(30_000, 15_000), Ok(45_000), "3 × $150.00");
        assert_eq!(line_amount(15_000, 12_500), Ok(18_750), "1.5 hours × $125.00");
        assert_eq!(line_amount(27_500, 3_333), Ok(9_166), "2.75 × $33.33 = $91.6575 → $91.66");
        assert_eq!(line_amount(5_000, 1), Ok(1), "0.5 × $0.01 = $0.005 rounds up to $0.01");
        assert!(line_amount(4_999, 1).unwrap_err().starts_with("INV_AMOUNT_ROUNDS_TO_ZERO"));
        assert!(line_amount(MAX_QUANTITY_E4, MAX_LINE_CENTS).is_err());
        assert!(line_amount(i64::MAX, i64::MAX).is_err(), "128-bit math cannot overflow silently");
        assert!(validate_line(0, 100, 1, "INV").unwrap_err().starts_with("INV_BAD_QUANTITY"));
    }

    #[tokio::test]
    async fn fractional_quantities_post_rounded_amounts() {
        let pool = test_pool().await;
        let (ar, services) = (account_id(&pool, "1200").await, account_id(&pool, "4100").await);
        let invoice = create_and_post_invoice(
            &pool,
            InvoiceInput {
                customer_name: "Hourly Client".into(),
                issue_date: d(1),
                due_date: d(31),
                lines: vec![
                    InvoiceLineInput{ description: "Consulting".into(), quantity_e4: 15_000, rate_cents: 12_500, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false },
                    InvoiceLineInput{ description: "Review".into(), quantity_e4: 27_500, rate_cents: 3_333, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false },
                ],
                currency: None,
                tax_rate_id: None,
                discount_cents: 0,
            },
        )
        .await
        .expect("fractional invoice posts");
        assert_eq!(invoice.total_cents, 18_750 + 9_166);
        assert_eq!(balance(&pool, ar).await, 27_916);
        let detail = invoice_detail(&pool, invoice.id).await.unwrap();
        assert_eq!(detail.items.iter().map(|i| i.quantity_e4).collect::<Vec<_>>(), vec![15_000, 27_500]);
        // The database enforces the rounding rule independently.
        let err = sqlx::query("UPDATE invoice_items SET amount_cents = 9_165 WHERE invoice_id = ?1 AND line_number = 2")
            .bind(invoice.id)
            .execute(&pool)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("INV_IMMUTABLE") || err.to_string().contains("CHECK"), "{err}");
    }

    #[tokio::test]
    async fn foreign_invoice_books_home_ar_and_payments_realize_fx() {
        let pool = test_pool().await;
        let (ar, checking, services) = (account_id(&pool, "1200").await, account_id(&pool, "1010").await, account_id(&pool, "4100").await);
        let fx_account: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = 'Exchange Gain or Loss' AND is_system = 1")
            .fetch_one(&pool)
            .await
            .expect("exchange account seeded");

        // EUR 1,000.00 at 1.08 → A/R and income of $1,080.00.
        let invoice = create_and_post_invoice(
            &pool,
            InvoiceInput {
                customer_name: "Berlin GmbH".into(),
                issue_date: d(1),
                due_date: d(31),
                lines: vec![InvoiceLineInput{ description: "Project".into(), quantity_e4: 10_000, rate_cents: 100_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
                currency: Some(ForeignCurrency { code: "EUR".into(), exchange_rate_e6: 1_080_000 }),
                tax_rate_id: None,
                discount_cents: 0,
            },
        )
        .await
        .unwrap();
        assert_eq!((invoice.currency_code.as_deref(), invoice.total_cents, invoice.home_total_cents), (Some("EUR"), 100_000, 108_000));
        assert_eq!((balance(&pool, ar).await, balance(&pool, services).await), (108_000, 108_000));

        let pay = |day, cents, rate| PaymentInput {
            invoice_id: invoice.id,
            deposit_account_id: checking,
            payment_date: d(day),
            amount_cents: cents,
            payment_method: "WIRE".into(),
            reference: None,
            exchange_rate_e6: Some(rate),
        };
        // EUR 400 at 1.10 → $440 received; A/R relieved 40% of $1,080 = $432; $8 gain.
        let partial = record_payment(&pool, pay(10, 40_000, 1_100_000)).await.unwrap();
        assert_eq!((partial.status.as_str(), partial.balance_cents, partial.home_balance_cents), ("PARTIAL", 60_000, 64_800));
        assert_eq!((balance(&pool, checking).await, balance(&pool, ar).await, balance(&pool, fx_account).await), (44_000, 64_800, -800));

        // EUR 600 at 1.05 → $630 received; final payment relieves the remaining $648 exactly; $18 loss.
        let paid = record_payment(&pool, pay(20, 60_000, 1_050_000)).await.unwrap();
        assert_eq!((paid.status.as_str(), paid.home_balance_cents), ("PAID", 0));
        assert_eq!((balance(&pool, ar).await, balance(&pool, checking).await, balance(&pool, fx_account).await), (0, 107_000, 1_000));
        assert_eq!(ar_aging(&pool, d(31)).await.unwrap().total_cents, 0);

        // Voiding the last payment reverses its home-currency effect and reopens the invoice.
        let last = list_payments(&pool, invoice.id).await.unwrap().pop().unwrap();
        crate::ledger::void_transaction(&pool, last.journal_entry_id.unwrap()).await.unwrap();
        let reopened = get_summary(&pool, invoice.id).await.unwrap();
        assert_eq!((reopened.status.as_str(), reopened.balance_cents, reopened.home_balance_cents), ("PARTIAL", 60_000, 64_800));
        assert_eq!((balance(&pool, ar).await, balance(&pool, fx_account).await), (64_800, -800));

        let err = record_payment(&pool, PaymentInput { exchange_rate_e6: Some(1_000_000), ..pay(21, 100, 1) }).await;
        assert!(err.is_ok(), "a foreign invoice accepts an explicit rate");
        let tb = crate::reports::trial_balance(&pool).await.unwrap();
        assert_eq!(tb.total_debit_cents, tb.total_credit_cents);
    }

    #[tokio::test]
    async fn foreign_payment_deposits_its_home_amount() {
        let pool = test_pool().await;
        let (checking, undeposited, services) = (account_id(&pool, "1010").await, account_id(&pool, "1300").await, account_id(&pool, "4100").await);
        let invoice = create_and_post_invoice(
            &pool,
            InvoiceInput {
                customer_name: "London Ltd".into(),
                issue_date: d(1),
                due_date: d(31),
                lines: vec![InvoiceLineInput{ description: "Work".into(), quantity_e4: 10_000, rate_cents: 50_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
                currency: Some(ForeignCurrency { code: "GBP".into(), exchange_rate_e6: 1_270_000 }),
                tax_rate_id: None,
                discount_cents: 0,
            },
        )
        .await
        .unwrap();
        record_payment(
            &pool,
            PaymentInput { invoice_id: invoice.id, deposit_account_id: undeposited, payment_date: d(5), amount_cents: 50_000, payment_method: "CHECK".into(), reference: None, exchange_rate_e6: None },
        )
        .await
        .unwrap();
        let waiting = crate::banking::list_undeposited(&pool).await.unwrap();
        assert_eq!(waiting[0].amount_cents, 63_500, "GBP 500 at the invoice rate 1.27");
        crate::banking::post_bank_deposit(
            &pool,
            crate::banking::DepositInput {
                bank_account_id: checking,
                deposit_date: d(6),
                memo: None,
                items: vec![crate::banking::DepositItemRef { kind: "PAYMENT".into(), id: waiting[0].id }],
                other_lines: vec![],
            },
        )
        .await
        .expect("home amount deposits");
        assert_eq!((balance(&pool, checking).await, balance(&pool, undeposited).await), (63_500, 0));
    }
}
