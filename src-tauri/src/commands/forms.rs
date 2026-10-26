//! Company logo, form designer and PDF forms.

use super::prelude::*;

handlers![
    set_company_logo,
    clear_company_logo,
    get_company_logo,
    list_form_styles,
    create_form_style,
    rename_form_style,
    set_default_form_style,
    delete_form_style,
    get_form_template,
    save_form_template,
    reset_form_template,
    preview_form,
    render_form,
    save_form_pdf,
];

#[tauri::command]
pub async fn set_company_logo(app: AppHandle, db: State<'_, Db>, source_path: String) -> Result<String, String> {
    let bytes = std::fs::read(&source_path).map_err(|e| format!("PREF_LOGO_READ: cannot read {source_path}: {e}"))?;
    let url = preferences::set_logo(&db.pool().await, &bytes).await?;
    notify(&app);
    Ok(url)
}

#[tauri::command]
pub async fn clear_company_logo(app: AppHandle, db: State<'_, Db>) -> Result<(), String> {
    preferences::clear_logo(&db.pool().await).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn get_company_logo(db: State<'_, Db>) -> Result<Option<String>, String> {
    preferences::get_logo(&db.pool().await).await
}

#[tauri::command]
pub async fn list_form_styles(db: State<'_, Db>) -> Result<Vec<FormStyle>, String> {
    forms::list_styles(&db.pool().await).await
}

/// A new form style, optionally starting as a copy of another.
#[tauri::command]
pub async fn create_form_style(db: State<'_, Db>, name: String, copy_from: Option<i64>) -> Result<FormStyle, String> {
    forms::create_style(&db.pool().await, &name, copy_from).await
}

#[tauri::command]
pub async fn rename_form_style(db: State<'_, Db>, id: i64, name: String) -> Result<FormStyle, String> {
    forms::rename_style(&db.pool().await, id, &name).await
}

#[tauri::command]
pub async fn set_default_form_style(db: State<'_, Db>, id: i64) -> Result<(), String> {
    forms::set_default_style(&db.pool().await, id).await
}

#[tauri::command]
pub async fn delete_form_style(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    forms::delete_style(&db.pool().await, id).await?;
    // Customers who used the style go back to the default.
    notify(&app);
    Ok(())
}

/// A style's template for a form type (`style_id` None = the default style).
#[tauri::command]
pub async fn get_form_template(db: State<'_, Db>, style_id: Option<i64>, kind: FormKind) -> Result<FormTemplate, String> {
    forms::get_template(&db.pool().await, style_id, kind).await
}

#[tauri::command]
pub async fn save_form_template(db: State<'_, Db>, style_id: Option<i64>, kind: FormKind, template: FormTemplate) -> Result<FormTemplate, String> {
    forms::save_template(&db.pool().await, style_id, kind, template).await
}

#[tauri::command]
pub async fn reset_form_template(db: State<'_, Db>, style_id: Option<i64>, kind: FormKind) -> Result<FormTemplate, String> {
    forms::reset_template(&db.pool().await, style_id, kind).await
}

/// Lays out a form with an unsaved template: a real document, or the sample without one. For a
/// statement, `document_id` is the customer and `period` the statement period.
#[tauri::command]
pub async fn preview_form(db: State<'_, Db>, kind: FormKind, template: FormTemplate, document_id: Option<i64>, period: Option<Period>) -> Result<RenderedForm, String> {
    Ok(forms::render(&db.pool().await, kind, document_id, period, Some(template), false).await?.form)
}

/// Lays out a document with its customer's form style, for printing.
#[tauri::command]
pub async fn render_form(db: State<'_, Db>, kind: FormKind, document_id: i64, period: Option<Period>) -> Result<RenderedForm, String> {
    Ok(forms::render(&db.pool().await, kind, Some(document_id), period, None, false).await?.form)
}

/// Prompts with the native Save dialog and writes the document's PDF there. Returns the saved
/// path, or `None` if the user cancelled.
#[tauri::command]
pub async fn save_form_pdf(app: AppHandle, db: State<'_, Db>, kind: FormKind, document_id: i64, period: Option<Period>) -> Result<Option<String>, String> {
    let (file_name, bytes) = forms::pdf_file_for(&db.pool().await, kind, document_id, period).await?;
    let dialog_app = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || dialog_app.dialog().file().set_file_name(&file_name).add_filter("PDF", &["pdf"]).blocking_save_file())
        .await
        .map_err(|e| format!("EXPORT_DIALOG: {e}"))?;
    let Some(target) = chosen else { return Ok(None) };
    let path = target.into_path().map_err(|e| format!("EXPORT_BAD_PATH: {e}"))?;
    export_engine::write_atomically(&path, &bytes)?;
    Ok(Some(path.display().to_string()))
}
