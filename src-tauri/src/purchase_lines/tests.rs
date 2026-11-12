use super::*;
use crate::db::test_pool;
use crate::money::Cents;
use chrono::NaiveDate;
use serde_json::json;
use sqlx::SqlitePool;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1")
        .bind(account)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn payable(pool: &SqlitePool) -> i64 {
    let ap: i64 =
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'ACCOUNTS_PAYABLE' AND is_system = 1").fetch_one(pool).await.unwrap();
    balance(pool, ap).await
}

fn line(after: usize, kind: PurchaseLineKind, percent: Option<i64>, amount: Option<i64>, account_id: i64) -> PurchaseFormLineInput {
    PurchaseFormLineInput {
        after_line: after,
        kind,
        description: String::new(),
        product_service_id: None,
        percent_ppm: percent,
        amount_cents: amount.map(Cents),
        account_id,
        class_id: None,
    }
}

fn code(err: String) -> String {
    err.split(':').next().unwrap_or_default().to_string()
}

#[test]
fn lay_out_works_out_each_line_and_the_total() {
    use PurchaseLineKind::{Charge, Discount};
    // 10% off 100.00, then a 2.5% charge on 33.33 (0.83325 rounds to 0.83), then 5.00 off.
    let form = [line(1, Discount, Some(100_000), None, 9), line(2, Charge, Some(25_000), None, 9), line(3, Discount, None, Some(500), 9)];
    let (amounts, total) = lay_out(&[10_000, 3_333, 2_000], &form, "BILL").unwrap();
    assert_eq!(amounts, [1_000, 83, 500]);
    assert_eq!(total, 10_000 + 3_333 + 2_000 - 1_000 + 83 - 500);
    // Half a cent rounds away from zero: 0.5% of 1.00 is half a cent.
    assert_eq!(lay_out(&[100], &[line(1, Charge, Some(5_000), None, 9)], "BILL").unwrap(), (vec![1], 101));

    let refused = |splits: &[i64], form: &[PurchaseFormLineInput]| code(lay_out(splits, form, "BILL").unwrap_err());
    assert_eq!(refused(&[1_000], &[line(2, Discount, None, Some(1), 9)]), "BILL_FORM_LINE_PLACE");
    assert_eq!(refused(&[1_000], &[line(0, Discount, None, Some(1), 9)]), "BILL_FORM_LINE_PLACE");
    assert_eq!(refused(&[1_000], &[line(1, Discount, None, Some(1), 9), line(1, Charge, None, Some(1), 9)]), "BILL_FORM_LINE_NEEDS_BASE");
    assert_eq!(refused(&[1_000], &[line(1, Discount, Some(10_000), Some(1), 9)]), "BILL_FORM_LINE_AMOUNT");
    assert_eq!(refused(&[1_000], &[line(1, Discount, None, None, 9)]), "BILL_FORM_LINE_AMOUNT");
    assert_eq!(refused(&[1_000], &[line(1, Charge, None, Some(0), 9)]), "BILL_FORM_LINE_AMOUNT");
    assert_eq!(refused(&[1_000], &[line(1, Discount, Some(1_000_001), None, 9)]), "BILL_FORM_LINE_PERCENT");
    assert_eq!(refused(&[10], &[line(1, Discount, Some(1), None, 9)]), "BILL_FORM_LINE_ZERO");
    assert_eq!(refused(&[1_000], &[line(1, Discount, None, Some(1_001), 9)]), "BILL_DISCOUNT_TOO_LARGE");
    assert_eq!(refused(&[1_000], &[line(1, Discount, Some(1_000_000), None, 9)]), "BILL_TOTAL_NOT_POSITIVE");
}

