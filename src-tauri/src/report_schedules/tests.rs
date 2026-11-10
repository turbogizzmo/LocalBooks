use super::*;
use crate::db::test_pool;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

#[test]
fn periods_are_worked_out_from_the_run_date() {
    let run = d(2026, 5, 6); // a Wednesday
    assert_eq!(Period::Yesterday.range(run), (d(2026, 5, 5), d(2026, 5, 5)));
    assert_eq!(Period::LastWeek.range(run), (d(2026, 4, 27), d(2026, 5, 3)));
    assert_eq!(Period::LastMonth.range(run), (d(2026, 4, 1), d(2026, 4, 30)));
    assert_eq!(Period::MonthToDate.range(run), (d(2026, 5, 1), run));
    assert_eq!(Period::LastQuarter.range(run), (d(2026, 1, 1), d(2026, 3, 31)));
    assert_eq!(Period::QuarterToDate.range(run), (d(2026, 4, 1), run));
    assert_eq!(Period::LastYear.range(run), (d(2025, 1, 1), d(2025, 12, 31)));
    assert_eq!(Period::YearToDate.range(run), (d(2026, 1, 1), run));
    assert_eq!(Period::LastMonth.range(d(2026, 1, 15)), (d(2025, 12, 1), d(2025, 12, 31)));
}

#[test]
fn schedules_step_daily_to_quarterly_and_catch_up_once() {
    assert_eq!(Frequency::Daily.after(d(2026, 2, 28)), d(2026, 3, 1));
    assert_eq!(Frequency::Weekly.after(d(2026, 2, 28)), d(2026, 3, 7));
    assert_eq!(Frequency::Monthly.after(d(2026, 1, 31)), d(2026, 2, 28));
    assert_eq!(Frequency::Quarterly.after(d(2026, 1, 1)), d(2026, 4, 1));
    // Not due yet.
    assert_eq!(due_run(Frequency::Monthly, d(2026, 6, 1), None, d(2026, 5, 31)), None);
    // Due today.
    assert_eq!(due_run(Frequency::Monthly, d(2026, 6, 1), None, d(2026, 6, 1)), Some((d(2026, 6, 1), d(2026, 7, 1))));
    // Closed for three months: one run, for the latest date that was due.
    assert_eq!(due_run(Frequency::Monthly, d(2026, 3, 1), None, d(2026, 6, 10)), Some((d(2026, 6, 1), d(2026, 7, 1))));
    // An end date stops it, but a run due before the end still happens.
    assert_eq!(due_run(Frequency::Monthly, d(2026, 3, 1), Some(d(2026, 4, 15)), d(2026, 6, 10)), Some((d(2026, 4, 1), d(2026, 5, 1))));
    assert_eq!(due_run(Frequency::Monthly, d(2026, 5, 1), Some(d(2026, 4, 15)), d(2026, 6, 10)), None);
}

/// A credential store for tests (schedules without a PDF password never touch it).
fn store() -> SecretStore {
    SecretStore::in_file(std::env::temp_dir().join(format!("lb-report-passwords-{}.json", std::process::id())), "test", "the report PDF password")
}

fn schedule(destination: Destination) -> Schedule {
    serde_json::from_value(serde_json::json!({
        "name": "Month end",
        "reports": [{ "kind": "statement", "report": "profit_and_loss", "period": "LAST_MONTH" }, { "kind": "statement", "report": "balance_sheet", "period": "LAST_MONTH" }],
        "frequency": "MONTHLY", "startDate": "2026-05-01",
        "destination": destination, "recipients": "owner@example.com", "folder": std::env::temp_dir().display().to_string(), "format": "PDF",
    }))
    .unwrap()
}

