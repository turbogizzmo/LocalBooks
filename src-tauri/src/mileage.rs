//! Mileage tracking, like classic desktop accounting software's Enter Vehicle Mileage: a vehicle
//! list, standard mileage rates with effective dates, and trips.
//!
//! Trips are records; they never post to the ledger. A billable trip can be billed on an invoice
//! (`bill_trips`): one line per trip, the miles as the quantity, at the sales rate chosen for the
//! mileage item. The invoice posts like any other, and the trip remembers it. Voiding that invoice
//! frees the trip to be billed again; editing it keeps the trip on the edited invoice.
//!
//! Miles and odometer readings are scaled by 10,000 like invoice quantities. A mileage rate is
//! dollars per mile scaled by 10,000 (0.725 → 7_250). A trip's deductible amount is
//! round_half_up(miles × the rate in effect on the trip date), in cents; report totals add up the
//! trips' rounded amounts. LocalBooks has no built-in rates: the user enters the official rate.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::custom_reports::{ColumnHead, ColumnKind, RowKind};
use crate::invoices::{InvoiceInput, InvoiceLineInput, InvoiceSummary, MAX_QUANTITY_E4};
use crate::ledger::{db_err, mul_div_round, rollback, QUANTITY_SCALE};

/// Rates are dollars per mile × 10,000; the largest is $100 a mile.
pub const RATE_SCALE: i64 = 10_000;
const MAX_RATE_E4: i64 = 100 * RATE_SCALE;
/// One trip is at most 10,000 miles.
const MAX_TRIP_E4: i64 = 10_000 * QUANTITY_SCALE;

const NO_JOB: &str = "No customer/job";

// ------------------------------------------------------------------------------------------------
// Vehicles
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct VehicleView {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub is_active: bool,
    pub trip_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VehicleInput {
    pub name: String,
    pub description: Option<String>,
    #[serde(default = "active_by_default")]
    pub is_active: bool,
}

fn active_by_default() -> bool {
    true
}

const VEHICLE_SELECT: &str = "SELECT v.id, v.name, v.description, v.is_active,
       (SELECT COUNT(*) FROM mileage_trips t WHERE t.vehicle_id = v.id) AS trip_count
  FROM vehicles v";

pub async fn list_vehicles(pool: &SqlitePool) -> Result<Vec<VehicleView>, String> {
    sqlx::query_as::<_, VehicleView>(&format!("{VEHICLE_SELECT} ORDER BY v.is_active DESC, v.name COLLATE NOCASE"))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Adds a vehicle (`id` = None) or changes one: its name, description, or active status.
pub async fn save_vehicle(pool: &SqlitePool, id: Option<i64>, input: VehicleInput) -> Result<VehicleView, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("MILE_NO_VEHICLE_NAME: enter the vehicle's name".into());
    }
    if name.chars().count() > 100 {
        return Err("MILE_VEHICLE_NAME_TOO_LONG: a vehicle name can be at most 100 characters".into());
    }
    let description = input.description.as_deref().map(str::trim).filter(|d| !d.is_empty());
    let result = match id {
        None => sqlx::query_scalar::<_, i64>("INSERT INTO vehicles (name, description, is_active) VALUES (?1, ?2, ?3) RETURNING id")
            .bind(name)
            .bind(description)
            .bind(input.is_active)
            .fetch_one(pool)
            .await,
        Some(id) => sqlx::query_scalar::<_, i64>("UPDATE vehicles SET name = ?1, description = ?2, is_active = ?3 WHERE id = ?4 RETURNING id")
            .bind(name)
            .bind(description)
            .bind(input.is_active)
            .bind(id)
            .fetch_optional(pool)
            .await
            .and_then(|found| found.ok_or(sqlx::Error::RowNotFound)),
    };
    let id = result.map_err(|e| match e {
        sqlx::Error::Database(db) if db.message().contains("UNIQUE") => format!("MILE_DUPLICATE_VEHICLE: '{name}' already exists"),
        sqlx::Error::RowNotFound => "MILE_UNKNOWN_VEHICLE: that vehicle does not exist".into(),
        other => db_err(other),
    })?;
    sqlx::query_as::<_, VehicleView>(&format!("{VEHICLE_SELECT} WHERE v.id = ?1")).bind(id).fetch_one(pool).await.map_err(db_err)
}

