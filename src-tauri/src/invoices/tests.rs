use super::*;
use crate::db::test_pool;

async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1")
        .bind(number)
        .fetch_one(pool)
        .await
        .expect("seeded account exists")
}

async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1")
        .bind(account)
        .fetch_one(pool)
        .await
        .expect("balance")
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).expect("valid date")
}

fn payment(invoice_id: i64, account: i64, day: u32, cents: i64) -> PaymentInput {
    PaymentInput {
        invoice_id,
        deposit_account_id: account,
        payment_date: d(day),
        amount_cents: cents,
        payment_method: "CHECK".into(),
        reference: Some("1042".into()),
        exchange_rate_e6: None,
    }
}

async fn simple_invoice(pool: &SqlitePool, customer: &str, issue: NaiveDate, due: NaiveDate, cents: i64) -> InvoiceSummary {
    let services = account_id(pool, "4100").await;
    create_and_post_invoice(
        pool,
        InvoiceInput {
            customer_name: customer.into(),
            issue_date: issue,
            due_date: due,
            lines: vec![InvoiceLineInput{ description: "Work".into(), quantity_e4: 10000, rate_cents: cents, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        },
    )
    .await
    .expect("invoice posts")
}

#[tokio::test]
async fn invoice_posts_to_ar_and_income_then_payment_clears_ar() {
    let pool = test_pool().await;
    let (ar, checking, sales, services) = (
        account_id(&pool, "1200").await,
        account_id(&pool, "1010").await,
        account_id(&pool, "4000").await,
        account_id(&pool, "4100").await,
    );

    let input = InvoiceInput {
        customer_name: "Acme Corp".into(),
        issue_date: d(1),
        due_date: d(31),
        lines: vec![
            InvoiceLineInput{ description: "Consulting".into(), quantity_e4: 30000, rate_cents: 15_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false },
            InvoiceLineInput{ description: "Widget".into(), quantity_e4: 10000, rate_cents: 2_500, income_account_id: sales, product_service_id: None, class_id: None, is_taxable: false },
            InvoiceLineInput{ description: "Support".into(), quantity_e4: 20000, rate_cents: 1_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false },
        ],
        currency: None,
        tax_rate_id: None,
        discount_cents: 0,
        details: Default::default(),
    };
    let invoice = create_and_post_invoice(&pool, input).await.expect("invoice posts");

    assert_eq!(invoice.status, "UNPAID");
    assert_eq!(invoice.invoice_number, 1001);
    assert_eq!(invoice.total_cents, 49_500);
    assert_eq!(invoice.item_count, 3);
    assert_eq!(balance(&pool, ar).await, 49_500);
    assert_eq!(balance(&pool, services).await, 47_000);
    assert_eq!(balance(&pool, sales).await, 2_500);

    let paid = record_payment(&pool, payment(invoice.id, checking, 15, 49_500)).await.expect("payment posts");
    assert_eq!(paid.status, "PAID");
    assert_eq!(paid.balance_cents, 0);
    assert_eq!(paid.last_payment_date.as_deref(), Some("2026-10-15"));
    assert_eq!(balance(&pool, ar).await, 0);
    assert_eq!(balance(&pool, checking).await, 49_500);

    let err = record_payment(&pool, payment(invoice.id, checking, 16, 100)).await.expect_err("cannot pay twice");
    assert!(err.starts_with("INV_NOT_OPEN"), "{err}");
}

#[tokio::test]
async fn partial_payments_track_remaining_balance() {
    let pool = test_pool().await;
    let (ar, checking) = (account_id(&pool, "1200").await, account_id(&pool, "1010").await);
    let invoice = simple_invoice(&pool, "Globex", d(1), d(31), 100_000).await;

    let first = record_payment(&pool, payment(invoice.id, checking, 5, 30_000)).await.expect("first payment");
    assert_eq!((first.status.as_str(), first.amount_paid_cents, first.balance_cents), ("PARTIAL", 30_000, 70_000));

    let err = record_payment(&pool, payment(invoice.id, checking, 6, 70_001)).await.expect_err("overpayment");
    assert!(err.starts_with("INV_OVERPAYMENT"), "{err}");

    let second = record_payment(&pool, payment(invoice.id, checking, 7, 45_000)).await.expect("second payment");
    assert_eq!((second.status.as_str(), second.balance_cents), ("PARTIAL", 25_000));

    let last = record_payment(&pool, payment(invoice.id, checking, 9, 25_000)).await.expect("final payment");
    assert_eq!((last.status.as_str(), last.balance_cents), ("PAID", 0));

    let payments = list_payments(&pool, invoice.id).await.unwrap();
    assert_eq!(payments.iter().map(|p| p.amount_cents).collect::<Vec<_>>(), vec![30_000, 45_000, 25_000]);
    assert!(payments.iter().all(|p| p.journal_entry_id.is_some()));
    assert_eq!(balance(&pool, ar).await, 0);
    assert_eq!(balance(&pool, checking).await, 100_000);

    // The schema itself refuses to let amount_paid drift from the payment rows.
    let tamper = sqlx::query("UPDATE invoices SET amount_paid_cents = 1 WHERE id = ?1")
        .bind(invoice.id)
        .execute(&pool)
        .await
        .expect_err("trigger blocks tampering");
    assert!(tamper.to_string().contains("INV_PAID_MISMATCH"), "{tamper}");
}

#[tokio::test]
async fn aging_buckets_open_balances_by_days_overdue() {
    let pool = test_pool().await;
    let checking = account_id(&pool, "1010").await;
    let as_of = d(31);
    let sept = |day| NaiveDate::from_ymd_opt(2026, 9, day).unwrap();
    let aug = |day| NaiveDate::from_ymd_opt(2026, 8, day).unwrap();

    simple_invoice(&pool, "Not due", d(20), NaiveDate::from_ymd_opt(2026, 11, 19).unwrap(), 10_000).await;
    simple_invoice(&pool, "Due today", d(1), d(31), 20_000).await;
    simple_invoice(&pool, "1 day late", d(1), d(30), 30_000).await;
    let partial = simple_invoice(&pool, "30 days late", sept(1), d(1), 40_000).await;
    record_payment(&pool, payment(partial.id, checking, 2, 15_000)).await.unwrap();
    simple_invoice(&pool, "31 days late", aug(30), sept(30), 50_000).await;
    simple_invoice(&pool, "61 days late", aug(1), aug(31), 60_000).await;
    let settled = simple_invoice(&pool, "Paid", aug(1), aug(2), 70_000).await;
    record_payment(&pool, payment(settled.id, checking, 1, 70_000)).await.unwrap();

    let aging = ar_aging(&pool, as_of).await.unwrap();
    let summary: Vec<(&str, i64, i64)> = aging.buckets.iter().map(|b| (b.key, b.invoice_count, b.amount_cents)).collect();
    assert_eq!(
        summary,
        vec![("current", 2, 30_000), ("days1To30", 2, 55_000), ("days31To60", 1, 50_000), ("days61Plus", 1, 60_000)]
    );
    assert_eq!(aging.total_cents, 195_000);
    assert_eq!(balance(&pool, account_id(&pool, "1200").await).await, aging.total_cents);
}

#[tokio::test]
async fn non_income_account_rolls_back_everything() {
    let pool = test_pool().await;
    let services = account_id(&pool, "4100").await;
    let rent = account_id(&pool, "6300").await;
    let input = InvoiceInput {
        customer_name: "Acme Corp".into(),
        issue_date: d(1),
        due_date: d(31),
        lines: vec![
            InvoiceLineInput{ description: "Consulting".into(), quantity_e4: 10000, rate_cents: 10_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false },
            InvoiceLineInput{ description: "Oops".into(), quantity_e4: 10000, rate_cents: 10_000, income_account_id: rent, product_service_id: None, class_id: None, is_taxable: false },
        ],
        currency: None,
        tax_rate_id: None,
        discount_cents: 0,
        details: Default::default(),
    };

    let err = create_and_post_invoice(&pool, input).await.expect_err("expense account rejected");
    assert!(err.starts_with("INV_NOT_INCOME_ACCOUNT: line 2"), "{err}");

    let (invoices, items, entries): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM invoices), (SELECT COUNT(*) FROM invoice_items), (SELECT COUNT(*) FROM journal_entries)",
    )
    .fetch_one(&pool)
    .await
    .expect("counts");
    assert_eq!((invoices, items, entries), (0, 0, 0));
}

