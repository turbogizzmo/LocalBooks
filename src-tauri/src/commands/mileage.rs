//! Mileage: vehicles, mileage rates, trips, billing trips and the mileage reports.

use super::prelude::*;
use crate::invoices::InvoiceSummary;
use crate::mileage::{self, BillTripsInput, MileageRateInput, MileageRateView, MileageReport, MileageReportKind, MileageReportParams, TripInput, TripView, VehicleInput, VehicleView};

handlers![
    list_vehicles,
    save_vehicle,
    list_mileage_rates,
    save_mileage_rate,
    delete_mileage_rate,
    list_mileage_trips,
    list_unbilled_mileage,
    create_mileage_trip,
    update_mileage_trip,
    delete_mileage_trip,
    bill_mileage_trips,
    get_mileage_report,
];

#[tauri::command]
pub async fn list_vehicles(db: State<'_, Db>) -> Result<Vec<VehicleView>, String> {
    mileage::list_vehicles(&db.pool().await).await
}

#[tauri::command]
pub async fn save_vehicle(app: AppHandle, db: State<'_, Db>, id: Option<i64>, input: VehicleInput) -> Result<VehicleView, String> {
    let vehicle = mileage::save_vehicle(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(vehicle)
}

#[tauri::command]
pub async fn list_mileage_rates(db: State<'_, Db>) -> Result<Vec<MileageRateView>, String> {
    mileage::list_mileage_rates(&db.pool().await).await
}

#[tauri::command]
pub async fn save_mileage_rate(app: AppHandle, db: State<'_, Db>, id: Option<i64>, input: MileageRateInput) -> Result<MileageRateView, String> {
    let rate = mileage::save_mileage_rate(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(rate)
}

#[tauri::command]
pub async fn delete_mileage_rate(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    mileage::delete_mileage_rate(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn list_mileage_trips(db: State<'_, Db>, from: NaiveDate, to: NaiveDate) -> Result<Vec<TripView>, String> {
    mileage::list_trips(&db.pool().await, from, to).await
}

#[tauri::command]
pub async fn list_unbilled_mileage(db: State<'_, Db>, customer_id: Option<i64>) -> Result<Vec<TripView>, String> {
    mileage::list_unbilled_trips(&db.pool().await, customer_id).await
}

#[tauri::command]
pub async fn create_mileage_trip(app: AppHandle, db: State<'_, Db>, input: TripInput) -> Result<TripView, String> {
    let trip = mileage::create_trip(&db.pool().await, input).await?;
    notify(&app);
    Ok(trip)
}

#[tauri::command]
pub async fn update_mileage_trip(app: AppHandle, db: State<'_, Db>, id: i64, input: TripInput) -> Result<TripView, String> {
    let trip = mileage::update_trip(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(trip)
}

#[tauri::command]
pub async fn delete_mileage_trip(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    mileage::delete_trip(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn bill_mileage_trips(app: AppHandle, db: State<'_, Db>, input: BillTripsInput) -> Result<InvoiceSummary, String> {
    let invoice = mileage::bill_trips(&db.pool().await, input).await?;
    notify(&app);
    Ok(invoice)
}

#[tauri::command]
pub async fn get_mileage_report(db: State<'_, Db>, kind: MileageReportKind, params: MileageReportParams) -> Result<MileageReport, String> {
    mileage::mileage_report(&db.pool().await, kind, &params, Local::now().date_naive()).await
}
