use super::*;
use crate::db::test_pool;
use crate::sales::{create_customer, create_product, ProductInput};

fn d(month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, day).unwrap()
}

fn miles(m: i64) -> i64 {
    m * QUANTITY_SCALE
}

async fn vehicle(pool: &SqlitePool, name: &str) -> i64 {
    save_vehicle(pool, None, VehicleInput { name: name.into(), description: None, is_active: true }).await.unwrap().id
}

async fn rate(pool: &SqlitePool, date: NaiveDate, rate_e4: i64) {
    save_mileage_rate(pool, None, MileageRateInput { effective_date: date, rate_e4, note: Some("IRS business rate".into()) }).await.unwrap();
}

fn trip(vehicle_id: i64, date: NaiveDate, miles_e4: i64, customer_id: Option<i64>, billable: bool) -> TripInput {
    TripInput {
        vehicle_id,
        trip_date: date,
        odometer_start_e4: None,
        odometer_end_e4: None,
        miles_e4: Some(miles_e4),
        notes: Some("Site visit".into()),
        customer_id,
        billable,
    }
}

async fn mileage_item(pool: &SqlitePool) -> i64 {
    let income: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(pool).await.unwrap();
    create_product(pool, ProductInput { name: "Mileage".into(), description: None, rate_cents: 75, income_account_id: income, is_taxable: false })
        .await
        .unwrap()
        .id
}

#[tokio::test]
async fn the_rate_in_effect_is_the_latest_on_or_before_the_trip_date() {
    let pool = test_pool().await;
    assert_eq!(rate_on(&pool, d(1, 1)).await.unwrap(), None, "no rates are built in");
    rate(&pool, d(1, 1), 7_000).await;
    rate(&pool, d(7, 1), 7_250).await;

    assert_eq!(rate_on(&pool, NaiveDate::from_ymd_opt(2025, 12, 31).unwrap()).await.unwrap(), None);
    assert_eq!(rate_on(&pool, d(1, 1)).await.unwrap(), Some(7_000));
    assert_eq!(rate_on(&pool, d(6, 30)).await.unwrap(), Some(7_000));
    assert_eq!(rate_on(&pool, d(7, 1)).await.unwrap(), Some(7_250));
    assert_eq!(rate_on(&pool, d(12, 31)).await.unwrap(), Some(7_250));

    // One rate per date; rates must be positive.
    let dup = save_mileage_rate(&pool, None, MileageRateInput { effective_date: d(7, 1), rate_e4: 6_700, note: None }).await.unwrap_err();
    assert!(dup.starts_with("MILE_DUPLICATE_RATE"), "{dup}");
    let zero = save_mileage_rate(&pool, None, MileageRateInput { effective_date: d(8, 1), rate_e4: 0, note: None }).await.unwrap_err();
    assert!(zero.starts_with("MILE_BAD_RATE"), "{zero}");

    // Changing and deleting a rate changes what applies.
    let july = list_mileage_rates(&pool).await.unwrap().into_iter().find(|r| r.effective_date == "2026-07-01").unwrap();
    save_mileage_rate(&pool, Some(july.id), MileageRateInput { effective_date: d(9, 1), rate_e4: 7_250, note: None }).await.unwrap();
    assert_eq!(rate_on(&pool, d(8, 15)).await.unwrap(), Some(7_000));
    delete_mileage_rate(&pool, july.id).await.unwrap();
    assert_eq!(rate_on(&pool, d(12, 31)).await.unwrap(), Some(7_000));

    // A trip's deductible amount uses its date's rate and rounds half up to the cent.
    let car = vehicle(&pool, "Truck").await;
    let t = create_trip(&pool, trip(car, d(3, 2), 123_000, None, false)).await.unwrap(); // 12.3 mi × $0.70
    assert_eq!((t.rate_e4, t.deductible_cents), (Some(7_000), Some(861)));
    assert_eq!(deductible_cents(10_000 + 70, 7_250), 73); // 1.007 mi × $0.725 = 73.0075¢
    assert_eq!(deductible_cents(miles(1), 6_550), 66); // 65.5¢ rounds up
}

