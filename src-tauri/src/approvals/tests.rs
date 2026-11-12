use super::*;
use crate::db::test_pool;
use serde_json::json;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn bill(pool: &SqlitePool, vendor: &str, cents: i64) -> Value {
    let rent = account(pool, "6300").await;
    json!({ "vendorName": vendor, "billNumber": "B-7", "billDate": "2026-03-02", "dueDate": "2026-04-01", "memo": null, "totalCents": cents,
            "lines": [{ "accountId": rent, "amountCents": cents, "memo": "March" }] })
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn rules_hold_matching_bills_until_approved() {
    let pool = test_pool().await;
    let lessor = crate::expenses::create_vendor(&pool, "Harborview Leasing", None).await.unwrap();
    assert!(check_not_held(&pool, Kind::Bill, "Harborview Leasing", 500_000).await.is_ok(), "no rules: nothing is held");

    add_rule(&pool, &RuleInput { kind: Kind::Bill, min_total_cents: 100_000, vendor_id: None }).await.unwrap();
    add_rule(&pool, &RuleInput { kind: Kind::Bill, min_total_cents: 0, vendor_id: Some(lessor.id) }).await.unwrap();
    assert!(add_rule(&pool, &RuleInput { kind: Kind::Bill, min_total_cents: -1, vendor_id: None }).await.unwrap_err().starts_with("APPROVAL_RULE_AMOUNT"));
    assert!(check_not_held(&pool, Kind::Bill, "Corner Store", 99_999).await.is_ok());
    let held = check_not_held(&pool, Kind::Bill, "Corner Store", 100_000).await.unwrap_err();
    assert!(held.starts_with("APPROVAL_REQUIRED") && held.contains("$1,000.00"), "{held}");
    assert!(check_not_held(&pool, Kind::Bill, "harborview leasing", 50).await.unwrap_err().contains("from Harborview Leasing"));
    assert!(check_not_held(&pool, Kind::PurchaseOrder, "Corner Store", 900_000).await.is_ok(), "bill rules don't hold POs");

    // Submitted: nothing is posted, so A/P and Pay Bills don't see it.
    let request = submit(&pool, Kind::Bill, &bill(&pool, "Harborview Leasing", 250_000).await, "Sam").await.unwrap();
    assert_eq!(
        (request.status.as_str(), request.total_cents, request.reference.as_deref(), request.requested_by.as_str()),
        ("PENDING", 250_000, Some("B-7"), "Sam")
    );
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM bills").await, 0);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM journal_entries").await, 0);
    assert!(crate::expenses::list_bills(&pool).await.unwrap().is_empty());

    // Approving posts the bill exactly as entered.
    let approved = approve(&pool, request.id, "Alex").await.unwrap();
    assert_eq!((approved.status.as_str(), approved.decided_by.as_deref()), ("APPROVED", Some("Alex")));
    let bills = crate::expenses::list_bills(&pool).await.unwrap();
    assert_eq!(bills.len(), 1);
    assert_eq!((Some(bills[0].id), bills[0].total_cents, bills[0].vendor_name.as_str()), (approved.bill_id, 250_000, "Harborview Leasing"));
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM journal_entries WHERE status = 'POSTED'").await, 1);
    assert!(approve(&pool, request.id, "Alex").await.unwrap_err().starts_with("APPROVAL_DECIDED"), "approved once only");
    assert!(reject(&pool, request.id, "Alex", "late").await.unwrap_err().starts_with("APPROVAL_DECIDED"));
}

#[tokio::test]
async fn rejections_are_kept_with_a_note_and_record_nothing() {
    let pool = test_pool().await;
    let request = submit(&pool, Kind::Bill, &bill(&pool, "Corner Store", 12_000).await, "Sam").await.unwrap();
    assert!(reject(&pool, request.id, "Alex", "   ").await.unwrap_err().starts_with("APPROVAL_NOTE"));
    let rejected = reject(&pool, request.id, "Alex", "Duplicate of B-6").await.unwrap();
    assert_eq!((rejected.status.as_str(), rejected.note.as_deref(), rejected.bill_id), ("REJECTED", Some("Duplicate of B-6"), None));
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM bills").await, 0);
    assert!(approve(&pool, request.id, "Alex").await.unwrap_err().starts_with("APPROVAL_DECIDED"));
    let listed = list(&pool, Some("REJECTED")).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(list(&pool, Some("PENDING")).await.unwrap().is_empty());
}

