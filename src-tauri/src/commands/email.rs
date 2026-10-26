//! Emailing forms (Mail app and SMTP) and the email log.

use super::prelude::*;

handlers![
    get_email_draft,
    email_form_with_mail_app,
    get_email_history,
    list_email_statuses,
    set_email_later,
    list_email_queue,
    email_form_with_smtp,
    send_smtp_test_email,
    get_smtp_settings,
    save_smtp_settings,
    test_smtp_settings,
];

/// The email for a document: the customer's address and the form's subject and message.
#[tauri::command]
pub async fn get_email_draft(db: State<'_, Db>, kind: FormKind, document_id: i64, period: Option<Period>) -> Result<EmailDraft, String> {
    forms::email_draft(&db.pool().await, kind, document_id, period).await
}

/// The message the user confirmed in the email dialog.
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailInput {
    pub to: String,
    pub subject: String,
    pub body: String,
}

impl EmailInput {
    fn checked(self) -> Result<(Vec<String>, String, String), String> {
        let to = mail_app::parse_recipients(&self.to)?;
        let subject = self.subject.split_whitespace().collect::<Vec<_>>().join(" ");
        if subject.chars().count() > 200 {
            return Err("MAIL_SUBJECT_TOO_LONG: keep the subject under 200 characters".into());
        }
        if self.body.chars().count() > 20_000 {
            return Err("MAIL_BODY_TOO_LONG: keep the message under 20,000 characters".into());
        }
        Ok((to, subject, self.body))
    }
}

/// Opens a new message in the user's mail app with the document's PDF attached.
#[tauri::command]
pub async fn email_form_with_mail_app(
    app: AppHandle,
    db: State<'_, Db>,
    kind: FormKind,
    document_id: i64,
    period: Option<Period>,
    email: EmailInput,
) -> Result<ComposeOutcome, String> {
    let (to, subject, body) = email.checked()?;
    let pool = db.pool().await;
    let (file_name, bytes) = forms::pdf_file_for(&pool, kind, document_id, period).await?;
    let outgoing = app.path().app_cache_dir().map_err(|e| format!("MAIL_STAGE: {e}"))?.join("outgoing");
    let message = mail_app::Compose { to: to.clone(), subject: subject.clone(), body, attachment: Default::default() };
    let outcome = tauri::async_runtime::spawn_blocking(move || {
        let attachment = mail_app::stage_attachment(&outgoing, &file_name, &bytes)?;
        mail_app::compose(&mail_app::Compose { attachment, ..message })
    })
    .await
    .map_err(|e| format!("MAIL_OPEN: {e}"))??;
    record_sent(&app, &pool, kind, document_id, &to, email_log::Method::MailApp, &subject).await;
    Ok(outcome)
}

/// Adds the send to the form's history. The message has already gone out (or is open in the mail
/// app), so a failure here is logged rather than reported as a failed send.
async fn record_sent(app: &AppHandle, pool: &sqlx::SqlitePool, kind: FormKind, document_id: i64, to: &[String], method: email_log::Method, subject: &str) {
    match email_log::record(pool, kind, document_id, to, method, subject).await {
        Ok(()) => notify(app),
        Err(err) => eprintln!("could not record the emailed form in its history: {err}"),
    }
}

#[tauri::command]
pub async fn get_email_history(db: State<'_, Db>, kind: FormKind, document_id: i64) -> Result<Vec<SentRecord>, String> {
    email_log::history(&db.pool().await, kind, document_id).await
}

/// The forms of a type that were emailed or are marked Email later.
#[tauri::command]
pub async fn list_email_statuses(db: State<'_, Db>, kind: FormKind) -> Result<Vec<EmailStatus>, String> {
    email_log::statuses(&db.pool().await, kind).await
}

#[tauri::command]
pub async fn set_email_later(app: AppHandle, db: State<'_, Db>, kind: FormKind, document_id: i64, queued: bool) -> Result<(), String> {
    email_log::set_queued(&db.pool().await, kind, document_id, queued).await?;
    notify(&app);
    Ok(())
}

/// The "Email later" queue, for Send Forms.
#[tauri::command]
pub async fn list_email_queue(db: State<'_, Db>) -> Result<Vec<QueuedForm>, String> {
    email_log::queue(&db.pool().await).await
}

/// Sends the document's PDF to the customer through the company's email server.
#[tauri::command]
pub async fn email_form_with_smtp(app: AppHandle, db: State<'_, Db>, kind: FormKind, document_id: i64, period: Option<Period>, email: EmailInput) -> Result<(), String> {
    let (to, subject, body) = email.checked()?;
    let pool = db.pool().await;
    let (file_name, pdf) = forms::pdf_file_for(&pool, kind, document_id, period).await?;
    smtp::send(&pool, &smtp_secrets(&app)?, smtp::Outgoing { to: to.clone(), subject: subject.clone(), body, file_name, pdf }).await?;
    record_sent(&app, &pool, kind, document_id, &to, email_log::Method::Smtp, &subject).await;
    Ok(())
}

/// Sends a short test message to the from address with the settings as entered.
#[tauri::command]
pub async fn send_smtp_test_email(app: AppHandle, db: State<'_, Db>, settings: SmtpSettings, password: Option<String>) -> Result<String, String> {
    let company = forms::load_company(&db.pool().await).await?;
    smtp::send_test(settings, &smtp_secrets(&app)?, password, &company.name).await
}

fn smtp_secrets(app: &AppHandle) -> Result<SecretStore, String> {
    Ok(smtp::secret_store(&app.path().app_config_dir().map_err(|e| format!("SMTP_CONFIG: {e}"))?))
}

#[tauri::command]
pub async fn get_smtp_settings(app: AppHandle, db: State<'_, Db>) -> Result<SmtpStatus, String> {
    smtp::status(&db.pool().await, &smtp_secrets(&app)?).await
}

/// Saves the email server settings; `password` replaces the saved one (empty removes it).
#[tauri::command]
pub async fn save_smtp_settings(app: AppHandle, db: State<'_, Db>, settings: SmtpSettings, password: Option<String>) -> Result<SmtpStatus, String> {
    smtp::save_settings(&db.pool().await, &smtp_secrets(&app)?, settings, password).await
}

/// Connects and signs in with the settings as entered, without saving or sending anything.
#[tauri::command]
pub async fn test_smtp_settings(app: AppHandle, settings: SmtpSettings, password: Option<String>) -> Result<(), String> {
    smtp::test_connection(settings, &smtp_secrets(&app)?, password).await
}