#[tokio::test]
async fn invoice_detail_has_items_and_live_payments() {
    let pool = test_pool().await;
    let checking = account_id(&pool, "1010").await;
    let invoice = simple_invoice(&pool, "Print Co", d(1), d(31), 25_000).await;
    record_payment(&pool, payment(invoice.id, checking, 2, 10_000)).await.unwrap();
    let detail = invoice_detail(&pool, invoice.id).await.unwrap();
    assert_eq!(detail.items.len(), 1);
    assert_eq!(detail.items[0].amount_cents, 25_000);
    assert_eq!(detail.payments.iter().map(|p| p.amount_cents).sum::<i64>(), 10_000);
    assert_eq!(detail.invoice.balance_cents, 15_000);
}

#[test]
fn scaled_quantities_round_half_up_and_reject_overflow() {
    assert_eq!(line_amount(30_000, 15_000), Ok(45_000), "3 × $150.00");
    assert_eq!(line_amount(15_000, 12_500), Ok(18_750), "1.5 hours × $125.00");
    assert_eq!(line_amount(27_500, 3_333), Ok(9_166), "2.75 × $33.33 = $91.6575 → $91.66");
    assert_eq!(line_amount(5_000, 1), Ok(1), "0.5 × $0.01 = $0.005 rounds up to $0.01");
    assert!(line_amount(4_999, 1).unwrap_err().starts_with("INV_AMOUNT_ROUNDS_TO_ZERO"));
    assert!(line_amount(MAX_QUANTITY_E4, MAX_LINE_CENTS).is_err());
    assert!(line_amount(i64::MAX, i64::MAX).is_err(), "128-bit math cannot overflow silently");
    assert!(validate_line(0, 100, 1, "INV").unwrap_err().starts_with("INV_BAD_QUANTITY"));
}