#[tokio::test]
async fn a_bill_posts_its_discounts_and_charges_and_keeps_them() {
    let pool = test_pool().await;
    let (rent, utilities, services) = (account(&pool, "6400").await, account(&pool, "6500").await, account(&pool, "4100").await);
    let bill = |total: i64| -> crate::expenses::BillInput {
        serde_json::from_value(json!({
            "vendorName": "Supplier", "billNumber": "B-1", "billDate": "2026-03-01", "dueDate": "2026-03-31", "memo": null, "totalCents": total,
            "lines": [{ "accountId": rent, "amountCents": 10_000, "memo": "Work" }, { "accountId": utilities, "amountCents": 5_000, "memo": "Power" }],
            "formLines": [
                { "afterLine": 1, "kind": "DISCOUNT", "percentPpm": 100_000, "accountId": services, "description": "Early payment" },
                { "afterLine": 2, "kind": "CHARGE", "amountCents": 500, "accountId": utilities }
            ]
        }))
        .unwrap()
    };
    // The total is the split lines less the discount plus the charge.
    assert_eq!(code(crate::expenses::post_vendor_bill(&pool, bill(15_000)).await.unwrap_err()), "BILL_SPLIT_MISMATCH");
    let posted = crate::expenses::post_vendor_bill(&pool, bill(14_500)).await.unwrap();
    assert_eq!(posted.total_cents, Cents(14_500));
    assert_eq!(payable(&pool).await, -14_500);
    assert_eq!(balance(&pool, rent).await, 10_000);
    assert_eq!(balance(&pool, utilities).await, 5_500, "the charge posts with the split lines");
    assert_eq!(balance(&pool, services).await, -1_000, "the discount is credited to its account");

    let stored = lines_of(&pool, PurchaseDoc::Bill, posted.id).await.unwrap();
    assert_eq!(
        stored.iter().map(|l| (l.after_line, l.kind.as_str(), l.description.as_str(), l.amount_cents.get())).collect::<Vec<_>>(),
        [(1, "DISCOUNT", "Early payment", 1_000), (2, "CHARGE", "Charge", 500)]
    );
    assert_eq!(stored[0].percent_ppm, Some(100_000));
}

#[tokio::test]
async fn checks_card_credits_and_vendor_credits_post_each_line_on_its_side() {
    let pool = test_pool().await;
    let (checking, card, rent, services) =
        (account(&pool, "1010").await, account(&pool, "2100").await, account(&pool, "6400").await, account(&pool, "4100").await);
    let expense = |kind: &str, from: i64| -> crate::expenses::ExpenseInput {
        serde_json::from_value(json!({
            "kind": kind, "payeeName": "Supplier", "paymentAccountId": from, "expenseDate": "2026-03-02", "checkNumber": null, "paymentMethod": null, "memo": null,
            "lines": [{ "accountId": rent, "amountCents": 20_000, "memo": null }],
            "formLines": [{ "afterLine": 1, "kind": "DISCOUNT", "amountCents": 2_000, "accountId": services }]
        }))
        .unwrap()
    };
    let check = crate::expenses::post_expense(&pool, expense("CHECK", checking)).await.unwrap();
    assert_eq!(check.total_cents, Cents(18_000));
    assert_eq!((balance(&pool, checking).await, balance(&pool, rent).await, balance(&pool, services).await), (-18_000, 20_000, -2_000));
    assert_eq!(lines_of(&pool, PurchaseDoc::Expense, check.id).await.unwrap().len(), 1);

    // A card credit posts the other way round, its discount too.
    let credit = crate::expenses::post_expense(&pool, expense("CARD_CREDIT", card)).await.unwrap();
    assert_eq!(credit.total_cents, Cents(18_000));
    assert_eq!((balance(&pool, card).await, balance(&pool, rent).await, balance(&pool, services).await), (18_000, 0, 0));

    // A vendor credit with a 10% charge: A/P is debited the whole 110.00.
    let vc: crate::credits::VendorCreditInput = serde_json::from_value(json!({
        "vendorName": "Supplier", "reference": "VC-1", "creditDate": "2026-03-03", "memo": null,
        "lines": [{ "accountId": rent, "amountCents": 10_000, "memo": null }],
        "formLines": [{ "afterLine": 1, "kind": "CHARGE", "percentPpm": 100_000, "accountId": rent }]
    }))
    .unwrap();
    let saved = crate::credits::create_vendor_credit(&pool, vc).await.unwrap();
    assert_eq!(saved.total_cents, Cents(11_000));
    assert_eq!(payable(&pool).await, 11_000);
    assert_eq!(balance(&pool, rent).await, -11_000);
}