#[tokio::test]
async fn a_schedule_is_kept_runs_its_reports_and_saves_them_to_a_folder() {
    let pool = test_pool().await;
    let saved = save(&pool, None, &schedule(Destination::Email), d(2026, 4, 20), &store()).await.unwrap();
    assert_eq!(saved.next_run, Some(d(2026, 5, 1)), "starts on its start date");
    assert_eq!(saved.reports.len(), 2);
    assert_eq!(list(&pool).await.unwrap().len(), 1);
    // Saved after its start date: the first run is the next scheduled date from today.
    let later = save(&pool, Some(saved.id), &schedule(Destination::Email), d(2026, 5, 10), &store()).await.unwrap();
    assert_eq!(later.next_run, Some(d(2026, 6, 1)));

    assert!(due(&pool, d(2026, 5, 31)).await.unwrap().is_empty());
    let now_due = due(&pool, d(2026, 6, 3)).await.unwrap();
    assert_eq!(now_due.iter().map(|(s, run, next)| (s.id, *run, *next)).collect::<Vec<_>>(), [(saved.id, d(2026, 6, 1), Some(d(2026, 7, 1)))]);

    // The run's files: May's Profit and Loss and the Balance Sheet as of May 31.
    let files = files_for(&pool, &later, d(2026, 6, 1), &store()).await.unwrap();
    assert_eq!(files.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["Profit and Loss 2026-06-01.pdf", "Balance Sheet 2026-06-01.pdf"]);
    assert!(files.iter().all(|f| f.1.starts_with(b"%PDF") && f.2 == "application/pdf"));
    let sheet = sheet_for(&pool, &later.reports[1], d(2026, 6, 1)).await.unwrap();
    assert_eq!(sheet.subtitle, "As of May 31, 2026");

    let dir = std::env::temp_dir().join(format!("lb-schedules-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    save_to_folder(&dir.display().to_string(), &files).unwrap();
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(save_to_folder("/no/such/folder/here", &files).unwrap_err().starts_with("SCHEDULE_FOLDER"));

    record_run(&pool, saved.id, Some(d(2026, 7, 1)), "Emailed 2 reports to owner@example.com").await.unwrap();
    let after = get(&pool, saved.id).await.unwrap();
    assert_eq!((after.next_run, after.last_result.as_deref()), (Some(d(2026, 7, 1)), Some("Emailed 2 reports to owner@example.com")));
    assert!(due(&pool, d(2026, 6, 30)).await.unwrap().is_empty());
    delete(&pool, saved.id, &store()).await.unwrap();
    assert!(get(&pool, saved.id).await.unwrap_err().starts_with("SCHEDULE_NOT_FOUND"));
}

#[tokio::test]
async fn schedules_are_checked_and_report_builder_reports_can_be_scheduled() {
    let pool = test_pool().await;
    let mut bad = schedule(Destination::Email);
    bad.recipients = "not an address".into();
    assert!(save(&pool, None, &bad, d(2026, 4, 1), &store()).await.is_err());
    let mut bad = schedule(Destination::Folder);
    bad.folder = "relative/folder".into();
    assert!(save(&pool, None, &bad, d(2026, 4, 1), &store()).await.unwrap_err().starts_with("SCHEDULE_FOLDER"));
    let mut bad = schedule(Destination::Email);
    bad.reports = vec![ScheduledReport::Statement { report: "drop_tables".into(), period: Period::LastMonth }];
    assert!(save(&pool, None, &bad, d(2026, 4, 1), &store()).await.unwrap_err().starts_with("SCHEDULE_REPORT"));
    let mut bad = schedule(Destination::Email);
    bad.end_date = Some(d(2026, 1, 1));
    assert!(save(&pool, None, &bad, d(2026, 4, 1), &store()).await.unwrap_err().starts_with("SCHEDULE_DATES"));

    // Every preset of the report builder runs as a scheduled report.
    for preset in crate::custom_reports::presets() {
        let report = ScheduledReport::Preset { id: preset.id.to_string(), period: Period::LastMonth };
        let sheet = sheet_for(&pool, &report, d(2026, 6, 1)).await.unwrap_or_else(|e| panic!("{}: {e}", preset.id));
        assert_eq!(sheet.title, preset.title);
    }
    let mut xlsx = schedule(Destination::Email);
    xlsx.format = FileFormat::Xlsx;
    let files = files_for(&pool, &xlsx, d(2026, 6, 1), &store()).await.unwrap();
    assert!(files.iter().all(|f| f.0.ends_with(".xlsx") && f.1.starts_with(b"PK")));
}

#[tokio::test]
async fn a_schedule_can_send_password_protected_pdfs_with_the_password_kept_off_the_company_file() {
    let pool = test_pool().await;
    let passwords = SecretStore::in_file(std::env::temp_dir().join(format!("lb-report-pw-{}-{}.json", std::process::id(), chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default())), "test", "the report PDF password");
    let mut s = schedule(Destination::Email);
    s.protect_pdf = true;
    // A password is needed the first time.
    assert!(save(&pool, None, &s, d(2026, 4, 20), &passwords).await.unwrap_err().starts_with("SCHEDULE_PDF_PASSWORD"));
    s.pdf_password = Some("month-end 2026".into());
    let saved = save(&pool, None, &s, d(2026, 4, 20), &passwords).await.unwrap();
    assert!(saved.protect_pdf && saved.pdf_password.is_none());
    assert!(!serde_json::to_string(&saved).unwrap().contains("month-end"), "the password is never sent back");
    // Nothing about the password is in the company file but the name it's stored under.
    let stored: Option<String> = sqlx::query_scalar("SELECT pdf_password_key FROM report_schedules WHERE id = ?1").bind(saved.id).fetch_one(&pool).await.unwrap();
    let key = stored.expect("protected");
    assert!(!key.contains("month-end"));
    assert_eq!(passwords.get(&key).await.unwrap().as_deref(), Some("month-end 2026"));

    let files = files_for(&pool, &saved, d(2026, 6, 1), &passwords).await.unwrap();
    for (name, bytes, _) in &files {
        assert!(lopdf::Document::load_mem_with_options(bytes, lopdf::LoadOptions::with_password("month-end 2026")).is_ok(), "{name} opens with the password");
        assert!(lopdf::Document::load_mem_with_options(bytes, lopdf::LoadOptions::with_password("guess")).is_err(), "{name} not with another");
    }

    // Saving again without a new password keeps it; turning protection off forgets it.
    let mut again = get(&pool, saved.id).await.unwrap();
    assert!(save(&pool, Some(saved.id), &again, d(2026, 4, 20), &passwords).await.unwrap().protect_pdf);
    again.protect_pdf = false;
    assert!(!save(&pool, Some(saved.id), &again, d(2026, 4, 20), &passwords).await.unwrap().protect_pdf);
    assert_eq!(passwords.get(&key).await.unwrap(), None);
    let plain = files_for(&pool, &get(&pool, saved.id).await.unwrap(), d(2026, 6, 1), &passwords).await.unwrap();
    assert!(lopdf::Document::load_mem(&plain[0].1).unwrap().extract_text(&[1]).unwrap().contains("Profit and Loss"));

    // Excel files can't have a password; a password lost from the computer stops the run with a reason.
    let mut xlsx = get(&pool, saved.id).await.unwrap();
    xlsx.format = FileFormat::Xlsx;
    xlsx.protect_pdf = true;
    xlsx.pdf_password = Some("x".into());
    assert!(save(&pool, Some(saved.id), &xlsx, d(2026, 4, 20), &passwords).await.unwrap_err().starts_with("SCHEDULE_PDF_PASSWORD"));
    let mut lost = get(&pool, saved.id).await.unwrap();
    lost.protect_pdf = true;
    lost.pdf_password = Some("secret".into());
    let lost = save(&pool, Some(saved.id), &lost, d(2026, 4, 20), &passwords).await.unwrap();
    passwords.delete(lost.pdf_password_key.as_deref().unwrap()).await.unwrap();
    assert!(files_for(&pool, &lost, d(2026, 6, 1), &passwords).await.unwrap_err().starts_with("SCHEDULE_PDF_PASSWORD"));
    delete(&pool, saved.id, &passwords).await.unwrap();
}
