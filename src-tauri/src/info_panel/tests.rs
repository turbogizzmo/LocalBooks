use super::*;
use crate::harness::fixture::d;
use crate::workflows::{PurchaseOrderInput, PurchaseOrderLineInput};

/// Far past the fixture's dates, so every document counts however the fixture grows.
fn later() -> NaiveDate {
    d(2099, 12, 31)
}

#[tokio::test]
async fn open_balances_match_the_aging_reports_for_every_name() {
    let pool = crate::harness::job_costing::company().await;
    for (kind, payable) in [("customer", false), ("vendor", true)] {
        let aging = crate::report_center::aging(&pool, payable, later()).await.unwrap();
        let names = if payable { crate::expenses::list_vendors(&pool).await.unwrap() } else { crate::sales::list_customers(&pool).await.unwrap() };
        let mut checked = 0;
        for p in &names {
            let summary = party_summary(&pool, kind, p.id).await.unwrap();
            let report = aging.rows.iter().find(|r| r.party_name == p.name).map_or(0, |r| r.total_cents);
            assert_eq!(summary.open_balance_cents, report, "{kind} {}: panel vs aging report", p.name);
            checked += usize::from(report != 0);
        }
        assert!(checked > 0, "some {kind} has an open balance in the fixture");
    }
}

#[tokio::test]
async fn customer_estimates_and_unbilled_costs_match_their_lists() {
    let pool = crate::harness::job_costing::company().await;
    let maple: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Kestrel Homes:Maple St Addition'").fetch_one(&pool).await.unwrap();
    let summary = party_summary(&pool, "customer", maple).await.unwrap();
    // Maple St's estimate is still pending; Lakeview's became its invoice and doesn't count.
    let pending: Vec<_> = crate::estimates::list_estimates(&pool)
        .await
        .unwrap()
        .into_iter()
        .filter(|e| e.customer_name.ends_with("Maple St Addition") && e.status == "PENDING")
        .collect();
    assert_eq!((summary.active_estimate_count, summary.active_estimate_cents), (pending.len() as i64, pending.iter().map(|e| e.total_cents).sum()));
    assert!(summary.active_estimate_count > 0);
    let lakeview: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Kestrel Homes:Lakeview Remodel'").fetch_one(&pool).await.unwrap();
    assert_eq!(party_summary(&pool, "customer", lakeview).await.unwrap().active_estimate_count, 0);
    // Unbilled costs are what Sales › Bill Costs lists for the job.
    for id in [maple, lakeview] {
        let costs = crate::billable_costs::list_unbilled(&pool, Some(id)).await.unwrap();
        let s = party_summary(&pool, "customer", id).await.unwrap();
        assert_eq!((s.unbilled_cost_count, s.unbilled_cost_cents), (costs.len() as i64, costs.iter().map(|c| c.amount_cents).sum()));
    }
}

#[tokio::test]
async fn open_purchase_orders_match_the_report() {
    let pool = crate::harness::job_costing::company().await;
    let vendor = crate::expenses::list_vendors(&pool).await.unwrap().into_iter().next().unwrap();
    let account = crate::harness::account(&pool, "6300").await;
    let line = |cents| PurchaseOrderLineInput {
        account_id: account,
        product_service_id: None,
        description: Some("Paper".into()),
        quantity_e4: None,
        rate_cents: None,
        amount_cents: crate::money::Cents(cents),
        class_id: None,
        customer_id: None,
    };
    for cents in [12_500, 7_500] {
        let po = PurchaseOrderInput {
            form_lines: Vec::new(),
            vendor_name: vendor.name.clone(),
            po_date: d(2026, 1, 5),
            expected_date: None,
            memo: None,
            lines: vec![line(cents)],
        };
        crate::workflows::create_purchase_order(&pool, po).await.unwrap();
    }
    let summary = party_summary(&pool, "vendor", vendor.id).await.unwrap();
    let report = crate::accountant_reports::open_purchase_orders(&pool, later()).await.unwrap();
    let rows = serde_json::to_value(&report).unwrap()["rows"].clone();
    let total = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["cells"][0] == serde_json::json!(format!("Total {}", vendor.name)))
        .map(|r| r["cells"].as_array().unwrap().iter().rev().find(|c| c.is_i64()).unwrap().as_i64().unwrap())
        .expect("a subtotal for the vendor");
    assert_eq!((summary.open_po_count, summary.open_po_cents), (2, total));
    assert_eq!(total, 20_000);
}

