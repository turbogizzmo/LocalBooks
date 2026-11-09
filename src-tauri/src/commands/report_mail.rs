//! Reports, lists and registers as PDF (`report_pdf.rs`): emailed through the company's email
//! server or the user's mail app, or saved; and Scheduled Reports (`report_schedules.rs`).

use super::email::{smtp_secrets, EmailInput};
use super::prelude::*;
use crate::report_schedules::{self, Destination, Schedule};
use crate::xlsx_export::Sheet;

handlers![
    email_report_with_smtp,
    email_report_with_mail_app,
    save_report_pdf,
    report_sheet,
    list_report_schedules,
    save_report_schedule,
    delete_report_schedule,
    run_report_schedule_now,
];

async fn pdf_of(db: &Db, sheet: &Sheet) -> Result<(sqlx::SqlitePool, String, Vec<u8>), String> {
    let pool = db.pool().await;
    let symbol = crate::preferences::currency_symbol(&pool).await?;
    Ok((pool, crate::report_pdf::file_name(sheet), crate::report_pdf::to_pdf(sheet, &symbol)))
}

/// Sends the report as a PDF through the email server set up in Settings › Email.
#[tauri::command]
pub async fn email_report_with_smtp(app: AppHandle, db: State<'_, Db>, sheet: Sheet, email: EmailInput) -> Result<(), String> {
    let (to, subject, body) = email.checked()?;
    let (pool, file_name, pdf) = pdf_of(&db, &sheet).await?;
    smtp::send(&pool, &smtp_secrets(&app)?, smtp::Outgoing { to, subject, body, file_name, pdf, more: Vec::new() }).await
}

/// Opens a new message in the user's mail app with the report's PDF attached.
#[tauri::command]
pub async fn email_report_with_mail_app(app: AppHandle, db: State<'_, Db>, sheet: Sheet, email: EmailInput) -> Result<ComposeOutcome, String> {
    let (to, subject, body) = email.checked()?;
    let (_, file_name, bytes) = pdf_of(&db, &sheet).await?;
    let outgoing = app.path().app_cache_dir().map_err(|e| format!("MAIL_STAGE: {e}"))?.join("outgoing");
    let message = mail_app::Compose { to, subject, body, attachment: Default::default() };
    tauri::async_runtime::spawn_blocking(move || {
        let attachment = mail_app::stage_attachment(&outgoing, &file_name, &bytes)?;
        mail_app::compose(&mail_app::Compose { attachment, ..message })
    })
    .await
    .map_err(|e| format!("MAIL_OPEN: {e}"))?
}

/// Saves the report as a PDF where the user chooses. Returns the path, or None if cancelled.
#[tauri::command]
pub async fn save_report_pdf(app: AppHandle, db: State<'_, Db>, sheet: Sheet) -> Result<Option<String>, String> {
    let (_, file_name, bytes) = pdf_of(&db, &sheet).await?;
    let Some(path) = super::report_export::choose_export_path(app, file_name).await? else { return Ok(None) };
    export_engine::write_atomically(&path, &bytes)?;
    Ok(Some(path.display().to_string()))
}

/// A financial statement (`profit_and_loss`, `profit_and_loss_by_class`, `balance_sheet`) as a
/// sheet, for emailing it as a PDF.
#[tauri::command]
pub async fn report_sheet(
    db: State<'_, Db>,
    report: String,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    as_of: Option<NaiveDate>,
    class_filter: Option<crate::statements::ClassFilter>,
) -> Result<Sheet, String> {
    crate::xlsx_export::report_sheet(&db.pool().await, &report, start_date, end_date, as_of, class_filter.as_ref()).await
}

#[tauri::command]
pub async fn list_report_schedules(db: State<'_, Db>) -> Result<Vec<Schedule>, String> {
    report_schedules::list(&db.pool().await).await
}

#[tauri::command]
pub async fn save_report_schedule(app: AppHandle, db: State<'_, Db>, id: Option<i64>, schedule: Schedule) -> Result<Schedule, String> {
    let saved = report_schedules::save(&db.pool().await, id, &schedule, Local::now().date_naive()).await?;
    notify(&app);
    Ok(saved)
}

#[tauri::command]
pub async fn delete_report_schedule(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    report_schedules::delete(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

/// Runs a schedule's reports for today now, without changing when it next runs.
#[tauri::command]
pub async fn run_report_schedule_now(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<String, String> {
    let pool = db.pool().await;
    let schedule = report_schedules::get(&pool, id).await?;
    let today = Local::now().date_naive();
    let result = deliver(&app, &pool, &schedule, today).await;
    let text = result.clone().unwrap_or_else(|e| e);
    report_schedules::record_run(&pool, id, schedule.next_run, &text).await?;
    notify(&app);
    result
}

/// Makes and sends (or saves) one run's files; returns what happened, in words.
async fn deliver(app: &AppHandle, pool: &sqlx::SqlitePool, schedule: &Schedule, run: NaiveDate) -> Result<String, String> {
    let files = report_schedules::files_for(pool, schedule, run).await?;
    let count = files.len();
    match schedule.destination {
        Destination::Folder => {
            let folder = schedule.folder.clone();
            tauri::async_runtime::spawn_blocking(move || report_schedules::save_to_folder(&folder, &files)).await.map_err(|e| format!("SCHEDULE_FOLDER: {e}"))??;
            Ok(format!("Saved {count} report{} to {}", if count == 1 { "" } else { "s" }, schedule.folder.trim()))
        }
        Destination::Email => {
            let to = mail_app::parse_recipients(&schedule.recipients)?;
            let company = crate::preferences::get(pool, "company_name").await.unwrap_or_default();
            let subject = format!("{} from {}", schedule.name.trim(), company.trim()).trim_end_matches(" from ").to_string();
            let body = format!("The reports for {} are attached.\n\nSent by LocalBooks: {}, {}.", run.format("%B %-d, %Y"), schedule.name.trim(), schedule_words(schedule));
            smtp::send(pool, &smtp_secrets(app)?, smtp::Outgoing { to: to.clone(), subject, body, file_name: String::new(), pdf: Vec::new(), more: files }).await?;
            Ok(format!("Emailed {count} report{} to {}", if count == 1 { "" } else { "s" }, to.join(", ")))
        }
    }
}

fn schedule_words(s: &Schedule) -> &'static str {
    match s.frequency {
        report_schedules::Frequency::Daily => "every day",
        report_schedules::Frequency::Weekly => "every week",
        report_schedules::Frequency::Monthly => "every month",
        report_schedules::Frequency::Quarterly => "every quarter",
    }
}

/// Runs every schedule that is due (missed runs catch up once). Called at launch and every few
/// minutes while LocalBooks is open; without an open company it does nothing.
pub async fn run_due_schedules(app: &AppHandle) {
    let Some(db) = app.try_state::<Db>() else { return };
    let pool = db.pool().await;
    let today = Local::now().date_naive();
    let due = match report_schedules::due(&pool, today).await {
        Ok(due) => due,
        // No company open, or an older company file without schedules.
        Err(_) => return,
    };
    for (schedule, run, next) in due {
        let text = deliver(app, &pool, &schedule, run).await.unwrap_or_else(|e| format!("Not sent: {e}"));
        if let Err(err) = report_schedules::record_run(&pool, schedule.id, next, &text).await {
            eprintln!("scheduled report {}: {err}", schedule.id);
        }
    }
    notify(app);
}
