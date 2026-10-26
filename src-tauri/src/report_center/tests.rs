use super::*;
use crate::db::test_pool;

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn aging_is_as_of_a_date_and_summaries_and_ledger_add_up() {
    let pool = test_pool().await;
    let (checking, services, supplies) = (account(&pool, "1010").await, account(&pool, "4100").await, account(&pool, "6250").await);
    let invoice = |customer: &str, issue: &str, due: &str, cents: i64| {
        let pool = pool.clone();
        let input: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
            "customerName": customer, "issueDate": issue, "dueDate": due,
            "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }]
        }))
        .unwrap();
        async move { crate::invoices::create_and_post_invoice(&pool, input).await.unwrap().id }
    };
    let old = invoice("Acme", "2026-01-01", "2026-01-31", 10_000).await;
    invoice("Acme", "2026-03-01", "2026-03-31", 20_000).await;
    invoice("Globex", "2026-03-15", "2026-04-14", 5_000).await;
    crate::invoices::record_payment(
        &pool,
        crate::invoices::PaymentInput { invoice_id: old, deposit_account_id: checking, payment_date: d(4, 10), amount_cents: 10_000, payment_method: "CHECK".into(), reference: None, exchange_rate_e6: None },
    )
    .await
    .unwrap();

    // On April 1 the January invoice was still open (paid April 10) and 60 days past due.
    let ar = aging(&pool, false, d(4, 1)).await.unwrap();
    let acme = ar.rows.iter().find(|r| r.party_name == "Acme").unwrap();
    assert_eq!(acme.buckets_cents, [0, 20_000, 10_000, 0, 0], "March invoice 1 day late; January invoice 60 days late");
    assert_eq!(ar.total_cents, 35_000);
    let later = aging(&pool, false, d(4, 30)).await.unwrap();
    assert_eq!(later.total_cents, 25_000);
    assert_eq!(later.items.len(), 2);

    let bill: crate::expenses::BillInput = serde_json::from_value(serde_json::json!({
        "vendorName": "Staples", "billNumber": "B-9", "billDate": "2026-03-01", "dueDate": "2026-03-11", "memo": null, "totalCents": 7_000,
        "lines": [{ "accountId": supplies, "amountCents": 7_000, "memo": null }]
    }))
    .unwrap();
    crate::expenses::post_vendor_bill(&pool, bill).await.unwrap();
    let ap = aging(&pool, true, d(5, 15)).await.unwrap();
    assert_eq!(ap.rows[0].buckets_cents, [0, 0, 0, 7_000, 0], "65 days past due");

    let sales = by_party(&pool, false, d(1, 1), d(12, 31)).await.unwrap();
    let names: Vec<(&str, i64)> = sales.rows.iter().map(|r| (r.label.as_str(), r.amount_cents)).collect();
    assert_eq!(names, [("Acme", 30_000), ("Globex", 5_000)]);
    let spend = by_party(&pool, true, d(1, 1), d(12, 31)).await.unwrap();
    assert_eq!((spend.rows[0].label.as_str(), spend.total_cents), ("Staples", 7_000));
    let products = sales_by_product(&pool, d(1, 1), d(12, 31)).await.unwrap();
    assert_eq!((products.rows[0].label.as_str(), products.rows[0].quantity_e4, products.total_cents), ("No product/service", Some(30_000), 35_000));

    let gl = general_ledger(&pool, d(3, 1), d(4, 30)).await.unwrap();
    let checking_gl = gl.accounts.iter().find(|a| a.account_id == checking).unwrap();
    assert_eq!((checking_gl.opening_balance_cents, checking_gl.ending_balance_cents), (0, 10_000));
    let services_gl = gl.accounts.iter().find(|a| a.account_id == services).unwrap();
    assert_eq!((services_gl.opening_balance_cents, services_gl.ending_balance_cents), (10_000, 35_000), "income opens at its year-to-date amount");
    let debits: i64 = gl.accounts.iter().map(|a| a.total_debit_cents).sum();
    let credits: i64 = gl.accounts.iter().map(|a| a.total_credit_cents).sum();
    assert_eq!(debits, credits);

    let tb = crate::statements::trial_balance_as_of(&pool, d(4, 30)).await.unwrap();
    assert_eq!(tb.total_debit_cents, tb.total_credit_cents);
    let cash = crate::statements::cash_flow(&pool, d(1, 1), d(12, 31)).await.unwrap();
    assert_eq!((cash.net_change_cents, cash.cash_end_cents), (10_000, 10_000));
    let net = cash.lines.iter().find(|l| l.label == "Net Income").unwrap().amount_cents;
    assert_eq!(net, Some(28_000), "35,000 invoiced less 7,000 of supplies");
}

async fn check(pool: &SqlitePool, from: i64, date: NaiveDate, number: Option<&str>, cents: i64) -> crate::expenses::ExpenseView {
    let expense = account(pool, "6250").await;
    let input: crate::expenses::ExpenseInput = serde_json::from_value(serde_json::json!({
        "kind": "CHECK", "payeeName": "Staples", "paymentAccountId": from, "expenseDate": date, "checkNumber": number,
        "paymentMethod": null, "memo": null, "lines": [{ "accountId": expense, "amountCents": cents, "memo": null }]
    }))
    .unwrap();
    crate::expenses::post_expense(pool, input).await.unwrap()
}

