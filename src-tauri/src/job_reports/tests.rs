//! Job costing reports on small companies built for each case. The fixture company with jobs, and
//! the checks that every report adds up to the Profit & Loss, are in `harness/job_costing.rs`.

use super::*;
use crate::db::test_pool;
use crate::expenses::{BillInput, BillLineInput, ExpenseInput, SplitLineInput};

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
}

async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.expect("seeded account exists")
}

fn bill(lines: Vec<(i64, i64, Option<i64>)>) -> BillInput {
    BillInput {
        vendor_name: "Timberline Supply".into(),
        bill_number: None,
        bill_date: d(5),
        due_date: d(30),
        memo: None,
        total_cents: lines.iter().map(|(_, cents, _)| cents).sum(),
        lines: lines
            .into_iter()
            .map(|(account_id, amount_cents, job)| BillLineInput { account_id, amount_cents, memo: None, quantity_e4: None, rate_cents: None, product_service_id: None, class_id: None, customer_id: job })
            .collect(),
    }
}

#[test]
fn shares_add_up_exactly() {
    assert_eq!(split(100, &[1, 1, 1]), vec![33, 34, 33]);
    assert_eq!(split(-100, &[1, 1, 1]), vec![-33, -34, -33]);
    assert_eq!(split(1_001, &[120_000, 80_000, 30_000]), vec![522, 348, 131]);
    assert_eq!(split(50, &[0, 0]), vec![25, 25], "no weights share equally");
    assert_eq!(split(7, &[-3, -4]), vec![3, 4], "weights count by size");
    for amount in [1, 2, 99, 12_345, -6_789] {
        assert_eq!(split(amount, &[3, 5, 7, 11]).iter().sum::<i64>(), amount);
    }
}

#[tokio::test]
async fn an_unknown_job_is_refused() {
    let pool = test_pool().await;
    let supplies = account_id(&pool, "6250").await;
    let err = crate::expenses::post_vendor_bill(&pool, bill(vec![(supplies, 1_000, Some(9_999))])).await.unwrap_err();
    assert!(err.starts_with("BILL_UNKNOWN_JOB"), "{err}");
    let input = ExpenseInput {
        kind: "EXPENSE".into(),
        payee_name: None,
        payment_account_id: account_id(&pool, "1010").await,
        expense_date: d(6),
        check_number: None,
        payment_method: None,
        memo: None,
        lines: vec![SplitLineInput { account_id: supplies, amount_cents: 500, memo: None, class_id: None, customer_id: Some(9_999) }],
        currency: None,
    };
    let err = crate::expenses::post_expense(&pool, input).await.unwrap_err();
    assert!(err.starts_with("EXP_UNKNOWN_JOB"), "{err}");
}

#[tokio::test]
async fn a_bill_line_is_shared_among_its_jobs() {
    let pool = test_pool().await;
    let supplies = account_id(&pool, "6250").await;
    let job = crate::sales::create_customer(&pool, "Acme:Kitchen", None).await.unwrap().id;
    let acme: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Acme'").fetch_one(&pool).await.unwrap();
    // Three lines on one account post as one ledger line of $10.01.
    let posted = crate::expenses::post_vendor_bill(&pool, bill(vec![(supplies, 600, Some(job)), (supplies, 300, Some(acme)), (supplies, 101, None)])).await.unwrap();
    let lines = job_lines(&pool, None, d(31)).await.unwrap();
    let shares: Vec<(Option<i64>, i64)> = lines.iter().map(|l| (l.job, l.cost())).collect();
    assert_eq!(shares, vec![(Some(job), 600), (Some(acme), 300), (None, 101)]);

    // Editing keeps the jobs: the lines load back with them.
    let loaded = crate::edits::document_lines(&pool, "bill", posted.id).await.unwrap();
    assert_eq!(loaded.iter().map(|l| l.customer_id).collect::<Vec<_>>(), vec![Some(job), Some(acme), None]);

    // Voiding the bill takes the cost off each job again.
    crate::ledger::void_transaction(&pool, posted.journal_entry_id.unwrap()).await.unwrap();
    let report = run(&pool, JobReportKind::JobProfitSummary, &JobReportParams::default(), d(31)).await.unwrap();
    let total = report.rows.last().unwrap();
    assert_eq!(total.cells[1], json!(0));
    let kitchen = report.rows.iter().find(|r| r.cells[0] == "Kitchen").expect("job row");
    assert_eq!(kitchen.cells[1], json!(0), "the void nets the job's cost to zero");
}

#[tokio::test]
async fn estimates_without_invoices() {
    let pool = test_pool().await;
    let services = account_id(&pool, "4100").await;
    let line = crate::sales::SalesLineInput { product_service_id: None, description: "Design".into(), quantity_e4: 10_000, rate_cents: 50_000, income_account_id: services, class_id: None, is_taxable: false };
    let input = crate::estimates::EstimateInput {
        customer_name: "Acme:Kitchen".into(),
        estimate_date: d(2),
        expiration_date: None,
        memo: None,
        lines: vec![line],
        tax_rate_id: None,
        discount_cents: 0,
        details: Default::default(),
    };
    let estimate = crate::estimates::create_estimate(&pool, input).await.unwrap();
    let report = run(&pool, JobReportKind::ProgressInvoices, &JobReportParams::default(), d(31)).await.unwrap();
    let row = report.rows.iter().find(|r| r.estimate_id == Some(estimate.id)).unwrap();
    assert_eq!((row.cells[3].clone(), row.cells[4].clone(), row.cells[5].clone()), (json!(50_000), json!(0), json!(0.0)));
    assert_eq!(row.journal_entry_id, None);

    // Rejected estimates are left out of estimates vs. actuals, but still listed by job.
    crate::estimates::set_estimate_status(&pool, estimate.id, "REJECTED").await.unwrap();
    let summary = run(&pool, JobReportKind::JobEstimatesSummary, &JobReportParams::default(), d(31)).await.unwrap();
    assert_eq!(summary.rows.last().unwrap().cells[2], json!(0));
    let listed = run(&pool, JobReportKind::EstimatesByJob, &JobReportParams::default(), d(31)).await.unwrap();
    assert_eq!(listed.line_count, 1);
}

#[tokio::test]
async fn bad_range_is_refused() {
    let pool = test_pool().await;
    let params = JobReportParams { start_date: Some(d(20)), end_date: Some(d(10)), ..Default::default() };
    assert!(run(&pool, JobReportKind::JobProfitSummary, &params, d(31)).await.unwrap_err().starts_with("RPT_BAD_RANGE"));
    assert!(profit_and_loss_by_job(&pool, d(20), d(10)).await.unwrap_err().starts_with("RPT_BAD_RANGE"));
}
