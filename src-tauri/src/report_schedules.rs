//! Scheduled reports (Reports › Scheduled Reports): chosen reports run on a schedule and are
//! emailed through the company's email server, or saved to a folder, as PDF or Excel.
//!
//! Schedules run only while LocalBooks is open (the app checks every few minutes). A run that fell
//! due while it was closed happens once, as soon as it opens, for the latest date that was due;
//! earlier missed dates are skipped rather than sent in a burst. Nothing is sent anywhere a person
//! did not set up: a schedule emails only through the email server set up in Settings › Email, to
//! the addresses typed on the schedule.

use chrono::{Datelike, Duration, Months, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::ledger::db_err;
use crate::secrets::SecretStore;
use crate::xlsx_export::{Column, ColumnKind, Row, RowKind, Sheet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Frequency {
    Daily,
    Weekly,
    Monthly,
    Quarterly,
}

impl Frequency {
    fn key(self) -> &'static str {
        match self {
            Frequency::Daily => "DAILY",
            Frequency::Weekly => "WEEKLY",
            Frequency::Monthly => "MONTHLY",
            Frequency::Quarterly => "QUARTERLY",
        }
    }

    fn parse(key: &str) -> Frequency {
        match key {
            "DAILY" => Frequency::Daily,
            "WEEKLY" => Frequency::Weekly,
            "QUARTERLY" => Frequency::Quarterly,
            _ => Frequency::Monthly,
        }
    }

    /// The run after `date`.
    pub fn after(self, date: NaiveDate) -> NaiveDate {
        match self {
            Frequency::Daily => date + Duration::days(1),
            Frequency::Weekly => date + Duration::days(7),
            Frequency::Monthly => date.checked_add_months(Months::new(1)).unwrap_or(date + Duration::days(30)),
            Frequency::Quarterly => date.checked_add_months(Months::new(3)).unwrap_or(date + Duration::days(91)),
        }
    }
}

/// The dates a report covers, worked out from the day it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Period {
    /// The day before the run (daily reports).
    Yesterday,
    LastWeek,
    LastMonth,
    MonthToDate,
    LastQuarter,
    QuarterToDate,
    LastYear,
    YearToDate,
}

fn first_of_month(d: NaiveDate) -> NaiveDate {
    d.with_day(1).unwrap_or(d)
}

fn first_of_quarter(d: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year(), (d.month0() / 3) * 3 + 1, 1).unwrap_or(d)
}

impl Period {
    /// (first day, last day) for a run on `run`. Calendar periods; "to date" periods end on the run day.
    pub fn range(self, run: NaiveDate) -> (NaiveDate, NaiveDate) {
        match self {
            Period::Yesterday => {
                let y = run - Duration::days(1);
                (y, y)
            }
            Period::LastWeek => {
                // Monday to Sunday of the week before the run's.
                let monday = run - Duration::days(i64::from(run.weekday().num_days_from_monday()));
                (monday - Duration::days(7), monday - Duration::days(1))
            }
            Period::LastMonth => {
                let end = first_of_month(run) - Duration::days(1);
                (first_of_month(end), end)
            }
            Period::MonthToDate => (first_of_month(run), run),
            Period::LastQuarter => {
                let end = first_of_quarter(run) - Duration::days(1);
                (first_of_quarter(end), end)
            }
            Period::QuarterToDate => (first_of_quarter(run), run),
            Period::LastYear => (NaiveDate::from_ymd_opt(run.year() - 1, 1, 1).unwrap_or(run), NaiveDate::from_ymd_opt(run.year() - 1, 12, 31).unwrap_or(run)),
            Period::YearToDate => (NaiveDate::from_ymd_opt(run.year(), 1, 1).unwrap_or(run), run),
        }
    }
}

