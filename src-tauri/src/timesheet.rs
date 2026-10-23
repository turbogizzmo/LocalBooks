//! Employee time tracking: single time activities and weekly timesheets. Time is non-posting; it
//! records hours (as whole minutes) and, for billable work, the customer and hourly rate.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{db_err, rollback};
use crate::sales::PartyView;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeActivityInput {
    pub employee_id: i64,
    pub activity_date: NaiveDate,
    pub customer_id: Option<i64>,
    pub product_service_id: Option<i64>,
    pub minutes: i64,
    pub billable: bool,
    pub hourly_rate_cents: Option<i64>,
    pub description: Option<String>,
    #[serde(default)]
    pub class_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TimeActivityView {
    pub id: i64,
    pub employee_id: i64,
    pub employee_name: String,
    pub activity_date: String,
    pub customer_id: Option<i64>,
    pub customer_name: Option<String>,
    pub product_service_id: Option<i64>,
    pub service_name: Option<String>,
    pub minutes: i64,
    pub billable: bool,
    pub hourly_rate_cents: Option<i64>,
    pub description: Option<String>,
    pub class_id: Option<i64>,
    pub class_name: Option<String>,
}

pub async fn list_employees(pool: &SqlitePool) -> Result<Vec<PartyView>, String> {
    sqlx::query_as::<_, PartyView>(
        "SELECT id, name, email, is_active, 0 AS open_balance_cents FROM employees ORDER BY name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn create_employee(pool: &SqlitePool, name: &str, email: Option<&str>) -> Result<PartyView, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("EMP_NO_NAME: enter the employee's name".into());
    }
    sqlx::query_as::<_, PartyView>(
        "INSERT INTO employees (name, email) VALUES (?1, ?2) RETURNING id, name, email, is_active, 0 AS open_balance_cents",
    )
    .bind(name)
    .bind(email.map(str::trim).filter(|e| !e.is_empty()))
    .fetch_one(pool)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(db) if db.message().contains("UNIQUE") => format!("EMP_DUPLICATE: '{name}' already exists"),
        sqlx::Error::Database(db) if db.message().contains("CHECK") => "EMP_BAD_EMAIL: enter a valid email address".into(),
        other => db_err(other),
    })
}

fn validate(input: &TimeActivityInput, n: usize) -> Result<(), String> {
    if !(1..=1440).contains(&input.minutes) {
        return Err(format!("TIME_BAD_DURATION: entry {n} must be between 1 minute and 24 hours"));
    }
    if input.billable && (input.customer_id.is_none() || input.hourly_rate_cents.is_none()) {
        return Err(format!("TIME_BILLABLE_INCOMPLETE: billable entry {n} needs a customer and an hourly rate"));
    }
    if input.hourly_rate_cents.is_some_and(|r| r < 0) {
        return Err(format!("TIME_BAD_RATE: entry {n} has a negative rate"));
    }
    Ok(())
}

async fn write_activity(tx: &mut Transaction<'_, Sqlite>, input: &TimeActivityInput) -> Result<(), String> {
    if let Some(class) = input.class_id {
        crate::classes::check_usable(tx, class, "TIME: an entry").await?;
    }
    sqlx::query(
        "INSERT INTO time_activities
            (employee_id, activity_date, customer_id, product_service_id, minutes, billable, hourly_rate_cents, description, class_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )
    .bind(input.employee_id)
    .bind(input.activity_date)
    .bind(input.customer_id)
    .bind(input.product_service_id)
    .bind(input.minutes)
    .bind(input.billable)
    .bind(if input.billable { input.hourly_rate_cents } else { None })
    .bind(input.description.as_deref().map(str::trim).filter(|d| !d.is_empty()))
    .bind(input.class_id)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Saves one activity or a whole weekly timesheet; every entry saves, or none do.
pub async fn create_time_activities(pool: &SqlitePool, inputs: Vec<TimeActivityInput>) -> Result<usize, String> {
    if inputs.is_empty() {
        return Err("TIME_EMPTY: enter hours for at least one day".into());
    }
    for (idx, input) in inputs.iter().enumerate() {
        validate(input, idx + 1)?;
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    for input in &inputs {
        if let Err(err) = write_activity(&mut tx, input).await {
            return Err(rollback(tx, err).await);
        }
    }
    tx.commit().await.map_err(db_err)?;
    Ok(inputs.len())
}

pub async fn list_time_activities(pool: &SqlitePool, from: NaiveDate, to: NaiveDate) -> Result<Vec<TimeActivityView>, String> {
    sqlx::query_as::<_, TimeActivityView>(
        "SELECT t.id, t.employee_id, e.name AS employee_name, t.activity_date, t.customer_id, c.name AS customer_name,
                t.product_service_id, p.name AS service_name, t.minutes, t.billable, t.hourly_rate_cents, t.description,
                t.class_id, k.full_name AS class_name
         FROM time_activities t
         LEFT JOIN v_classes k ON k.id = t.class_id
         JOIN employees e ON e.id = t.employee_id
         LEFT JOIN customers c ON c.id = t.customer_id
         LEFT JOIN products_services p ON p.id = t.product_service_id
         WHERE t.activity_date BETWEEN ?1 AND ?2
         ORDER BY t.activity_date DESC, e.name, t.id DESC",
    )
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::sales::create_customer;

    fn d(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
    }

    #[tokio::test]
    async fn weekly_timesheet_saves_atomically_and_caps_the_day() {
        let pool = test_pool().await;
        let ana = create_employee(&pool, "Ana Ortiz", Some("ana@example.com")).await.unwrap();
        let acme = create_customer(&pool, "Acme", None).await.unwrap();
        let entry = |day, minutes, billable| TimeActivityInput {
            employee_id: ana.id,
            activity_date: d(day),
            customer_id: Some(acme.id),
            product_service_id: None,
            minutes,
            billable,
            hourly_rate_cents: Some(12_500),
            description: Some("Implementation".into()),
            class_id: None,
        };

        let saved = create_time_activities(&pool, vec![entry(12, 480, true), entry(13, 450, true), entry(14, 90, false)]).await.unwrap();
        assert_eq!(saved, 3);
        let week = list_time_activities(&pool, d(12), d(18)).await.unwrap();
        assert_eq!(week.iter().map(|t| t.minutes).sum::<i64>(), 1_020);
        assert_eq!(week.iter().find(|t| !t.billable).and_then(|t| t.hourly_rate_cents), None);

        // 16h already logged on the 12th + 9h more would exceed 24h: the whole batch is rejected.
        create_time_activities(&pool, vec![entry(12, 480, false)]).await.unwrap();
        let err = create_time_activities(&pool, vec![entry(15, 60, false), entry(12, 540, false)]).await.expect_err("over 24h");
        assert!(err.starts_with("TIME_OVER_24H"), "{err}");
        assert!(list_time_activities(&pool, d(15), d(15)).await.unwrap().is_empty());

        let mut no_customer = entry(16, 60, true);
        no_customer.customer_id = None;
        assert!(create_time_activities(&pool, vec![no_customer]).await.unwrap_err().starts_with("TIME_BILLABLE_INCOMPLETE"));
        assert!(create_employee(&pool, "ana ortiz", None).await.unwrap_err().starts_with("EMP_DUPLICATE"));
    }
}
