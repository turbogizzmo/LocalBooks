//! Custom fields on list records.

use super::prelude::*;

handlers![
    list_custom_fields,
    create_custom_field,
    update_custom_field,
    delete_custom_field,
    get_custom_values,
    save_custom_values,
];

#[tauri::command]
pub async fn list_custom_fields(db: State<'_, Db>) -> Result<Vec<CustomFieldView>, String> {
    custom_fields::list_custom_fields(&db.pool().await).await
}

/// `list` is `CUSTOMER`, `VENDOR` or `ITEM`.
#[tauri::command]
pub async fn create_custom_field(app: AppHandle, db: State<'_, Db>, list: String, input: CustomFieldInput) -> Result<CustomFieldView, String> {
    let field = custom_fields::create_custom_field(&db.pool().await, custom_fields::parse_list(&list)?, input).await?;
    notify(&app);
    Ok(field)
}

#[tauri::command]
pub async fn update_custom_field(app: AppHandle, db: State<'_, Db>, id: i64, input: CustomFieldInput) -> Result<CustomFieldView, String> {
    let field = custom_fields::update_custom_field(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(field)
}

#[tauri::command]
pub async fn delete_custom_field(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    custom_fields::delete_custom_field(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

/// Every field of the list with this customer's, vendor's or item's value.
#[tauri::command]
pub async fn get_custom_values(db: State<'_, Db>, list: String, record_id: i64) -> Result<Vec<RecordField>, String> {
    custom_fields::record_fields(&db.pool().await, custom_fields::parse_list(&list)?, record_id).await
}

#[tauri::command]
pub async fn save_custom_values(app: AppHandle, db: State<'_, Db>, list: String, record_id: i64, values: Vec<FieldValue>) -> Result<Vec<RecordField>, String> {
    let fields = custom_fields::save_record_values(&db.pool().await, custom_fields::parse_list(&list)?, record_id, &values).await?;
    notify(&app);
    Ok(fields)
}
