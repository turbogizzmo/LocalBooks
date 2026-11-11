use super::*;
use crate::db::test_pool;
use crate::money::Cents;

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
        "taxRateId": rate.id, "poNumber": "PO-77", "discountCents": 3_000,
        "lines": [{ "description": "Parts", "quantityE4": 10_000, "rateCents": 20_000, "incomeAccountId": services, "isTaxable": true },
                  { "description": "Labor", "quantityE4": 10_000, "rateCents": 10_000, "incomeAccountId": services, "isTaxable": false }] })).unwrap();
    let inv = crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap();
    assert_eq!((inv.total_cents, inv.tax_cents, inv.discount_cents), (28_800, 1_800, 3_000));
    assert_eq!(inv.customer_name, "Acme:Kitchen");
    let discounts: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = 'Discounts Given'").fetch_one(&pool).await.unwrap();
    assert_eq!(balance(&pool, discounts).await, 3_000);

    crate::invoices::record_payment(&pool, crate::invoices::PaymentInput { discount_cents: Cents::ZERO, payment_method_id: None, invoice_id: inv.id, deposit_account_id: checking, payment_date: d(3, 20), amount_cents: Cents(10_000), payment_method: "CHECK".into(), reference: Some("1001".into()), exchange_rate_e6: None }).await.unwrap();
    let acme: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Acme'").fetch_one(&pool).await.unwrap();
    let statement = customer_statement(&pool, acme, d(3, 1), d(4, 30)).await.unwrap();
    assert_eq!(statement.rows.len(), 2, "the job's invoice and payment appear on the customer's statement");
    assert_eq!((statement.rows[0].po_number.as_deref(), statement.rows[1].po_number.as_deref()), (Some("PO-77"), None), "the invoice's P.O. number");
    assert_eq!((statement.opening_balance_cents, statement.closing_balance_cents), (0, 18_800));
    assert_eq!(statement.aging_cents, [0, 18_800, 0, 0, 0]);
    assert_eq!(balance(&pool, ar).await, 18_800);
    let listed = statement_customers(&pool, d(3, 1), d(4, 30)).await.unwrap();
    assert_eq!(listed.iter().map(|c| c.1.as_str()).collect::<Vec<_>>(), ["Acme"]);

    // 1099s count payments from the bank, not by card ($600 threshold for 2025 payments).
    let vendor = crate::expenses::create_vendor(&pool, "Joe Contractor", None).await.unwrap();
    sqlx::query("UPDATE vendors SET is_1099 = 1, tax_id = '123-45-6789' WHERE id = ?1").bind(vendor.id).execute(&pool).await.unwrap();
    let pay = |from: i64, cents: i64| -> ExpenseInput {
        serde_json::from_value(serde_json::json!({ "kind": "EXPENSE", "payeeName": "Joe Contractor", "paymentAccountId": from, "expenseDate": "2025-05-01",
            "checkNumber": null, "paymentMethod": null, "memo": null, "lines": [{ "accountId": rent, "amountCents": cents, "memo": null }] })).unwrap()
    };
    crate::expenses::post_expense(&pool, pay(checking, 50_000)).await.unwrap();
    crate::expenses::post_expense(&pool, pay(card, 90_000)).await.unwrap();
    let rows = vendors_1099(&pool, 2025, crate::tax_ids::Access::OPEN).await.unwrap();
    assert_eq!((rows[0].paid_cents, rows[0].over_threshold), (50_000, false));
    crate::expenses::post_expense(&pool, pay(checking, 10_000)).await.unwrap();
    assert!(vendors_1099(&pool, 2025, crate::tax_ids::Access::OPEN).await.unwrap()[0].over_threshold);
}