// ------------------------------------------------------------------------------------------------
// Mileage rates
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct MileageRateView {
    pub id: i64,
    pub effective_date: String,
    pub rate_e4: i64,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MileageRateInput {
    pub effective_date: NaiveDate,
    /// Dollars per mile × 10,000 (0.725 → 7_250).
    pub rate_e4: i64,
    pub note: Option<String>,
}

pub async fn list_mileage_rates(pool: &SqlitePool) -> Result<Vec<MileageRateView>, String> {
    sqlx::query_as::<_, MileageRateView>("SELECT id, effective_date, rate_e4, note FROM mileage_rates ORDER BY effective_date DESC")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Adds a rate (`id` = None) or changes one. Each effective date has one rate.
pub async fn save_mileage_rate(pool: &SqlitePool, id: Option<i64>, input: MileageRateInput) -> Result<MileageRateView, String> {
    if !(1..=MAX_RATE_E4).contains(&input.rate_e4) {
        return Err("MILE_BAD_RATE: a mileage rate must be more than $0 and at most $100 a mile".into());
    }
    let note = input.note.as_deref().map(str::trim).filter(|n| !n.is_empty());
    let result = match id {
        None => sqlx::query_as::<_, MileageRateView>(
            "INSERT INTO mileage_rates (effective_date, rate_e4, note) VALUES (?1, ?2, ?3) RETURNING id, effective_date, rate_e4, note",
        )
        .bind(input.effective_date)
        .bind(input.rate_e4)
        .bind(note)
        .fetch_one(pool)
        .await,
        Some(id) => sqlx::query_as::<_, MileageRateView>(
            "UPDATE mileage_rates SET effective_date = ?1, rate_e4 = ?2, note = ?3 WHERE id = ?4 RETURNING id, effective_date, rate_e4, note",
        )
        .bind(input.effective_date)
        .bind(input.rate_e4)
        .bind(note)
        .bind(id)
        .fetch_optional(pool)
        .await
        .and_then(|found| found.ok_or(sqlx::Error::RowNotFound)),
    };
    result.map_err(|e| match e {
        sqlx::Error::Database(db) if db.message().contains("UNIQUE") => {
            format!("MILE_DUPLICATE_RATE: there is already a rate effective {}", input.effective_date)
        }
        sqlx::Error::RowNotFound => "MILE_UNKNOWN_RATE: that rate does not exist".into(),
        other => db_err(other),
    })
}

pub async fn delete_mileage_rate(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let deleted = sqlx::query("DELETE FROM mileage_rates WHERE id = ?1").bind(id).execute(pool).await.map_err(db_err)?;
    if deleted.rows_affected() == 0 {
        return Err("MILE_UNKNOWN_RATE: that rate does not exist".into());
    }
    Ok(())
}

/// The rate in effect on `date`: the one with the latest effective date on or before it. Trips and
/// reports look it up in their own queries the same way; this checks that rule on its own.
#[cfg(test)]
pub async fn rate_on(pool: &SqlitePool, date: NaiveDate) -> Result<Option<i64>, String> {
    sqlx::query_scalar("SELECT rate_e4 FROM mileage_rates WHERE effective_date <= ?1 ORDER BY effective_date DESC LIMIT 1")
        .bind(date)
        .fetch_optional(pool)
        .await
        .map_err(db_err)
}

/// round_half_up(miles × rate) in cents.
pub fn deductible_cents(miles_e4: i64, rate_e4: i64) -> i64 {
    // miles_e4 × rate_e4 is miles × dollars × 10^8; cents are dollars × 100.
    mul_div_round(miles_e4, rate_e4, QUANTITY_SCALE * RATE_SCALE / 100).unwrap_or(i64::MAX)
}

// ------------------------------------------------------------------------------------------------
// Trips
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TripInput {
    pub vehicle_id: i64,
    pub trip_date: NaiveDate,
    /// Both odometer readings, or neither (then `miles_e4` is the total).
    #[serde(default)]
    pub odometer_start_e4: Option<i64>,
    #[serde(default)]
    pub odometer_end_e4: Option<i64>,
    /// Total miles; worked out from the odometer when both readings are given.
    #[serde(default)]
    pub miles_e4: Option<i64>,
    pub notes: Option<String>,
    #[serde(default)]
    pub customer_id: Option<i64>,
    #[serde(default)]
    pub billable: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TripView {
    pub id: i64,
    pub vehicle_id: i64,
    pub vehicle_name: String,
    pub trip_date: String,
    pub odometer_start_e4: Option<i64>,
    pub odometer_end_e4: Option<i64>,
    pub miles_e4: i64,
    pub notes: Option<String>,
    pub customer_id: Option<i64>,
    pub customer_name: Option<String>,
    pub billable: bool,
    /// The live invoice the trip is billed on; `None` while it is unbilled (or its invoice was voided).
    pub invoice_id: Option<i64>,
    pub invoice_number: Option<i64>,
    /// The rate in effect on the trip date, if any.
    pub rate_e4: Option<i64>,
    #[sqlx(skip)]
    pub deductible_cents: Option<i64>,
}

const TRIP_SELECT: &str = "
    SELECT t.id, t.vehicle_id, v.name AS vehicle_name, t.trip_date, t.odometer_start_e4, t.odometer_end_e4,
           t.miles_e4, t.notes, t.customer_id, c.name AS customer_name, t.billable,
           CASE WHEN i.is_voided = 0 THEN t.invoice_id END AS invoice_id,
           CASE WHEN i.is_voided = 0 THEN i.invoice_number END AS invoice_number,
           (SELECT r.rate_e4 FROM mileage_rates r WHERE r.effective_date <= t.trip_date ORDER BY r.effective_date DESC LIMIT 1) AS rate_e4
    FROM mileage_trips t
    JOIN vehicles v ON v.id = t.vehicle_id
    LEFT JOIN customers c ON c.id = t.customer_id
    LEFT JOIN invoices i ON i.id = t.invoice_id";

fn with_deductible(mut trips: Vec<TripView>) -> Vec<TripView> {
    for trip in &mut trips {
        trip.deductible_cents = trip.rate_e4.map(|rate| deductible_cents(trip.miles_e4, rate));
    }
    trips
}

/// The miles a trip records, after checking its odometer readings and total.
fn trip_miles(input: &TripInput) -> Result<i64, String> {
    let miles = match (input.odometer_start_e4, input.odometer_end_e4) {
        (Some(start), Some(end)) => {
            if start < 0 {
                return Err("MILE_BAD_ODOMETER: odometer readings can't be negative".into());
            }
            if end <= start {
                return Err("MILE_BAD_ODOMETER: the ending odometer must be more than the starting one".into());
            }
            let miles = end - start;
            if input.miles_e4.is_some_and(|m| m != miles) {
                return Err("MILE_ODOMETER_MISMATCH: total miles don't match the odometer readings".into());
            }
            miles
        }
        (None, None) => input.miles_e4.ok_or("MILE_NO_MILES: enter the total miles or both odometer readings")?,
        _ => return Err("MILE_BAD_ODOMETER: enter both odometer readings, or neither".into()),
    };
    if !(1..=MAX_TRIP_E4).contains(&miles) {
        return Err("MILE_BAD_MILES: a trip must be more than 0 and at most 10,000 miles".into());
    }
    Ok(miles)
}

async fn check_trip(pool: &SqlitePool, input: &TripInput) -> Result<i64, String> {
    let miles = trip_miles(input)?;
    if input.billable && input.customer_id.is_none() {
        return Err("MILE_BILLABLE_NO_CUSTOMER: a billable trip needs a customer or job".into());
    }
    let vehicle: Option<bool> = sqlx::query_scalar("SELECT is_active FROM vehicles WHERE id = ?1").bind(input.vehicle_id).fetch_optional(pool).await.map_err(db_err)?;
    match vehicle {
        None => return Err("MILE_UNKNOWN_VEHICLE: that vehicle does not exist".into()),
        Some(false) => return Err("MILE_INACTIVE_VEHICLE: that vehicle is inactive; make it active to record trips".into()),
        Some(true) => {}
    }
    if let Some(customer) = input.customer_id {
        let exists: Option<i64> = sqlx::query_scalar("SELECT id FROM customers WHERE id = ?1").bind(customer).fetch_optional(pool).await.map_err(db_err)?;
        if exists.is_none() {
            return Err("MILE_UNKNOWN_CUSTOMER: that customer or job does not exist".into());
        }
    }
    Ok(miles)
}

pub async fn get_trip(pool: &SqlitePool, id: i64) -> Result<TripView, String> {
    let trip = sqlx::query_as::<_, TripView>(&format!("{TRIP_SELECT} WHERE t.id = ?1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or("MILE_UNKNOWN_TRIP: that trip does not exist")?;
    Ok(with_deductible(vec![trip]).remove(0))
}

pub async fn create_trip(pool: &SqlitePool, input: TripInput) -> Result<TripView, String> {
    let miles = check_trip(pool, &input).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO mileage_trips (vehicle_id, trip_date, odometer_start_e4, odometer_end_e4, miles_e4, notes, customer_id, billable)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING id",
    )
    .bind(input.vehicle_id)
    .bind(input.trip_date)
    .bind(input.odometer_start_e4)
    .bind(input.odometer_end_e4)
    .bind(miles)
    .bind(input.notes.as_deref().map(str::trim).filter(|n| !n.is_empty()))
    .bind(input.customer_id)
    .bind(input.billable)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    get_trip(pool, id).await
}

/// Changes a trip that isn't on a live invoice.
pub async fn update_trip(pool: &SqlitePool, id: i64, input: TripInput) -> Result<TripView, String> {
    if get_trip(pool, id).await?.invoice_id.is_some() {
        return Err("MILE_TRIP_BILLED: this trip is on an invoice; void the invoice to change it".into());
    }
    let miles = check_trip(pool, &input).await?;
    sqlx::query(
        "UPDATE mileage_trips SET vehicle_id = ?1, trip_date = ?2, odometer_start_e4 = ?3, odometer_end_e4 = ?4, miles_e4 = ?5,
                notes = ?6, customer_id = ?7, billable = ?8, invoice_id = NULL
         WHERE id = ?9",
    )
    .bind(input.vehicle_id)
    .bind(input.trip_date)
    .bind(input.odometer_start_e4)
    .bind(input.odometer_end_e4)
    .bind(miles)
    .bind(input.notes.as_deref().map(str::trim).filter(|n| !n.is_empty()))
    .bind(input.customer_id)
    .bind(input.billable)
    .bind(id)
    .execute(pool)
    .await
    .map_err(db_err)?;
    get_trip(pool, id).await
}

/// Deletes a trip that isn't on a live invoice.
pub async fn delete_trip(pool: &SqlitePool, id: i64) -> Result<(), String> {
    if get_trip(pool, id).await?.invoice_id.is_some() {
        return Err("MILE_TRIP_BILLED: this trip is on an invoice; void the invoice to delete it".into());
    }
    sqlx::query("DELETE FROM mileage_trips WHERE id = ?1").bind(id).execute(pool).await.map_err(db_err)?;
    Ok(())
}

pub async fn list_trips(pool: &SqlitePool, from: NaiveDate, to: NaiveDate) -> Result<Vec<TripView>, String> {
    let trips = sqlx::query_as::<_, TripView>(&format!("{TRIP_SELECT} WHERE t.trip_date BETWEEN ?1 AND ?2 ORDER BY t.trip_date DESC, t.id DESC"))
        .bind(from)
        .bind(to)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    Ok(with_deductible(trips))
}

/// Billable trips not yet on a live invoice, oldest first; for one customer or job when given.
pub async fn list_unbilled_trips(pool: &SqlitePool, customer_id: Option<i64>) -> Result<Vec<TripView>, String> {
    let trips = sqlx::query_as::<_, TripView>(&format!(
        "{TRIP_SELECT} WHERE t.billable = 1 AND (t.invoice_id IS NULL OR i.is_voided = 1) AND (?1 IS NULL OR t.customer_id = ?1)
         ORDER BY c.name COLLATE NOCASE, t.trip_date, t.id"
    ))
    .bind(customer_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(with_deductible(trips))
}

// ------------------------------------------------------------------------------------------------
// Billing
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillTripsInput {
    pub customer_id: i64,
    pub trip_ids: Vec<i64>,
    /// The mileage item (a product or service) the lines bill; its income account gets the sale.
    pub product_service_id: i64,
    /// The sales rate per mile, in cents.
    pub rate_cents: i64,
    pub issue_date: NaiveDate,
    pub due_date: NaiveDate,
    #[serde(default)]
    pub tax_rate_id: Option<i64>,
}

/// Miles for an invoice description: "12.5", "120".
fn miles_text(miles_e4: i64) -> String {
    let whole = miles_e4 / QUANTITY_SCALE;
    let frac = miles_e4 % QUANTITY_SCALE;
    if frac == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{}", format!("{frac:04}").trim_end_matches('0'))
    }
}

/// Bills trips on a new invoice to their customer or job: one line per trip, the trip's miles at
/// `rate_cents` a mile, for the chosen mileage item. The invoice posts and the trips are marked
/// billed, together or not at all.
pub async fn bill_trips(pool: &SqlitePool, input: BillTripsInput) -> Result<InvoiceSummary, String> {
    if input.trip_ids.is_empty() {
        return Err("MILE_NOTHING_TO_BILL: choose at least one trip to bill".into());
    }
    if input.rate_cents <= 0 {
        return Err("MILE_BAD_SALES_RATE: enter a sales rate per mile above zero".into());
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let invoice_id = match write_billing(&mut tx, &input).await {
        Ok(id) => id,
        Err(err) => return Err(rollback(tx, err).await),
    };
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    crate::invoices::get_summary(pool, invoice_id).await
}

async fn write_billing(tx: &mut Transaction<'_, Sqlite>, input: &BillTripsInput) -> Result<i64, String> {
    let customer: String = sqlx::query_scalar("SELECT name FROM customers WHERE id = ?1")
        .bind(input.customer_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?
        .ok_or("MILE_UNKNOWN_CUSTOMER: that customer or job does not exist")?;
    let item: Option<(String, i64, bool, bool)> = sqlx::query_as("SELECT name, income_account_id, is_active, is_taxable FROM products_services WHERE id = ?1")
        .bind(input.product_service_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    let (item_name, income_account, item_taxable) = match item {
        None => return Err("MILE_UNKNOWN_ITEM: choose the mileage item to bill".into()),
        Some((_, _, false, _)) => return Err("MILE_INACTIVE_ITEM: that item is inactive".into()),
        Some((name, account, true, taxable)) => (name, account, taxable),
    };

    let mut lines = Vec::with_capacity(input.trip_ids.len());
    for (n, trip_id) in input.trip_ids.iter().enumerate() {
        let trip: Option<(String, i64, Option<String>, Option<i64>, bool, Option<i64>, Option<bool>, String)> = sqlx::query_as(
            "SELECT t.trip_date, t.miles_e4, t.notes, t.customer_id, t.billable, t.invoice_id, i.is_voided, v.name
             FROM mileage_trips t JOIN vehicles v ON v.id = t.vehicle_id LEFT JOIN invoices i ON i.id = t.invoice_id
             WHERE t.id = ?1",
        )
        .bind(trip_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        let Some((date, miles, notes, trip_customer, billable, invoice, voided, vehicle)) = trip else {
            return Err(format!("MILE_UNKNOWN_TRIP: trip {} does not exist", n + 1));
        };
        if !billable {
            return Err(format!("MILE_NOT_BILLABLE: the trip on {date} isn't billable"));
        }
        if invoice.is_some() && voided != Some(true) {
            return Err(format!("MILE_TRIP_BILLED: the trip on {date} is already on an invoice"));
        }
        if trip_customer != Some(input.customer_id) {
            return Err(format!("MILE_OTHER_CUSTOMER: the trip on {date} is for a different customer or job"));
        }
        if miles > MAX_QUANTITY_E4 {
            return Err(format!("MILE_BAD_MILES: the trip on {date} is too long to bill"));
        }
        let mut description = format!("{item_name}: {} mi, {date}, {vehicle}", miles_text(miles));
        if let Some(notes) = notes.filter(|n| !n.is_empty()) {
            description.push_str(" — ");
            description.push_str(&notes);
        }
        lines.push(InvoiceLineInput {
            description,
            quantity_e4: miles,
            rate_cents: input.rate_cents,
            income_account_id: income_account,
            product_service_id: Some(input.product_service_id),
            class_id: None,
            is_taxable: item_taxable,
        });
    }

    let invoice = InvoiceInput {
        customer_name: customer,
        issue_date: input.issue_date,
        due_date: input.due_date,
        lines,
        currency: None,
        tax_rate_id: input.tax_rate_id,
        discount_cents: 0,
        details: Default::default(),
    };
    let invoice_id = crate::invoices::write_invoice(tx, &invoice).await?;
    for trip_id in &input.trip_ids {
        sqlx::query("UPDATE mileage_trips SET invoice_id = ?1 WHERE id = ?2").bind(invoice_id).bind(trip_id).execute(&mut **tx).await.map_err(db_err)?;
    }
    Ok(invoice_id)
}

/// Keeps billed trips with an invoice that is edited (voided and replaced) in the same transaction.
pub(crate) async fn follow_replaced_invoice(tx: &mut Transaction<'_, Sqlite>, old_invoice: i64, new_invoice: i64) -> Result<(), String> {
    sqlx::query("UPDATE mileage_trips SET invoice_id = ?1 WHERE invoice_id = ?2")
        .bind(new_invoice)
        .bind(old_invoice)
        .execute(&mut **tx)
        .await
        .map(|_| ())
        .map_err(db_err)
}

// ------------------------------------------------------------------------------------------------
// Reports
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MileageReportKind {
    /// Miles and deductible amount per vehicle.
    VehicleSummary,
    /// Each trip, grouped by vehicle.
    VehicleDetail,
    /// Miles, billable miles and deductible amount per customer and job.
    JobSummary,
    /// Each trip, grouped by customer and job.
    JobDetail,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MileageReportParams {
    /// First day; `None` for all dates.
    pub start_date: Option<NaiveDate>,
    /// Last day; `None` for today.
    pub end_date: Option<NaiveDate>,
    pub vehicle_id: Option<i64>,
    pub customer_id: Option<i64>,
    /// Only trips with no customer or job.
    pub no_job: bool,
}

/// The detail report a summary row opens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MileageDrill {
    pub report: MileageReportKind,
    pub vehicle_id: Option<i64>,
    pub customer_id: Option<i64>,
    pub no_job: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MileageRow {
    pub kind: RowKind,
    pub depth: u32,
    pub cells: Vec<Value>,
    pub drill: Option<MileageDrill>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MileageReport {
    pub kind: MileageReportKind,
    pub start_date: Option<NaiveDate>,
    pub end_date: NaiveDate,
    pub columns: Vec<ColumnHead>,
    pub rows: Vec<MileageRow>,
    /// Trips the report is built from (named like the other engine reports, so the shared report
    /// table shows the count and its empty state).
    pub line_count: usize,
    /// Trips dated before the first mileage rate; they have no deductible amount.
    pub trips_without_rate: usize,
}

fn col(label: &str, kind: ColumnKind) -> ColumnHead {
    ColumnHead { label: label.into(), kind }
}

/// A group of trips: the vehicle or customer/job they belong to.
struct Group<'a> {
    label: String,
    vehicle_id: Option<i64>,
    customer_id: Option<i64>,
    trips: Vec<&'a TripView>,
}

fn totals(trips: &[&TripView]) -> (i64, i64, i64) {
    let miles = trips.iter().map(|t| t.miles_e4).sum();
    let billable = trips.iter().filter(|t| t.billable).map(|t| t.miles_e4).sum();
    let deductible = trips.iter().filter_map(|t| t.deductible_cents).sum();
    (miles, billable, deductible)
}

fn billing_status(trip: &TripView) -> String {
    match (trip.billable, trip.invoice_number) {
        (false, _) => "Not billable".into(),
        (true, Some(number)) => format!("Billed, invoice #{number}"),
        (true, None) => "Unbilled".into(),
    }
}

pub async fn mileage_report(pool: &SqlitePool, kind: MileageReportKind, params: &MileageReportParams, today: NaiveDate) -> Result<MileageReport, String> {
    let end = params.end_date.unwrap_or(today);
    let start = params.start_date;
    if start.is_some_and(|s| s > end) {
        return Err("MILE_BAD_RANGE: the start date is after the end date".into());
    }
    // A customer filter takes in its jobs ("Customer:Job") too.
    let trips = sqlx::query_as::<_, TripView>(&format!(
        "{TRIP_SELECT}
         LEFT JOIN customers p ON p.id = ?4
         WHERE (?1 IS NULL OR t.trip_date >= ?1) AND t.trip_date <= ?2
           AND (?3 IS NULL OR t.vehicle_id = ?3)
           AND (?4 IS NULL OR t.customer_id = ?4 OR substr(c.name, 1, length(p.name) + 1) = p.name || ':')
           AND (?5 = 0 OR t.customer_id IS NULL)
         ORDER BY t.trip_date, t.id"
    ))
    .bind(start)
    .bind(end)
    .bind(params.vehicle_id)
    .bind(params.customer_id)
    .bind(params.no_job)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let trips = with_deductible(trips);

    let by_vehicle = matches!(kind, MileageReportKind::VehicleSummary | MileageReportKind::VehicleDetail);
    let mut groups: Vec<Group> = Vec::new();
    for trip in &trips {
        let (label, vehicle_id, customer_id) = if by_vehicle {
            (trip.vehicle_name.clone(), Some(trip.vehicle_id), None)
        } else {
            (trip.customer_name.clone().unwrap_or_else(|| NO_JOB.into()), None, trip.customer_id)
        };
        match groups.iter_mut().find(|g| g.vehicle_id == vehicle_id && g.customer_id == customer_id) {
            Some(group) => group.trips.push(trip),
            None => groups.push(Group { label, vehicle_id, customer_id, trips: vec![trip] }),
        }
    }
    // Alphabetical, with "No customer/job" last.
    groups.sort_by(|a, b| {
        let none = |g: &Group| !by_vehicle && g.customer_id.is_none();
        none(a).cmp(&none(b)).then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });

    let all: Vec<&TripView> = trips.iter().collect();
    let (columns, rows) = match kind {
        MileageReportKind::VehicleSummary | MileageReportKind::JobSummary => {
            let job = kind == MileageReportKind::JobSummary;
            let mut columns = vec![col(if job { "Customer:Job" } else { "Vehicle" }, ColumnKind::Text), col("Trips", ColumnKind::Number), col("Miles", ColumnKind::Quantity)];
            if job {
                columns.push(col("Billable miles", ColumnKind::Quantity));
            }
            columns.push(col("Deductible amount", ColumnKind::Money));
            let summary_row = |kind: RowKind, label: String, trips: &[&TripView], drill: Option<MileageDrill>| {
                let (m, b, d) = totals(trips);
                let mut cells = vec![json!(label), json!(trips.len()), json!(m)];
                if job {
                    cells.push(json!(b));
                }
                cells.push(json!(d));
                MileageRow { kind, depth: 0, cells, drill }
            };
            let mut rows: Vec<MileageRow> = groups
                .iter()
                .map(|g| {
                    let drill = MileageDrill {
                        report: if job { MileageReportKind::JobDetail } else { MileageReportKind::VehicleDetail },
                        vehicle_id: g.vehicle_id,
                        customer_id: g.customer_id,
                        no_job: job && g.customer_id.is_none(),
                    };
                    summary_row(RowKind::Detail, g.label.clone(), &g.trips, Some(drill))
                })
                .collect();
            rows.push(summary_row(RowKind::Total, "TOTAL".into(), &all, None));
            (columns, rows)
        }
        MileageReportKind::VehicleDetail | MileageReportKind::JobDetail => {
            let job = kind == MileageReportKind::JobDetail;
            // The first column is text: it also holds the subtotal and total labels.
            let columns = vec![
                col(if job { "Vehicle" } else { "Customer:Job" }, ColumnKind::Text),
                col("Trip date", ColumnKind::Date),
                col("Notes", ColumnKind::Text),
                col("Billing status", ColumnKind::Text),
                col("Odometer start", ColumnKind::Quantity),
                col("Odometer end", ColumnKind::Quantity),
                col("Miles", ColumnKind::Quantity),
                col("Rate ($/mile)", ColumnKind::Quantity),
                col("Deductible amount", ColumnKind::Money),
            ];
            let width = columns.len();
            let total_row = |kind: RowKind, depth: u32, label: String, trips: &[&TripView]| {
                let (m, _, d) = totals(trips);
                let mut cells = vec![Value::Null; width];
                cells[0] = json!(label);
                cells[6] = json!(m);
                cells[8] = json!(d);
                MileageRow { kind, depth, cells, drill: None }
            };
            let mut rows = Vec::new();
            for group in &groups {
                rows.push(MileageRow { kind: RowKind::Section, depth: 0, cells: vec![json!(group.label)], drill: None });
                for trip in &group.trips {
                    let other = if job { Some(trip.vehicle_name.clone()) } else { trip.customer_name.clone() };
                    rows.push(MileageRow {
                        kind: RowKind::Detail,
                        depth: 1,
                        cells: vec![
                            json!(other),
                            json!(trip.trip_date),
                            json!(trip.notes),
                            json!(billing_status(trip)),
                            json!(trip.odometer_start_e4),
                            json!(trip.odometer_end_e4),
                            json!(trip.miles_e4),
                            json!(trip.rate_e4),
                            json!(trip.deductible_cents),
                        ],
                        drill: None,
                    });
                }
                rows.push(total_row(RowKind::Subtotal, 0, format!("Total {}", group.label), &group.trips));
            }
            rows.push(total_row(RowKind::Total, 0, "TOTAL".into(), &all));
            (columns, rows)
        }
    };
    Ok(MileageReport {
        kind,
        start_date: start,
        end_date: end,
        columns,
        rows,
        line_count: trips.len(),
        trips_without_rate: trips.iter().filter(|t| t.rate_e4.is_none()).count(),
    })
}

#[cfg(test)]
mod tests;
