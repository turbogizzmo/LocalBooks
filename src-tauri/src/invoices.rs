//! Invoices: creation posts Dr Accounts Receivable / Cr income; each payment (full or partial) posts
//! Dr deposit / Cr A/R. Every document row and its journal entry are written in one transaction.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{
    db_err, format_cents, post_entry_in_tx, rollback, EntrySource, JournalEntry, JournalLine, MAX_LINE_CENTS,
};

pub const MAX_QUANTITY: i64 = 1_000_000_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceLineInput {
    pub description: String,
    pub quantity: i64,
    pub rate_cents: i64,
    pub income_account_id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceInput {
    pub customer_name: String,
    pub issue_date: NaiveDate,
    pub due_date: NaiveDate,
    pub lines: Vec<InvoiceLineInput>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceSummary {
    pub id: i64,
    pub invoice_number: i64,
    pub customer_name: String,
    pub issue_date: String,
    pub due_date: String,
    pub total_cents: i64,
    pub amount_paid_cents: i64,
    pub balance_cents: i64,
    pub status: String,
    pub journal_entry_id: Option<i64>,
    pub last_payment_date: Option<String>,
    pub item_count: i64,
    pub is_voided: bool,
}

const SUMMARY_SELECT: &str = "
    SELECT i.id, i.invoice_number, i.customer_name, i.issue_date, i.due_date, i.total_cents,
           i.amount_paid_cents, i.total_cents - i.amount_paid_cents AS balance_cents, i.status,
           i.journal_entry_id,
           (SELECT MAX(payment_date) FROM invoice_payments WHERE invoice_id = i.id AND is_voided = 0) AS last_payment_date,
           (SELECT COUNT(*) FROM invoice_items WHERE invoice_id = i.id) AS item_count,
           i.is_voided
    FROM invoices i";

pub const PAYMENT_METHODS: &[&str] = &["CASH", "CHECK", "CREDIT_CARD", "ACH", "WIRE", "OTHER"];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentInput {
    pub invoice_id: i64,
    pub deposit_account_id: i64,
    pub payment_date: NaiveDate,
    pub amount_cents: i64,
    pub payment_method: String,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PaymentView {
    pub id: i64,
    pub payment_date: String,
    pub amount_cents: i64,
    pub payment_method: String,
    pub reference: Option<String>,
    pub deposit_account_id: i64,
    pub journal_entry_id: Option<i64>,
    pub is_voided: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgingBucket {
    /// `current`, `days1To30`, `days31To60`, `days61Plus`
    pub key: &'static str,
    pub label: &'static str,
    pub invoice_count: i64,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArAging {
    pub as_of: NaiveDate,
    pub buckets: Vec<AgingBucket>,
    pub total_cents: i64,
}

/// `quantity * rate`, rejecting overflow and amounts beyond the per-line ledger ceiling.
pub fn line_amount(quantity: i64, rate_cents: i64) -> Result<i64, String> {
    quantity
        .checked_mul(rate_cents)
        .filter(|amount| *amount <= MAX_LINE_CENTS)
        .ok_or_else(|| format!("INV_AMOUNT_TOO_LARGE: line amount exceeds {}", format_cents(MAX_LINE_CENTS)))
}

/// Pure validation; returns the invoice total in cents.
pub fn validate_invoice(input: &InvoiceInput) -> Result<i64, String> {
    if input.customer_name.trim().is_empty() {
        return Err("INV_NO_CUSTOMER: enter a customer name".into());
    }
    if input.due_date < input.issue_date {
        return Err("INV_DUE_BEFORE_ISSUE: due date cannot be earlier than the invoice date".into());
    }
    if input.lines.is_empty() {
        return Err("INV_NO_ITEMS: an invoice requires at least one line item".into());
    }

    let mut total: i64 = 0;
    for (idx, line) in input.lines.iter().enumerate() {
        let n = idx + 1;
        if !(1..=MAX_QUANTITY).contains(&line.quantity) {
            return Err(format!("INV_BAD_QUANTITY: line {n} quantity must be a whole number from 1 to {MAX_QUANTITY}"));
        }
        if line.rate_cents <= 0 {
            return Err(format!("INV_BAD_RATE: line {n} rate must be greater than zero"));
        }
        let amount = line_amount(line.quantity, line.rate_cents).map_err(|e| format!("{e} (line {n})"))?;
        total = total
            .checked_add(amount)
            .filter(|t| *t <= MAX_LINE_CENTS)
            .ok_or_else(|| format!("INV_AMOUNT_TOO_LARGE: invoice total exceeds {}", format_cents(MAX_LINE_CENTS)))?;
    }
    Ok(total)
}

pub async fn list_invoices(pool: &SqlitePool) -> Result<Vec<InvoiceSummary>, String> {
    sqlx::query_as::<_, InvoiceSummary>(&format!("{SUMMARY_SELECT} ORDER BY i.is_voided, i.invoice_number DESC"))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

async fn get_summary(pool: &SqlitePool, invoice_id: i64) -> Result<InvoiceSummary, String> {
    sqlx::query_as::<_, InvoiceSummary>(&format!("{SUMMARY_SELECT} WHERE i.id = ?1"))
        .bind(invoice_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

/// Creates the invoice and posts it to the ledger atomically; the invoice comes back UNPAID.
pub async fn create_and_post_invoice(pool: &SqlitePool, input: InvoiceInput) -> Result<InvoiceSummary, String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let invoice_id = match write_invoice(&mut tx, &input).await {
        Ok(id) => id,
        Err(err) => return Err(rollback(tx, err).await),
    };
    tx.commit().await.map_err(db_err)?;
    get_summary(pool, invoice_id).await
}

pub(crate) async fn write_invoice(tx: &mut Transaction<'_, Sqlite>, input: &InvoiceInput) -> Result<i64, String> {
    let total = validate_invoice(input)?;
    let customer = input.customer_name.trim();
    let customer_id = crate::sales::find_or_create_named(tx, crate::sales::Party::Customer, customer).await?;

    let (invoice_id, invoice_number): (i64, i64) = sqlx::query_as(
        "INSERT INTO invoices (invoice_number, customer_name, issue_date, due_date, total_cents, status, customer_id)
         VALUES ((SELECT COALESCE(MAX(invoice_number), 1000) + 1 FROM invoices), ?1, ?2, ?3, ?4, 'DRAFT', ?5)
         RETURNING id, invoice_number",
    )
    .bind(customer)
    .bind(input.issue_date)
    .bind(input.due_date)
    .bind(total)
    .bind(customer_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    // Credits aggregated per income account, in first-appearance order.
    let mut credits: Vec<(i64, i64)> = Vec::new();

    for (idx, line) in input.lines.iter().enumerate() {
        let n = idx + 1;
        let account: Option<(String, i64)> = sqlx::query_as(
            "SELECT t.classification, a.is_active
             FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type
             WHERE a.id = ?1",
        )
        .bind(line.income_account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        match account {
            None => return Err(format!("INV_UNKNOWN_ACCOUNT: line {n} references an account that does not exist")),
            Some((classification, _)) if classification != "INCOME" => {
                return Err(format!("INV_NOT_INCOME_ACCOUNT: line {n} must use an Income account"))
            }
            Some((_, 0)) => return Err(format!("INV_INACTIVE_ACCOUNT: line {n} uses an inactive account")),
            Some(_) => {}
        }

        let amount = line_amount(line.quantity, line.rate_cents)?;
        sqlx::query(
            "INSERT INTO invoice_items
                (invoice_id, line_number, description, quantity, rate_cents, amount_cents, income_account_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(invoice_id)
        .bind(n as i64)
        .bind(line.description.trim())
        .bind(line.quantity)
        .bind(line.rate_cents)
        .bind(amount)
        .bind(line.income_account_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;

        match credits.iter_mut().find(|(id, _)| *id == line.income_account_id) {
            Some((_, sum)) => *sum += amount,
            None => credits.push((line.income_account_id, amount)),
        }
    }

    let ar_account = system_account(tx, "ACCOUNTS_RECEIVABLE").await?;
    let mut lines = vec![JournalLine::debit(ar_account, total)];
    lines.extend(credits.into_iter().map(|(account, cents)| JournalLine::credit(account, cents)));

    let entry = JournalEntry {
        id: None,
        date: input.issue_date,
        description: format!("Invoice #{invoice_number} - {customer}"),
        lines,
    };
    let entry_id = post_entry_in_tx(tx, &entry, EntrySource::Invoice(invoice_id)).await?;

    sqlx::query("UPDATE invoices SET status = 'UNPAID', journal_entry_id = ?1 WHERE id = ?2")
        .bind(entry_id)
        .bind(invoice_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;

    Ok(invoice_id)
}

/// Applies a full or partial payment to an open invoice. The invoice becomes PARTIAL while a balance
/// remains and PAID once the payments sum to the total; overpayment is rejected.
pub async fn record_payment(pool: &SqlitePool, input: PaymentInput) -> Result<InvoiceSummary, String> {
    let invoice_id = input.invoice_id;
    record_payments(pool, vec![input]).await?;
    get_summary(pool, invoice_id).await
}

/// Receive Payment: applies one customer payment across several invoices. All post, or none do.
pub async fn record_payments(pool: &SqlitePool, inputs: Vec<PaymentInput>) -> Result<usize, String> {
    if inputs.is_empty() {
        return Err("INV_NOTHING_TO_APPLY: enter an amount for at least one invoice".into());
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    for input in &inputs {
        if let Err(err) = write_payment(&mut tx, input).await {
            return Err(rollback(tx, err).await);
        }
    }
    tx.commit().await.map_err(db_err)?;
    Ok(inputs.len())
}

async fn write_payment(tx: &mut Transaction<'_, Sqlite>, input: &PaymentInput) -> Result<(), String> {
    if !PAYMENT_METHODS.contains(&input.payment_method.as_str()) {
        return Err(format!("INV_BAD_METHOD: unknown payment method '{}'", input.payment_method));
    }
    if input.amount_cents <= 0 {
        return Err("INV_BAD_AMOUNT: payment amount must be greater than zero".into());
    }

    let invoice: Option<(i64, String, i64, i64, String, String, bool)> = sqlx::query_as(
        "SELECT invoice_number, customer_name, total_cents, amount_paid_cents, status, issue_date, is_voided
         FROM invoices WHERE id = ?1",
    )
    .bind(input.invoice_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let (number, customer, total, paid, status, issue_date, voided) =
        invoice.ok_or_else(|| format!("INV_NOT_FOUND: invoice {} does not exist", input.invoice_id))?;
    if voided {
        return Err(format!("INV_NOT_OPEN: invoice #{number} is voided"));
    }
    if status != "UNPAID" && status != "PARTIAL" {
        return Err(format!("INV_NOT_OPEN: invoice #{number} is {status}; only open invoices accept payments"));
    }
    let open_balance = total - paid;
    if input.amount_cents > open_balance {
        return Err(format!(
            "INV_OVERPAYMENT: payment of {} exceeds the open balance of {} on invoice #{number}",
            format_cents(input.amount_cents),
            format_cents(open_balance)
        ));
    }
    if input.payment_date.format("%Y-%m-%d").to_string() < issue_date {
        return Err(format!("INV_PAYMENT_BEFORE_ISSUE: payment date is earlier than invoice #{number}'s date"));
    }

    let deposit_type: Option<String> = sqlx::query_scalar("SELECT account_type FROM chart_of_accounts WHERE id = ?1")
        .bind(input.deposit_account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    match deposit_type.as_deref() {
        Some("BANK") | Some("OTHER_CURRENT_ASSET") => {}
        Some(_) => return Err("INV_BAD_DEPOSIT_ACCOUNT: deposit to a Bank or Undeposited Funds account".into()),
        None => return Err("INV_BAD_DEPOSIT_ACCOUNT: deposit account does not exist".into()),
    }

    let reference = input.reference.as_deref().map(str::trim).filter(|r| !r.is_empty());
    let payment_id: i64 = sqlx::query_scalar(
        "INSERT INTO invoice_payments (invoice_id, payment_date, amount_cents, payment_method, reference, deposit_account_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         RETURNING id",
    )
    .bind(input.invoice_id)
    .bind(input.payment_date)
    .bind(input.amount_cents)
    .bind(&input.payment_method)
    .bind(reference)
    .bind(input.deposit_account_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    let ar_account = system_account(tx, "ACCOUNTS_RECEIVABLE").await?;
    let entry = JournalEntry {
        id: None,
        date: input.payment_date,
        description: format!("Payment - Invoice #{number} - {customer}"),
        lines: vec![
            JournalLine::debit(input.deposit_account_id, input.amount_cents),
            JournalLine::credit(ar_account, input.amount_cents),
        ],
    };
    let entry_id = post_entry_in_tx(tx, &entry, EntrySource::ReceivePayment(payment_id)).await?;

    sqlx::query("UPDATE invoice_payments SET journal_entry_id = ?1 WHERE id = ?2")
        .bind(entry_id)
        .bind(payment_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;

    let new_paid = paid + input.amount_cents;
    let new_status = if new_paid == total { "PAID" } else { "PARTIAL" };
    sqlx::query("UPDATE invoices SET amount_paid_cents = ?1, status = ?2 WHERE id = ?3")
        .bind(new_paid)
        .bind(new_status)
        .bind(input.invoice_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;

    Ok(())
}

pub async fn list_payments(pool: &SqlitePool, invoice_id: i64) -> Result<Vec<PaymentView>, String> {
    sqlx::query_as::<_, PaymentView>(
        "SELECT id, payment_date, amount_cents, payment_method, reference, deposit_account_id, journal_entry_id, is_voided
         FROM invoice_payments WHERE invoice_id = ?1 ORDER BY payment_date, id",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceItemView {
    pub line_number: i64,
    pub description: String,
    pub quantity: i64,
    pub rate_cents: i64,
    pub amount_cents: i64,
}

/// Everything needed to print an invoice.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceDetail {
    pub invoice: InvoiceSummary,
    pub items: Vec<InvoiceItemView>,
    pub payments: Vec<PaymentView>,
}

pub async fn invoice_detail(pool: &SqlitePool, invoice_id: i64) -> Result<InvoiceDetail, String> {
    let invoice = get_summary(pool, invoice_id).await?;
    let items = sqlx::query_as::<_, InvoiceItemView>(
        "SELECT line_number, description, quantity, rate_cents, amount_cents FROM invoice_items WHERE invoice_id = ?1 ORDER BY line_number",
    )
    .bind(invoice_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let payments = list_payments(pool, invoice_id).await?.into_iter().filter(|p| !p.is_voided).collect();
    Ok(InvoiceDetail { invoice, items, payments })
}

/// Groups open invoice balances by days past due as of `as_of`.
pub async fn ar_aging(pool: &SqlitePool, as_of: NaiveDate) -> Result<ArAging, String> {
    let open: Vec<(NaiveDate, i64)> = sqlx::query_as(
        "SELECT due_date, total_cents - amount_paid_cents FROM invoices WHERE status IN ('UNPAID', 'PARTIAL') AND is_voided = 0",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    let mut buckets = vec![
        AgingBucket { key: "current", label: "Current", invoice_count: 0, amount_cents: 0 },
        AgingBucket { key: "days1To30", label: "1–30 days", invoice_count: 0, amount_cents: 0 },
        AgingBucket { key: "days31To60", label: "31–60 days", invoice_count: 0, amount_cents: 0 },
        AgingBucket { key: "days61Plus", label: "61+ days", invoice_count: 0, amount_cents: 0 },
    ];
    let mut total_cents = 0;
    for (due, balance) in open {
        let days_overdue = (as_of - due).num_days();
        let idx = match days_overdue {
            d if d <= 0 => 0,
            1..=30 => 1,
            31..=60 => 2,
            _ => 3,
        };
        buckets[idx].invoice_count += 1;
        buckets[idx].amount_cents += balance;
        total_cents += balance;
    }
    Ok(ArAging { as_of, buckets, total_cents })
}

async fn system_account(tx: &mut Transaction<'_, Sqlite>, account_type: &str) -> Result<i64, String> {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = ?1 AND is_system = 1 ORDER BY id LIMIT 1")
        .bind(account_type)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("COA_SYSTEM_ACCOUNT_MISSING: no system {account_type} account"))
}

#[cfg(test)]
mod tests {
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
                lines: vec![InvoiceLineInput { description: "Work".into(), quantity: 1, rate_cents: cents, income_account_id: services }],
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
                InvoiceLineInput { description: "Consulting".into(), quantity: 3, rate_cents: 15_000, income_account_id: services },
                InvoiceLineInput { description: "Widget".into(), quantity: 1, rate_cents: 2_500, income_account_id: sales },
                InvoiceLineInput { description: "Support".into(), quantity: 2, rate_cents: 1_000, income_account_id: services },
            ],
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
                InvoiceLineInput { description: "Consulting".into(), quantity: 1, rate_cents: 10_000, income_account_id: services },
                InvoiceLineInput { description: "Oops".into(), quantity: 1, rate_cents: 10_000, income_account_id: rent },
            ],
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
    fn rejects_overflowing_line_amount() {
        assert!(line_amount(MAX_QUANTITY, MAX_LINE_CENTS).is_err());
        assert!(line_amount(i64::MAX, 2).is_err());
        assert_eq!(line_amount(3, 15_000), Ok(45_000));
    }
}