/// Preferences › Checking › "Change check date when a check is printed": the re-dated check is
/// posted again with its discount and charge lines, at the same total.
#[tokio::test]
async fn a_queued_check_redated_when_printed_keeps_its_discount_and_charge_lines() {
    let pool = test_pool().await;
    let (checking, rent, utilities, services) =
        (account(&pool, "1010").await, account(&pool, "6400").await, account(&pool, "6500").await, account(&pool, "4100").await);
    crate::preferences::set(&pool, crate::preferences::tax_time::DATE_CHECKS_PRINTED, "true").await.unwrap();
    let input: crate::expenses::ExpenseInput = serde_json::from_value(json!({
        "kind": "CHECK", "payeeName": "Supplier", "paymentAccountId": checking, "expenseDate": "2026-03-02", "checkNumber": null, "paymentMethod": null, "memo": null,
        "lines": [{ "accountId": rent, "amountCents": 20_000, "memo": null }, { "accountId": utilities, "amountCents": 5_000, "memo": null }],
        "formLines": [
            { "afterLine": 1, "kind": "DISCOUNT", "percentPpm": 100_000, "accountId": services, "description": "Early pay" },
            { "afterLine": 2, "kind": "CHARGE", "amountCents": 750, "accountId": utilities, "description": "Delivery" }
        ]
    }))
    .unwrap();
    let queued = crate::print_queue::post_expense(&pool, input, true).await.unwrap();
    // 200.00 - 20.00 + 50.00 + 7.50.
    assert_eq!(queued.total_cents, Cents(23_750));
    let before = lines_of(&pool, PurchaseDoc::Expense, queued.id).await.unwrap();
    let printed = vec![crate::print_queue::PrintedCheck { expense_id: queued.id, check_number: "601".into(), bill_check_id: None }];
    crate::print_queue::mark_checks_printed_on(&pool, printed, NaiveDate::from_ymd_opt(2026, 3, 9).unwrap()).await.unwrap();

    let (id, date, total): (i64, String, i64) =
        sqlx::query_as("SELECT id, expense_date, total_cents FROM expenses WHERE check_number = '601' AND is_voided = 0").fetch_one(&pool).await.unwrap();
    assert_ne!(id, queued.id, "the queued check is replaced");
    assert_eq!((date.as_str(), total), ("2026-03-09", 23_750));
    let after = lines_of(&pool, PurchaseDoc::Expense, id).await.unwrap();
    let shape = |v: &[PurchaseFormLineView]| {
        v.iter().map(|l| (l.after_line, l.kind.clone(), l.description.clone(), l.percent_ppm, l.amount_cents, l.account_id)).collect::<Vec<_>>()
    };
    assert_eq!(shape(&after), shape(&before));
    let splits: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM expense_lines WHERE expense_id = ?1").bind(id).fetch_one(&pool).await.unwrap();
    assert_eq!(splits, 2, "the form lines are not added to the split lines");
    // Only the replacement counts: checking paid 237.50 once, the discount and charge posted once.
    assert_eq!(
        (balance(&pool, checking).await, balance(&pool, rent).await, balance(&pool, utilities).await, balance(&pool, services).await),
        (-23_750, 20_000, 5_750, -2_000)
    );
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}