#[tokio::test]
async fn bad_forms_are_refused_and_a_failed_approval_stays_pending() {
    let pool = test_pool().await;
    let mut unbalanced = bill(&pool, "Corner Store", 12_000).await;
    unbalanced["totalCents"] = json!(13_000);
    assert!(submit(&pool, Kind::Bill, &unbalanced, "Sam").await.unwrap_err().starts_with("BILL_"), "validated like the form");
    assert!(submit(&pool, Kind::Bill, &json!({ "nope": 1 }), "Sam").await.unwrap_err().starts_with("APPROVAL_BAD_INPUT"));

    // Books closed through the bill's date after it was submitted: approving fails, nothing changes.
    let request = submit(&pool, Kind::Bill, &bill(&pool, "Corner Store", 12_000).await, "Sam").await.unwrap();
    let lock = crate::closing::ClosingInput { lock_date: NaiveDate::from_ymd_opt(2026, 3, 31), current_passcode: None, new_passcode: None };
    crate::closing::set_closing_date(&pool, &lock).await.unwrap();
    assert!(approve(&pool, request.id, "Alex").await.is_err());
    assert_eq!(get(&pool, request.id).await.unwrap().status, "PENDING");
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM bills").await, 0);
}

#[tokio::test]
async fn purchase_orders_wait_out_of_open_orders_until_approved() {
    let pool = test_pool().await;
    crate::preferences::set(&pool, crate::preferences::tax_time::INVENTORY, "true").await.unwrap();
    add_rule(&pool, &RuleInput { kind: Kind::PurchaseOrder, min_total_cents: 50_000, vendor_id: None }).await.unwrap();
    let supplies = account(&pool, "6300").await;
    let po = json!({ "vendorName": "Ridgeline Lumber", "poDate": "2026-03-02", "memo": "Deck", "lines": [{ "accountId": supplies, "description": "Boards", "amountCents": 80_000 }] });
    let typed: PurchaseOrderInput = serde_json::from_value(po.clone()).unwrap();
    assert!(check_not_held(&pool, Kind::PurchaseOrder, &typed.vendor_name, crate::workflows::validate_purchase_order(&typed).unwrap())
        .await
        .unwrap_err()
        .starts_with("APPROVAL_REQUIRED"));

    let request = submit(&pool, Kind::PurchaseOrder, &po, "Sam").await.unwrap();
    assert!(crate::workflows::list_purchase_orders(&pool).await.unwrap().is_empty(), "not an open order yet");
    let approved = approve(&pool, request.id, "Alex").await.unwrap();
    let orders = crate::workflows::list_purchase_orders(&pool).await.unwrap();
    assert_eq!((orders.len(), Some(orders[0].id), orders[0].total_cents.get()), (1, approved.purchase_order_id, 80_000));
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM journal_entries").await, 0, "a purchase order posts nothing");
}

/// An open purchase order for $800 and the bill Create Bill would make from it.
async fn order_and_bill(pool: &SqlitePool) -> (i64, BillInput) {
    crate::preferences::set(pool, crate::preferences::tax_time::INVENTORY, "true").await.unwrap();
    let supplies = account(pool, "6300").await;
    let po: PurchaseOrderInput =
        serde_json::from_value(json!({ "vendorName": "Ridgeline Lumber", "poDate": "2026-03-02", "memo": "Deck", "lines": [{ "accountId": supplies, "description": "Boards", "amountCents": 80_000 }] }))
            .unwrap();
    let id = crate::workflows::create_purchase_order(pool, po).await.unwrap();
    let d = |day| NaiveDate::from_ymd_opt(2026, 3, day).unwrap();
    let bill = crate::workflows::purchase_order_bill(pool, id, d(9), d(31), Some("RL-41".into())).await.unwrap();
    (id, bill)
}

