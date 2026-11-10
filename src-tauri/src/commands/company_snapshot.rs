//! Company Snapshot: the page of widgets, and adding, removing, ordering and dating them.

use super::prelude::*;
use crate::company_snapshot::{self, Period, Widget, WidgetKind};

handlers![get_company_snapshot, add_snapshot_widget, remove_snapshot_widget, set_snapshot_widget_period, arrange_snapshot_widgets];

#[tauri::command]
pub async fn get_company_snapshot(db: State<'_, Db>) -> Result<Vec<Widget>, String> {
    company_snapshot::snapshot(&db.pool().await, Local::now().date_naive()).await
}

#[tauri::command]
pub async fn add_snapshot_widget(app: AppHandle, db: State<'_, Db>, kind: WidgetKind) -> Result<i64, String> {
    let id = company_snapshot::add(&db.pool().await, kind).await?;
    notify(&app);
    Ok(id)
}

#[tauri::command]
pub async fn remove_snapshot_widget(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    company_snapshot::remove(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn set_snapshot_widget_period(app: AppHandle, db: State<'_, Db>, id: i64, period: Period) -> Result<(), String> {
    company_snapshot::set_period(&db.pool().await, id, period).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn arrange_snapshot_widgets(app: AppHandle, db: State<'_, Db>, order: Vec<i64>) -> Result<(), String> {
    company_snapshot::arrange(&db.pool().await, &order).await?;
    notify(&app);
    Ok(())
}
