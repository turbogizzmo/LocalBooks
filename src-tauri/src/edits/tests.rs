use super::*;
use crate::db::test_pool;
use chrono::NaiveDate;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn balance(pool: &SqlitePool, account: i64) -> i64 {
    sqlx::query_scalar("SELECT COALESCE(SUM(debit_cents - credit_cents), 0) FROM v_posted_lines WHERE account_id = ?1").bind(account).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn edits_void_and_repost_and_respect_payments() {
    let pool = test_pool().await;
    let (checking, ar, services, rent) = (account(&pool, "1010").await, account(&pool, "1200").await, account(&pool, "4100").await, account(&pool, "6300").await);
    let invoice = |cents: i64| -> InvoiceInput {
        serde_json::from_value(serde_json::json!({ "customerName": "Acme", "issueDate": "2026-05-01", "dueDate": "2026-05-31",
            "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": services }] }))
        .unwrap()
    };
    let original = crate::invoices::create_and_post_invoice(&pool, invoice(10_000)).await.unwrap();
    let edited = replace_invoice(&pool, original.id, invoice(12_500)).await.unwrap();
    assert_eq!(balance(&pool, ar).await, 12_500, "only the edited invoice counts");
    let voided: bool = sqlx::query_scalar("SELECT is_voided FROM invoices WHERE id = ?1").bind(original.id).fetch_one(&pool).await.unwrap();
    assert!(voided);
    let edits: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transaction_edits").fetch_one(&pool).await.unwrap();
    assert_eq!(edits, 1);

    // Once paid, it must be unpaid before it can change.
    crate::invoices::record_payment(&pool, crate::invoices::PaymentInput { invoice_id: edited, deposit_account_id: checking, payment_date: NaiveDate::from_ymd_opt(2026, 5, 9).unwrap(), amount_cents: 100, payment_method: "CASH".into(), reference: None, exchange_rate_e6: None }).await.unwrap();
    let err = replace_invoice(&pool, edited, invoice(1)).await.unwrap_err();
    assert!(err.starts_with("VOID_HAS_PAYMENTS"), "{err}");
    assert_eq!(balance(&pool, ar).await, 12_400, "a refused edit changes nothing");

    let expense: ExpenseInput = serde_json::from_value(serde_json::json!({ "kind": "CHECK", "payeeName": "Landlord", "paymentAccountId": checking, "expenseDate": "2026-05-02",
        "checkNumber": "101", "paymentMethod": null, "memo": null, "lines": [{ "accountId": rent, "amountCents": 90_000, "memo": "May" }] })).unwrap();
    let (expense_id, _) = { let mut tx = crate::ledger::begin_write(&pool).await.unwrap(); let r = crate::expenses::write_expense(&mut tx, &expense).await.unwrap(); crate::ledger::commit_write(tx).await.unwrap(); r };
    assert_eq!(document_lines(&pool, "expense", expense_id).await.unwrap()[0].memo.as_deref(), Some("May"));
    let mut fixed = expense.clone();
    fixed.lines[0].amount_cents = 95_000;
    replace_expense(&pool, expense_id, fixed).await.unwrap();
    assert_eq!(balance(&pool, rent).await, 95_000);

    let je = crate::ledger::post_journal_entry(&pool, JournalEntry { id: None, date: NaiveDate::from_ymd_opt(2026, 5, 3).unwrap(), description: "Accrual".into(),
        lines: vec![crate::ledger::JournalLine::debit(rent, 500), crate::ledger::JournalLine::credit(checking, 500)], currency: None }).await.unwrap();
    let new_je = replace_journal_entry(&pool, je, JournalEntry { id: None, date: NaiveDate::from_ymd_opt(2026, 5, 3).unwrap(), description: "Accrual".into(),
        lines: vec![crate::ledger::JournalLine::debit(rent, 700), crate::ledger::JournalLine::credit(checking, 700)], currency: None }).await.unwrap();
    assert_eq!(balance(&pool, rent).await, 95_700);
    let invoice_entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM invoices WHERE id = ?1").bind(edited).fetch_one(&pool).await.unwrap();
    assert!(replace_journal_entry(&pool, invoice_entry, JournalEntry { id: None, date: NaiveDate::from_ymd_opt(2026, 5, 3).unwrap(), description: String::new(), lines: vec![], currency: None })
        .await
        .unwrap_err()
        .starts_with("EDIT_NOT_MANUAL"));
    assert!(new_je > je);
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}