#[tokio::test]
async fn a_voided_payment_comes_off_the_customers_statement() {
    let pool = test_pool().await;
    let (checking, services) = (account(&pool, "1010").await, account(&pool, "4100").await);
    let invoice: InvoiceInput = serde_json::from_value(serde_json::json!({ "customerName": "Birch", "issueDate": "2026-03-01", "dueDate": "2026-03-31",
        "lines": [{ "description": "Pruning", "quantityE4": 10_000, "rateCents": 50_000, "incomeAccountId": services, "isTaxable": false }] })).unwrap();
    let inv = crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap();
    crate::invoices::record_payment(&pool, crate::invoices::PaymentInput { discount_cents: Cents::ZERO, payment_method_id: None, invoice_id: inv.id, deposit_account_id: checking, payment_date: d(3, 20), amount_cents: Cents(20_000), payment_method: "CHECK".into(), reference: Some("77".into()), exchange_rate_e6: None }).await.unwrap();
    let birch: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Birch'").fetch_one(&pool).await.unwrap();
    assert_eq!(customer_statement(&pool, birch, d(3, 1), d(3, 31)).await.unwrap().closing_balance_cents, 30_000);

    let payment_entry: i64 = sqlx::query_scalar("SELECT e.id FROM journal_entries e JOIN invoice_payments p ON p.id = e.source_id WHERE e.source_type = 'RECEIVE_PAYMENT' AND p.invoice_id = ?1")
        .bind(inv.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    crate::ledger::void_transaction(&pool, payment_entry).await.unwrap();

    let statement = customer_statement(&pool, birch, d(3, 1), d(3, 31)).await.unwrap();
    assert_eq!(statement.closing_balance_cents, 50_000, "the whole invoice is owed again, as A/R and the invoice say");
    let kinds: Vec<(&str, i64)> = statement.rows.iter().map(|r| (r.source_type.as_str(), r.amount_cents)).collect();
    assert_eq!(kinds, [("INVOICE", 50_000), ("RECEIVE_PAYMENT", -20_000), ("VOID", 20_000)]);
    // The reversal carries the customer's name everywhere names come from.
    let named: Option<String> = sqlx::query_scalar("SELECT p.party_name FROM v_entry_party p JOIN journal_entries e ON e.id = p.journal_entry_id WHERE e.reverses_entry_id = ?1")
        .bind(payment_entry)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(named.as_deref(), Some("Birch"));
}

#[tokio::test]
async fn recurring_purchase_orders_search_attachments_and_budgets() {
    let pool = test_pool().await;
    let (checking, rent, supplies, services) = (account(&pool, "1010").await, account(&pool, "6300").await, account(&pool, "6250").await, account(&pool, "4100").await);
    let rent_check = serde_json::json!({ "kind": "CHECK", "payeeName": "Landlord", "paymentAccountId": checking, "expenseDate": "2026-01-01",
        "checkNumber": null, "paymentMethod": null, "memo": "Monthly rent", "lines": [{ "accountId": rent, "amountCents": 120_000, "memo": null }] });
    let id = save_recurring(&pool, None, RecurringInput { anchor_day: None, name: "Rent".into(), kind: "EXPENSE".into(), payload: rent_check, frequency: "MONTHLY".into(), every: 1, next_date: d(1, 31), end_date: Some(d(4, 30)) }).await.unwrap();
    assert_eq!(enter_due(&pool, d(3, 15), None).await.unwrap(), 2, "January 31 and February 28");
    assert_eq!(balance(&pool, rent).await, 240_000);
    let listed = list_recurring(&pool).await.unwrap();
    assert_eq!((listed[0].next_date, listed[0].run_count, listed[0].amount_cents), (d(3, 31), 2, 120_000), "back to month end after February");
    assert_eq!(enter_due(&pool, d(12, 31), Some(id)).await.unwrap(), 2, "March and April, then the end date");
    assert!(!list_recurring(&pool).await.unwrap()[0].is_active);

    let po = create_purchase_order(&pool, PurchaseOrderInput { form_lines: Vec::new(), vendor_name: "Staples".into(), po_date: d(5, 1), expected_date: None, memo: None,
        lines: vec![PurchaseOrderLineInput { account_id: supplies, product_service_id: None, description: Some("Paper".into()), quantity_e4: Some(20_000), rate_cents: Some(2_500), amount_cents: 5_000, class_id: None, customer_id: None }] }).await.unwrap();
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
    add_attachment(&pool, entry, "invoice.png", b"png").await.unwrap();
    assert_eq!(attachment_counts(&pool).await.unwrap(), std::collections::HashMap::from([(entry, 2)]));
    delete_attachment(&pool, file.id).await.unwrap();
    assert_eq!(list_attachments(&pool, entry).await.unwrap().len(), 1);

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

/// Preferences › Tax: 1099 maps expense accounts to boxes; the 1099 Summary and Detail split each
/// payment over what it paid, leave out unmapped accounts, and use the threshold for the year.
#[tokio::test]
async fn the_1099_report_follows_the_account_mapping_and_the_threshold_by_year() {
    let pool = test_pool().await;
    let (checking, contractors, rent, supplies) = (account(&pool, "1010").await, account(&pool, "6400").await, account(&pool, "6300").await, account(&pool, "6250").await);
    let vendor = crate::expenses::create_vendor(&pool, "Lee Builder", None).await.unwrap();
    sqlx::query("UPDATE vendors SET is_1099 = 1 WHERE id = ?1").bind(vendor.id).execute(&pool).await.unwrap();
    // A check in 2026: $1,500 of labor, $400 of supplies.
    let check: ExpenseInput = serde_json::from_value(serde_json::json!({ "kind": "CHECK", "payeeName": "Lee Builder", "paymentAccountId": checking, "expenseDate": "2026-03-01",
        "checkNumber": "101", "paymentMethod": null, "memo": null,
        "lines": [{ "accountId": contractors, "amountCents": 150_000, "memo": null }, { "accountId": supplies, "amountCents": 40_000, "memo": null }] })).unwrap();
    crate::expenses::post_expense(&pool, check).await.unwrap();
    // A bill for $1,000 rent and $1,000 labor, half paid: $500 to each.
    let bill: BillInput = serde_json::from_value(serde_json::json!({ "vendorName": "Lee Builder", "billNumber": "B-1", "billDate": "2026-04-01", "dueDate": "2026-05-01", "memo": null, "totalCents": 200_000,
        "lines": [{ "accountId": rent, "amountCents": 100_000, "memo": null }, { "accountId": contractors, "amountCents": 100_000, "memo": null }] })).unwrap();
    let bill = crate::expenses::post_vendor_bill(&pool, bill).await.unwrap();
    crate::expenses::pay_vendor_bill(&pool, crate::expenses::BillPaymentInput { bill_id: bill.id, payment_account_id: checking, payment_date: d(4, 15), amount_cents: 100_000, reference: Some("102".into()), to_be_printed: false }).await.unwrap();

    // No mapping yet: everything paid from the bank is nonemployee compensation, as before.
    let rows = vendors_1099(&pool, 2026, crate::tax_ids::Access::OPEN).await.unwrap();
    assert_eq!(rows[0].paid_cents, 290_000);
    assert_eq!(rows[0].boxes.iter().map(|b| (b.code.as_str(), b.paid_cents, b.threshold_cents)).collect::<Vec<_>>(), [("NEC-1", 290_000, 200_000)]);
    assert!(rows[0].over_threshold, "over $2,000, the threshold for 2026 payments");

    // Labor → NEC box 1, rent → MISC box 1; supplies unmapped.
    let map = format!(r#"{{"{contractors}":"NEC-1","{rent}":"MISC-1"}}"#);
    crate::preferences::set(&pool, "tax_1099_accounts", &map).await.unwrap();
    let rows = vendors_1099(&pool, 2026, crate::tax_ids::Access::OPEN).await.unwrap();
    let boxes: Vec<_> = rows[0].boxes.iter().map(|b| (b.code.as_str(), b.paid_cents, b.over_threshold)).collect();
    assert_eq!(boxes, [("NEC-1", 150_000 + 50_000, true), ("MISC-1", 50_000, false)]);
    assert_eq!(rows[0].paid_cents, 250_000, "supplies are left out");
    let detail: Vec<_> = rows[0].details.iter().map(|d| (d.kind.as_str(), d.number.as_deref(), d.box_code.as_str(), d.amount_cents)).collect();
    assert_eq!(detail, [("CHECK", Some("101"), "NEC-1", 150_000), ("BILL_PAYMENT", Some("102"), "MISC-1", 50_000), ("BILL_PAYMENT", Some("102"), "NEC-1", 50_000)]);

    // The threshold by payment year (gaps row R2), and by box.
    assert_eq!((threshold_1099_cents("NEC-1", 2025), threshold_1099_cents("NEC-1", 2026), threshold_1099_cents("NEC-1", 2027)), (60_000, 200_000, 200_000));
    assert_eq!((threshold_1099_cents("MISC-2", 2026), threshold_1099_cents("MISC-10", 2026)), (1_000, 60_000));

    // Bad maps are refused.
    assert!(crate::preferences::set(&pool, "tax_1099_accounts", &format!(r#"{{"{rent}":"MISC-99"}}"#)).await.unwrap_err().starts_with("PREF_BAD_1099_BOX"));
    assert!(crate::preferences::set(&pool, "tax_1099_accounts", r#"{"999999":"NEC-1"}"#).await.unwrap_err().starts_with("PREF_BAD_1099_ACCOUNT"));
    assert!(crate::preferences::set(&pool, "tax_1099_accounts", "not json").await.unwrap_err().starts_with("PREF_BAD_1099_ACCOUNTS"));
}

#[tokio::test]
async fn an_open_purchase_order_can_be_changed_or_deleted_until_something_is_received_or_billed() {
    let pool = test_pool().await;
    let supplies = account(&pool, "6250").await;
    let line = |cents: i64| PurchaseOrderLineInput { account_id: supplies, product_service_id: None, description: Some("Paper".into()), quantity_e4: None, rate_cents: None, amount_cents: cents, class_id: None, customer_id: None };
    let input = |vendor: &str, lines: Vec<PurchaseOrderLineInput>| PurchaseOrderInput { form_lines: Vec::new(), vendor_name: vendor.into(), po_date: d(5, 1), expected_date: None, memo: None, lines };
    let po = create_purchase_order(&pool, input("Staples", vec![line(5_000)])).await.unwrap();
    let number = |pool: SqlitePool| async move { list_purchase_orders(&pool).await.unwrap()[0].po_number };
    let before = number(pool.clone()).await;

    // Changed in place: another vendor and two lines, the same number, nothing posted.
    update_purchase_order(&pool, po, input("Office Depot", vec![line(2_000), line(3_500)])).await.unwrap();
    let order = &list_purchase_orders(&pool).await.unwrap()[0];
    assert_eq!((order.po_number, order.vendor_name.as_str(), order.total_cents), (before, "Office Depot", 5_500));
    assert_eq!(purchase_order_lines(&pool, po).await.unwrap().len(), 2);
    assert_eq!(balance(&pool, supplies).await, 0);

    // Billed, it can be neither changed nor deleted.
    bill_purchase_order(&pool, po, d(5, 9), d(6, 8), None).await.unwrap();
    assert!(update_purchase_order(&pool, po, input("Staples", vec![line(1_000)])).await.unwrap_err().starts_with("PO_BILLED"));
    assert!(delete_purchase_order(&pool, po).await.unwrap_err().starts_with("PO_BILLED"));

    // Another, deleted with its lines.
    let other = create_purchase_order(&pool, input("Staples", vec![line(1_000)])).await.unwrap();
    delete_purchase_order(&pool, other).await.unwrap();
    assert_eq!(list_purchase_orders(&pool).await.unwrap().len(), 1);
    let lines: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM purchase_order_lines WHERE purchase_order_id = ?1").bind(other).fetch_one(&pool).await.unwrap();
    assert_eq!(lines, 0);
}

#[tokio::test]
async fn statements_label_payments_refunds_and_write_offs_and_credit_memo_numbers_dont_skip() {
    use crate::credits::{create_credit_memo, refund_customer_credit, write_off_customer_credit, CreditMemoInput, RefundCreditInput, UnappliedPaymentInput};
    let pool = test_pool().await;
    let (checking, services) = (account(&pool, "1010").await, account(&pool, "4100").await);
    let memo = |pool: SqlitePool, day: u32| async move {
        create_credit_memo(
            &pool,
            CreditMemoInput {
                customer_name: "Acme".into(),
                credit_number: None,
                credit_date: d(3, day),
                memo: None,
                refund_account_id: None,
                tax_rate_id: None,
                lines: vec![crate::sales::SalesLineInput { product_service_id: None, description: "Return".into(), quantity_e4: 10_000, rate_cents: crate::money::Cents::from(5_000), income_account_id: services, class_id: None, is_taxable: false, service_date: None }],
                header: Default::default(),
                form_lines: Vec::new(),
            },
        )
        .await
        .unwrap()
    };
    let first = memo(pool.clone(), 2).await;
    // An overpayment kept as credit takes no credit memo number.
    let mut tx = crate::ledger::begin_write(&pool).await.unwrap();
    crate::credits::write_unapplied_payment(
        &mut tx,
        &UnappliedPaymentInput { customer_name: "Acme".into(), deposit_account_id: checking, payment_date: d(3, 3), amount_cents: 2_500, payment_method: "CHECK".into(), payment_method_id: None, reference: Some("8812".into()), keep_as: None, account_id: None },
    )
    .await
    .unwrap();
    crate::ledger::commit_write(tx).await.unwrap();
    let second = memo(pool.clone(), 4).await;
    assert_eq!((first.credit_number, second.credit_number), (1, 2), "no number taken by the overpayment");

    write_off_customer_credit(&pool, RefundCreditInput { credit_id: first.id, account_id: services, refund_date: d(3, 5), amount_cents: 1_000, reference: None }).await.unwrap();
    refund_customer_credit(&pool, RefundCreditInput { credit_id: second.id, account_id: checking, refund_date: d(3, 6), amount_cents: 2_000, reference: Some("1042".into()) }).await.unwrap();

    let acme: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Acme'").fetch_one(&pool).await.unwrap();
    let period = crate::forms::Period { start_date: d(3, 1), end_date: d(3, 31) };
    let doc = crate::forms::statement_doc(&pool, acme, period, "$").await.unwrap();
    let labels: Vec<&str> = doc.statement.as_ref().unwrap().rows.iter().skip(1).map(|r| r[1].as_str()).collect();
    assert_eq!(labels, ["Credit Memo #1", "Payment #8812", "Credit Memo #2", "Write-off", "Refund #1042"]);
}

#[tokio::test]
async fn an_old_low_numbered_overpayment_is_skipped_once_and_payment_credits_stay_out_of_credit_memo_lists() {
    use crate::credits::{create_credit_memo, CreditMemoInput, UnappliedPaymentInput, PAYMENT_CREDIT_NUMBERS};
    let pool = test_pool().await;
    let (checking, services) = (account(&pool, "1010").await, account(&pool, "4100").await);
    let memo = |pool: SqlitePool| async move {
        create_credit_memo(
            &pool,
            CreditMemoInput {
                customer_name: "Acme".into(),
                credit_number: None,
                credit_date: d(3, 2),
                memo: None,
                refund_account_id: None,
                tax_rate_id: None,
                lines: vec![crate::sales::SalesLineInput { product_service_id: None, description: "Return".into(), quantity_e4: 10_000, rate_cents: crate::money::Cents::from(5_000), income_account_id: services, class_id: None, is_taxable: false, service_date: None }],
                header: Default::default(),
                form_lines: Vec::new(),
            },
        )
        .await
        .unwrap()
    };
    assert_eq!(memo(pool.clone()).await.credit_number, 1);
    // A file from before: an overpayment kept as credit took #2.
    let acme: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Acme'").fetch_one(&pool).await.unwrap();
    sqlx::query("INSERT INTO credit_memos (credit_number, customer_id, credit_date, total_cents, payment_account_id, payment_method, payment_reference) VALUES (2, ?1, '2026-03-03', 100, ?2, 'CHECK', 'OLD-1')")
        .bind(acme)
        .bind(checking)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(memo(pool.clone()).await.credit_number, 3, "the old number is skipped, never reused");
    let mut tx = crate::ledger::begin_write(&pool).await.unwrap();
    let paid = crate::credits::write_unapplied_payment(
        &mut tx,
        &UnappliedPaymentInput { customer_name: "Acme".into(), deposit_account_id: checking, payment_date: d(3, 4), amount_cents: 2_500, payment_method: "CHECK".into(), payment_method_id: None, reference: Some("8812".into()), keep_as: None, account_id: None },
    )
    .await
    .unwrap();
    crate::ledger::commit_write(tx).await.unwrap();
    let paid_number: i64 = sqlx::query_scalar("SELECT credit_number FROM credit_memos WHERE id = ?1").bind(paid).fetch_one(&pool).await.unwrap();
    assert_eq!(paid_number, PAYMENT_CREDIT_NUMBERS + 1);
    assert_eq!(memo(pool.clone()).await.credit_number, 4);

    // The payment isn't a credit memo: not in the credit memo print batch, and it can't be queued for email.
    let batch = crate::print_batches::list(&pool, crate::print_batches::BatchKind::CreditMemo, d(3, 1), d(3, 31)).await.unwrap();
    assert_eq!(batch.iter().map(|r| r.number.as_deref().unwrap_or("")).collect::<Vec<_>>(), ["1", "3", "4"]);
    assert!(crate::email_log::set_queued(&pool, crate::forms::FormKind::CreditMemo, paid, true).await.unwrap_err().starts_with("EMAIL_NOT_FOUND"));

    // Once used on an invoice, voiding the payment names it as a payment, not by its number.
    let invoice: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({
        "customerName": "Acme", "issueDate": "2026-03-05", "dueDate": "2026-04-05",
        "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": 5_000, "incomeAccountId": services, "isTaxable": false }]
    }))
    .unwrap();
    let inv = crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap();
    crate::credits::apply_customer_credit(&pool, crate::credits::ApplyCreditInput { credit_id: paid, document_id: inv.id, amount_cents: 1_000, applied_date: d(3, 6) }).await.unwrap();
    let entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM credit_memos WHERE id = ?1").bind(paid).fetch_one(&pool).await.unwrap();
    let err = crate::ledger::void_transaction(&pool, entry).await.unwrap_err();
    assert!(err.contains("this unapplied payment") && !err.contains("1000000001"), "{err}");
}

#[tokio::test]
async fn a_monthly_schedule_from_the_31st_keeps_to_month_end_after_a_short_month() {
    use chrono::Datelike;
    let pool = test_pool().await;
    let (checking, rent) = (account(&pool, "1010").await, account(&pool, "6300").await);
    let check = serde_json::json!({ "kind": "CHECK", "payeeName": "Landlord", "paymentAccountId": checking, "expenseDate": "2026-01-31",
        "checkNumber": null, "paymentMethod": null, "memo": null, "lines": [{ "accountId": rent, "amountCents": 1_000, "memo": null }] });
    let monthly = save_recurring(&pool, None, RecurringInput { anchor_day: None, name: "Month end".into(), kind: "EXPENSE".into(), payload: check.clone(), frequency: "MONTHLY".into(), every: 1, next_date: d(1, 31), end_date: None }).await.unwrap();
    let quarterly = save_recurring(&pool, None, RecurringInput { anchor_day: None, name: "Quarter end".into(), kind: "EXPENSE".into(), payload: check, frequency: "QUARTERLY".into(), every: 1, next_date: d(11, 30).with_year(2025).unwrap(), end_date: None }).await.unwrap();
    enter_due(&pool, d(5, 31), None).await.unwrap();
    let runs = |id: i64| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, NaiveDate>("SELECT run_date FROM recurring_runs WHERE template_id = ?1 ORDER BY run_date").bind(id).fetch_all(&pool).await.unwrap() }
    };
    assert_eq!(runs(monthly).await, vec![d(1, 31), d(2, 28), d(3, 31), d(4, 30), d(5, 31)]);
    // November 30 → February 28 → May 30: the 30th, not the 28th.
    assert_eq!(runs(quarterly).await, vec![d(11, 30).with_year(2025).unwrap(), d(2, 28), d(5, 30)]);
    let next: NaiveDate = sqlx::query_scalar("SELECT next_date FROM recurring_templates WHERE id = ?1").bind(monthly).fetch_one(&pool).await.unwrap();
    assert_eq!(next, d(6, 30));

    // Saved again on June 30 (renamed, same next date), it still goes to July 31.
    let check = serde_json::json!({ "kind": "CHECK", "payeeName": "Landlord", "paymentAccountId": checking, "expenseDate": "2026-01-31",
        "checkNumber": null, "paymentMethod": null, "memo": null, "lines": [{ "accountId": rent, "amountCents": 1_000, "memo": null }] });
    save_recurring(&pool, Some(monthly), RecurringInput { anchor_day: None, name: "Rent at month end".into(), kind: "EXPENSE".into(), payload: check, frequency: "MONTHLY".into(), every: 1, next_date: d(6, 30), end_date: None }).await.unwrap();
    enter_due(&pool, d(7, 31), Some(monthly)).await.unwrap();
    assert_eq!(runs(monthly).await.last().copied(), Some(d(7, 31)));
}

#[tokio::test]
async fn memorizing_a_transaction_from_the_31st_keeps_its_day_though_it_first_repeats_on_the_28th() {
    let pool = test_pool().await;
    let (checking, rent) = (account(&pool, "1010").await, account(&pool, "6300").await);
    let check = serde_json::json!({ "kind": "CHECK", "payeeName": "Landlord", "paymentAccountId": checking, "expenseDate": "2026-01-31",
        "checkNumber": null, "paymentMethod": null, "memo": null, "lines": [{ "accountId": rent, "amountCents": 1_000, "memo": null }] });
    // Memorize on a January 31 check: Next date is February 28, the day kept is the 31st.
    let memorized = save_recurring(&pool, None, RecurringInput { anchor_day: Some(31), name: "Rent".into(), kind: "EXPENSE".into(), payload: check.clone(), frequency: "MONTHLY".into(), every: 1, next_date: d(2, 28), end_date: None }).await.unwrap();
    // A Next date typed over to the 15th doesn't match the 31st, so the 15th is kept.
    let typed = save_recurring(&pool, None, RecurringInput { anchor_day: Some(31), name: "Typed".into(), kind: "EXPENSE".into(), payload: check, frequency: "MONTHLY".into(), every: 1, next_date: d(2, 15), end_date: None }).await.unwrap();
    enter_due(&pool, d(4, 30), None).await.unwrap();
    let runs = |id: i64| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, NaiveDate>("SELECT run_date FROM recurring_runs WHERE template_id = ?1 ORDER BY run_date").bind(id).fetch_all(&pool).await.unwrap() }
    };
    assert_eq!(runs(memorized).await, vec![d(2, 28), d(3, 31), d(4, 30)]);
    assert_eq!(runs(typed).await, vec![d(2, 15), d(3, 15), d(4, 15)]);
}

#[tokio::test]
async fn enter_all_due_skips_a_copy_in_a_closed_period_lists_it_and_enters_the_rest() {
    let pool = test_pool().await;
    let (checking, rent) = (account(&pool, "1010").await, account(&pool, "6300").await);
    let check = |date: &str| serde_json::json!({ "kind": "CHECK", "payeeName": "Landlord", "paymentAccountId": checking, "expenseDate": date,
        "checkNumber": null, "paymentMethod": null, "memo": null, "lines": [{ "accountId": rent, "amountCents": 1_000, "memo": null }] });
    let schedule = |name: &str, next: NaiveDate| RecurringInput { anchor_day: None, name: name.into(), kind: "EXPENSE".into(), payload: check("2026-01-01"), frequency: "MONTHLY".into(), every: 1, next_date: next, end_date: None };
    let late = save_recurring(&pool, None, schedule("Old rent", d(2, 15))).await.unwrap();
    let ok = save_recurring(&pool, None, schedule("New rent", d(4, 1))).await.unwrap();
    // The books are closed through March 31: February and March's copies of Old rent can't post.
    crate::closing::set_closing_date(&pool, &crate::closing::ClosingInput { lock_date: Some(d(3, 31)), current_passcode: None, new_passcode: None }).await.unwrap();

    let report = enter_due_report(&pool, d(4, 20), None).await.unwrap();
    assert_eq!(report.entered, 1, "New rent's April copy");
    assert_eq!(report.skipped.len(), 1);
    assert_eq!((report.skipped[0].template_id, report.skipped[0].name.as_str(), report.skipped[0].date), (late, "Old rent", d(2, 15)));
    assert!(!report.skipped[0].reason.is_empty());
    let next = |id: i64| {
        let pool = pool.clone();
        async move { sqlx::query_scalar::<_, NaiveDate>("SELECT next_date FROM recurring_templates WHERE id = ?1").bind(id).fetch_one(&pool).await.unwrap() }
    };
    assert_eq!((next(late).await, next(ok).await), (d(2, 15), d(5, 1)), "the skipped schedule waits on the copy it couldn't enter");
    assert_eq!(balance(&pool, rent).await, 1_000, "nothing half-entered");

    // Edit…: rename Old rent and move it past the closed period; Enter all due then enters it.
    edit_recurring_schedule(&pool, late, &ScheduleInput { name: "Storage rent".into(), frequency: "MONTHLY".into(), every: 1, next_date: d(4, 15), end_date: None, anchor_day: None }).await.unwrap();
    let again = enter_due_report(&pool, d(4, 20), None).await.unwrap();
    assert_eq!((again.entered, again.skipped.len()), (1, 0));
    assert_eq!(list_recurring(&pool).await.unwrap().iter().find(|t| t.id == late).unwrap().name, "Storage rent");
    assert!(edit_recurring_schedule(&pool, late, &ScheduleInput { name: " ".into(), frequency: "MONTHLY".into(), every: 1, next_date: d(5, 15), end_date: None, anchor_day: None }).await.unwrap_err().starts_with("REC_NAME_REQUIRED"));
}

#[tokio::test]
async fn a_memorized_transactions_lines_can_be_changed_and_only_later_copies_follow() {
    let pool = test_pool().await;
    let (checking, rent, supplies) = (account(&pool, "1010").await, account(&pool, "6300").await, account(&pool, "6250").await);
    let check = |account: i64, cents: i64| serde_json::json!({ "kind": "CHECK", "payeeName": "Landlord", "paymentAccountId": checking, "expenseDate": "2026-01-31",
        "checkNumber": null, "paymentMethod": null, "memo": null, "lines": [{ "accountId": account, "amountCents": cents, "memo": null }] });
    let id = save_recurring(&pool, None, RecurringInput { anchor_day: Some(31), name: "Rent".into(), kind: "EXPENSE".into(), payload: check(rent, 100_000), frequency: "MONTHLY".into(), every: 1, next_date: d(1, 31), end_date: None }).await.unwrap();
    enter_due(&pool, d(2, 28), None).await.unwrap();
    assert_eq!(balance(&pool, rent).await, 200_000, "January and February at $1,000");

    // Edit transaction…: the form gets the memorized transaction back and saves a changed one.
    let template = get_recurring(&pool, id).await.unwrap();
    assert_eq!((template.kind.as_str(), template.next_date, template.payload["lines"][0]["amountCents"].as_i64()), ("EXPENSE", d(3, 31), Some(100_000)));
    save_recurring(&pool, Some(id), RecurringInput {
        anchor_day: None,
        name: template.name.clone(),
        kind: template.kind.clone(),
        payload: check(supplies, 120_000),
        frequency: template.frequency.clone(),
        every: template.every,
        next_date: template.next_date,
        end_date: template.end_date,
    })
    .await
    .unwrap();
    enter_due(&pool, d(4, 30), None).await.unwrap();
    assert_eq!(balance(&pool, rent).await, 200_000, "the copies already entered are untouched");
    assert_eq!(balance(&pool, supplies).await, 240_000, "March 31 and April 30 use the changed lines");
    assert!(get_recurring(&pool, 99_999).await.unwrap_err().starts_with("REC_NOT_FOUND"));
}

#[tokio::test]
async fn a_bill_from_a_purchase_order_takes_the_vendors_terms_and_the_bill_checks() {
    let pool = test_pool().await;
    let supplies = account(&pool, "6250").await;
    let terms: i64 = sqlx::query_scalar("INSERT INTO terms (name, net_days, discount_ppm, discount_days) VALUES ('2% 10 Net 30', 30, 20000, 10) RETURNING id").fetch_one(&pool).await.unwrap();
    let order = |pool: SqlitePool| async move {
        create_purchase_order(&pool, PurchaseOrderInput { form_lines: Vec::new(), vendor_name: "Staples".into(), po_date: d(5, 1), expected_date: None, memo: None,
            lines: vec![PurchaseOrderLineInput { account_id: supplies, product_service_id: None, description: Some("Paper".into()), quantity_e4: Some(10_000), rate_cents: Some(10_000), amount_cents: 10_000, class_id: None, customer_id: None }] }).await.unwrap()
    };
    let po = order(pool.clone()).await;
    sqlx::query("UPDATE vendors SET terms_id = ?1 WHERE name = 'Staples'").bind(terms).execute(&pool).await.unwrap();

    let bill = bill_purchase_order(&pool, po, d(5, 9), d(6, 8), Some("123".into())).await.unwrap();
    let kept: Option<i64> = sqlx::query_scalar("SELECT terms_id FROM bills WHERE id = ?1").bind(bill).fetch_one(&pool).await.unwrap();
    assert_eq!(kept, Some(terms), "the bill carries the vendor's terms, so Pay Bills offers the discount");

    // A second order billed with the same vendor bill number is caught like Enter Bills catches it.
    let again = order(pool.clone()).await;
    let input = purchase_order_bill(&pool, again, d(5, 10), d(6, 9), Some("123".into())).await.unwrap();
    assert_eq!(input.terms_id, Some(terms));
    let warned = crate::guards::check(&pool, crate::guards::bill(&input, None), None).await.unwrap_err();
    assert!(warned.starts_with("NEEDS_CONFIRMATION"), "{warned}");
}

#[tokio::test]
async fn a_typed_credit_memo_number_stays_below_the_unapplied_payment_range() {
    let pool = test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let err = crate::doc_numbers::check_number(&mut conn, crate::doc_numbers::DocKind::CreditMemo, crate::credits::PAYMENT_CREDIT_NUMBERS).await.unwrap_err();
    assert!(err.starts_with("DOC_NUMBER_RANGE") && err.contains("999999999"), "{err}");
    assert!(crate::doc_numbers::check_number(&mut conn, crate::doc_numbers::DocKind::CreditMemo, 42).await.is_ok());
    assert!(crate::doc_numbers::check_number(&mut conn, crate::doc_numbers::DocKind::Invoice, crate::credits::PAYMENT_CREDIT_NUMBERS).await.is_ok(), "other forms keep their range");
}