/// One report on a schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ScheduledReport {
    /// `profit_and_loss`, `profit_and_loss_by_class` or `balance_sheet` (as of the period's end).
    Statement { report: String, period: Period },
    /// A report builder preset (by its id).
    Preset { id: String, period: Period },
    /// A memorized report.
    Saved { id: i64, period: Period },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Destination {
    Email,
    Folder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FileFormat {
    Pdf,
    Xlsx,
}

/// A schedule as the Scheduled Reports center shows and saves it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    pub reports: Vec<ScheduledReport>,
    pub frequency: Frequency,
    pub start_date: NaiveDate,
    #[serde(default)]
    pub end_date: Option<NaiveDate>,
    pub destination: Destination,
    /// Email: addresses separated by commas.
    #[serde(default)]
    pub recipients: String,
    /// Folder: where the files are saved.
    #[serde(default)]
    pub folder: String,
    pub format: FileFormat,
    #[serde(default = "yes")]
    pub is_active: bool,
    /// Set by LocalBooks: the next date it runs (None once it has ended).
    #[serde(default)]
    pub next_run: Option<NaiveDate>,
    #[serde(default)]
    pub last_run_at: Option<String>,
    #[serde(default)]
    pub last_result: Option<String>,
    /// The PDFs open only with a password, kept in this computer's credential store.
    #[serde(default)]
    pub protect_pdf: bool,
    /// When saving: a new password for the PDFs (never sent back).
    #[serde(default, skip_serializing)]
    pub pdf_password: Option<String>,
    /// The name the password is stored under (`pdf_password_key`).
    #[serde(skip)]
    pub pdf_password_key: Option<String>,
}

fn yes() -> bool {
    true
}

/// The latest scheduled date on or before `today` (and the end date), and the run after it; None
/// when nothing is due.
pub fn due_run(frequency: Frequency, next_run: NaiveDate, end_date: Option<NaiveDate>, today: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    let limit = end_date.map_or(today, |end| end.min(today));
    if next_run > limit {
        return None;
    }
    let mut run = next_run;
    let mut next = frequency.after(run);
    // Missed dates while LocalBooks was closed: only the latest one runs.
    while next <= limit {
        run = next;
        next = frequency.after(run);
    }
    Some((run, next))
}

fn check(s: &Schedule) -> Result<(), String> {
    if s.name.trim().is_empty() || s.name.trim().chars().count() > 80 {
        return Err("SCHEDULE_NAME: give the schedule a name of up to 80 characters".into());
    }
    if s.reports.is_empty() || s.reports.len() > 20 {
        return Err("SCHEDULE_REPORTS: choose from 1 to 20 reports".into());
    }
    if s.end_date.is_some_and(|end| end < s.start_date) {
        return Err("SCHEDULE_DATES: the end date is before the start date".into());
    }
    for r in &s.reports {
        if let ScheduledReport::Statement { report, .. } = r {
            if !["profit_and_loss", "profit_and_loss_by_class", "balance_sheet"].contains(&report.as_str()) {
                return Err(format!("SCHEDULE_REPORT: '{report}' can't be scheduled"));
            }
        }
    }
    match s.destination {
        Destination::Email => {
            crate::mail_app::parse_recipients(&s.recipients)?;
        }
        Destination::Folder => {
            if s.folder.trim().is_empty() || !std::path::Path::new(s.folder.trim()).is_absolute() {
                return Err("SCHEDULE_FOLDER: choose the folder the reports are saved to".into());
            }
        }
    }
    Ok(())
}