#[tokio::test]
async fn fractional_quantities_post_rounded_amounts() {
    let pool = test_pool().await;
    let (ar, services) = (account_id(&pool, "1200").await, account_id(&pool, "4100").await);
    let invoice = create_and_post_invoice(
        &pool,
        InvoiceInput {
            customer_name: "Hourly Client".into(),
            issue_date: d(1),
            due_date: d(31),
            lines: vec![
                InvoiceLineInput{ description: "Consulting".into(), quantity_e4: 15_000, rate_cents: 12_500, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false },
                InvoiceLineInput{ description: "Review".into(), quantity_e4: 27_500, rate_cents: 3_333, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false },
            ],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        },
    )
    .await
    .expect("fractional invoice posts");
    assert_eq!(invoice.total_cents, 18_750 + 9_166);
    assert_eq!(balance(&pool, ar).await, 27_916);
    let detail = invoice_detail(&pool, invoice.id).await.unwrap();
    assert_eq!(detail.items.iter().map(|i| i.quantity_e4).collect::<Vec<_>>(), vec![15_000, 27_500]);
    // The database enforces the rounding rule independently.
    let err = sqlx::query("UPDATE invoice_items SET amount_cents = 9_165 WHERE invoice_id = ?1 AND line_number = 2")
        .bind(invoice.id)
        .execute(&pool)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("INV_IMMUTABLE") || err.to_string().contains("CHECK"), "{err}");
}

