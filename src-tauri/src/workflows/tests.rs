use super::*;
use crate::db::test_pool;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(account).fetch_one(pool).await.unwrap()
}

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

#[tokio::test]
async fn discounts_statements_and_1099s() {
    let pool = test_pool().await;
    let (checking, card, ar, services, rent) = (account(&pool, "1010").await, account(&pool, "2100").await, account(&pool, "1200").await, account(&pool, "4100").await, account(&pool, "6400").await);
    let rate = crate::sales_tax::create_tax_rate(&pool, crate::sales_tax::TaxRateInput { name: "10%".into(), rate_ppm: 100_000, agency_name: None, liability_account_id: None }).await.unwrap();
    // 200.00 taxable + 100.00 not, less a 30.00 discount: tax on 200 - 20 = 180 is 18.00.
    let invoice: InvoiceInput = serde_json::from_value(serde_json::json!({ "customerName": "Acme:Kitchen", "issueDate": "2026-03-01", "dueDate": "2026-03-31",
        "taxRateId": rate.id, "discountCents": 3_000,
        "lines": [{ "description": "Parts", "quantityE4": 10_000, "rateCents": 20_000, "incomeAccountId": services, "isTaxable": true },
                  { "description": "Labor", "quantityE4": 10_000, "rateCents": 10_000, "incomeAccountId": services, "isTaxable": false }] })).unwrap();
    let inv = crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap();
    assert_eq!((inv.total_cents, inv.tax_cents, inv.discount_cents), (28_800, 1_800, 3_000));
    assert_eq!(inv.customer_name, "Acme:Kitchen");
    let discounts: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = 'Discounts Given'").fetch_one(&pool).await.unwrap();
    assert_eq!(balance(&pool, discounts).await, 3_000);

    crate::invoices::record_payment(&pool, crate::invoices::PaymentInput { invoice_id: inv.id, deposit_account_id: checking, payment_date: d(3, 20), amount_cents: 10_000, payment_method: "CHECK".into(), reference: Some("1001".into()), exchange_rate_e6: None }).await.unwrap();
    let acme: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Acme'").fetch_one(&pool).await.unwrap();
    let statement = customer_statement(&pool, acme, d(3, 1), d(4, 30)).await.unwrap();
    assert_eq!(statement.rows.len(), 2, "the job's invoice and payment appear on the customer's statement");
    assert_eq!((statement.opening_balance_cents, statement.closing_balance_cents), (0, 18_800));
    assert_eq!(statement.aging_cents, [0, 18_800, 0, 0, 0]);
    assert_eq!(balance(&pool, ar).await, 18_800);
    let listed = statement_customers(&pool, d(3, 1), d(4, 30)).await.unwrap();
    assert_eq!(listed.iter().map(|c| c.1.as_str()).collect::<Vec<_>>(), ["Acme"]);

    // 1099s count payments from the bank, not by card.
    let vendor = crate::expenses::create_vendor(&pool, "Joe Contractor", None).await.unwrap();
    sqlx::query("UPDATE vendors SET is_1099 = 1, tax_id = '123-45-6789' WHERE id = ?1").bind(vendor.id).execute(&pool).await.unwrap();
    let pay = |from: i64, cents: i64| -> ExpenseInput {
        serde_json::from_value(serde_json::json!({ "kind": "EXPENSE", "payeeName": "Joe Contractor", "paymentAccountId": from, "expenseDate": "2026-05-01",
            "checkNumber": null, "paymentMethod": null, "memo": null, "lines": [{ "accountId": rent, "amountCents": cents, "memo": null }] })).unwrap()
    };
    crate::expenses::post_expense(&pool, pay(checking, 50_000)).await.unwrap();
    crate::expenses::post_expense(&pool, pay(card, 90_000)).await.unwrap();
    let rows = vendors_1099(&pool, 2026, crate::tax_ids::Access::OPEN).await.unwrap();
    assert_eq!((rows[0].paid_cents, rows[0].over_threshold), (50_000, false));
    crate::expenses::post_expense(&pool, pay(checking, 10_000)).await.unwrap();
    assert!(vendors_1099(&pool, 2026, crate::tax_ids::Access::OPEN).await.unwrap()[0].over_threshold);
}

