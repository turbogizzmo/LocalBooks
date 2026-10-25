//! Sales: customers, products & services, sales receipts (cash sales) and the All Sales register.
//! A sales receipt posts Dr deposit account / Cr income account(s) in a single transaction.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::invoices::{line_amount, validate_line, PAYMENT_METHODS};
use crate::ledger::{db_err, format_cents, post_entry_in_tx, rollback, EntrySource, JournalEntry, JournalLine, MAX_LINE_CENTS};

#[derive(Debug, Clone, Copy)]
pub enum Party {
    Customer,
    Vendor,
}

impl Party {
    fn table(self) -> &'static str {
        match self {
            Party::Customer => "customers",
            Party::Vendor => "vendors",
        }
    }
}

/// Returns the id of the customer/vendor named `name` (case-insensitive), creating it if needed.
///
/// A customer name written "Customer:Job" is a job of that customer: the customer is found or
/// created first, and the job is stored under its full name with the customer as its parent.
pub(crate) async fn find_or_create_named(tx: &mut Transaction<'_, Sqlite>, party: Party, name: &str) -> Result<i64, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(format!("PARTY_NO_NAME: enter a {} name", if matches!(party, Party::Customer) { "customer" } else { "vendor" }));
    }
    let table = party.table();
    if matches!(party, Party::Customer) {
        return customer_path(tx, name).await;
    }
    sqlx::query(&format!("INSERT OR IGNORE INTO {table} (name) VALUES (?1)"))
        .bind(name)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    sqlx::query_scalar(&format!("SELECT id FROM {table} WHERE name = ?1 COLLATE NOCASE"))
        .bind(name)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)
}

/// "Acme : Kitchen" → "Acme:Kitchen"; an empty part is refused.
pub(crate) fn job_name(name: &str) -> Result<String, String> {
    let parts: Vec<&str> = name.split(':').map(str::trim).collect();
    if parts.iter().any(|p| p.is_empty()) {
        return Err("PARTY_BAD_JOB: write a job as Customer:Job".into());
    }
    Ok(parts.join(":"))
}

