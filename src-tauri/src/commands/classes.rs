//! Classes and reclassifying lines.

use super::prelude::*;

handlers![
    list_classes,
    create_class,
    update_class,
    set_class_active,
    list_reclass_lines,
    reclassify_lines,
    delete_class,
];

#[tauri::command]
pub async fn list_classes(db: State<'_, Db>) -> Result<Vec<ClassView>, String> {
    classes::list_classes(&db.pool().await).await
}

#[tauri::command]
pub async fn create_class(app: AppHandle, db: State<'_, Db>, input: ClassInput) -> Result<ClassView, String> {
    let class = classes::create_class(&db.pool().await, input).await?;
    notify(&app);
    Ok(class)
}

#[tauri::command]
pub async fn update_class(app: AppHandle, db: State<'_, Db>, id: i64, input: ClassInput) -> Result<ClassView, String> {
    let class = classes::update_class(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(class)
}

#[tauri::command]
pub async fn set_class_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<ClassView, String> {
    let class = classes::set_class_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(class)
}

#[tauri::command]
pub async fn list_reclass_lines(db: State<'_, Db>, query: ReclassQuery) -> Result<Vec<ReclassLine>, String> {
    classes::reclass_candidates(&db.pool().await, &query).await
}

#[tauri::command]
pub async fn reclassify_lines(app: AppHandle, db: State<'_, Db>, line_ids: Vec<i64>, class_id: Option<i64>) -> Result<usize, String> {
    let moved = classes::reclassify(&db.pool().await, &line_ids, class_id).await?;
    notify(&app);
    Ok(moved)
}

#[tauri::command]
pub async fn delete_class(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    classes::delete_class(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}