#[tokio::test]
async fn missing_checks_finds_gaps_and_duplicates_and_keeps_voids() {
    let pool = test_pool().await;
    let (checking, savings, supplies) = (account(&pool, "1010").await, account(&pool, "1020").await, account(&pool, "6250").await);
    for (number, day) in [("1001", 2), ("1002", 3), ("1005", 4), ("1005", 5), ("1009", 6)] {
        check(&pool, checking, d(1, day), Some(number), 1_000).await;
    }
    // A voided check keeps its number; an edited check is still one check.
    let voided = check(&pool, checking, d(1, 7), Some("1003"), 2_000).await;
    crate::ledger::void_transaction(&pool, voided.journal_entry_id.unwrap()).await.unwrap();
    let edited = check(&pool, checking, d(1, 8), Some("1004"), 3_000).await;
    let replacement: crate::expenses::ExpenseInput = serde_json::from_value(serde_json::json!({
        "kind": "CHECK", "payeeName": "Staples", "paymentAccountId": checking, "expenseDate": d(1, 8), "checkNumber": "1004",
        "paymentMethod": null, "memo": "Corrected", "lines": [{ "accountId": supplies, "amountCents": 3_500, "memo": null }]
    }))
    .unwrap();
    crate::edits::replace_expense(&pool, edited.id, replacement).await.unwrap();
    // One check paying two bills of the same vendor counts once; an EFT reference is not a check.
    let bill = |n: &str| {
        let pool = pool.clone();
        let input: crate::expenses::BillInput = serde_json::from_value(serde_json::json!({
            "vendorName": "Acme Supply", "billNumber": n, "billDate": "2026-01-10", "dueDate": "2026-02-10", "memo": null, "totalCents": 5_000,
            "lines": [{ "accountId": supplies, "amountCents": 5_000, "memo": null }]
        }))
        .unwrap();
        async move { crate::expenses::post_vendor_bill(&pool, input).await.unwrap().id }
    };
    let (b1, b2, b3) = (bill("A").await, bill("B").await, bill("C").await);
    let pay = |bill_id, reference: &str| crate::expenses::BillPaymentInput { bill_id, payment_account_id: checking, payment_date: d(1, 20), amount_cents: 5_000, reference: Some(reference.into()) };
    crate::expenses::pay_vendor_bills(&pool, vec![pay(b1, "1010"), pay(b2, "1010")]).await.unwrap();
    crate::expenses::pay_vendor_bill(&pool, pay(b3, "EFT 88")).await.unwrap();
    // Checks from another account are not listed.
    check(&pool, savings, d(1, 9), Some("1006"), 1_000).await;

    let report = missing_checks(&pool, checking, None, None).await.unwrap();
    let numbers: Vec<&str> = report.rows.iter().map(|r| r.number.as_str()).collect();
    assert_eq!(numbers, ["1001", "1002", "1003", "1004", "1005", "1005", "1009", "1010"]);
    assert!(report.rows[2].is_voided && !report.rows[3].is_voided);
    assert_eq!(report.rows[3].amount_cents, 3_500, "the edited check, once, at its corrected amount");
    assert_eq!((report.rows[7].kind.as_str(), report.rows[7].amount_cents), ("Bill Payment", 10_000));
    assert!(report.rows[4].is_duplicate && report.rows[5].is_duplicate && !report.rows[6].is_duplicate);
    assert_eq!(report.duplicate_count, 2);
    let gaps: Vec<(usize, u64, u64)> = report.gaps.iter().map(|g| (g.before, g.first, g.last)).collect();
    assert_eq!(gaps, [(6, 1006, 1008)], "1006 to 1008 are missing before 1009");
    assert_eq!((report.missing_count, report.unnumbered_count), (3, 1));

    let january_first_week = missing_checks(&pool, checking, Some(d(1, 1)), Some(d(1, 4))).await.unwrap();
    assert_eq!(january_first_week.rows.len(), 3);
    assert_eq!(january_first_week.missing_count, 2, "1003 and 1004 fall outside the dates");
    assert!(missing_checks(&pool, checking, Some(d(2, 1)), Some(d(1, 1))).await.unwrap_err().starts_with("RPT_BAD_RANGE"));
}