type ScheduleRow = (
    i64,
    String,
    String,
    String,
    NaiveDate,
    Option<NaiveDate>,
    Option<NaiveDate>,
    String,
    Option<String>,
    Option<String>,
    String,
    bool,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn from_row(r: ScheduleRow) -> Schedule {
    let (
        id,
        name,
        reports_json,
        frequency,
        start_date,
        end_date,
        next_run,
        destination,
        recipients,
        folder,
        format,
        is_active,
        last_run_at,
        last_result,
        pdf_password_key,
    ) = r;
    Schedule {
        id,
        name,
        // Reports a newer version wrote that this one can't read leave the list empty.
        reports: serde_json::from_str(&reports_json).unwrap_or_default(),
        frequency: Frequency::parse(&frequency),
        start_date,
        end_date,
        destination: if destination == "FOLDER" { Destination::Folder } else { Destination::Email },
        recipients: recipients.unwrap_or_default(),
        folder: folder.unwrap_or_default(),
        format: if format == "XLSX" { FileFormat::Xlsx } else { FileFormat::Pdf },
        is_active,
        next_run,
        last_run_at,
        last_result,
        protect_pdf: pdf_password_key.is_some(),
        pdf_password: None,
        pdf_password_key,
    }
}

const SELECT: &str =
    "SELECT id, name, reports_json, frequency, start_date, end_date, next_run, destination, recipients, folder, format, is_active, last_run_at, last_result, pdf_password_key FROM report_schedules";

pub async fn list(pool: &SqlitePool) -> Result<Vec<Schedule>, String> {
    let rows: Vec<ScheduleRow> = sqlx::query_as(&format!("{SELECT} ORDER BY name COLLATE NOCASE, id")).fetch_all(pool).await.map_err(db_err)?;
    Ok(rows.into_iter().map(from_row).collect())
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Schedule, String> {
    let row: Option<ScheduleRow> = sqlx::query_as(&format!("{SELECT} WHERE id = ?1")).bind(id).fetch_optional(pool).await.map_err(db_err)?;
    row.map(from_row).ok_or_else(|| "SCHEDULE_NOT_FOUND: that schedule no longer exists".into())
}

/// A new random name to store a schedule's PDF password under.
fn new_password_key() -> Result<String, String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).map_err(|e| format!("SCHEDULE_RANDOM: {e}"))?;
    Ok(format!("report-schedule-{}", bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()))
}

/// Adds or changes a schedule; its next run starts again from the start date (or today). A PDF
/// password goes to `passwords` (the computer's credential store), never into the company file.
pub async fn save(pool: &SqlitePool, id: Option<i64>, s: &Schedule, today: NaiveDate, passwords: &SecretStore) -> Result<Schedule, String> {
    check(s)?;
    let existing = match id {
        Some(id) => get(pool, id).await?.pdf_password_key,
        None => None,
    };
    let new_password = s.pdf_password.as_deref().filter(|p| !p.is_empty());
    let key = if s.protect_pdf {
        if s.format != FileFormat::Pdf {
            return Err("SCHEDULE_PDF_PASSWORD: only PDF files can have a password".into());
        }
        let key = match &existing {
            Some(key) => key.clone(),
            None => new_password_key()?,
        };
        match new_password {
            Some(password) => {
                crate::pdf::Protection::check_password(password)?;
                passwords.put(&key, password).await?;
            }
            None if passwords.get(&key).await?.is_none() => {
                return Err("SCHEDULE_PDF_PASSWORD: enter the password the PDFs will open with".into());
            }
            None => {}
        }
        Some(key)
    } else {
        None
    };
    let reports = serde_json::to_string(&s.reports).map_err(|e| format!("SCHEDULE_SAVE: {e}"))?;
    // The first run: the start date, or the first scheduled date from today on.
    let mut next = s.start_date;
    while next < today {
        next = s.frequency.after(next);
    }
    let next = s.end_date.is_none_or(|end| next <= end).then_some(next);
    let destination = match s.destination {
        Destination::Email => "EMAIL",
        Destination::Folder => "FOLDER",
    };
    let format = match s.format {
        FileFormat::Pdf => "PDF",
        FileFormat::Xlsx => "XLSX",
    };
    let recipients = Some(s.recipients.trim()).filter(|r| !r.is_empty() && s.destination == Destination::Email);
    let folder = Some(s.folder.trim()).filter(|f| !f.is_empty() && s.destination == Destination::Folder);
    let id = match id {
        Some(id) => {
            let done = sqlx::query(
                "UPDATE report_schedules SET name = ?1, reports_json = ?2, frequency = ?3, start_date = ?4, end_date = ?5, next_run = ?6, destination = ?7,
                        recipients = ?8, folder = ?9, format = ?10, is_active = ?11, pdf_password_key = ?13 WHERE id = ?12",
            )
            .bind(s.name.trim())
            .bind(&reports)
            .bind(s.frequency.key())
            .bind(s.start_date)
            .bind(s.end_date)
            .bind(next)
            .bind(destination)
            .bind(recipients)
            .bind(folder)
            .bind(format)
            .bind(s.is_active)
            .bind(id)
            .bind(&key)
            .execute(pool)
            .await
            .map_err(db_err)?;
            if done.rows_affected() == 0 {
                return Err("SCHEDULE_NOT_FOUND: that schedule no longer exists".into());
            }
            id
        }
        None => sqlx::query_scalar(
            "INSERT INTO report_schedules (name, reports_json, frequency, start_date, end_date, next_run, destination, recipients, folder, format, is_active, pdf_password_key)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) RETURNING id",
        )
        .bind(s.name.trim())
        .bind(&reports)
        .bind(s.frequency.key())
        .bind(s.start_date)
        .bind(s.end_date)
        .bind(next)
        .bind(destination)
        .bind(recipients)
        .bind(folder)
        .bind(format)
        .bind(s.is_active)
        .bind(&key)
        .fetch_one(pool)
        .await
        .map_err(db_err)?,
    };
    // A password no longer used leaves the credential store.
    if let Some(old) = existing.filter(|old| key.as_ref() != Some(old)) {
        passwords.delete(&old).await?;
    }
    get(pool, id).await
}