#[tokio::test]
async fn foreign_invoice_books_home_ar_and_payments_realize_fx() {
    let pool = test_pool().await;
    let (ar, checking, services) = (account_id(&pool, "1200").await, account_id(&pool, "1010").await, account_id(&pool, "4100").await);
    let fx_account: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = 'Exchange Gain or Loss' AND is_system = 1")
        .fetch_one(&pool)
        .await
        .expect("exchange account seeded");

    // EUR 1,000.00 at 1.08 → A/R and income of $1,080.00.
    let invoice = create_and_post_invoice(
        &pool,
        InvoiceInput {
            customer_name: "Berlin GmbH".into(),
            issue_date: d(1),
            due_date: d(31),
            lines: vec![InvoiceLineInput{ description: "Project".into(), quantity_e4: 10_000, rate_cents: 100_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
            currency: Some(ForeignCurrency { code: "EUR".into(), exchange_rate_e6: 1_080_000 }),
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        },
    )
    .await
    .unwrap();
    assert_eq!((invoice.currency_code.as_deref(), invoice.total_cents, invoice.home_total_cents), (Some("EUR"), 100_000, 108_000));
    assert_eq!((balance(&pool, ar).await, balance(&pool, services).await), (108_000, 108_000));

    let pay = |day, cents, rate| PaymentInput {
        invoice_id: invoice.id,
        deposit_account_id: checking,
        payment_date: d(day),
        amount_cents: cents,
        payment_method: "WIRE".into(),
        reference: None,
        exchange_rate_e6: Some(rate),
    };
    // EUR 400 at 1.10 → $440 received; A/R relieved 40% of $1,080 = $432; $8 gain.
    let partial = record_payment(&pool, pay(10, 40_000, 1_100_000)).await.unwrap();
    assert_eq!((partial.status.as_str(), partial.balance_cents, partial.home_balance_cents), ("PARTIAL", 60_000, 64_800));
    assert_eq!((balance(&pool, checking).await, balance(&pool, ar).await, balance(&pool, fx_account).await), (44_000, 64_800, -800));

    // EUR 600 at 1.05 → $630 received; final payment relieves the remaining $648 exactly; $18 loss.
    let paid = record_payment(&pool, pay(20, 60_000, 1_050_000)).await.unwrap();
    assert_eq!((paid.status.as_str(), paid.home_balance_cents), ("PAID", 0));
    assert_eq!((balance(&pool, ar).await, balance(&pool, checking).await, balance(&pool, fx_account).await), (0, 107_000, 1_000));
    assert_eq!(ar_aging(&pool, d(31)).await.unwrap().total_cents, 0);

    // Voiding the last payment reverses its home-currency effect and reopens the invoice.
    let last = list_payments(&pool, invoice.id).await.unwrap().pop().unwrap();
    crate::ledger::void_transaction(&pool, last.journal_entry_id.unwrap()).await.unwrap();
    let reopened = get_summary(&pool, invoice.id).await.unwrap();
    assert_eq!((reopened.status.as_str(), reopened.balance_cents, reopened.home_balance_cents), ("PARTIAL", 60_000, 64_800));
    assert_eq!((balance(&pool, ar).await, balance(&pool, fx_account).await), (64_800, -800));

    let err = record_payment(&pool, PaymentInput { exchange_rate_e6: Some(1_000_000), ..pay(21, 100, 1) }).await;
    assert!(err.is_ok(), "a foreign invoice accepts an explicit rate");
    let tb = crate::reports::trial_balance(&pool).await.unwrap();
    assert_eq!(tb.total_debit_cents, tb.total_credit_cents);
}

#[tokio::test]
async fn foreign_payment_after_voided_partial_payments_is_accepted() {
    // Found by the ledger property tests: EUR 0.27 at 0.7579 books $0.20 of A/R. Paying 21, 1,
    // 3, 1 and 1 cents relieves 16, 0, 3, 0 and 1 cents; voiding two of the one-cent payments
    // leaves 20 cents relieved against 25 paid, more than the proportional 19 for the next
    // payment, whose relief used to come out at -1 cent and was refused by the database.
    let pool = test_pool().await;
    let (ar, checking, services) = (account_id(&pool, "1200").await, account_id(&pool, "1010").await, account_id(&pool, "4100").await);
    let rate = 757_900;
    let invoice = create_and_post_invoice(
        &pool,
        InvoiceInput {
            customer_name: "Lyon SARL".into(),
            issue_date: d(1),
            due_date: d(31),
            lines: vec![InvoiceLineInput{ description: "Sample".into(), quantity_e4: 10_000, rate_cents: 27, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
            currency: Some(ForeignCurrency { code: "EUR".into(), exchange_rate_e6: rate }),
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        },
    )
    .await
    .unwrap();
    assert_eq!(invoice.home_total_cents, 20);
    let pay = |cents| PaymentInput { exchange_rate_e6: Some(rate), ..payment(invoice.id, checking, 2, cents) };
    for cents in [21, 1, 3, 1, 1] {
        record_payment(&pool, pay(cents)).await.unwrap();
    }
    let payments = list_payments(&pool, invoice.id).await.unwrap();
    assert_eq!(payments.iter().map(|p| p.ar_relief_cents.unwrap()).collect::<Vec<_>>(), [16, 0, 3, 0, 1]);
    for p in [&payments[1], &payments[3]] {
        crate::ledger::void_transaction(&pool, p.journal_entry_id.unwrap()).await.unwrap();
    }
    assert_eq!(balance(&pool, ar).await, 0, "the voided payments relieved nothing");

    let summary = record_payment(&pool, pay(1)).await.expect("the payment is accepted");
    assert_eq!((summary.balance_cents, summary.home_balance_cents, balance(&pool, ar).await), (1, 0, 0));
    let last = record_payment(&pool, pay(1)).await.expect("the final payment is accepted");
    assert_eq!((last.status.as_str(), last.home_balance_cents, balance(&pool, ar).await), ("PAID", 0, 0));
    let tb = crate::reports::trial_balance(&pool).await.unwrap();
    assert_eq!(tb.total_debit_cents, tb.total_credit_cents);
}

#[tokio::test]
async fn foreign_payment_deposits_its_home_amount() {
    let pool = test_pool().await;
    let (checking, undeposited, services) = (account_id(&pool, "1010").await, account_id(&pool, "1300").await, account_id(&pool, "4100").await);
    let invoice = create_and_post_invoice(
        &pool,
        InvoiceInput {
            customer_name: "London Ltd".into(),
            issue_date: d(1),
            due_date: d(31),
            lines: vec![InvoiceLineInput{ description: "Work".into(), quantity_e4: 10_000, rate_cents: 50_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
            currency: Some(ForeignCurrency { code: "GBP".into(), exchange_rate_e6: 1_270_000 }),
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        },
    )
    .await
    .unwrap();
    record_payment(
        &pool,
        PaymentInput { invoice_id: invoice.id, deposit_account_id: undeposited, payment_date: d(5), amount_cents: 50_000, payment_method: "CHECK".into(), reference: None, exchange_rate_e6: None },
    )
    .await
    .expect_err("a foreign payment needs its own rate");
    record_payment(
        &pool,
        PaymentInput { invoice_id: invoice.id, deposit_account_id: undeposited, payment_date: d(5), amount_cents: 50_000, payment_method: "CHECK".into(), reference: None, exchange_rate_e6: Some(1_270_000) },
    )
    .await
    .unwrap();
    let waiting = crate::banking::list_undeposited(&pool).await.unwrap();
    assert_eq!(waiting[0].amount_cents, 63_500, "GBP 500 at the invoice rate 1.27");
    crate::banking::post_bank_deposit(
        &pool,
        crate::banking::DepositInput {
            bank_account_id: checking,
            deposit_date: d(6),
            memo: None,
            items: vec![crate::banking::DepositItemRef { kind: "PAYMENT".into(), id: waiting[0].id }],
            other_lines: vec![],
        },
    )
    .await
    .expect("home amount deposits");
    assert_eq!((balance(&pool, checking).await, balance(&pool, undeposited).await), (63_500, 0));
}