#[tokio::test]
async fn recurring_purchase_orders_search_attachments_and_budgets() {
    let pool = test_pool().await;
    let (checking, rent, supplies, services) = (account(&pool, "1010").await, account(&pool, "6300").await, account(&pool, "6250").await, account(&pool, "4100").await);
    let rent_check = serde_json::json!({ "kind": "CHECK", "payeeName": "Landlord", "paymentAccountId": checking, "expenseDate": "2026-01-01",
        "checkNumber": null, "paymentMethod": null, "memo": "Monthly rent", "lines": [{ "accountId": rent, "amountCents": 120_000, "memo": null }] });
    let id = save_recurring(&pool, None, RecurringInput { name: "Rent".into(), kind: "EXPENSE".into(), payload: rent_check, frequency: "MONTHLY".into(), every: 1, next_date: d(1, 31), end_date: Some(d(4, 30)) }).await.unwrap();
    assert_eq!(enter_due(&pool, d(3, 15), None).await.unwrap(), 2, "January 31 and February 28");
    assert_eq!(balance(&pool, rent).await, 240_000);
    let listed = list_recurring(&pool).await.unwrap();
    assert_eq!((listed[0].next_date, listed[0].run_count, listed[0].amount_cents), (d(3, 28), 2, 120_000));
    assert_eq!(enter_due(&pool, d(12, 31), Some(id)).await.unwrap(), 2, "March and April, then the end date");
    assert!(!list_recurring(&pool).await.unwrap()[0].is_active);

    let po = create_purchase_order(&pool, PurchaseOrderInput { vendor_name: "Staples".into(), po_date: d(5, 1), expected_date: None, memo: None,
        lines: vec![PurchaseOrderLineInput { account_id: supplies, product_service_id: None, description: Some("Paper".into()), quantity_e4: Some(20_000), rate_cents: Some(2_500), amount_cents: 5_000, class_id: None }] }).await.unwrap();
    assert_eq!(balance(&pool, supplies).await, 0, "a purchase order posts nothing");
    let bill = bill_purchase_order(&pool, po, d(5, 9), d(6, 8), Some("INV-77".into())).await.unwrap();
    assert_eq!(balance(&pool, supplies).await, 5_000);
    let order = list_purchase_orders(&pool).await.unwrap();
    assert_eq!((order[0].status.as_str(), order[0].bill_id), ("BILLED", Some(bill)));
    assert!(bill_purchase_order(&pool, po, d(5, 9), d(6, 8), None).await.unwrap_err().starts_with("PO_NOT_OPEN"));

    let hits = search(&pool, "landlord").await.unwrap();
    assert_eq!(hits.iter().filter(|h| h.kind == "TRANSACTION").count(), 4);
    assert!(hits.iter().any(|h| h.kind == "VENDOR" && h.label == "Landlord"));
    assert_eq!(search(&pool, "1,200.00").await.unwrap().iter().filter(|h| h.kind == "TRANSACTION").count(), 4);
    assert_eq!(search(&pool, "INV-77").await.unwrap().iter().filter(|h| h.kind == "TRANSACTION").count(), 1);

    let entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM bills WHERE id = ?1").bind(bill).fetch_one(&pool).await.unwrap();
    let file = add_attachment(&pool, entry, "/tmp/receipt.pdf", b"%PDF-1.4 receipt").await.unwrap();
    assert_eq!((file.file_name.as_str(), file.media_type.as_str(), file.size_bytes), ("receipt.pdf", "application/pdf", 16));
    assert_eq!(attachment_content(&pool, file.id).await.unwrap().1, b"%PDF-1.4 receipt");
    delete_attachment(&pool, file.id).await.unwrap();
    assert!(list_attachments(&pool, entry).await.unwrap().is_empty());

    // Budget vs. Actual for the first quarter (calendar fiscal year).
    let budget = crate::statements::create_budget(&pool, "2026 plan", d(6, 1)).await.unwrap();
    assert_eq!(budget.fiscal_year_start, d(1, 1));
    let amounts: Vec<crate::statements::BudgetAmount> = (1..=3)
        .flat_map(|m| [crate::statements::BudgetAmount { account_id: rent, period: m, amount_cents: 100_000 }, crate::statements::BudgetAmount { account_id: services, period: m, amount_cents: 50_000 }])
        .collect();
    crate::statements::set_budget_amounts(&pool, budget.id, &amounts).await.unwrap();
    let report = crate::statements::budget_vs_actual(&pool, budget.id, 1, 3).await.unwrap();
    let rent_row = report.lines.iter().find(|l| l.label == "6300 Rent & Lease").unwrap();
    assert_eq!(rent_row.amounts_cents, [Some(360_000), Some(300_000), Some(60_000), Some(12_000)], "three rent checks against 3 × 1,000 budgeted");
    let services_row = report.lines.iter().find(|l| l.label == "4100 Services").unwrap();
    assert_eq!(services_row.amounts_cents, [Some(0), Some(150_000), Some(-150_000), Some(0)]);
}
