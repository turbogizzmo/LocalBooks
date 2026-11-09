use super::*;
use crate::db::test_pool;

fn d(text: &str) -> NaiveDate {
    text.parse().unwrap()
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn invoice(pool: &SqlitePool, customer: &str, cents: i64, due: &str) -> i64 {
    let income = account(pool, "4100").await;
    let input: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({ "customerName": customer, "issueDate": "2026-10-01", "dueDate": due,
        "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": cents, "incomeAccountId": income, "isTaxable": false }] }))
    .unwrap();
    crate::invoices::create_and_post_invoice(pool, input).await.unwrap().id
}

async fn bill(pool: &SqlitePool, vendor: &str, cents: i64, due: &str) {
    let rent = account(pool, "6300").await;
    let input: crate::expenses::BillInput = serde_json::from_value(serde_json::json!({ "vendorName": vendor, "billNumber": vendor, "billDate": "2026-10-01", "dueDate": due,
        "totalCents": cents, "lines": [{ "accountId": rent, "amountCents": cents }] }))
    .unwrap();
    crate::expenses::post_vendor_bill(pool, input).await.unwrap();
}

#[tokio::test]
async fn money_due_in_and_out_lands_in_its_week_and_ties_out_to_the_agings() {
    let pool = test_pool().await;
    let checking = account(&pool, "1010").await;
    let equity: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'EQUITY' ORDER BY id LIMIT 1").fetch_one(&pool).await.unwrap();
    let opening = crate::ledger::JournalEntry {
        id: None,
        date: d("2026-10-01"),
        description: "Opening cash".into(),
        lines: vec![crate::ledger::JournalLine::debit(checking, 500_000), crate::ledger::JournalLine::credit(equity, 500_000)],
        currency: None,
    };
    crate::ledger::post_journal_entry(&pool, opening).await.unwrap();
    invoice(&pool, "Acme", 100_000, "2026-11-01").await; // past due on Nov 9
    invoice(&pool, "Globex", 40_000, "2026-11-12").await; // week 1
    let partly = invoice(&pool, "Initech", 30_000, "2026-11-20").await; // week 2, 100.00 paid
    crate::invoices::record_payment(
        &pool,
        crate::invoices::PaymentInput { discount_cents: 0, payment_method_id: None, invoice_id: partly, deposit_account_id: checking, payment_date: d("2026-11-02"), amount_cents: 10_000, payment_method: "CHECK".into(), reference: None, exchange_rate_e6: None },
    )
    .await
    .unwrap();
    invoice(&pool, "Umbrella", 7_000, "2027-03-01").await; // later
    bill(&pool, "Landlord", 120_000, "2026-11-15").await; // week 1
    bill(&pool, "Power Co", 9_000, "2026-11-23").await; // week 3

    let f = forecast(&pool, d("2026-11-09"), 4).await.unwrap();
    assert_eq!((f.bank_cents, f.undeposited_cents, f.starting_cash_cents), (510_000, 0, 510_000));
    let cols: Vec<(&str, i64, i64)> = f.periods.iter().map(|p| (p.label.as_str(), p.money_in_cents, p.money_out_cents)).collect();
    assert_eq!(
        cols,
        vec![
            ("Past due", 100_000, 0),
            ("Week of Nov 9", 40_000, 120_000),
            ("Week of Nov 16", 20_000, 0),
            ("Week of Nov 23", 0, 9_000),
            ("Week of Nov 30", 0, 0),
            ("Later", 7_000, 0),
        ]
    );
    assert_eq!(f.periods.last().unwrap().ending_cash_cents, 510_000 + 167_000 - 129_000);
    assert_eq!(f.ending_cash_cents, f.periods.last().unwrap().ending_cash_cents);

    // It ties out: every dollar in is A/R, every dollar out is A/P, as the agings say.
    let ar = crate::report_center::aging(&pool, false, d("2026-11-09")).await.unwrap().total_cents;
    let ap = crate::report_center::aging(&pool, true, d("2026-11-09")).await.unwrap().total_cents;
    assert_eq!((f.receivable_cents, f.payable_cents), (ar, ap));
    assert_eq!(f.periods.iter().map(|p| p.money_in_cents).sum::<i64>(), ar);
    assert_eq!(f.periods.iter().map(|p| p.money_out_cents).sum::<i64>(), ap);
    assert_eq!(f.periods[1].items.iter().map(|i| (i.direction, i.party_name.as_str())).collect::<Vec<_>>(), [("IN", "Globex"), ("OUT", "Landlord")]);

    assert!(forecast(&pool, d("2026-11-09"), 0).await.unwrap_err().starts_with("FORECAST_WEEKS"));
}