pub async fn delete(pool: &SqlitePool, id: i64, passwords: &SecretStore) -> Result<(), String> {
    let key = get(pool, id).await?.pdf_password_key;
    let done = sqlx::query("DELETE FROM report_schedules WHERE id = ?1").bind(id).execute(pool).await.map_err(db_err)?;
    if done.rows_affected() == 0 {
        return Err("SCHEDULE_NOT_FOUND: that schedule no longer exists".into());
    }
    if let Some(key) = key {
        passwords.delete(&key).await?;
    }
    Ok(())
}

/// A report builder report as a sheet.
fn custom_sheet(company: &str, title: &str, report: &crate::custom_reports::CustomReport, period: &str) -> Sheet {
    use crate::custom_reports::{ColumnKind as K, RowKind as R};
    let columns = report
        .columns
        .iter()
        .map(|c| Column {
            label: c.label.clone(),
            kind: match c.kind {
                K::Text => ColumnKind::Text,
                K::Date => ColumnKind::Date,
                K::Money => ColumnKind::Money,
                K::Quantity | K::Number => ColumnKind::Number,
            },
        })
        .collect();
    let rows = report
        .rows
        .iter()
        .map(|r| Row {
            kind: match r.kind {
                R::Section => RowKind::Section,
                R::Detail => RowKind::Detail,
                R::Subtotal => RowKind::Subtotal,
                R::Total => RowKind::Total,
            },
            depth: u8::try_from(r.depth.min(15)).unwrap_or(15),
            cells: r
                .cells
                .iter()
                .zip(&report.columns)
                .map(|(v, c)| match (c.kind, v.as_i64()) {
                    // Quantities are scaled by 10,000; they print as decimals.
                    (K::Quantity, Some(e4)) => json!(crate::forms::format_quantity(e4)),
                    _ => v.clone(),
                })
                .collect(),
        })
        .collect();
    Sheet {
        company: company.to_string(),
        title: title.to_string(),
        subtitle: period.to_string(),
        notes: Vec::new(),
        columns,
        rows,
        indent_column: 0,
        footer: None,
    }
}

fn long_date(d: NaiveDate) -> String {
    d.format("%B %-d, %Y").to_string()
}

/// One scheduled report as a sheet, for a run on `run`.
pub async fn sheet_for(pool: &SqlitePool, report: &ScheduledReport, run: NaiveDate) -> Result<Sheet, String> {
    match report {
        ScheduledReport::Statement { report, period } => {
            let (start, end) = period.range(run);
            crate::xlsx_export::report_sheet(pool, report, Some(start), Some(end), Some(end), None).await
        }
        ScheduledReport::Preset { id, period } => {
            let preset = crate::custom_reports::presets()
                .into_iter()
                .find(|p| p.id == id)
                .ok_or_else(|| format!("SCHEDULE_REPORT: the report '{id}' no longer exists"))?;
            let as_of = matches!(preset.date_mode, crate::custom_reports::DateMode::AsOf);
            run_custom(pool, preset.title, preset.settings, *period, as_of, run).await
        }
        ScheduledReport::Saved { id, period } => {
            let saved = crate::custom_reports::list_saved(pool)
                .await?
                .into_iter()
                .find(|s| s.id == *id)
                .ok_or_else(|| "SCHEDULE_REPORT: a memorized report on this schedule was deleted".to_string())?;
            let settings: crate::custom_reports::ReportSettings =
                serde_json::from_value(saved.settings.clone()).map_err(|e| format!("SCHEDULE_REPORT: {e}"))?;
            let as_of = saved.settings.get("dateMode").and_then(Value::as_str) == Some("asOf");
            run_custom(pool, &saved.name, settings, *period, as_of, run).await
        }
    }
}