#[tokio::test]
async fn item_figures_match_inventory_valuation_and_stock_status() {
    let pool = crate::harness::job_costing::company().await;
    let oak: i64 = sqlx::query_scalar("SELECT id FROM products_services WHERE name = 'Oak Panels'").fetch_one(&pool).await.unwrap();
    let item: i64 = sqlx::query_scalar("SELECT id FROM inventory_items WHERE product_id = ?1").bind(oak).fetch_one(&pool).await.unwrap();
    let today = later();
    let summary = item_summary(&pool, oak, today).await.unwrap();
    let valuation = crate::report_center::inventory_valuation(&pool, today).await.unwrap();
    let row = valuation.rows.iter().find(|r| r.item_id == item).unwrap();
    assert!(summary.tracked);
    assert_eq!(
        (summary.qty_on_hand_e4, summary.average_cost_cents, summary.asset_value_cents),
        (row.qty_on_hand_e4, row.average_cost_cents, row.asset_value_cents)
    );
    assert!(summary.qty_on_hand_e4 > 0);
    let status = crate::report_center::stock_status(&pool, today, today).await.unwrap();
    let s = status.rows.iter().find(|r| r.item_id == item).unwrap();
    assert_eq!((summary.qty_on_order_e4, summary.next_delivery), (s.qty_on_order_e4, s.next_delivery));
    // A service isn't tracked: just its price.
    let service: (i64, i64) = sqlx::query_as(
        "SELECT p.id, p.rate_cents FROM products_services p WHERE NOT EXISTS (SELECT 1 FROM inventory_items i WHERE i.product_id = p.id) LIMIT 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    let plain = item_summary(&pool, service.0, today).await.unwrap();
    assert_eq!(plain, ItemSummary { sales_price_cents: service.1, ..ItemSummary::default() });
    // A service can't take a reorder point, but can name a preferred vendor.
    assert!(set_item_purchasing(&pool, service.0, Some(10_000), None).await.unwrap_err().starts_with("ITEM_REORDER_UNTRACKED"));
}

#[tokio::test]
async fn the_emails_list_has_every_form_sent_to_the_customer() {
    use crate::email_log::{record, sent_to_customer, Method};
    use crate::forms::FormKind;
    let pool = crate::harness::job_costing::company().await;
    let (maple, lakeview): (i64, i64) = (
        sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Kestrel Homes:Maple St Addition'").fetch_one(&pool).await.unwrap(),
        sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Kestrel Homes:Lakeview Remodel'").fetch_one(&pool).await.unwrap(),
    );
    let (invoice, number): (i64, i64) =
        sqlx::query_as("SELECT id, invoice_number FROM invoices WHERE customer_id = ?1 LIMIT 1").bind(maple).fetch_one(&pool).await.unwrap();
    let estimate: i64 = sqlx::query_scalar("SELECT id FROM estimates WHERE customer_id = ?1 LIMIT 1").bind(maple).fetch_one(&pool).await.unwrap();
    let to = ["site@kestrel.example".to_string()];
    record(&pool, FormKind::Invoice, invoice, &to, Method::Smtp, "Invoice").await.unwrap();
    record(&pool, FormKind::Estimate, estimate, &to, Method::MailApp, "Estimate").await.unwrap();
    record(&pool, FormKind::Statement, maple, &to, Method::Smtp, "Statement").await.unwrap();
    record(&pool, FormKind::Statement, lakeview, &to, Method::Smtp, "Someone else's statement").await.unwrap();

    let sent = sent_to_customer(&pool, maple).await.unwrap();
    let got: Vec<(&str, Option<i64>)> = sent.iter().map(|s| (s.form_type.as_str(), s.number)).collect();
    assert_eq!(got[0], ("STATEMENT", None), "newest first");
    assert_eq!(got.len(), 3, "only Maple St's own forms: {got:?}");
    assert!(got.contains(&("INVOICE", Some(number))));
    assert_eq!(sent_to_customer(&pool, lakeview).await.unwrap().len(), 1);
}

#[tokio::test]
async fn notes_save_from_the_panel_and_are_audited() {
    let pool = crate::harness::job_costing::company().await;
    let vendor = crate::expenses::list_vendors(&pool).await.unwrap().into_iter().next().unwrap();
    set_notes(&pool, "vendor", vendor.id, "  11/08/2026: call about the oak order  ").await.unwrap();
    let notes: Option<String> = sqlx::query_scalar("SELECT notes FROM vendors WHERE id = ?1").bind(vendor.id).fetch_one(&pool).await.unwrap();
    assert_eq!(notes.as_deref(), Some("11/08/2026: call about the oak order"));
    set_notes(&pool, "vendor", vendor.id, " ").await.unwrap();
    let notes: Option<String> = sqlx::query_scalar("SELECT notes FROM vendors WHERE id = ?1").bind(vendor.id).fetch_one(&pool).await.unwrap();
    assert_eq!(notes, None);
    assert!(set_notes(&pool, "vendor", 99_999, "x").await.unwrap_err().starts_with("PARTY_NOT_FOUND"));
    assert!(set_notes(&pool, "item", vendor.id, "x").await.unwrap_err().starts_with("PARTY_KIND"));
    assert!(crate::audit_chain::verify(&pool).await.unwrap().problems.is_empty());
}

#[tokio::test]
async fn reorder_point_and_preferred_vendor_save_and_show() {
    let pool = crate::harness::job_costing::company().await;
    let oak: i64 = sqlx::query_scalar("SELECT id FROM products_services WHERE name = 'Oak Panels'").fetch_one(&pool).await.unwrap();
    let vendor = crate::expenses::list_vendors(&pool).await.unwrap().into_iter().find(|v| v.is_active).unwrap();
    let today = later();
    let on_hand = item_summary(&pool, oak, today).await.unwrap().qty_on_hand_e4;

    set_item_purchasing(&pool, oak, Some(on_hand + 10_000), Some(vendor.id)).await.unwrap();
    let s = item_summary(&pool, oak, today).await.unwrap();
    assert_eq!((s.reorder_point_e4, s.at_reorder_point), (Some(on_hand + 10_000), true));
    assert_eq!((s.preferred_vendor_id, s.preferred_vendor_name.as_deref()), (Some(vendor.id), Some(vendor.name.as_str())));

    set_item_purchasing(&pool, oak, Some(on_hand - 10_000), Some(vendor.id)).await.unwrap();
    assert!(!item_summary(&pool, oak, today).await.unwrap().at_reorder_point, "above the point");
    set_item_purchasing(&pool, oak, None, None).await.unwrap();
    let cleared = item_summary(&pool, oak, today).await.unwrap();
    assert_eq!((cleared.reorder_point_e4, cleared.preferred_vendor_id, cleared.at_reorder_point), (None, None, false));

    assert!(set_item_purchasing(&pool, oak, Some(-1), None).await.unwrap_err().starts_with("ITEM_REORDER_NEGATIVE"));
    assert!(set_item_purchasing(&pool, oak, None, Some(99_999)).await.unwrap_err().starts_with("PARTY_NOT_FOUND"));
    assert!(set_item_purchasing(&pool, 99_999, None, None).await.unwrap_err().starts_with("ITEM_NOT_FOUND"));
    assert!(crate::audit_chain::verify(&pool).await.unwrap().problems.is_empty());
}