#[tokio::test]
async fn a_purchase_orders_lines_carry_to_its_bill() {
    let pool = test_pool().await;
    crate::preferences::set(&pool, crate::preferences::tax_time::INVENTORY, "true").await.unwrap();
    let (rent, utilities) = (account(&pool, "6400").await, account(&pool, "6500").await);
    let order: crate::workflows::PurchaseOrderInput = serde_json::from_value(json!({
        "vendorName": "Supplier", "poDate": "2026-04-01", "memo": null,
        "lines": [{ "accountId": rent, "description": "Paper", "amountCents": 30_000 }],
        "formLines": [{ "afterLine": 1, "kind": "CHARGE", "percentPpm": 50_000, "accountId": utilities, "description": "Freight" }]
    }))
    .unwrap();
    let id = crate::workflows::create_purchase_order(&pool, order).await.unwrap();
    let po = crate::workflows::list_purchase_orders(&pool).await.unwrap().into_iter().find(|o| o.id == id).unwrap();
    assert_eq!(po.total_cents.get(), 31_500);
    let d = NaiveDate::from_ymd_opt(2026, 4, 10).unwrap();
    let bill_id = crate::workflows::bill_purchase_order(&pool, id, d, d, None).await.unwrap();
    let carried = lines_of(&pool, PurchaseDoc::Bill, bill_id).await.unwrap();
    assert_eq!(carried.iter().map(|l| (l.description.as_str(), l.amount_cents.get(), l.percent_ppm)).collect::<Vec<_>>(), [("Freight", 1_500, Some(50_000))]);
    assert_eq!(payable(&pool).await, -31_500);
    assert_eq!(balance(&pool, utilities).await, 1_500);
}

#[tokio::test]
async fn items_name_their_purchase_account_and_lines_their_item() {
    let pool = test_pool().await;
    let (rent, services, checking) = (account(&pool, "6400").await, account(&pool, "4100").await, account(&pool, "1010").await);
    let input = |name: &str| crate::sales::ProductInput {
        name: name.into(),
        description: None,
        rate_cents: Cents::ZERO,
        income_account_id: services,
        is_taxable: false,
    };
    let kind = |t: &str, acct: Option<i64>| crate::sales::ItemKind { item_type: t.into(), percent_ppm: None, expense_account_id: acct };
    let freight = crate::sales::create_item(&pool, input("Freight"), kind("OTHER_CHARGE", Some(rent))).await.unwrap();
    assert_eq!(freight.expense_account_id, Some(rent));
    assert_eq!(code(crate::sales::create_item(&pool, input("Widget"), kind("ITEM", Some(rent))).await.unwrap_err()), "ITEM_BAD_EXPENSE_ACCOUNT");
    assert_eq!(code(crate::sales::create_item(&pool, input("Bank fee"), kind("OTHER_CHARGE", Some(checking))).await.unwrap_err()), "ITEM_BAD_EXPENSE_ACCOUNT");
    let changed = crate::parties::update_item(&pool, freight.id, input("Freight"), Some(kind("OTHER_CHARGE", None))).await.unwrap();
    assert_eq!(changed.expense_account_id, None);

    // A discount line can't name a charge item, nor post to a bank account.
    let bill = |product: i64, account_id: i64| -> crate::expenses::BillInput {
        serde_json::from_value(json!({
            "vendorName": "Supplier", "billNumber": null, "billDate": "2026-03-01", "dueDate": "2026-03-31", "memo": null, "totalCents": 9_000,
            "lines": [{ "accountId": rent, "amountCents": 10_000, "memo": null }],
            "formLines": [{ "afterLine": 1, "kind": "DISCOUNT", "amountCents": 1_000, "accountId": account_id, "productServiceId": product }]
        }))
        .unwrap()
    };
    assert_eq!(code(crate::expenses::post_vendor_bill(&pool, bill(freight.id, services)).await.unwrap_err()), "BILL_FORM_LINE_ITEM");
    let discount = crate::sales::create_item(&pool, input("Early pay"), kind("DISCOUNT", Some(services))).await.unwrap();
    assert_eq!(code(crate::expenses::post_vendor_bill(&pool, bill(discount.id, checking)).await.unwrap_err()), "BILL_FORM_LINE_ACCOUNT");
    crate::expenses::post_vendor_bill(&pool, bill(discount.id, services)).await.unwrap();
}