#[tokio::test]
async fn trips_check_odometer_miles_customer_and_vehicle() {
    let pool = test_pool().await;
    let car = vehicle(&pool, "Sedan").await;

    let mut by_odometer = trip(car, d(3, 2), 0, None, false);
    by_odometer.miles_e4 = None;
    by_odometer.odometer_start_e4 = Some(miles(12_000));
    by_odometer.odometer_end_e4 = Some(miles(12_042) + 5_000);
    let saved = create_trip(&pool, by_odometer.clone()).await.unwrap();
    assert_eq!(saved.miles_e4, 425_000, "42.5 miles from the odometer");

    by_odometer.miles_e4 = Some(miles(40));
    assert!(create_trip(&pool, by_odometer.clone()).await.unwrap_err().starts_with("MILE_ODOMETER_MISMATCH"));
    by_odometer.miles_e4 = None;
    by_odometer.odometer_end_e4 = Some(miles(11_000));
    assert!(create_trip(&pool, by_odometer.clone()).await.unwrap_err().starts_with("MILE_BAD_ODOMETER"));
    by_odometer.odometer_end_e4 = None;
    assert!(create_trip(&pool, by_odometer).await.unwrap_err().starts_with("MILE_BAD_ODOMETER"));

    assert!(create_trip(&pool, trip(car, d(3, 2), 0, None, false)).await.unwrap_err().starts_with("MILE_BAD_MILES"));
    assert!(create_trip(&pool, trip(car, d(3, 2), miles(5), None, true)).await.unwrap_err().starts_with("MILE_BILLABLE_NO_CUSTOMER"));

    save_vehicle(&pool, Some(car), VehicleInput { name: "Sedan".into(), description: Some("2019, plate 4XYZ".into()), is_active: false }).await.unwrap();
    assert!(create_trip(&pool, trip(car, d(3, 3), miles(5), None, false)).await.unwrap_err().starts_with("MILE_INACTIVE_VEHICLE"));
    assert!(save_vehicle(&pool, None, VehicleInput { name: "sedan".into(), description: None, is_active: true }).await.unwrap_err().starts_with("MILE_DUPLICATE_VEHICLE"));
    let listed = list_vehicles(&pool).await.unwrap();
    assert_eq!((listed[0].is_active, listed[0].trip_count, listed[0].description.as_deref()), (false, 1, Some("2019, plate 4XYZ")));
}

#[tokio::test]
async fn billing_trips_posts_an_invoice_and_marks_them_billed() {
    let pool = test_pool().await;
    let car = vehicle(&pool, "Truck").await;
    let acme = create_customer(&pool, "Acme", None).await.unwrap().id;
    let kitchen = create_customer(&pool, "Acme:Kitchen", None).await.unwrap().id;
    let item = mileage_item(&pool).await;
    let first = create_trip(&pool, trip(car, d(3, 2), 125_000, Some(kitchen), true)).await.unwrap(); // 12.5 mi
    let second = create_trip(&pool, trip(car, d(3, 9), miles(30), Some(kitchen), true)).await.unwrap();
    let not_billable = create_trip(&pool, trip(car, d(3, 9), miles(8), Some(kitchen), false)).await.unwrap();
    let other_job = create_trip(&pool, trip(car, d(3, 9), miles(8), Some(acme), true)).await.unwrap();

    let unbilled = list_unbilled_trips(&pool, Some(kitchen)).await.unwrap();
    assert_eq!(unbilled.iter().map(|t| t.id).collect::<Vec<_>>(), vec![first.id, second.id]);

    let bill = |ids: Vec<i64>| BillTripsInput {
        customer_id: kitchen,
        trip_ids: ids,
        product_service_id: item,
        rate_cents: 75,
        issue_date: d(3, 31),
        due_date: d(4, 30),
        tax_rate_id: None,
    };
    // Trips that aren't billable or belong to another job are refused, and nothing is written.
    assert!(bill_trips(&pool, bill(vec![first.id, not_billable.id])).await.unwrap_err().starts_with("MILE_NOT_BILLABLE"));
    assert!(bill_trips(&pool, bill(vec![other_job.id])).await.unwrap_err().starts_with("MILE_OTHER_CUSTOMER"));
    assert!(crate::invoices::list_invoices(&pool).await.unwrap().is_empty());
    assert_eq!(get_trip(&pool, first.id).await.unwrap().invoice_id, None);

    let invoice = bill_trips(&pool, bill(vec![first.id, second.id])).await.unwrap();
    // 12.5 × $0.75 = $9.38 (rounded half up) + 30 × $0.75 = $22.50
    assert_eq!((invoice.customer_name.as_str(), invoice.total_cents, invoice.item_count), ("Acme:Kitchen", 938 + 2_250, 2));
    let detail = crate::invoices::invoice_detail(&pool, invoice.id).await.unwrap();
    assert_eq!(detail.items[0].quantity_e4, 125_000);
    assert!(detail.items[0].description.starts_with("Mileage: 12.5 mi, 2026-03-02, Truck"), "{}", detail.items[0].description);
    assert_eq!(get_trip(&pool, first.id).await.unwrap().invoice_number, Some(invoice.invoice_number));
    assert!(list_unbilled_trips(&pool, Some(kitchen)).await.unwrap().is_empty());

    // A billed trip can't be billed twice, changed or deleted.
    assert!(bill_trips(&pool, bill(vec![first.id])).await.unwrap_err().starts_with("MILE_TRIP_BILLED"));
    assert!(delete_trip(&pool, first.id).await.unwrap_err().starts_with("MILE_TRIP_BILLED"));
    assert!(update_trip(&pool, first.id, trip(car, d(3, 2), miles(13), Some(kitchen), true)).await.unwrap_err().starts_with("MILE_TRIP_BILLED"));

    // Editing the invoice keeps the trips on the edited invoice.
    let mut edited = InvoiceInput {
        customer_name: "Acme:Kitchen".into(),
        issue_date: d(3, 31),
        due_date: d(5, 15),
        lines: Vec::new(),
        currency: None,
        tax_rate_id: None,
        discount_cents: 0,
        details: Default::default(),
    };
    edited.lines = detail
        .items
        .iter()
        .map(|i| InvoiceLineInput {
            description: i.description.clone(),
            quantity_e4: i.quantity_e4,
            rate_cents: i.rate_cents,
            income_account_id: i.income_account_id,
            product_service_id: i.product_service_id,
            class_id: None,
            is_taxable: false,
        })
        .collect();
    let replacement = crate::edits::replace_invoice(&pool, invoice.id, edited).await.unwrap();
    assert_eq!(get_trip(&pool, second.id).await.unwrap().invoice_id, Some(replacement));
    assert!(list_unbilled_trips(&pool, Some(kitchen)).await.unwrap().is_empty());

    // Voiding the invoice frees the trips to be billed again.
    let entry = crate::invoices::get_summary(&pool, replacement).await.unwrap().journal_entry_id.unwrap();
    crate::ledger::void_transaction(&pool, entry).await.unwrap();
    assert_eq!(list_unbilled_trips(&pool, Some(kitchen)).await.unwrap().len(), 2);
    assert_eq!(get_trip(&pool, first.id).await.unwrap().invoice_id, None);
    let again = bill_trips(&pool, bill(vec![first.id])).await.unwrap();
    assert_eq!(again.total_cents, 938);
    assert_eq!(list_unbilled_trips(&pool, None).await.unwrap().iter().map(|t| t.id).collect::<Vec<_>>(), vec![other_job.id, second.id]);
}