async fn order_state(pool: &SqlitePool, id: i64) -> (String, Option<i64>, bool) {
    let o = crate::workflows::list_purchase_orders(pool).await.unwrap().into_iter().find(|o| o.id == id).unwrap();
    (o.status, o.bill_id, o.awaiting_approval)
}

#[tokio::test]
async fn approving_a_held_bill_from_a_purchase_order_posts_it_and_marks_the_order_billed() {
    let pool = test_pool().await;
    add_rule(&pool, &RuleInput { kind: Kind::Bill, min_total_cents: 50_000, vendor_id: None }).await.unwrap();
    let (po, bill) = order_and_bill(&pool).await;
    assert!(check_not_held(&pool, Kind::Bill, &bill.vendor_name, bill.total_cents.get()).await.unwrap_err().starts_with("APPROVAL_REQUIRED"));

    let request = submit_purchase_order_bill(&pool, po, &bill, "Sam").await.unwrap();
    assert_eq!(
        (request.status.as_str(), request.total_cents, request.reference.as_deref(), request.from_purchase_order_id),
        ("PENDING", 80_000, Some("RL-41"), Some(po))
    );
    assert_eq!(request.from_po_number, Some(1));
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM bills").await, 0, "nothing posts while it waits");
    assert_eq!(order_state(&pool, po).await, ("OPEN".into(), None, true));

    // While it waits, the order can't be billed again, changed or deleted.
    assert!(submit_purchase_order_bill(&pool, po, &bill, "Sam").await.unwrap_err().starts_with("PO_AWAITING_APPROVAL"));
    assert!(crate::workflows::post_purchase_order_bill(&pool, po, &bill).await.unwrap_err().starts_with("PO_AWAITING_APPROVAL"));
    assert!(crate::workflows::delete_purchase_order(&pool, po).await.unwrap_err().starts_with("PO_AWAITING_APPROVAL"));

    let approved = approve(&pool, request.id, "Alex").await.unwrap();
    assert_eq!(approved.status, "APPROVED");
    let bills = crate::expenses::list_bills(&pool).await.unwrap();
    assert_eq!((bills.len(), Some(bills[0].id), bills[0].total_cents), (1, approved.bill_id, 80_000));
    assert_eq!(order_state(&pool, po).await, ("BILLED".into(), approved.bill_id, false), "the order is billed with the approved bill's id");
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM journal_entries WHERE status = 'POSTED'").await, 1);
}

#[tokio::test]
async fn rejecting_a_held_bill_from_a_purchase_order_leaves_the_order_open() {
    let pool = test_pool().await;
    add_rule(&pool, &RuleInput { kind: Kind::Bill, min_total_cents: 0, vendor_id: None }).await.unwrap();
    let (po, bill) = order_and_bill(&pool).await;
    let request = submit_purchase_order_bill(&pool, po, &bill, "Sam").await.unwrap();
    let rejected = reject(&pool, request.id, "Alex", "Wrong price on the boards").await.unwrap();
    assert_eq!((rejected.status.as_str(), rejected.bill_id), ("REJECTED", None));
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM bills").await, 0);
    assert_eq!(order_state(&pool, po).await, ("OPEN".into(), None, false));

    // The order is open again for a fresh bill, which can be submitted and approved.
    let again = submit_purchase_order_bill(&pool, po, &bill, "Sam").await.unwrap();
    let approved = approve(&pool, again.id, "Alex").await.unwrap();
    assert_eq!(order_state(&pool, po).await, ("BILLED".into(), approved.bill_id, false));
}

#[tokio::test]
async fn a_held_bill_whose_order_was_closed_meanwhile_stays_pending() {
    let pool = test_pool().await;
    let (po, bill) = order_and_bill(&pool).await;
    let request = submit_purchase_order_bill(&pool, po, &bill, "Sam").await.unwrap();
    crate::workflows::set_purchase_order_closed(&pool, po, true).await.unwrap();
    assert!(approve(&pool, request.id, "Alex").await.unwrap_err().starts_with("PO_NOT_OPEN"));
    assert_eq!(get(&pool, request.id).await.unwrap().status, "PENDING");
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM bills").await, 0, "nothing posted");
    assert!(submit_purchase_order_bill(&pool, po, &bill, "Sam").await.unwrap_err().starts_with("PO_NOT_OPEN"));
}