#[test]
fn a_fixed_discount_too_big_for_a_part_shipment_waits() {
    use PurchaseLineKind::{Charge, Discount};
    let from = |source: usize, line: PurchaseFormLineInput| CarriedLine { line, source };
    let form = vec![
        from(0, line(1, Discount, None, Some(5_000), 9)),
        from(1, line(2, Discount, Some(100_000), None, 9)),
        from(2, line(3, Charge, None, Some(9_000), 9)),
    ];
    let kept = fit_carried(form, &[3_000, 3_000, 2_000]);
    assert_eq!(kept.iter().map(|l| l.after_line).collect::<Vec<_>>(), [2, 3], "only the fixed discount over its line is held back");

    // Two receipt lines from one order line: the fixed discount goes after the first that can take
    // it, once; a percent goes after both.
    let twice = |a: usize, b: usize| {
        vec![
            from(0, line(a, Discount, None, Some(5_000), 9)),
            from(1, line(a, Discount, Some(100_000), None, 9)),
            from(0, line(b, Discount, None, Some(5_000), 9)),
            from(1, line(b, Discount, Some(100_000), None, 9)),
        ]
    };
    let placed = |kept: Vec<PurchaseFormLineInput>| kept.iter().map(|l| (l.after_line, l.amount_cents.is_some())).collect::<Vec<_>>();
    assert_eq!(placed(fit_carried(twice(1, 2), &[3_000, 8_000])), [(1, false), (2, true), (2, false)], "too big for line 1, so it goes after line 2");
    assert_eq!(placed(fit_carried(twice(1, 2), &[6_000, 8_000])), [(1, true), (1, false), (2, false)], "once, after the first");
    assert_eq!(placed(fit_carried(twice(1, 2), &[3_000, 4_000])), [(1, false), (2, false)], "neither can take it: it waits");
}

#[tokio::test]
async fn editing_a_bill_reposts_its_discount_from_the_items_purchase_account_once() {
    let pool = test_pool().await;
    let (rent, services, office) = (account(&pool, "6400").await, account(&pool, "4100").await, account(&pool, "6500").await);
    // A Discount item whose income account is used on sales forms and whose purchase account,
    // here an expense account, is used on bills.
    let rebate = crate::sales::create_item(
        &pool,
        crate::sales::ProductInput { name: "Vendor rebate".into(), description: None, rate_cents: Cents::ZERO, income_account_id: services, is_taxable: false },
        crate::sales::ItemKind { item_type: "DISCOUNT".into(), percent_ppm: Some(50_000), expense_account_id: Some(office) },
    )
    .await
    .unwrap();
    let bill = |amount: i64, total: i64| -> crate::expenses::BillInput {
        serde_json::from_value(json!({
            "vendorName": "Supplier", "billNumber": "B-7", "billDate": "2026-05-01", "dueDate": "2026-05-31", "memo": null, "totalCents": total,
            "lines": [{ "accountId": rent, "amountCents": amount, "memo": null }],
            "formLines": [{ "afterLine": 1, "kind": "DISCOUNT", "percentPpm": 50_000, "accountId": rebate.expense_account_id.unwrap(), "productServiceId": rebate.id }]
        }))
        .unwrap()
    };
    // 200.00 less 5% = 190.00.
    let first = crate::expenses::post_vendor_bill(&pool, bill(20_000, 19_000)).await.unwrap();
    assert_eq!((payable(&pool).await, balance(&pool, office).await), (-19_000, -1_000), "the discount is credited to the item's purchase account");
    assert_eq!(balance(&pool, services).await, 0, "not to its income account");

    // Edited to 300.00 less 5% = 285.00: the old bill's entry is voided, so each account counts once.
    let edited = crate::edits::replace_bill(&pool, first.id, bill(30_000, 28_500)).await.unwrap();
    assert_ne!(edited, first.id);
    assert_eq!((payable(&pool).await, balance(&pool, rent).await, balance(&pool, office).await), (-28_500, 30_000, -1_500));
    let stored = lines_of(&pool, PurchaseDoc::Bill, edited).await.unwrap();
    assert_eq!(
        stored.iter().map(|l| (l.kind.as_str(), l.product_service_id, l.percent_ppm, l.amount_cents.get())).collect::<Vec<_>>(),
        [("DISCOUNT", Some(rebate.id), Some(50_000), 1_500)]
    );
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}