#[tokio::test]
async fn reports_total_miles_and_deductible_by_vehicle_and_job() {
    let pool = test_pool().await;
    rate(&pool, d(1, 1), 7_000).await;
    rate(&pool, d(7, 1), 7_250).await;
    let truck = vehicle(&pool, "Truck").await;
    let van = vehicle(&pool, "Van").await;
    let acme = create_customer(&pool, "Acme", None).await.unwrap().id;
    let kitchen = create_customer(&pool, "Acme:Kitchen", None).await.unwrap().id;
    let beta = create_customer(&pool, "Beta", None).await.unwrap().id;
    create_trip(&pool, trip(truck, d(6, 30), miles(100), Some(kitchen), true)).await.unwrap(); // $70.00
    create_trip(&pool, trip(truck, d(7, 1), miles(100), Some(acme), false)).await.unwrap(); // $72.50
    create_trip(&pool, trip(van, d(7, 2), 105_000, Some(beta), true)).await.unwrap(); // 10.5 × .725 = $7.6125 → $7.61
    create_trip(&pool, trip(van, d(7, 3), miles(20), None, false)).await.unwrap(); // $14.50
    let all = MileageReportParams::default();
    let today = d(12, 31);

    let summary = mileage_report(&pool, MileageReportKind::VehicleSummary, &all, today).await.unwrap();
    let cells: Vec<Vec<Value>> = summary.rows.iter().map(|r| r.cells.clone()).collect();
    assert_eq!(
        cells,
        vec![
            vec![json!("Truck"), json!(2), json!(miles(200)), json!(14_250)],
            vec![json!("Van"), json!(2), json!(miles(20) + 105_000), json!(761 + 1_450)],
            vec![json!("TOTAL"), json!(4), json!(miles(220) + 105_000), json!(14_250 + 761 + 1_450)],
        ]
    );
    assert_eq!(summary.rows[1].drill.as_ref().map(|d| (d.report, d.vehicle_id)), Some((MileageReportKind::VehicleDetail, Some(van))));
    assert_eq!(summary.trips_without_rate, 0);

    // Detail: one row per trip with the rate on its date, grouped and subtotaled by vehicle.
    let detail = mileage_report(&pool, MileageReportKind::VehicleDetail, &MileageReportParams { vehicle_id: Some(truck), ..Default::default() }, today).await.unwrap();
    let rates: Vec<Value> = detail.rows.iter().filter(|r| r.kind == RowKind::Detail).map(|r| r.cells[7].clone()).collect();
    assert_eq!(rates, vec![json!(7_000), json!(7_250)]);
    let subtotal = detail.rows.iter().find(|r| r.kind == RowKind::Subtotal).unwrap();
    assert_eq!((subtotal.cells[0].clone(), subtotal.cells[6].clone(), subtotal.cells[8].clone()), (json!("Total Truck"), json!(miles(200)), json!(14_250)));
    assert_eq!(detail.rows.iter().filter(|r| r.kind == RowKind::Detail).next().unwrap().cells[3], json!("Unbilled"));

    // By job: customers alphabetically, jobs under their own name, no customer/job last.
    let jobs = mileage_report(&pool, MileageReportKind::JobSummary, &all, today).await.unwrap();
    let labels: Vec<Value> = jobs.rows.iter().map(|r| r.cells[0].clone()).collect();
    assert_eq!(labels, vec![json!("Acme"), json!("Acme:Kitchen"), json!("Beta"), json!(NO_JOB), json!("TOTAL")]);
    assert_eq!(jobs.rows[1].cells[3], json!(miles(100)), "billable miles");
    assert_eq!(jobs.rows[4].cells[4], json!(14_250 + 761 + 1_450));

    // A customer filter takes in its jobs; the date range narrows the trips.
    let acme_only = MileageReportParams { customer_id: Some(acme), ..Default::default() };
    let acme_detail = mileage_report(&pool, MileageReportKind::JobDetail, &acme_only, today).await.unwrap();
    assert_eq!(acme_detail.line_count, 2);
    let july = MileageReportParams { start_date: Some(d(7, 1)), end_date: Some(d(7, 2)), ..Default::default() };
    assert_eq!(mileage_report(&pool, MileageReportKind::JobSummary, &july, today).await.unwrap().line_count, 2);
    let none = MileageReportParams { no_job: true, ..Default::default() };
    assert_eq!(mileage_report(&pool, MileageReportKind::JobDetail, &none, today).await.unwrap().line_count, 1);

    // Trips before the first rate have no deductible amount, and the report says how many.
    create_trip(&pool, trip(truck, NaiveDate::from_ymd_opt(2025, 12, 1).unwrap(), miles(10), None, false)).await.unwrap();
    let early = mileage_report(&pool, MileageReportKind::VehicleDetail, &all, today).await.unwrap();
    assert_eq!(early.trips_without_rate, 1);
    assert_eq!(early.rows[1].cells[8], Value::Null);
}

