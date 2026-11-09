//! Vouchers (bills, checks, expenses, credits, deposits, journal entries) as PDF: saved, or emailed
//! to the vendor or customer named on them with the same mail app and SMTP paths as forms.

use super::email::{smtp_secrets, EmailInput};
use super::prelude::*;
use crate::vouchers::{self, VoucherDoc};

handlers![
    save_voucher_pdf,
    get_voucher_email_draft,
    email_voucher_with_mail_app,
    email_voucher_with_smtp,
];

/// Asks where to save the voucher's PDF and writes it; `None` when the dialog is cancelled.
#[tauri::command]
pub async fn save_voucher_pdf(app: AppHandle, db: State<'_, Db>, doc: VoucherDoc) -> Result<Option<String>, String> {
    let (file_name, bytes) = vouchers::pdf_file(&db.pool().await, &doc).await?;
    let dialog_app = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || dialog_app.dialog().file().set_file_name(&file_name).add_filter("PDF", &["pdf"]).blocking_save_file())
        .await
        .map_err(|e| format!("EXPORT_DIALOG: {e}"))?;
    let Some(target) = chosen else { return Ok(None) };
    let path = target.into_path().map_err(|e| format!("EXPORT_BAD_PATH: {e}"))?;
    export_engine::write_atomically(&path, &bytes)?;
    Ok(Some(path.display().to_string()))
}

/// The email for a voucher: the named vendor's or customer's address, a subject and a message.
#[tauri::command]
pub async fn get_voucher_email_draft(db: State<'_, Db>, doc: VoucherDoc) -> Result<EmailDraft, String> {
    vouchers::email_draft(&db.pool().await, &doc).await
}

/// Opens a new message in the user's mail app with the voucher's PDF attached.
#[tauri::command]
pub async fn email_voucher_with_mail_app(app: AppHandle, db: State<'_, Db>, doc: VoucherDoc, email: EmailInput) -> Result<ComposeOutcome, String> {
    let (to, subject, body) = email.checked()?;
    let (file_name, bytes) = vouchers::pdf_file(&db.pool().await, &doc).await?;
    let outgoing = app.path().app_cache_dir().map_err(|e| format!("MAIL_STAGE: {e}"))?.join("outgoing");
    let message = mail_app::Compose { to, subject, body, attachment: Default::default() };
    tauri::async_runtime::spawn_blocking(move || {
        let attachment = mail_app::stage_attachment(&outgoing, &file_name, &bytes)?;
        mail_app::compose(&mail_app::Compose { attachment, ..message })
    })
    .await
    .map_err(|e| format!("MAIL_OPEN: {e}"))?
}

/// Sends the voucher's PDF from LocalBooks (Preferences › Send Forms' email server).
#[tauri::command]
pub async fn email_voucher_with_smtp(app: AppHandle, db: State<'_, Db>, doc: VoucherDoc, email: EmailInput) -> Result<(), String> {
    let (to, subject, body) = email.checked()?;
    let pool = db.pool().await;
    let (file_name, pdf) = vouchers::pdf_file(&pool, &doc).await?;
    smtp::send(&pool, &smtp_secrets(&app)?, smtp::Outgoing { to, subject, body, file_name, pdf, more: Vec::new() }).await
}
