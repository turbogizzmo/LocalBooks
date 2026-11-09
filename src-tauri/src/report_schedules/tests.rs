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
    let saved = save(&pool, None, &schedule(Destination::Email), d(2026, 4, 20)).await.unwrap();
    assert_eq!(saved.next_run, Some(d(2026, 5, 1)), "starts on its start date");
    assert_eq!(saved.reports.len(), 2);
    assert_eq!(list(&pool).await.unwrap().len(), 1);
    // Saved after its start date: the first run is the next scheduled date from today.
    let later = save(&pool, Some(saved.id), &schedule(Destination::Email), d(2026, 5, 10)).await.unwrap();
    assert_eq!(later.next_run, Some(d(2026, 6, 1)));

    assert!(due(&pool, d(2026, 5, 31)).await.unwrap().is_empty());
    let now_due = due(&pool, d(2026, 6, 3)).await.unwrap();
    assert_eq!(now_due.iter().map(|(s, run, next)| (s.id, *run, *next)).collect::<Vec<_>>(), [(saved.id, d(2026, 6, 1), Some(d(2026, 7, 1)))]);

    // The run's files: May's Profit and Loss and the Balance Sheet as of May 31.
    let files = files_for(&pool, &later, d(2026, 6, 1)).await.unwrap();
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
    delete(&pool, saved.id).await.unwrap();
    assert!(get(&pool, saved.id).await.unwrap_err().starts_with("SCHEDULE_NOT_FOUND"));
}

#[tokio::test]
async fn schedules_are_checked_and_report_builder_reports_can_be_scheduled() {
    let pool = test_pool().await;
    let mut bad = schedule(Destination::Email);
    bad.recipients = "not an address".into();
    assert!(save(&pool, None, &bad, d(2026, 4, 1)).await.is_err());
    let mut bad = schedule(Destination::Folder);
    bad.folder = "relative/folder".into();
    assert!(save(&pool, None, &bad, d(2026, 4, 1)).await.unwrap_err().starts_with("SCHEDULE_FOLDER"));
    let mut bad = schedule(Destination::Email);
    bad.reports = vec![ScheduledReport::Statement { report: "drop_tables".into(), period: Period::LastMonth }];
    assert!(save(&pool, None, &bad, d(2026, 4, 1)).await.unwrap_err().starts_with("SCHEDULE_REPORT"));
    let mut bad = schedule(Destination::Email);
    bad.end_date = Some(d(2026, 1, 1));
    assert!(save(&pool, None, &bad, d(2026, 4, 1)).await.unwrap_err().starts_with("SCHEDULE_DATES"));

    // Every preset of the report builder runs as a scheduled report.
    for preset in crate::custom_reports::presets() {
        let report = ScheduledReport::Preset { id: preset.id.to_string(), period: Period::LastMonth };
        let sheet = sheet_for(&pool, &report, d(2026, 6, 1)).await.unwrap_or_else(|e| panic!("{}: {e}", preset.id));
        assert_eq!(sheet.title, preset.title);
    }
    let mut xlsx = schedule(Destination::Email);
    xlsx.format = FileFormat::Xlsx;
    let files = files_for(&pool, &xlsx, d(2026, 6, 1)).await.unwrap();
    assert!(files.iter().all(|f| f.0.ends_with(".xlsx") && f.1.starts_with(b"PK")));
}