#[tokio::test]
async fn reports_send_the_line_count_the_report_table_reads() {
    let pool = test_pool().await;
    let today = d(12, 31);
    let empty = mileage_report(&pool, MileageReportKind::VehicleDetail, &MileageReportParams::default(), today).await.unwrap();
    let json = serde_json::to_value(&empty).unwrap();
    assert_eq!(json["lineCount"], json!(0), "the empty state shows for a report with no trips");

    let car = vehicle(&pool, "Truck").await;
    create_trip(&pool, trip(car, d(3, 2), miles(5), None, false)).await.unwrap();
    create_trip(&pool, trip(car, d(3, 3), miles(7), None, false)).await.unwrap();
    for kind in [MileageReportKind::VehicleSummary, MileageReportKind::VehicleDetail, MileageReportKind::JobSummary, MileageReportKind::JobDetail] {
        let report = serde_json::to_value(mileage_report(&pool, kind, &MileageReportParams::default(), today).await.unwrap()).unwrap();
        assert_eq!(report["lineCount"], json!(2), "{kind:?}");
    }
}

#[tokio::test]
async fn every_report_starts_with_a_text_column_for_its_labels() {
    let pool = test_pool().await;
    for kind in [MileageReportKind::VehicleSummary, MileageReportKind::VehicleDetail, MileageReportKind::JobSummary, MileageReportKind::JobDetail] {
        let report = mileage_report(&pool, kind, &MileageReportParams::default(), d(12, 31)).await.unwrap();
        // The report table formats a cell by its column's kind, so "Total Truck" in a date column would show as a bad date.
        assert_eq!(report.columns[0].kind, ColumnKind::Text, "{kind:?}");
    }
}
