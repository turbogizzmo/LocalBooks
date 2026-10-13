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
    pub invoice_id: Option<i64>,
    pub invoice_number: Option<i64>,
    pub line_count: i64,
}

const SUMMARY_SELECT: &str = "
    SELECT e.id, e.estimate_number, c.name AS customer_name, e.estimate_date, e.expiration_date, e.status, e.memo,
           e.total_cents, e.invoice_id, i.invoice_number,
           (SELECT COUNT(*) FROM estimate_lines WHERE estimate_id = e.id) AS line_count
    FROM estimates e
    JOIN customers c ON c.id = e.customer_id
    LEFT JOIN invoices i ON i.id = e.invoice_id";

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
    let customer_id = find_or_create_named(tx, Party::Customer, &input.customer_name).await?;

    let estimate_id: i64 = sqlx::query_scalar(
        "INSERT INTO estimates (estimate_number, customer_id, estimate_date, expiration_date, memo, total_cents)
         VALUES ((SELECT COALESCE(MAX(estimate_number), 1000) + 1 FROM estimates), ?1, ?2, ?3, ?4, ?5) RETURNING id",
    )
    .bind(customer_id)
    .bind(input.estimate_date)
    .bind(input.expiration_date)
    .bind(input.memo.as_deref().map(str::trim).filter(|m| !m.is_empty()))
    .bind(total)
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
        sqlx::query(
            "INSERT INTO estimate_lines
                (estimate_id, line_number, product_service_id, description, quantity_e4, rate_cents, amount_cents, income_account_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(estimate_id)
        .bind(idx as i64 + 1)
        .bind(line.product_service_id)
        .bind(line.description.trim())
        .bind(line.quantity_e4)
        .bind(line.rate_cents)
        .bind(amount)
        .bind(line.income_account_id)
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
    let result = sqlx::query("UPDATE estimates SET status = ?1 WHERE id = ?2 AND status <> 'CLOSED'")
        .bind(status)
        .bind(estimate_id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if result.rows_affected() != 1 {
        return Err("EST_CLOSED: this estimate is already converted or does not exist".into());
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
        let header: Option<(String, String)> = sqlx::query_as(
            "SELECT c.name, e.status FROM estimates e JOIN customers c ON c.id = e.customer_id WHERE e.id = ?1",
        )
        .bind(estimate_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db_err)?;
        let (customer_name, status) = header.ok_or_else(|| format!("EST_NOT_FOUND: estimate {estimate_id} does not exist"))?;
        if status == "CLOSED" {
            return Err("EST_CLOSED: this estimate has already been converted to an invoice".to_string());
        }
        if status == "REJECTED" {
            return Err("EST_REJECTED: a rejected estimate cannot be invoiced; mark it pending or accepted first".to_string());
        }
        let lines: Vec<(String, i64, i64, i64)> = sqlx::query_as(
            "SELECT description, quantity_e4, rate_cents, income_account_id FROM estimate_lines WHERE estimate_id = ?1 ORDER BY line_number",
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
                .map(|(description, quantity_e4, rate_cents, income_account_id)| InvoiceLineInput { description, quantity_e4, rate_cents, income_account_id })
                .collect(),
            currency: None,
        };
        let invoice_id = write_invoice(&mut tx, &invoice).await?;
        sqlx::query("UPDATE estimates SET status = 'CLOSED', invoice_id = ?1 WHERE id = ?2")
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
        let line = |qty: i64, rate: i64| SalesLineInput { product_service_id: None, description: "Design".into(), quantity_e4: qty * 10_000, rate_cents: rate, income_account_id: services };
        let estimate = create_estimate(
            &pool,
            EstimateInput { customer_name: "Initech".into(), estimate_date: d(1), expiration_date: Some(d(31)), memo: None, lines: vec![line(10, 12_500), line(1, 5_000)] },
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
        assert_eq!(closed.status, "CLOSED");
        assert!(closed.invoice_number.is_some());
        let ar_balance: i64 = sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1")
            .bind(ar)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(ar_balance, 130_000);
        assert!(convert_to_invoice(&pool, estimate.id, d(5), d(30)).await.unwrap_err().starts_with("EST_CLOSED"));
        assert!(set_estimate_status(&pool, estimate.id, "PENDING").await.is_err());
    }
}
