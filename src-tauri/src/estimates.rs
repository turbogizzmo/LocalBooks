//! Estimates: non-posting quotes. Converting an accepted estimate creates and posts the invoice and
//! closes the estimate in one transaction.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::invoices::{validate_line, write_invoice, InvoiceInput, InvoiceLineInput};
use crate::ledger::{db_err, format_cents, rollback, MAX_LINE_CENTS};
use crate::sales::{find_or_create_named, Party, SalesLineInput};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EstimateInput {
    pub customer_name: String,
    pub estimate_date: NaiveDate,
    pub expiration_date: Option<NaiveDate>,
    pub memo: Option<String>,
    pub lines: Vec<SalesLineInput>,
    #[serde(default)]
    pub tax_rate_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct EstimateSummary {
    pub id: i64,
    pub estimate_number: i64,
    pub customer_name: String,
    pub estimate_date: String,
    pub expiration_date: Option<String>,
    pub status: String,
    pub memo: Option<String>,
    pub total_cents: i64,
    pub tax_rate_id: Option<i64>,
    pub tax_cents: i64,
    /// The invoice this estimate became (kept as a permanent link).
    pub converted_to_invoice_id: Option<i64>,
    pub invoice_number: Option<i64>,
    pub line_count: i64,
}

const SUMMARY_SELECT: &str = "
    SELECT e.id, e.estimate_number, c.name AS customer_name, e.estimate_date, e.expiration_date, e.status, e.memo,
           e.total_cents, e.tax_rate_id, e.tax_cents, e.converted_to_invoice_id, i.invoice_number,
           (SELECT COUNT(*) FROM estimate_lines WHERE estimate_id = e.id) AS line_count
    FROM estimates e
    JOIN customers c ON c.id = e.customer_id
    LEFT JOIN invoices i ON i.id = e.converted_to_invoice_id";

pub async fn list_estimates(pool: &SqlitePool) -> Result<Vec<EstimateSummary>, String> {
    sqlx::query_as::<_, EstimateSummary>(&format!("{SUMMARY_SELECT} ORDER BY e.estimate_number DESC"))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

async fn get_estimate(pool: &SqlitePool, id: i64) -> Result<EstimateSummary, String> {
    sqlx::query_as::<_, EstimateSummary>(&format!("{SUMMARY_SELECT} WHERE e.id = ?1"))
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

pub async fn create_estimate(pool: &SqlitePool, input: EstimateInput) -> Result<EstimateSummary, String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let id = match write_estimate(&mut tx, &input).await {
        Ok(id) => id,
        Err(err) => return Err(rollback(tx, err).await),
    };
    tx.commit().await.map_err(db_err)?;
    get_estimate(pool, id).await
}

async fn write_estimate(tx: &mut Transaction<'_, Sqlite>, input: &EstimateInput) -> Result<i64, String> {
    if input.lines.is_empty() {
        return Err("EST_NO_ITEMS: an estimate requires at least one line".into());
    }
    if input.expiration_date.is_some_and(|exp| exp < input.estimate_date) {
        return Err("EST_EXPIRES_BEFORE_DATE: expiration date cannot be before the estimate date".into());
    }
    let mut total: i64 = 0;
    let mut amounts = Vec::with_capacity(input.lines.len());
    for (idx, line) in input.lines.iter().enumerate() {
        let amount = validate_line(line.quantity_e4, line.rate_cents, idx + 1, "EST")?;
        total = total
            .checked_add(amount)
            .filter(|t| *t <= MAX_LINE_CENTS)
            .ok_or_else(|| format!("EST_AMOUNT_TOO_LARGE: estimate total exceeds {}", format_cents(MAX_LINE_CENTS)))?;
        amounts.push(amount);
    }
    let taxable: i64 = input.lines.iter().zip(&amounts).filter(|(l, _)| l.is_taxable).map(|(_, a)| a).sum();
    let tax = crate::sales_tax::document_tax(tx, input.tax_rate_id, taxable, "EST").await?;
    let tax_cents = tax.map_or(0, |t| t.cents);
    let total = total.checked_add(tax_cents).filter(|t| *t <= MAX_LINE_CENTS).ok_or("EST_AMOUNT_TOO_LARGE: the estimate total is too large")?;
    let customer_id = find_or_create_named(tx, Party::Customer, &input.customer_name).await?;

    let estimate_id: i64 = sqlx::query_scalar(
        "INSERT INTO estimates (estimate_number, customer_id, estimate_date, expiration_date, memo, total_cents, tax_rate_id, tax_cents)
         VALUES ((SELECT COALESCE(MAX(estimate_number), 1000) + 1 FROM estimates), ?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING id",
    )
    .bind(customer_id)
    .bind(input.estimate_date)
    .bind(input.expiration_date)
    .bind(input.memo.as_deref().map(str::trim).filter(|m| !m.is_empty()))
    .bind(total)
    .bind(tax.and_then(|t| t.rate_id))
    .bind(tax_cents)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    for (idx, (line, amount)) in input.lines.iter().zip(amounts).enumerate() {
        let classification: Option<String> = sqlx::query_scalar(
            "SELECT t.classification FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1",
        )
        .bind(line.income_account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        if classification.as_deref() != Some("INCOME") {
            return Err(format!("EST_NOT_INCOME_ACCOUNT: line {} must use an Income account", idx + 1));
        }
        if let Some(class) = line.class_id {
            crate::classes::check_usable(tx, class, &format!("EST: line {}", idx + 1)).await?;
        }
        sqlx::query(
            "INSERT INTO estimate_lines
                (estimate_id, line_number, product_service_id, description, quantity_e4, rate_cents, amount_cents, income_account_id, class_id, is_taxable)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .bind(estimate_id)
        .bind(idx as i64 + 1)
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
    }
    Ok(estimate_id)
}

/// Marks an open estimate ACCEPTED, REJECTED or back to PENDING.
pub async fn set_estimate_status(pool: &SqlitePool, estimate_id: i64, status: &str) -> Result<EstimateSummary, String> {
    if !matches!(status, "PENDING" | "ACCEPTED" | "REJECTED") {
        return Err(format!("EST_BAD_STATUS: '{status}' is not a status you can set; convert to invoice to close"));
    }
    let result = sqlx::query("UPDATE estimates SET status = ?1 WHERE id = ?2 AND status <> 'CONVERTED'")
        .bind(status)
        .bind(estimate_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if result.rows_affected() != 1 {
        return Err("EST_CONVERTED: this estimate is already converted or does not exist".into());
    }
    get_estimate(pool, estimate_id).await
}

/// Creates and posts an invoice from the estimate's lines and closes the estimate, atomically.
pub async fn convert_to_invoice(
    pool: &SqlitePool,
    estimate_id: i64,
    issue_date: NaiveDate,
    due_date: NaiveDate,
) -> Result<EstimateSummary, String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let result = async {
        let customer_name = check_convertible(&mut tx, estimate_id).await?;
        let tax_rate_id: Option<i64> = sqlx::query_scalar("SELECT tax_rate_id FROM estimates WHERE id = ?1").bind(estimate_id).fetch_one(&mut *tx).await.map_err(db_err)?;
        let lines: Vec<(String, i64, i64, i64, Option<i64>, Option<i64>, bool)> = sqlx::query_as(
            "SELECT description, quantity_e4, rate_cents, income_account_id, product_service_id, class_id, is_taxable
             FROM estimate_lines WHERE estimate_id = ?1 ORDER BY line_number",
        )
        .bind(estimate_id)
        .fetch_all(&mut *tx)
        .await
        .map_err(db_err)?;
        let invoice = InvoiceInput {
            customer_name,
            issue_date,
            due_date,
            lines: lines
                .into_iter()
                .map(|(description, quantity_e4, rate_cents, income_account_id, product_service_id, class_id, is_taxable)| InvoiceLineInput {
                    description,
                    quantity_e4,
                    rate_cents,
                    income_account_id,
                    product_service_id,
                    class_id,
                    is_taxable,
                })
                .collect(),
            currency: None,
            tax_rate_id,
        };
        let invoice_id = write_invoice(&mut tx, &invoice).await?;
        sqlx::query("UPDATE estimates SET status = 'CONVERTED', converted_to_invoice_id = ?1 WHERE id = ?2")
            .bind(invoice_id)
            .bind(estimate_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        Ok(())
    }
    .await;
    match result {
        Ok(()) => {
            tx.commit().await.map_err(db_err)?;
            get_estimate(pool, estimate_id).await
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// One estimate line, for loading an estimate into the invoice form.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct EstimateLineView {
    pub product_service_id: Option<i64>,
    pub description: String,
    pub quantity_e4: i64,
    pub rate_cents: i64,
    pub income_account_id: i64,
    pub class_id: Option<i64>,
    pub is_taxable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EstimateDetail {
    pub estimate: EstimateSummary,
    pub lines: Vec<EstimateLineView>,
}

pub async fn estimate_detail(pool: &SqlitePool, estimate_id: i64) -> Result<EstimateDetail, String> {
    let estimate = get_estimate(pool, estimate_id).await?;
    let lines = sqlx::query_as(
        "SELECT product_service_id, description, quantity_e4, rate_cents, income_account_id, class_id, is_taxable
         FROM estimate_lines WHERE estimate_id = ?1 ORDER BY line_number",
    )
    .bind(estimate_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(EstimateDetail { estimate, lines })
}

/// Refuses converting an estimate that is already invoiced or was rejected.
async fn check_convertible(tx: &mut Transaction<'_, Sqlite>, estimate_id: i64) -> Result<String, String> {
    let header: Option<(String, String)> = sqlx::query_as(
        "SELECT c.name, e.status FROM estimates e JOIN customers c ON c.id = e.customer_id WHERE e.id = ?1",
    )
    .bind(estimate_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let (customer_name, status) = header.ok_or_else(|| format!("EST_NOT_FOUND: estimate {estimate_id} does not exist"))?;
    match status.as_str() {
        "CONVERTED" => Err("EST_CONVERTED: this estimate has already been converted to an invoice".to_string()),
        "REJECTED" => Err("EST_REJECTED: a rejected estimate cannot be invoiced; mark it pending or accepted first".to_string()),
        _ => Ok(customer_name),
    }
}

/// Posts an invoice that was started from an estimate (and possibly edited in the invoice form), and
/// marks the estimate converted, in one transaction: the estimate is never marked converted for an
/// invoice that did not post.
pub async fn invoice_from_estimate(pool: &SqlitePool, estimate_id: i64, input: InvoiceInput) -> Result<crate::invoices::InvoiceSummary, String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let result = async {
        check_convertible(&mut tx, estimate_id).await?;
        let invoice_id = write_invoice(&mut tx, &input).await?;
        sqlx::query("UPDATE estimates SET status = 'CONVERTED', converted_to_invoice_id = ?1 WHERE id = ?2")
            .bind(invoice_id)
            .bind(estimate_id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        Ok(invoice_id)
    }
    .await;
    match result {
        Ok(invoice_id) => {
            tx.commit().await.map_err(db_err)?;
            crate::invoices::get_summary(pool, invoice_id).await
        }
        Err(err) => Err(rollback(tx, err).await),
    }
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

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
    }

    #[tokio::test]
    async fn estimate_does_not_post_until_converted() {
        let pool = test_pool().await;
        let (services, ar) = (account_id(&pool, "4100").await, account_id(&pool, "1200").await);
        let line = |qty: i64, rate: i64| SalesLineInput { product_service_id: None, description: "Design".into(), quantity_e4: qty * 10_000, rate_cents: rate, income_account_id: services, class_id: None, is_taxable: false };
        let estimate = create_estimate(
            &pool,
            EstimateInput { customer_name: "Initech".into(), estimate_date: d(1), expiration_date: Some(d(31)), memo: None, lines: vec![line(10, 12_500), line(1, 5_000)], tax_rate_id: None },
        )
        .await
        .expect("estimate saves");
        assert_eq!((estimate.estimate_number, estimate.status.as_str(), estimate.total_cents), (1001, "PENDING", 130_000));

        let entries: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap();
        assert_eq!(entries, 0, "estimates are non-posting");

        set_estimate_status(&pool, estimate.id, "REJECTED").await.unwrap();
        assert!(convert_to_invoice(&pool, estimate.id, d(5), d(30)).await.unwrap_err().starts_with("EST_REJECTED"));
        set_estimate_status(&pool, estimate.id, "ACCEPTED").await.unwrap();

        let closed = convert_to_invoice(&pool, estimate.id, d(5), d(30)).await.expect("converts");
        assert_eq!(closed.status, "CONVERTED");
        assert!(closed.invoice_number.is_some());
        let ar_balance: i64 = sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1")
            .bind(ar)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(ar_balance, 130_000);
        assert!(convert_to_invoice(&pool, estimate.id, d(5), d(30)).await.unwrap_err().starts_with("EST_CONVERTED"));
        assert!(set_estimate_status(&pool, estimate.id, "PENDING").await.is_err());
    }

    #[tokio::test]
    async fn an_estimate_loaded_into_the_invoice_form_converts_only_when_the_invoice_posts() {
        let pool = test_pool().await;
        let services = account_id(&pool, "4100").await;
        let estimate = create_estimate(
            &pool,
            EstimateInput {
                customer_name: "Initech".into(),
                estimate_date: d(1),
                expiration_date: None,
                memo: None,
                lines: vec![SalesLineInput { product_service_id: None, description: "Design".into(), quantity_e4: 100_000, rate_cents: 12_500, income_account_id: services, class_id: None, is_taxable: false }],
                tax_rate_id: None,
            },
        )
        .await
        .unwrap();
        let detail = estimate_detail(&pool, estimate.id).await.unwrap();
        assert_eq!((detail.lines.len(), detail.lines[0].quantity_e4, detail.lines[0].rate_cents), (1, 100_000, 12_500));

        // The form's lines, edited before posting: 8 hours instead of 10.
        let form = |account: i64| InvoiceInput {
            customer_name: detail.estimate.customer_name.clone(),
            issue_date: d(5),
            due_date: d(30),
            lines: detail
                .lines
                .iter()
                .map(|l| InvoiceLineInput { description: l.description.clone(), quantity_e4: 80_000, rate_cents: l.rate_cents, income_account_id: account, product_service_id: l.product_service_id, class_id: None, is_taxable: false })
                .collect(),
            currency: None,
            tax_rate_id: None,
        };
        // A failed post leaves the estimate open.
        let checking = account_id(&pool, "1010").await;
        assert!(invoice_from_estimate(&pool, estimate.id, form(checking)).await.is_err());
        assert_eq!(get_estimate(&pool, estimate.id).await.unwrap().status, "PENDING");

        let invoice = invoice_from_estimate(&pool, estimate.id, form(services)).await.unwrap();
        assert_eq!(invoice.total_cents, 100_000, "the edited quantity was posted");
        let closed = get_estimate(&pool, estimate.id).await.unwrap();
        assert_eq!((closed.status.as_str(), closed.invoice_number), ("CONVERTED", Some(invoice.invoice_number)));
        assert!(invoice_from_estimate(&pool, estimate.id, form(services)).await.unwrap_err().starts_with("EST_CONVERTED"));

        // The invoice must still be for the estimate's customer.
        let other = create_estimate(
            &pool,
            EstimateInput { customer_name: "Initech".into(), estimate_date: d(2), expiration_date: None, memo: None, lines: vec![SalesLineInput { product_service_id: None, description: "Audit".into(), quantity_e4: 10_000, rate_cents: 5_000, income_account_id: services, class_id: None, is_taxable: false }], tax_rate_id: None },
        )
        .await
        .unwrap();
        // The converted estimate is kept exactly as quoted, linked to its invoice, and can't be removed.
        let kept = estimate_detail(&pool, estimate.id).await.unwrap();
        assert_eq!(kept.estimate.converted_to_invoice_id, Some(invoice.id));
        assert_eq!((kept.estimate.total_cents, kept.lines[0].quantity_e4), (125_000, 100_000), "the quote, not the edited invoice");
        let err = |r: Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error>| r.map_err(db_err).unwrap_err();
        assert!(err(sqlx::query("DELETE FROM estimate_lines WHERE estimate_id = ?1").bind(estimate.id).execute(&pool).await).starts_with("EST_CONVERTED"));
        assert!(err(sqlx::query("DELETE FROM estimates WHERE id = ?1").bind(estimate.id).execute(&pool).await).starts_with("EST_CONVERTED"));
        assert!(err(sqlx::query("UPDATE estimates SET memo = 'x' WHERE id = ?1").bind(estimate.id).execute(&pool).await).starts_with("EST_CONVERTED"));
        assert!(set_estimate_status(&pool, estimate.id, "PENDING").await.is_err());

        let elsewhere = InvoiceInput { customer_name: "Someone Else".into(), ..form(services) };
        assert!(invoice_from_estimate(&pool, other.id, elsewhere).await.unwrap_err().starts_with("EST_INVOICE_MISMATCH"));
        assert_eq!(get_estimate(&pool, other.id).await.unwrap().status, "PENDING");
    }
}