/// Finds or creates the customer at `name`, and each customer above it for a job.
async fn customer_path(tx: &mut Transaction<'_, Sqlite>, name: &str) -> Result<i64, String> {
    let full = job_name(name)?;
    let mut parent: Option<i64> = None;
    let mut prefix = String::new();
    for part in full.split(':') {
        if !prefix.is_empty() {
            prefix.push(':');
        }
        prefix.push_str(part);
        sqlx::query("INSERT OR IGNORE INTO customers (name, parent_id) VALUES (?1, ?2)").bind(&prefix).bind(parent).execute(&mut **tx).await.map_err(db_err)?;
        let id: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = ?1 COLLATE NOCASE").bind(&prefix).fetch_one(&mut **tx).await.map_err(db_err)?;
        parent = Some(id);
    }
    Ok(parent.expect("at least one part"))
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PartyView {
    pub id: i64,
    pub name: String,
    pub email: Option<String>,
    pub is_active: bool,
    pub open_balance_cents: i64,
    /// Customers: default sales tax rate and exemption; default terms (customers and vendors).
    #[sqlx(default)]
    pub tax_rate_id: Option<i64>,
    #[sqlx(default)]
    pub is_tax_exempt: bool,
    #[sqlx(default)]
    pub terms_days: Option<i64>,
    /// Customers: the default sales rep.
    #[sqlx(default)]
    pub sales_rep_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ProductView {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub rate_cents: i64,
    pub income_account_id: i64,
    pub is_active: bool,
    pub is_taxable: bool,
    /// Set for products whose stock is tracked: quantity on hand (scaled by 10,000).
    #[sqlx(default)]
    pub qty_on_hand_e4: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductInput {
    pub name: String,
    pub description: Option<String>,
    pub rate_cents: i64,
    pub income_account_id: i64,
    /// Charged sales tax when sold (on by default).
    #[serde(default = "taxable_by_default")]
    pub is_taxable: bool,
}

fn taxable_by_default() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesLineInput {
    pub product_service_id: Option<i64>,
    pub description: String,
    /// Quantity scaled by 10,000 (1.5 → 15_000).
    pub quantity_e4: i64,
    pub rate_cents: i64,
    pub income_account_id: i64,
    #[serde(default)]
    pub class_id: Option<i64>,
    #[serde(default)]
    pub is_taxable: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesReceiptInput {
    /// Optional: walk-in cash sales need no customer.
    pub customer_name: Option<String>,
    pub receipt_date: NaiveDate,
    pub deposit_account_id: i64,
    pub payment_method: String,
    pub reference: Option<String>,
    pub memo: Option<String>,
    pub lines: Vec<SalesLineInput>,
    #[serde(default)]
    pub tax_rate_id: Option<i64>,
    #[serde(default)]
    pub discount_cents: i64,
    #[serde(flatten)]
    pub details: crate::sales_lists::SalesDetails,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SalesReceiptSummary {
    pub id: i64,
    pub receipt_number: i64,
    pub customer_name: Option<String>,
    pub receipt_date: String,
    pub total_cents: i64,
    pub journal_entry_id: Option<i64>,
}

/// One row of the All Sales register: an invoice, a payment against one, or a sales receipt.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SalesTransaction {
    /// `INVOICE`, `PAYMENT` or `SALES_RECEIPT`
    pub kind: String,
    pub id: i64,
    pub number: i64,
    pub date: String,
    pub customer_name: String,
    pub due_date: Option<String>,
    pub balance_cents: i64,
    pub total_cents: i64,
    pub status: String,
    pub journal_entry_id: Option<i64>,
    pub is_voided: bool,
    /// Document currency (`None` = home). Totals above are in this currency.
    pub currency_code: Option<String>,
    /// Home-currency equivalents, for totals across documents in different currencies.
    pub home_total_cents: i64,
    pub home_balance_cents: i64,
}

fn clean(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|s| !s.is_empty())
}

// ------------------------------------------------------------------------------------------------
// Customers
// ------------------------------------------------------------------------------------------------

pub async fn list_customers(pool: &SqlitePool) -> Result<Vec<PartyView>, String> {
    sqlx::query_as::<_, PartyView>(
        "SELECT c.id, c.name, c.email, c.is_active, c.tax_rate_id, c.is_tax_exempt, c.terms_days, c.sales_rep_id,
                COALESCE((SELECT SUM(b.home_open_cents) FROM invoices i JOIN v_invoice_balances b ON b.invoice_id = i.id
                          WHERE i.customer_id = c.id AND i.status IN ('UNPAID', 'PARTIAL') AND i.is_voided = 0), 0)
                - COALESCE((SELECT SUM(remaining_cents) FROM v_credit_memo_balances WHERE customer_id = c.id), 0) AS open_balance_cents
         FROM customers c ORDER BY c.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn create_customer(pool: &SqlitePool, name: &str, email: Option<&str>) -> Result<PartyView, String> {
    let id = create_party(pool, Party::Customer, name, email).await?;
    list_customers(pool).await?.into_iter().find(|c| c.id == id)
        .ok_or_else(|| "PARTY_NOT_FOUND: customer was not saved".into())
}

pub(crate) async fn create_party(pool: &SqlitePool, party: Party, name: &str, email: Option<&str>) -> Result<i64, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("PARTY_NO_NAME: enter a name".into());
    }
    let full = if matches!(party, Party::Customer) { job_name(name)? } else { name.to_string() };
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    // A job's customer is created first if needed.
    let parent = match full.rsplit_once(':').filter(|_| matches!(party, Party::Customer)) {
        Some((above, _)) => Some(customer_path(&mut tx, above).await?),
        None => None,
    };
    let created = match party {
        Party::Customer => sqlx::query_scalar("INSERT INTO customers (name, email, parent_id) VALUES (?1, ?2, ?3) RETURNING id").bind(&full).bind(clean(email)).bind(parent),
        Party::Vendor => sqlx::query_scalar("INSERT INTO vendors (name, email) VALUES (?1, ?2) RETURNING id").bind(&full).bind(clean(email)),
    }
    .fetch_one(&mut *tx)
    .await;
    let id = match created {
        Ok(id) => id,
        Err(e) => {
            let _ = tx.rollback().await;
            return Err(party_err(e, name));
        }
    };
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(id)
}

fn party_err(e: sqlx::Error, name: &str) -> String {
    match e {
        sqlx::Error::Database(db) if db.message().contains("UNIQUE") => format!("PARTY_DUPLICATE: '{name}' already exists"),
        sqlx::Error::Database(db) if db.message().contains("CHECK") => "PARTY_BAD_EMAIL: enter a valid email address".into(),
        other => db_err(other),
    }
}

// ------------------------------------------------------------------------------------------------
// Products & services
// ------------------------------------------------------------------------------------------------

pub async fn list_products(pool: &SqlitePool) -> Result<Vec<ProductView>, String> {
    sqlx::query_as::<_, ProductView>(
        "SELECT p.id, p.name, p.description, p.rate_cents, p.income_account_id, p.is_active, p.is_taxable, i.qty_on_hand_e4
         FROM products_services p LEFT JOIN inventory_items i ON i.product_id = p.id
         ORDER BY p.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn create_product(pool: &SqlitePool, input: ProductInput) -> Result<ProductView, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("PRODUCT_NO_NAME: enter a product or service name".into());
    }
    if !(0..=MAX_LINE_CENTS).contains(&input.rate_cents) {
        return Err("PRODUCT_BAD_RATE: rate must be zero or more".into());
    }
    sqlx::query_as::<_, ProductView>(
        "INSERT INTO products_services (name, description, rate_cents, income_account_id, is_taxable) VALUES (?1, ?2, ?3, ?4, ?5)
         RETURNING id, name, description, rate_cents, income_account_id, is_active, is_taxable",
    )
    .bind(name)
    .bind(clean(input.description.as_deref()))
    .bind(input.rate_cents)
    .bind(input.income_account_id)
    .bind(input.is_taxable)
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db) if db.message().contains("UNIQUE") => format!("PRODUCT_DUPLICATE: '{name}' already exists"),
        other => db_err(other),
    })
}

