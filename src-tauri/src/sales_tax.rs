//! Sales tax: rates, the tax on sales documents, paying the agency, and the liability report.
//!
//! The tax on a document is round_half_up(taxable subtotal × rate) and is credited to the rate's
//! liability account (debited on a credit memo). Paying the agency debits that account and credits
//! the bank. The liability report shows, per rate, the taxable and non-taxable sales and the tax for
//! a period, with what was paid and what is still owed.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{db_err, mul_div_round, post_entry_in_tx, rollback, EntrySource, JournalEntry, JournalLine, MAX_LINE_CENTS};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TaxRateView {
    pub id: i64,
    pub name: String,
    /// Parts per million: 8.25% = 82,500.
    pub rate_ppm: i64,
    pub agency_vendor_id: Option<i64>,
    pub agency_name: Option<String>,
    pub liability_account_id: i64,
    pub liability_account_name: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxRateInput {
    pub name: String,
    pub rate_ppm: i64,
    #[serde(default)]
    pub agency_name: Option<String>,
    /// Defaults to the Sales Tax Payable account.
    #[serde(default)]
    pub liability_account_id: Option<i64>,
}

const RATE_SELECT: &str = "SELECT r.id, r.name, r.rate_ppm, r.agency_vendor_id, v.name AS agency_name, r.liability_account_id,
        a.full_name AS liability_account_name, r.is_active
 FROM tax_rates r LEFT JOIN vendors v ON v.id = r.agency_vendor_id JOIN v_chart_of_accounts a ON a.id = r.liability_account_id";

pub async fn list_tax_rates(pool: &SqlitePool) -> Result<Vec<TaxRateView>, String> {
    sqlx::query_as::<_, TaxRateView>(&format!("{RATE_SELECT} ORDER BY r.name COLLATE NOCASE")).fetch_all(pool).await.map_err(db_err)
}

async fn get_rate(pool: &SqlitePool, id: i64) -> Result<TaxRateView, String> {
    sqlx::query_as::<_, TaxRateView>(&format!("{RATE_SELECT} WHERE r.id = ?1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("TAX_NOT_FOUND: tax rate {id} does not exist"))
}

/// The liability account for a rate: the one given (a current liability), or Sales Tax Payable
/// (created when the company has none).
async fn liability_account(tx: &mut Transaction<'_, Sqlite>, chosen: Option<i64>) -> Result<i64, String> {
    if let Some(id) = chosen {
        let kind: Option<String> = sqlx::query_scalar("SELECT account_type FROM chart_of_accounts WHERE id = ?1").bind(id).fetch_optional(&mut **tx).await.map_err(db_err)?;
        return match kind.as_deref() {
            Some("OTHER_CURRENT_LIABILITY") => Ok(id),
            _ => Err("TAX_BAD_ACCOUNT: sales tax accrues in an Other Current Liability account".into()),
        };
    }
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM chart_of_accounts WHERE account_type = 'OTHER_CURRENT_LIABILITY' AND is_active = 1
           AND (name = 'Sales Tax Payable' COLLATE NOCASE OR account_number IN ('2200', '25500'))
         ORDER BY (name = 'Sales Tax Payable' COLLATE NOCASE) DESC, id LIMIT 1",
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    match found {
        Some(id) => Ok(id),
        None => crate::company_setup::ensure_standard_account(tx, crate::company_setup::SALES_TAX_PAYABLE).await,
    }
}

fn validate_rate(input: &TaxRateInput) -> Result<&str, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("TAX_NAME_REQUIRED: enter a name for the rate".into());
    }
    if !(0..=1_000_000).contains(&input.rate_ppm) {
        return Err("TAX_BAD_RATE: a rate is between 0% and 100%".into());
    }
    Ok(name)
}