async fn run_custom(
    pool: &SqlitePool,
    title: &str,
    mut settings: crate::custom_reports::ReportSettings,
    period: Period,
    as_of: bool,
    run: NaiveDate,
) -> Result<Sheet, String> {
    let (start, end) = period.range(run);
    settings.start_date = (!as_of).then_some(start);
    settings.end_date = Some(end);
    let report = crate::custom_reports::run(pool, &settings, run).await?;
    let company = crate::preferences::get(pool, "company_name").await?;
    let label = if as_of { format!("As of {}", long_date(end)) } else { format!("{} - {}", long_date(start), long_date(end)) };
    Ok(custom_sheet(&company, title, &report, &label))
}

/// A finished report file: (file name, bytes, media type).
pub type ReportFile = (String, Vec<u8>, &'static str);

const XLSX_TYPE: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

/// The schedule's reports for a run on `run`, as files.
pub async fn files_for(pool: &SqlitePool, schedule: &Schedule, run: NaiveDate, passwords: &SecretStore) -> Result<Vec<ReportFile>, String> {
    let symbol = crate::preferences::currency_symbol(pool).await?;
    let password = match (&schedule.pdf_password_key, schedule.format) {
        (Some(key), FileFormat::Pdf) => Some(
            passwords
                .get(key)
                .await?
                .ok_or("SCHEDULE_PDF_PASSWORD: this schedule's PDF password isn't on this computer. Open the schedule and enter it again")?,
        ),
        _ => None,
    };
    let mut out = Vec::with_capacity(schedule.reports.len());
    for report in &schedule.reports {
        let sheet = sheet_for(pool, report, run).await?;
        let stem = crate::report_pdf::file_name(&sheet);
        let stem = stem.trim_end_matches(".pdf");
        out.push(match schedule.format {
            FileFormat::Pdf => (format!("{stem} {run}.pdf"), crate::report_pdf::to_protected_pdf(&sheet, &symbol, password.as_deref())?, "application/pdf"),
            FileFormat::Xlsx => (format!("{stem} {run}.xlsx"), crate::xlsx_export::to_xlsx(&sheet, &symbol)?, XLSX_TYPE),
        });
    }
    Ok(out)
}

/// Saves the files in the schedule's folder (which must exist).
pub fn save_to_folder(folder: &str, files: &[ReportFile]) -> Result<(), String> {
    let dir = std::path::Path::new(folder.trim());
    if !dir.is_dir() {
        return Err(format!("SCHEDULE_FOLDER: the folder {} is not there", dir.display()));
    }
    for (name, bytes, _) in files {
        crate::export_engine::write_atomically(&dir.join(name), bytes)?;
    }
    Ok(())
}

/// Records how a run went and moves the schedule to its next date (None after the end date).
pub async fn record_run(pool: &SqlitePool, id: i64, next: Option<NaiveDate>, result: &str) -> Result<(), String> {
    sqlx::query("UPDATE report_schedules SET next_run = ?1, last_run_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'), last_result = ?2 WHERE id = ?3")
        .bind(next)
        .bind(result.chars().take(500).collect::<String>())
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    Ok(())
}

/// The active schedules due on `today`, with the date each runs for and its next date.
pub async fn due(pool: &SqlitePool, today: NaiveDate) -> Result<Vec<(Schedule, NaiveDate, Option<NaiveDate>)>, String> {
    Ok(list(pool)
        .await?
        .into_iter()
        .filter(|s| s.is_active)
        .filter_map(|s| {
            let (run, next) = due_run(s.frequency, s.next_run?, s.end_date, today)?;
            let next = s.end_date.is_none_or(|end| next <= end).then_some(next);
            Some((s, run, next))
        })
        .collect())
}

#[cfg(test)]
mod tests;