#[tokio::test]
async fn inventory_valuation_and_stock_status_follow_the_stock_ledger() {
    let pool = test_pool().await;
    let (sales, checking) = (account(&pool, "4000").await, account(&pool, "1010").await);
    let product = crate::sales::create_product(
        &pool,
        crate::sales::ProductInput { name: "Widget".into(), description: Some("Blue widget".into()), rate_cents: 2_500, income_account_id: sales, is_taxable: false },
    )
    .await
    .unwrap()
    .id;
    let tracked = crate::inventory::track_product(&pool, &crate::inventory::TrackInput { product_id: product, sku: Some("W-1".into()), asset_account_id: None, cogs_account_id: None })
        .await
        .unwrap();
    let buy = |date: &str, qty: i64, rate: i64| {
        let pool = pool.clone();
        let input: crate::expenses::BillInput = serde_json::from_value(serde_json::json!({
            "vendorName": "Supplier", "billNumber": null, "billDate": date, "dueDate": date, "memo": null, "totalCents": qty * rate,
            "lines": [{ "accountId": tracked.asset_account_id, "amountCents": qty * rate, "memo": null, "quantityE4": qty * 10_000, "rateCents": rate, "productServiceId": product }]
        }))
        .unwrap();
        async move { crate::expenses::post_vendor_bill(&pool, input).await.unwrap() }
    };
    let sell = |date: &str, qty: i64| {
        let pool = pool.clone();
        let input: crate::sales::SalesReceiptInput = serde_json::from_value(serde_json::json!({
            "customerName": "Walk-in", "receiptDate": date, "depositAccountId": checking, "paymentMethod": "CASH", "reference": null, "memo": null,
            "lines": [{ "productServiceId": product, "description": "Widget", "quantityE4": qty * 10_000, "rateCents": 2_500, "incomeAccountId": sales }]
        }))
        .unwrap();
        async move { crate::sales::post_sales_receipt(&pool, input).await.unwrap() }
    };
    buy("2026-03-02", 10, 1_000).await; // 10 at $10.00
    buy("2026-03-10", 5, 1_300).await; // 5 at $13.00: 15 worth $165.00
    sell("2026-03-15", 6).await; // 6 × $11.00 = $66.00 out: 9 worth $99.00
    let mistake = sell("2026-03-20", 2).await;
    let entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM sales_receipts WHERE id = ?1").bind(mistake.id).fetch_one(&pool).await.unwrap();
    crate::ledger::void_transaction(&pool, entry).await.unwrap();

    let early = inventory_valuation(&pool, d(3, 5)).await.unwrap();
    assert_eq!((early.rows[0].qty_on_hand_e4, early.rows[0].asset_value_cents, early.rows[0].average_cost_cents), (100_000, 10_000, Some(1_000)));
    let now = inventory_valuation(&pool, d(3, 31)).await.unwrap();
    let w = &now.rows[0];
    assert_eq!((w.name.as_str(), w.sku.as_deref(), w.description.as_deref()), ("Widget", Some("W-1"), Some("Blue widget")));
    assert_eq!((w.qty_on_hand_e4, w.asset_value_cents, w.average_cost_cents, w.retail_value_cents), (90_000, 9_900, Some(1_100), 22_500));
    let gl: i64 = sqlx::query_scalar("SELECT SUM(debit_cents - credit_cents) FROM v_posted_lines WHERE account_id = ?1 AND entry_date <= '2026-03-31'")
        .bind(tracked.asset_account_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(now.total_asset_value_cents, gl, "the valuation agrees with the Inventory Asset account");

    let detail = inventory_valuation_detail(&pool, d(3, 6), d(3, 31)).await.unwrap();
    let item = &detail.items[0];
    assert_eq!((item.opening_qty_e4, item.opening_value_cents), (100_000, 10_000));
    let kinds: Vec<&str> = item.rows.iter().map(|r| r.kind.as_str()).collect();
    assert_eq!(kinds, ["PURCHASE", "SALE", "SALE", "VOID"]);
    assert_eq!(item.rows[1].qty_on_hand_e4, 90_000);
    assert_eq!(item.rows[3].party_name.as_deref(), Some("Walk-in"), "a void names the voided sale's customer");
    assert_eq!((item.closing_qty_e4, item.closing_value_cents, detail.total_value_cents), (90_000, 9_900, 9_900));
    assert!(inventory_valuation_detail(&pool, d(4, 1), d(4, 30)).await.unwrap().items.len() == 1, "stock on hand keeps the item in a quiet period");

    let po = crate::workflows::PurchaseOrderInput {
        vendor_name: "Supplier".into(),
        po_date: d(3, 25),
        expected_date: Some(d(4, 15)),
        memo: None,
        lines: vec![crate::workflows::PurchaseOrderLineInput {
            account_id: tracked.asset_account_id,
            product_service_id: Some(product),
            description: None,
            quantity_e4: Some(120_000),
            rate_cents: Some(1_000),
            amount_cents: 12_000,
            class_id: None,
        }],
    };
    crate::workflows::create_purchase_order(&pool, po).await.unwrap();
    let status = stock_status(&pool, d(3, 1), d(3, 28)).await.unwrap();
    let s = &status.rows[0];
    assert_eq!((s.qty_on_hand_e4, s.qty_on_order_e4, s.next_delivery), (90_000, 120_000, Some(d(4, 15))));
    assert_eq!((s.qty_sold_e4, s.sales_per_week_e4), (60_000, 15_000), "6 sold over four weeks (the voided sale does not count)");
}