async fn agency(tx: &mut Transaction<'_, Sqlite>, name: &Option<String>) -> Result<Option<i64>, String> {
    match name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => Ok(Some(crate::sales::find_or_create_named(tx, crate::sales::Party::Vendor, n).await?)),
        None => Ok(None),
    }
}

fn unique(err: sqlx::Error, name: &str) -> String {
    let msg = db_err(err);
    if msg.contains("UNIQUE") { format!("TAX_DUPLICATE: there is already a rate named '{name}'") } else { msg }
}

pub async fn create_tax_rate(pool: &SqlitePool, input: TaxRateInput) -> Result<TaxRateView, String> {
    let name = validate_rate(&input)?;
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let result = async {
        let account = liability_account(&mut tx, input.liability_account_id).await?;
        let vendor = agency(&mut tx, &input.agency_name).await?;
        sqlx::query_scalar::<_, i64>("INSERT INTO tax_rates (name, rate_ppm, agency_vendor_id, liability_account_id) VALUES (?1, ?2, ?3, ?4) RETURNING id")
            .bind(name)
            .bind(input.rate_ppm)
            .bind(vendor)
            .bind(account)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| unique(e, name))
    }
    .await;
    match result {
        Ok(id) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            get_rate(pool, id).await
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// Changes a rate for documents from now on; documents already posted keep the tax they posted.
pub async fn update_tax_rate(pool: &SqlitePool, id: i64, input: TaxRateInput) -> Result<TaxRateView, String> {
    let name = validate_rate(&input)?;
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let result = async {
        let account = liability_account(&mut tx, input.liability_account_id).await?;
        let vendor = agency(&mut tx, &input.agency_name).await?;
        sqlx::query("UPDATE tax_rates SET name = ?1, rate_ppm = ?2, agency_vendor_id = ?3, liability_account_id = ?4 WHERE id = ?5")
            .bind(name)
            .bind(input.rate_ppm)
            .bind(vendor)
            .bind(account)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| unique(e, name))
    }
    .await;
    match result {
        Ok(_) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            get_rate(pool, id).await
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

pub async fn set_tax_rate_active(pool: &SqlitePool, id: i64, active: bool) -> Result<TaxRateView, String> {
    sqlx::query("UPDATE tax_rates SET is_active = ?1 WHERE id = ?2").bind(active).bind(id).execute(pool).await.map_err(db_err)?;
    get_rate(pool, id).await
}

/// The tax on a document.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DocumentTax {
    pub rate_id: Option<i64>,
    pub liability_account_id: i64,
    pub cents: i64,
}

impl DocumentTax {
    /// The tax line: a credit on a sale, a debit when `reverse` (a credit memo).
    pub fn line(&self, reverse: bool) -> Option<JournalLine> {
        (self.cents > 0).then(|| {
            let line = if reverse { JournalLine::debit(self.liability_account_id, self.cents) } else { JournalLine::credit(self.liability_account_id, self.cents) };
            line.with_memo(Some("Sales tax"))
        })
    }
}

/// round_half_up(taxable × rate). Rates are parts per million.
pub fn tax_on(taxable_cents: i64, rate_ppm: i64) -> Option<i64> {
    mul_div_round(taxable_cents, rate_ppm, 1_000_000)
}

/// Works out the tax for a document from its rate and the amounts of its taxable lines.
/// `None` when no rate is chosen.
pub(crate) async fn document_tax(tx: &mut Transaction<'_, Sqlite>, rate_id: Option<i64>, taxable_cents: i64, code: &str) -> Result<Option<DocumentTax>, String> {
    let Some(rate_id) = rate_id else { return Ok(None) };
    let row: Option<(i64, i64, bool)> = sqlx::query_as("SELECT rate_ppm, liability_account_id, is_active FROM tax_rates WHERE id = ?1")
        .bind(rate_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    let (ppm, account, active) = row.ok_or_else(|| format!("{code}_UNKNOWN_TAX: that tax rate does not exist"))?;
    if !active {
        return Err(format!("{code}_INACTIVE_TAX: that tax rate is inactive"));
    }
    let cents = tax_on(taxable_cents, ppm).filter(|c| *c <= MAX_LINE_CENTS).ok_or_else(|| format!("{code}_TAX_TOO_LARGE: the tax is out of range"))?;
    Ok(Some(DocumentTax { rate_id: Some(rate_id), liability_account_id: account, cents }))
}

// ------------------------------------------------------------------------------------------------
// Discounts (taken off before tax)
// ------------------------------------------------------------------------------------------------

/// A discount off a sales document's subtotal: debited to Discounts Given, in the lines' class
/// when they all share one.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DocumentDiscount {
    pub cents: i64,
    pub account_id: i64,
    pub class_id: Option<i64>,
}

impl DocumentDiscount {
    pub fn line(&self) -> Option<JournalLine> {
        (self.cents > 0).then(|| JournalLine::debit(self.account_id, self.cents).with_memo(Some("Discount")).with_class(self.class_id))
    }
}

/// The taxable amount after a discount taken off the whole subtotal (the taxable share of it).
pub(crate) fn taxable_after_discount(taxable: i64, discount: i64, subtotal: i64) -> i64 {
    if discount == 0 || subtotal == 0 {
        return taxable;
    }
    taxable - mul_div_round(discount, taxable, subtotal).unwrap_or(0)
}

/// Validates a discount and finds (or creates) the Discounts Given account it is debited to.
pub(crate) async fn document_discount(
    tx: &mut Transaction<'_, Sqlite>,
    discount_cents: i64,
    subtotal: i64,
    classes: impl Iterator<Item = Option<i64>>,
    code: &str,
) -> Result<Option<DocumentDiscount>, String> {
    if discount_cents == 0 {
        return Ok(None);
    }
    if discount_cents < 0 || discount_cents > subtotal {
        return Err(format!("{code}_BAD_DISCOUNT: a discount is between zero and the subtotal"));
    }
    let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'INCOME' AND name = 'Discounts Given' AND parent_id IS NULL")
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    let account_id = match existing {
        Some(id) => id,
        None => sqlx::query_scalar(
            "INSERT INTO chart_of_accounts (account_number, name, account_type, description)
             VALUES (CASE WHEN EXISTS (SELECT 1 FROM chart_of_accounts WHERE account_number = '4950') THEN NULL ELSE '4950' END,
                     'Discounts Given', 'INCOME', 'Discounts taken off invoices and sales receipts') RETURNING id",
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)?,
    };
    let mut classes = classes;
    let first = classes.next().flatten();
    let class_id = if classes.all(|c| c == first) { first } else { None };
    Ok(Some(DocumentDiscount { cents: discount_cents, account_id, class_id }))
}

// ------------------------------------------------------------------------------------------------
// Paying the agency
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesTaxPaymentInput {
    #[serde(default)]
    pub tax_rate_id: Option<i64>,
    /// The liability account paid down; defaults to the rate's, or Sales Tax Payable.
    #[serde(default)]
    pub liability_account_id: Option<i64>,
    /// The bank or credit card account it is paid from.
    pub account_id: i64,
    pub payment_date: NaiveDate,
    pub amount_cents: i64,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SalesTaxPaymentView {
    pub id: i64,
    pub tax_rate_name: Option<String>,
    pub payment_date: String,
    pub amount_cents: i64,
    pub account_name: String,
    pub reference: Option<String>,
    pub journal_entry_id: i64,
    pub is_voided: bool,
}

/// Pays sales tax to the agency: Dr the liability account / Cr the bank or card account.
pub async fn pay_sales_tax(pool: &SqlitePool, input: SalesTaxPaymentInput) -> Result<i64, String> {
    if !(1..=MAX_LINE_CENTS).contains(&input.amount_cents) {
        return Err("TAX_BAD_AMOUNT: enter the amount paid".into());
    }
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let result = async {
        let rate: Option<(String, i64, Option<String>)> = match input.tax_rate_id {
            Some(id) => sqlx::query_as("SELECT r.name, r.liability_account_id, v.name FROM tax_rates r LEFT JOIN vendors v ON v.id = r.agency_vendor_id WHERE r.id = ?1")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(db_err)?,
            None => None,
        };
        let liability = match (input.liability_account_id, &rate) {
            (Some(id), _) => liability_account(&mut tx, Some(id)).await?,
            (None, Some((_, id, _))) => *id,
            (None, None) => liability_account(&mut tx, None).await?,
        };
        let kind: Option<String> = sqlx::query_scalar("SELECT account_type FROM chart_of_accounts WHERE id = ?1").bind(input.account_id).fetch_optional(&mut *tx).await.map_err(db_err)?;
        if !matches!(kind.as_deref(), Some("BANK" | "CREDIT_CARD")) {
            return Err("TAX_BAD_PAYMENT_ACCOUNT: pay sales tax from a Bank or Credit Card account".to_string());
        }
        let to = rate.as_ref().and_then(|r| r.2.clone()).or_else(|| rate.as_ref().map(|r| r.0.clone())).unwrap_or_else(|| "tax agency".into());
        let entry = JournalEntry {
            id: None,
            date: input.payment_date,
            description: format!("Sales tax payment - {to}"),
            lines: vec![JournalLine::debit(liability, input.amount_cents), JournalLine::credit(input.account_id, input.amount_cents)],
            currency: None,
        };
        let entry_id = post_entry_in_tx(&mut tx, &entry, EntrySource::Journal).await?;
        sqlx::query_scalar::<_, i64>(
            "INSERT INTO sales_tax_payments (tax_rate_id, liability_account_id, account_id, payment_date, amount_cents, reference, journal_entry_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
        )
        .bind(input.tax_rate_id)
        .bind(liability)
        .bind(input.account_id)
        .bind(input.payment_date)
        .bind(input.amount_cents)
        .bind(input.reference.as_deref().map(str::trim).filter(|r| !r.is_empty()))
        .bind(entry_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(db_err)
    }
    .await;
    match result {
        Ok(id) => crate::ledger::commit_write(tx).await.map(|()| id).map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

pub async fn list_sales_tax_payments(pool: &SqlitePool) -> Result<Vec<SalesTaxPaymentView>, String> {
    sqlx::query_as::<_, SalesTaxPaymentView>(
        "SELECT p.id, r.name AS tax_rate_name, p.payment_date, p.amount_cents, a.full_name AS account_name, p.reference, p.journal_entry_id, e.is_voided
         FROM sales_tax_payments p JOIN journal_entries e ON e.id = p.journal_entry_id
         LEFT JOIN tax_rates r ON r.id = p.tax_rate_id JOIN v_chart_of_accounts a ON a.id = p.account_id
         ORDER BY p.payment_date DESC, p.id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

// ------------------------------------------------------------------------------------------------
// Liability report
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TaxLiabilityRow {
    pub tax_rate_id: Option<i64>,
    pub name: String,
    pub rate_ppm: Option<i64>,
    pub taxable_sales_cents: i64,
    pub non_taxable_sales_cents: i64,
    pub tax_collected_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxLiabilityReport {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    /// `ACCRUAL` (tax is owed when invoiced) or `CASH` (when the customer pays).
    pub basis: String,
    pub rows: Vec<TaxLiabilityRow>,
    pub tax_collected_cents: i64,
    pub paid_cents: i64,
    /// Balance of the sales tax liability accounts at the end of the period.
    pub owed_cents: i64,
}

/// Company preference: sales tax is owed when invoiced (`accrual`) or when paid (`cash`).
pub const TAX_BASIS: &str = "sales_tax_basis";
/// Company preference: how often the return is filed, `monthly`, `quarterly` or `annually`; the
/// liability report and Pay Sales Tax open on the period that just ended.
pub const FILING_FREQUENCY: &str = "sales_tax_filing_frequency";
/// Company preference: printed sales forms mark taxable lines with a T.
pub const MARK_TAXABLE: &str = "mark_taxable_lines";

/// The tax liability report on the basis set in Preferences › Sales Tax.
pub async fn tax_liability(pool: &SqlitePool, start_date: NaiveDate, end_date: NaiveDate) -> Result<TaxLiabilityReport, String> {
    let cash = crate::preferences::get(pool, TAX_BASIS).await? == "cash";
    tax_liability_on(pool, start_date, end_date, cash).await
}

/// Sales and tax per rate for the period, tax paid in the period, and what is owed.
///
/// On the accrual basis, invoices and sales receipts less credit memos count by document date. On
/// the cash basis (`cash`), sales receipts count on their date, an invoice counts as it is paid
/// (each payment in the period brings in its share of the invoice's taxable sales, other sales and
/// tax, and a bounced payment takes its share back out), and a credit memo counts as it is refunded;
/// a credit applied to an invoice moves no money, so neither side counts. Sales with no rate are
/// non-taxable. Home-currency documents only, as before. "Owed" is the balance of the sales tax
/// liability accounts, which LocalBooks always posts as invoiced.
pub async fn tax_liability_on(pool: &SqlitePool, start_date: NaiveDate, end_date: NaiveDate, cash: bool) -> Result<TaxLiabilityReport, String> {
    let rows = if cash { cash_basis_rows(pool, start_date, end_date).await? } else { accrual_rows(pool, start_date, end_date).await? };
    let (paid, owed) = paid_and_owed(pool, start_date, end_date).await?;
    let collected = rows.iter().try_fold(0i64, |sum, r| sum.checked_add(r.tax_collected_cents)).ok_or("TAX_OVERFLOW: amount out of range")?;
    Ok(TaxLiabilityReport { start_date, end_date, basis: if cash { "CASH" } else { "ACCRUAL" }.into(), tax_collected_cents: collected, rows, paid_cents: paid, owed_cents: owed })
}

/// One document's taxable sales, other sales and tax, and the part of its total (`share` of
/// `total`) that falls in the period, signed.
#[derive(sqlx::FromRow)]
struct CashShare {
    rate_id: Option<i64>,
    taxable: i64,
    other: i64,
    tax: i64,
    total: i64,
    share: i64,
}

async fn cash_basis_rows(pool: &SqlitePool, start_date: NaiveDate, end_date: NaiveDate) -> Result<Vec<TaxLiabilityRow>, String> {
    let shares: Vec<CashShare> = sqlx::query_as(
        "WITH inv AS (
            SELECT i.id, i.tax_rate_id, i.tax_cents, i.total_cents,
                   (SELECT COALESCE(SUM(amount_cents), 0) FROM invoice_items WHERE invoice_id = i.id AND is_taxable = 1) AS taxable,
                   (SELECT COALESCE(SUM(amount_cents), 0) FROM invoice_items WHERE invoice_id = i.id AND is_taxable = 0) AS other
            FROM invoices i WHERE i.is_voided = 0 AND i.journal_entry_id IS NOT NULL AND i.currency_code IS NULL
         ), memo AS (
            SELECT m.id, m.tax_rate_id, m.tax_cents, m.total_cents, m.credit_date, m.refund_account_id,
                   (SELECT COALESCE(SUM(amount_cents), 0) FROM credit_memo_lines WHERE credit_memo_id = m.id AND is_taxable = 1) AS taxable,
                   (SELECT COALESCE(SUM(amount_cents), 0) FROM credit_memo_lines WHERE credit_memo_id = m.id AND is_taxable = 0) AS other
            FROM credit_memos m WHERE m.is_voided = 0 AND m.journal_entry_id IS NOT NULL
         )
         SELECT inv.tax_rate_id AS rate_id, inv.taxable, inv.other, inv.tax_cents AS tax, inv.total_cents AS total, p.amount_cents AS share
         FROM invoice_payments p JOIN inv ON inv.id = p.invoice_id
         WHERE p.is_voided = 0 AND p.payment_date BETWEEN ?1 AND ?2
         UNION ALL
         SELECT inv.tax_rate_id, inv.taxable, inv.other, inv.tax_cents, inv.total_cents, -b.amount_cents
         FROM bounced_checks b JOIN inv ON inv.id = b.invoice_id
         WHERE b.is_voided = 0 AND b.bounce_date BETWEEN ?1 AND ?2
         UNION ALL
         SELECT r.tax_rate_id, (SELECT COALESCE(SUM(amount_cents), 0) FROM sales_receipt_lines WHERE sales_receipt_id = r.id AND is_taxable = 1),
                (SELECT COALESCE(SUM(amount_cents), 0) FROM sales_receipt_lines WHERE sales_receipt_id = r.id AND is_taxable = 0),
                r.tax_cents, 1, 1
         FROM sales_receipts r WHERE r.is_voided = 0 AND r.journal_entry_id IS NOT NULL AND r.receipt_date BETWEEN ?1 AND ?2
         UNION ALL
         SELECT memo.tax_rate_id, memo.taxable, memo.other, memo.tax_cents, memo.total_cents, -r.amount_cents
         FROM credit_refunds r JOIN memo ON memo.id = r.credit_memo_id JOIN journal_entries e ON e.id = r.journal_entry_id
         WHERE e.is_voided = 0 AND r.refund_date BETWEEN ?1 AND ?2
         UNION ALL
         SELECT memo.tax_rate_id, memo.taxable, memo.other, memo.tax_cents, 1, -1
         FROM memo WHERE memo.refund_account_id IS NOT NULL AND memo.credit_date BETWEEN ?1 AND ?2",
    )
    .bind(start_date)
    .bind(end_date)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    // Per rate: taxable, non-taxable and tax, each the period's share of the document.
    let mut by_rate: std::collections::BTreeMap<Option<i64>, (i64, i64, i64)> = std::collections::BTreeMap::new();
    let overflow = || "TAX_OVERFLOW: amount out of range".to_string();
    for s in shares {
        let part = |amount: i64| crate::ledger::mul_div_round(amount, s.share, s.total.max(1)).ok_or_else(overflow);
        let (taxable, other, tax) = (part(s.taxable)?, part(s.other)?, part(s.tax)?);
        let entry = by_rate.entry(s.rate_id).or_default();
        let (t, o) = if s.rate_id.is_some() { (taxable, other) } else { (0, other.checked_add(taxable).ok_or_else(overflow)?) };
        entry.0 = entry.0.checked_add(t).ok_or_else(overflow)?;
        entry.1 = entry.1.checked_add(o).ok_or_else(overflow)?;
        entry.2 = entry.2.checked_add(tax).ok_or_else(overflow)?;
    }
    let rates: Vec<(i64, String, i64)> = sqlx::query_as("SELECT id, name, rate_ppm FROM tax_rates").fetch_all(pool).await.map_err(db_err)?;
    let mut rows: Vec<TaxLiabilityRow> = by_rate
        .into_iter()
        .map(|(rate_id, (taxable, other, tax))| {
            let rate = rate_id.and_then(|id| rates.iter().find(|r| r.0 == id));
            TaxLiabilityRow {
                tax_rate_id: rate_id,
                name: rate.map_or_else(|| "No tax rate".to_string(), |r| r.1.clone()),
                rate_ppm: rate.map(|r| r.2),
                taxable_sales_cents: taxable,
                non_taxable_sales_cents: other,
                tax_collected_cents: tax,
            }
        })
        .collect();
    rows.sort_by(|a, b| (a.tax_rate_id.is_none(), a.name.to_lowercase()).cmp(&(b.tax_rate_id.is_none(), b.name.to_lowercase())));
    Ok(rows)
}

async fn accrual_rows(pool: &SqlitePool, start_date: NaiveDate, end_date: NaiveDate) -> Result<Vec<TaxLiabilityRow>, String> {
    let rows: Vec<TaxLiabilityRow> = sqlx::query_as(
        "WITH docs (rate_id, sign, tax, taxable, other) AS (
            SELECT i.tax_rate_id, 1, i.tax_cents,
                   (SELECT COALESCE(SUM(amount_cents), 0) FROM invoice_items WHERE invoice_id = i.id AND is_taxable = 1),
                   (SELECT COALESCE(SUM(amount_cents), 0) FROM invoice_items WHERE invoice_id = i.id AND is_taxable = 0)
            FROM invoices i WHERE i.is_voided = 0 AND i.journal_entry_id IS NOT NULL AND i.currency_code IS NULL AND i.issue_date BETWEEN ?1 AND ?2
            UNION ALL
            SELECT r.tax_rate_id, 1, r.tax_cents,
                   (SELECT COALESCE(SUM(amount_cents), 0) FROM sales_receipt_lines WHERE sales_receipt_id = r.id AND is_taxable = 1),
                   (SELECT COALESCE(SUM(amount_cents), 0) FROM sales_receipt_lines WHERE sales_receipt_id = r.id AND is_taxable = 0)
            FROM sales_receipts r WHERE r.is_voided = 0 AND r.journal_entry_id IS NOT NULL AND r.receipt_date BETWEEN ?1 AND ?2
            UNION ALL
            SELECT m.tax_rate_id, -1, m.tax_cents,
                   (SELECT COALESCE(SUM(amount_cents), 0) FROM credit_memo_lines WHERE credit_memo_id = m.id AND is_taxable = 1),
                   (SELECT COALESCE(SUM(amount_cents), 0) FROM credit_memo_lines WHERE credit_memo_id = m.id AND is_taxable = 0)
            FROM credit_memos m WHERE m.is_voided = 0 AND m.journal_entry_id IS NOT NULL AND m.credit_date BETWEEN ?1 AND ?2
         )
         SELECT d.rate_id AS tax_rate_id, COALESCE(t.name, 'No tax rate') AS name, t.rate_ppm,
                SUM(d.sign * CASE WHEN d.rate_id IS NULL THEN 0 ELSE d.taxable END) AS taxable_sales_cents,
                SUM(d.sign * (d.other + CASE WHEN d.rate_id IS NULL THEN d.taxable ELSE 0 END)) AS non_taxable_sales_cents,
                SUM(d.sign * d.tax) AS tax_collected_cents
         FROM docs d LEFT JOIN tax_rates t ON t.id = d.rate_id
         GROUP BY d.rate_id ORDER BY d.rate_id IS NULL, t.name COLLATE NOCASE",
    )
    .bind(start_date)
    .bind(end_date)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows)
}

/// Sales tax paid in the period, and the liability accounts' balance at its end.
async fn paid_and_owed(pool: &SqlitePool, start_date: NaiveDate, end_date: NaiveDate) -> Result<(i64, i64), String> {
    let paid: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(p.amount_cents), 0) FROM sales_tax_payments p JOIN journal_entries e ON e.id = p.journal_entry_id
         WHERE e.is_voided = 0 AND p.payment_date BETWEEN ?1 AND ?2",
    )
    .bind(start_date)
    .bind(end_date)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    let owed: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(credit_cents - debit_cents), 0) FROM v_posted_lines
         WHERE entry_date <= ?1 AND account_id IN (SELECT liability_account_id FROM tax_rates
                                                   UNION SELECT liability_account_id FROM sales_tax_payments)",
    )
    .bind(end_date)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    Ok((paid, owed))
}

#[cfg(test)]
mod tests;