// ------------------------------------------------------------------------------------------------
// Sales receipts
// ------------------------------------------------------------------------------------------------

/// Pure validation; returns the receipt total.
pub fn validate_sales_receipt(input: &SalesReceiptInput) -> Result<i64, String> {
    if !PAYMENT_METHODS.contains(&input.payment_method.as_str()) {
        return Err(format!("SR_BAD_METHOD: unknown payment method '{}'", input.payment_method));
    }
    if input.lines.is_empty() {
        return Err("SR_NO_ITEMS: a sales receipt requires at least one line".into());
    }
    let mut total: i64 = 0;
    for (idx, line) in input.lines.iter().enumerate() {
        let amount = validate_line(line.quantity_e4, line.rate_cents, idx + 1, "SR")?;
        total = total
            .checked_add(amount)
            .filter(|t| *t <= MAX_LINE_CENTS)
            .ok_or_else(|| format!("SR_AMOUNT_TOO_LARGE: receipt total exceeds {}", format_cents(MAX_LINE_CENTS)))?;
    }
    Ok(total)
}

/// Records a cash sale: Dr deposit account / Cr each line's income account, atomically.
pub async fn post_sales_receipt(pool: &SqlitePool, input: SalesReceiptInput) -> Result<SalesReceiptSummary, String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let id = match write_sales_receipt(&mut tx, &input).await {
        Ok(id) => id,
        Err(err) => return Err(rollback(tx, err).await),
    };
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    sqlx::query_as::<_, SalesReceiptSummary>(
        "SELECT r.id, r.receipt_number, c.name AS customer_name, r.receipt_date, r.total_cents, r.journal_entry_id
         FROM sales_receipts r LEFT JOIN customers c ON c.id = r.customer_id WHERE r.id = ?1",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

async fn write_sales_receipt(tx: &mut Transaction<'_, Sqlite>, input: &SalesReceiptInput) -> Result<i64, String> {
    let subtotal = validate_sales_receipt(input)?;
    let mut taxable = 0;
    for line in input.lines.iter().filter(|l| l.is_taxable) {
        taxable += line_amount(line.quantity_e4, line.rate_cents)?;
    }
    let discount = crate::sales_tax::document_discount(tx, input.discount_cents, subtotal, input.lines.iter().map(|l| l.class_id), "SR").await?;
    let taxable = crate::sales_tax::taxable_after_discount(taxable, input.discount_cents, subtotal);
    let tax = crate::sales_tax::document_tax(tx, input.tax_rate_id, taxable, "SR").await?;
    let tax_cents = tax.map_or(0, |t| t.cents);
    let total = (subtotal - input.discount_cents).checked_add(tax_cents).filter(|t| *t > 0 && *t <= crate::ledger::MAX_LINE_CENTS).ok_or("SR_TOO_LARGE: the receipt total must be above zero and in range")?;
    let customer_id = match clean(input.customer_name.as_deref()) {
        Some(name) => Some(find_or_create_named(tx, Party::Customer, name).await?),
        None => None,
    };

    let (receipt_id, receipt_number): (i64, i64) = sqlx::query_as(
        "INSERT INTO sales_receipts (receipt_number, customer_id, receipt_date, deposit_account_id, payment_method, reference, memo, total_cents, tax_rate_id, tax_cents, discount_cents)
         VALUES ((SELECT COALESCE(MAX(receipt_number), 0) + 1 FROM sales_receipts), ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         RETURNING id, receipt_number",
    )
    .bind(customer_id)
    .bind(input.receipt_date)
    .bind(input.deposit_account_id)
    .bind(&input.payment_method)
    .bind(clean(input.reference.as_deref()))
    .bind(clean(input.memo.as_deref()))
    .bind(total)
    .bind(tax.and_then(|t| t.rate_id))
    .bind(tax_cents)
    .bind(input.discount_cents)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    let mut credits: Vec<((i64, Option<i64>), i64)> = Vec::new();
    for (idx, line) in input.lines.iter().enumerate() {
        let n = idx + 1;
        let classification: Option<String> = sqlx::query_scalar(
            "SELECT t.classification FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1",
        )
        .bind(line.income_account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        if classification.as_deref() != Some("INCOME") {
            return Err(format!("SR_NOT_INCOME_ACCOUNT: line {n} must use an Income account"));
        }
        if let Some(class) = line.class_id {
            crate::classes::check_usable(tx, class, &format!("SR: line {n}")).await?;
        }
        let amount = line_amount(line.quantity_e4, line.rate_cents)?;
        sqlx::query(
            "INSERT INTO sales_receipt_lines
                (sales_receipt_id, line_number, product_service_id, description, quantity_e4, rate_cents, amount_cents, income_account_id, class_id, is_taxable)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .bind(receipt_id)
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
        match credits.iter_mut().find(|(k, _)| *k == key) {
            Some((_, sum)) => *sum += amount,
            None => credits.push((key, amount)),
        }
    }

    crate::sales_lists::write_details(tx, crate::sales_lists::SalesDoc::SalesReceipt, receipt_id, &input.details).await?;

    let mut lines = vec![JournalLine::debit(input.deposit_account_id, total)];
    lines.extend(credits.into_iter().map(|((account, class), cents)| JournalLine::credit(account, cents).with_class(class)));
    lines.extend(discount.and_then(|d| d.line()));
    lines.extend(tax.and_then(|t| t.line(false)));
    // Tracked products also move their cost from Inventory Asset to Cost of Goods Sold, in this entry.
    let sold: Vec<(Option<i64>, i64, Option<i64>)> = input.lines.iter().map(|l| (l.product_service_id, l.quantity_e4, l.class_id)).collect();
    let cost = crate::inventory::cost_of_sale(tx, &sold).await?;
    lines.extend(cost.lines.iter().cloned());
    let customer_label = clean(input.customer_name.as_deref()).map(|c| format!(" - {c}")).unwrap_or_default();
    let entry = JournalEntry {
        id: None,
        date: input.receipt_date,
        description: format!("Sales Receipt #{receipt_number}{customer_label}"),
        lines,
        currency: None,
    };
    let entry_id = post_entry_in_tx(tx, &entry, EntrySource::SalesReceipt(receipt_id)).await?;
    cost.record(tx, input.receipt_date, entry_id).await?;

    sqlx::query("UPDATE sales_receipts SET journal_entry_id = ?1 WHERE id = ?2")
        .bind(entry_id)
        .bind(receipt_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(receipt_id)
}

/// Every posted sales document, newest first.
pub async fn list_all_sales(pool: &SqlitePool) -> Result<Vec<SalesTransaction>, String> {
    sqlx::query_as::<_, SalesTransaction>(
        "SELECT 'INVOICE' AS kind, i.id, i.invoice_number AS number, i.issue_date AS date, i.customer_name,
                i.due_date, CASE WHEN i.is_voided = 1 THEN 0 ELSE i.total_cents - i.amount_paid_cents END AS balance_cents,
                i.total_cents, i.status, i.journal_entry_id, i.is_voided, i.currency_code,
                b.home_total_cents, CASE WHEN i.is_voided = 1 THEN 0 ELSE b.home_open_cents END AS home_balance_cents
         FROM invoices i JOIN v_invoice_balances b ON b.invoice_id = i.id WHERE i.status <> 'DRAFT'
         UNION ALL
         SELECT 'PAYMENT', p.id, i.invoice_number, p.payment_date, i.customer_name,
                NULL, 0, p.amount_cents, 'CLOSED', p.journal_entry_id, p.is_voided, i.currency_code,
                COALESCE(p.home_amount_cents, p.amount_cents), 0
         FROM invoice_payments p JOIN invoices i ON i.id = p.invoice_id
         WHERE p.journal_entry_id IS NOT NULL
         UNION ALL
         SELECT 'SALES_RECEIPT', r.id, r.receipt_number, r.receipt_date, COALESCE(c.name, ''),
                NULL, 0, r.total_cents, 'PAID', r.journal_entry_id, r.is_voided, NULL, r.total_cents, 0
         FROM sales_receipts r LEFT JOIN customers c ON c.id = r.customer_id
         WHERE r.journal_entry_id IS NOT NULL
         ORDER BY date DESC, kind, id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests;
