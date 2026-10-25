use super::*;
use crate::db::test_pool;
use crate::invoices::{create_and_post_invoice, record_payment, InvoiceInput, InvoiceLineInput, PaymentInput};
use crate::ledger::void_transaction;

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
        .unwrap()
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
}

#[tokio::test]
async fn deposit_sweeps_undeposited_funds_and_other_lines() {
    let pool = test_pool().await;
    let (checking, undeposited, services, interest) =
        (account_id(&pool, "1010").await, account_id(&pool, "1300").await, account_id(&pool, "4100").await, account_id(&pool, "7000").await);
    let invoice = create_and_post_invoice(
        &pool,
        InvoiceInput {
            customer_name: "Acme".into(),
            issue_date: d(1),
            due_date: d(31),
            lines: vec![InvoiceLineInput{ description: "Work".into(), quantity_e4: 10000, rate_cents: 80_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        },
    )
    .await
    .unwrap();
    let pay = |cents| PaymentInput {
        invoice_id: invoice.id,
        deposit_account_id: undeposited,
        payment_date: d(3),
        amount_cents: cents,
        payment_method: "CHECK".into(),
        reference: None,
        exchange_rate_e6: None,
    };
    record_payment(&pool, pay(50_000)).await.unwrap();
    record_payment(&pool, pay(30_000)).await.unwrap();
    let waiting = list_undeposited(&pool).await.unwrap();
    assert_eq!(waiting.len(), 2);
    assert_eq!(balance(&pool, undeposited).await, 80_000);

    let deposit_input = |items: Vec<&UndepositedItem>| DepositInput {
        bank_account_id: checking,
        deposit_date: d(4),
        memo: Some("Branch deposit".into()),
        items: items.iter().map(|i| DepositItemRef { kind: i.kind.clone(), id: i.id }).collect(),
        other_lines: vec![DepositOtherLine { account_id: interest, amount_cents: 125, memo: Some("Interest".into()), class_id: None }],
    };
    let deposit_id = post_bank_deposit(&pool, deposit_input(waiting.iter().collect())).await.expect("deposit posts");
    assert_eq!(balance(&pool, checking).await, 80_125);
    assert_eq!(balance(&pool, undeposited).await, 0);
    assert!(list_undeposited(&pool).await.unwrap().is_empty());

    // The same payment cannot be deposited twice, and a deposited payment cannot be voided.
    let err = post_bank_deposit(&pool, deposit_input(waiting.iter().take(1).collect())).await.expect_err("double deposit");
    assert!(err.starts_with("DEP_ALREADY_DEPOSITED"), "{err}");
    let payment_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM invoice_payments ORDER BY id LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    let err = void_transaction(&pool, payment_entry).await.expect_err("deposited payment");
    assert!(err.starts_with("VOID_DEPOSITED"), "{err}");

    // Voiding the deposit puts the payments back into Undeposited Funds.
    let deposit_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM deposits WHERE id = ?1")
        .bind(deposit_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    void_transaction(&pool, deposit_entry).await.expect("deposit voids");
    assert_eq!(balance(&pool, checking).await, 0);
    assert_eq!(balance(&pool, undeposited).await, 80_000);
    assert_eq!(list_undeposited(&pool).await.unwrap().len(), 2);
    void_transaction(&pool, payment_entry).await.expect("payment voids once undeposited");
}

#[tokio::test]
async fn transfer_moves_money_between_accounts() {
    let pool = test_pool().await;
    let (checking, savings, card, ar) =
        (account_id(&pool, "1010").await, account_id(&pool, "1020").await, account_id(&pool, "2100").await, account_id(&pool, "1200").await);
    let xfer = |from, to, cents| TransferInput { from_account_id: from, to_account_id: to, transfer_date: d(2), amount_cents: cents, memo: None };

    post_transfer(&pool, xfer(savings, checking, 250_000)).await.expect("transfer posts");
    post_transfer(&pool, xfer(checking, card, 40_000)).await.expect("card payment posts");
    assert_eq!((balance(&pool, checking).await, balance(&pool, savings).await, balance(&pool, card).await), (210_000, -250_000, -40_000));

    assert!(post_transfer(&pool, xfer(checking, checking, 1)).await.unwrap_err().starts_with("XFER_SAME_ACCOUNT"));
    assert!(post_transfer(&pool, xfer(checking, ar, 1)).await.unwrap_err().starts_with("XFER_BAD_ACCOUNT"));
    let activity = list_banking_activity(&pool).await.unwrap();
    assert_eq!(activity.len(), 2);
    assert!(activity.iter().all(|a| a.kind == "TRANSFER"));
}
