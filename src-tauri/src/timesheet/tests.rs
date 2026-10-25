use super::*;
use crate::db::test_pool;
use crate::sales::create_customer;

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
}

#[tokio::test]
async fn weekly_timesheet_saves_atomically_and_caps_the_day() {
    let pool = test_pool().await;
    let ana = create_employee(&pool, "Ana Ortiz", Some("ana@example.com")).await.unwrap();
    let acme = create_customer(&pool, "Acme", None).await.unwrap();
    let entry = |day, minutes, billable| TimeActivityInput {
        employee_id: ana.id,
        activity_date: d(day),
        customer_id: Some(acme.id),
        product_service_id: None,
        minutes,
        billable,
        hourly_rate_cents: Some(12_500),
        description: Some("Implementation".into()),
        class_id: None,
    };

    let saved = create_time_activities(&pool, vec![entry(12, 480, true), entry(13, 450, true), entry(14, 90, false)]).await.unwrap();
    assert_eq!(saved, 3);
    let week = list_time_activities(&pool, d(12), d(18)).await.unwrap();
    assert_eq!(week.iter().map(|t| t.minutes).sum::<i64>(), 1_020);
    assert_eq!(week.iter().find(|t| !t.billable).and_then(|t| t.hourly_rate_cents), None);

    // 16h already logged on the 12th + 9h more would exceed 24h: the whole batch is rejected.
    create_time_activities(&pool, vec![entry(12, 480, false)]).await.unwrap();
    let err = create_time_activities(&pool, vec![entry(15, 60, false), entry(12, 540, false)]).await.expect_err("over 24h");
    assert!(err.starts_with("TIME_OVER_24H"), "{err}");
    assert!(list_time_activities(&pool, d(15), d(15)).await.unwrap().is_empty());

    let mut no_customer = entry(16, 60, true);
    no_customer.customer_id = None;
    assert!(create_time_activities(&pool, vec![no_customer]).await.unwrap_err().starts_with("TIME_BILLABLE_INCOMPLETE"));
    assert!(create_employee(&pool, "ana ortiz", None).await.unwrap_err().starts_with("EMP_DUPLICATE"));
}
