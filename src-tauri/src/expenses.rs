//! Expenses: vendors, bills (accrual: Dr each split account / Cr Accounts Payable), bill payments
//! (Dr Accounts Payable / Cr bank or credit card), and expenses & checks (cash basis: Dr each split
//! account / Cr the bank or credit card). Bills support partial payments like invoices.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{convert_lines, db_err, format_cents, post_entry_in_tx, rollback, EntrySource, ForeignCurrency, JournalEntry, JournalLine, MAX_LINE_CENTS};
use crate::sales::{create_party, find_or_create_named, Party, PartyView};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillLineInput {
    pub account_id: i64,
    pub amount_cents: i64,
    pub memo: Option<String>,
    /// Optional item pricing: quantity scaled by 10,000 and unit rate. When given, the line amount
    /// must equal round_half_up(quantity_e4 × rate_cents / 10,000).
    #[serde(default)]
    pub quantity_e4: Option<i64>,
    #[serde(default)]
    pub rate_cents: Option<i64>,
    /// The product bought. A tracked inventory product adds the quantity to stock at this line's
    /// cost; the line must then post to that product's inventory account and give a quantity.
    #[serde(default)]
    pub product_service_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillInput {
    pub vendor_name: String,
    pub bill_number: Option<String>,
    pub bill_date: NaiveDate,
    pub due_date: NaiveDate,
    pub memo: Option<String>,
    /// The bill total as printed on the vendor's bill; the split lines must add up to it exactly.
    pub total_cents: i64,
    pub lines: Vec<BillLineInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillPaymentInput {
    pub bill_id: i64,
    pub payment_account_id: i64,
    pub payment_date: NaiveDate,
    pub amount_cents: i64,
    pub reference: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BillSummary {
    pub id: i64,
    pub vendor_id: i64,
    pub vendor_name: String,
    pub bill_number: Option<String>,
    pub bill_date: String,
    pub due_date: String,
    pub memo: Option<String>,
    pub total_cents: i64,
    pub amount_paid_cents: i64,
    pub balance_cents: i64,
    pub status: String,
    pub journal_entry_id: Option<i64>,
    pub last_payment_date: Option<String>,
    pub line_count: i64,
    /// The category when every line uses the same one.
    pub category_account_id: Option<i64>,
    pub first_description: Option<String>,
    pub is_voided: bool,
}

const BILL_SELECT: &str = "
    SELECT b.id, b.vendor_id, v.name AS vendor_name, b.bill_number, b.bill_date, b.due_date, b.memo,
           b.total_cents, b.amount_paid_cents, b.total_cents - b.amount_paid_cents AS balance_cents, b.status,
           b.journal_entry_id,
           (SELECT MAX(payment_date) FROM bill_payments WHERE bill_id = b.id AND is_voided = 0) AS last_payment_date,
           (SELECT COUNT(*) FROM bill_lines WHERE bill_id = b.id) AS line_count,
           (SELECT CASE WHEN COUNT(DISTINCT account_id) = 1 THEN MAX(account_id) END
            FROM bill_lines WHERE bill_id = b.id) AS category_account_id,
           (SELECT NULLIF(memo, '') FROM bill_lines WHERE bill_id = b.id ORDER BY line_number LIMIT 1) AS first_description,
           b.is_voided
    FROM bills b JOIN vendors v ON v.id = b.vendor_id";

fn clean(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|s| !s.is_empty())
}

pub async fn list_vendors(pool: &SqlitePool) -> Result<Vec<PartyView>, String> {
    sqlx::query_as::<_, PartyView>(
        "SELECT v.id, v.name, v.email, v.is_active,
                COALESCE((SELECT SUM(total_cents - amount_paid_cents) FROM bills
                          WHERE vendor_id = v.id AND status IN ('UNPAID', 'PARTIAL') AND is_voided = 0), 0) AS open_balance_cents
         FROM vendors v ORDER BY v.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn create_vendor(pool: &SqlitePool, name: &str, email: Option<&str>) -> Result<PartyView, String> {
    let id = create_party(pool, Party::Vendor, name, email).await?;
    list_vendors(pool).await?.into_iter().find(|v| v.id == id).ok_or_else(|| "PARTY_NOT_FOUND: vendor was not saved".into())
}

pub async fn list_bills(pool: &SqlitePool) -> Result<Vec<BillSummary>, String> {
    sqlx::query_as::<_, BillSummary>(&format!(
        "{BILL_SELECT} ORDER BY b.is_voided, CASE b.status WHEN 'PAID' THEN 1 ELSE 0 END, b.due_date, b.id"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

async fn get_bill(pool: &SqlitePool, bill_id: i64) -> Result<BillSummary, String> {
    sqlx::query_as::<_, BillSummary>(&format!("{BILL_SELECT} WHERE b.id = ?1"))
        .bind(bill_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

/// Pure validation; returns the bill total.
pub fn validate_bill(input: &BillInput) -> Result<i64, String> {
    if input.vendor_name.trim().is_empty() {
        return Err("BILL_NO_VENDOR: enter a vendor".into());
    }
    if input.due_date < input.bill_date {
        return Err("BILL_DUE_BEFORE_DATE: due date cannot be earlier than the bill date".into());
    }
    if input.lines.is_empty() {
        return Err("BILL_NO_ITEMS: a bill requires at least one line".into());
    }
    for (idx, line) in input.lines.iter().enumerate() {
        match (line.quantity_e4, line.rate_cents) {
            (None, None) => {}
            (Some(q), Some(r)) => {
                let priced = crate::invoices::validate_line(q, r, idx + 1, "BILL")?;
                if priced != line.amount_cents {
                    return Err(format!(
                        "BILL_LINE_MISMATCH: split line {} quantity × rate is {}, not {}",
                        idx + 1,
                        format_cents(priced),
                        format_cents(line.amount_cents)
                    ));
                }
            }
            _ => return Err(format!("BILL_LINE_INCOMPLETE: split line {} needs both a quantity and a rate, or neither", idx + 1)),
        }
    }
    let total = sum_split_lines(input.lines.iter().map(|l| l.amount_cents), "BILL")?;
    if total != input.total_cents {
        return Err(format!(
            "BILL_SPLIT_MISMATCH: split lines add up to {} but the bill total is {} ({} unallocated)",
            format_cents(total),
            format_cents(input.total_cents),
            format_cents(input.total_cents - total)
        ));
    }
    Ok(total)
}

/// Sums split-line amounts, rejecting non-positive lines and overflow. `code` prefixes the errors.
fn sum_split_lines(amounts: impl Iterator<Item = i64>, code: &str) -> Result<i64, String> {
    let mut total: i64 = 0;
    for (idx, amount) in amounts.enumerate() {
        if !(1..=MAX_LINE_CENTS).contains(&amount) {
            return Err(format!("{code}_BAD_AMOUNT: split line {} amount must be greater than zero", idx + 1));
        }
        total = total
            .checked_add(amount)
            .filter(|t| *t <= MAX_LINE_CENTS)
            .ok_or_else(|| format!("{code}_AMOUNT_TOO_LARGE: total exceeds {}", format_cents(MAX_LINE_CENTS)))?;
    }
    Ok(total)
}

/// Records a vendor bill: Dr each expense category / Cr Accounts Payable, atomically.
pub async fn post_vendor_bill(pool: &SqlitePool, input: BillInput) -> Result<BillSummary, String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let bill_id = match write_bill(&mut tx, &input).await {
        Ok(id) => id,
        Err(err) => return Err(rollback(tx, err).await),
    };
    tx.commit().await.map_err(db_err)?;
    get_bill(pool, bill_id).await
}

async fn write_bill(tx: &mut Transaction<'_, Sqlite>, input: &BillInput) -> Result<i64, String> {
    let total = validate_bill(input)?;
    let vendor_name = input.vendor_name.trim();
    let vendor_id = find_or_create_named(tx, Party::Vendor, vendor_name).await?;

    let bill_id: i64 = sqlx::query_scalar(
        "INSERT INTO bills (vendor_id, bill_number, bill_date, due_date, memo, total_cents, status)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'DRAFT') RETURNING id",
    )
    .bind(vendor_id)
    .bind(clean(input.bill_number.as_deref()))
    .bind(input.bill_date)
    .bind(input.due_date)
    .bind(clean(input.memo.as_deref()))
    .bind(total)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    let mut debits: Vec<(i64, i64)> = Vec::new();
    let mut received: Vec<(i64, i64, i64)> = Vec::new(); // (item, quantity, cost)
    for (idx, line) in input.lines.iter().enumerate() {
        let n = idx + 1;
        if let Some(product) = line.product_service_id {
            if let Some(item) = crate::inventory::check_purchase_line(tx, product, line.account_id, line.quantity_e4, n).await? {
                received.push((item, line.quantity_e4.unwrap_or_default(), line.amount_cents));
            }
        }
        let category: Option<(String, String, bool, bool)> = sqlx::query_as(
            "SELECT t.classification, a.account_type, a.is_active, a.is_system
             FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1",
        )
        .bind(line.account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        match category {
            None => return Err(format!("BILL_UNKNOWN_ACCOUNT: line {n} references an account that does not exist")),
            Some((_, _, false, _)) => return Err(format!("BILL_INACTIVE_ACCOUNT: line {n} uses an inactive account")),
            Some((class, kind, _, is_system))
                if is_system || !(class == "EXPENSE" || matches!(kind.as_str(), "FIXED_ASSET" | "OTHER_ASSET" | "OTHER_CURRENT_ASSET")) =>
            {
                return Err(format!("BILL_BAD_CATEGORY: line {n} must use an expense or asset category"))
            }
            Some(_) => {}
        }

        sqlx::query(
            "INSERT INTO bill_lines (bill_id, line_number, memo, account_id, amount_cents, quantity_e4, rate_cents, product_service_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(bill_id)
        .bind(n as i64)
        .bind(clean(line.memo.as_deref()).unwrap_or(""))
        .bind(line.account_id)
        .bind(line.amount_cents)
        .bind(line.quantity_e4)
        .bind(line.rate_cents)
        .bind(line.product_service_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
        // One debit per distinct account, in first-appearance order.
        match debits.iter_mut().find(|(id, _)| *id == line.account_id) {
            Some((_, sum)) => *sum += line.amount_cents,
            None => debits.push((line.account_id, line.amount_cents)),
        }
    }

    let ap_account = system_account(tx, "ACCOUNTS_PAYABLE").await?;
    let mut lines: Vec<JournalLine> = debits.into_iter().map(|(account, cents)| JournalLine::debit(account, cents)).collect();
    lines.push(JournalLine::credit(ap_account, total));
    let reference = clean(input.bill_number.as_deref()).map(|n| format!(" #{n}")).unwrap_or_default();
    let entry = JournalEntry { id: None, date: input.bill_date, description: format!("Bill{reference} - {vendor_name}"), lines, currency: None };
    let entry_id = post_entry_in_tx(tx, &entry, EntrySource::Bill(bill_id)).await?;
    for (item, qty, cost) in received {
        crate::inventory::record_purchase(tx, item, input.bill_date, qty, cost, entry_id).await?;
    }

    sqlx::query("UPDATE bills SET status = 'UNPAID', journal_entry_id = ?1 WHERE id = ?2")
        .bind(entry_id)
        .bind(bill_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(bill_id)
}

/// Pays all or part of an open bill: Dr Accounts Payable / Cr the bank or credit card account.
pub async fn pay_vendor_bill(pool: &SqlitePool, input: BillPaymentInput) -> Result<BillSummary, String> {
    let bill_id = input.bill_id;
    pay_vendor_bills(pool, vec![input]).await?;
    get_bill(pool, bill_id).await
}

/// Pay Bills: pays several bills at once. Every payment posts, or none do.
pub async fn pay_vendor_bills(pool: &SqlitePool, inputs: Vec<BillPaymentInput>) -> Result<usize, String> {
    if inputs.is_empty() {
        return Err("BILL_NOTHING_TO_PAY: select at least one bill".into());
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    for input in &inputs {
        if let Err(err) = write_bill_payment(&mut tx, input).await {
            return Err(rollback(tx, err).await);
        }
    }
    tx.commit().await.map_err(db_err)?;
    Ok(inputs.len())
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BillPaymentView {
    pub id: i64,
    pub payment_date: String,
    pub amount_cents: i64,
    pub payment_account_id: i64,
    pub reference: Option<String>,
    pub journal_entry_id: Option<i64>,
    pub is_voided: bool,
}

pub async fn list_bill_payments(pool: &SqlitePool, bill_id: i64) -> Result<Vec<BillPaymentView>, String> {
    sqlx::query_as::<_, BillPaymentView>(
        "SELECT id, payment_date, amount_cents, payment_account_id, reference, journal_entry_id, is_voided
         FROM bill_payments WHERE bill_id = ?1 ORDER BY payment_date, id",
    )
    .bind(bill_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

async fn write_bill_payment(tx: &mut Transaction<'_, Sqlite>, input: &BillPaymentInput) -> Result<(), String> {
    if input.amount_cents <= 0 {
        return Err("BILL_BAD_AMOUNT: payment amount must be greater than zero".into());
    }
    let bill: Option<(String, i64, i64, String, String, bool)> = sqlx::query_as(
        "SELECT v.name, b.total_cents, b.amount_paid_cents, b.status, b.bill_date, b.is_voided
         FROM bills b JOIN vendors v ON v.id = b.vendor_id WHERE b.id = ?1",
    )
    .bind(input.bill_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let (vendor, total, paid, status, bill_date, voided) =
        bill.ok_or_else(|| format!("BILL_NOT_FOUND: bill {} does not exist", input.bill_id))?;
    if voided {
        return Err(format!("BILL_NOT_OPEN: this bill from {vendor} is voided"));
    }
    if status != "UNPAID" && status != "PARTIAL" {
        return Err(format!("BILL_NOT_OPEN: this bill from {vendor} is {status}; only open bills can be paid"));
    }
    let open_balance = total - paid;
    if input.amount_cents > open_balance {
        return Err(format!(
            "BILL_OVERPAYMENT: payment of {} exceeds the open balance of {}",
            format_cents(input.amount_cents),
            format_cents(open_balance)
        ));
    }
    if input.payment_date.format("%Y-%m-%d").to_string() < bill_date {
        return Err("BILL_PAYMENT_BEFORE_BILL: payment date is earlier than the bill date".into());
    }
    let account_type: Option<String> = sqlx::query_scalar("SELECT account_type FROM chart_of_accounts WHERE id = ?1")
        .bind(input.payment_account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    if !matches!(account_type.as_deref(), Some("BANK") | Some("CREDIT_CARD")) {
        return Err("BILL_BAD_PAYMENT_ACCOUNT: pay bills from a Bank or Credit Card account".into());
    }

    let payment_id: i64 = sqlx::query_scalar(
        "INSERT INTO bill_payments (bill_id, payment_date, amount_cents, payment_account_id, reference)
         VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id",
    )
    .bind(input.bill_id)
    .bind(input.payment_date)
    .bind(input.amount_cents)
    .bind(input.payment_account_id)
    .bind(clean(input.reference.as_deref()))
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    let ap_account = system_account(tx, "ACCOUNTS_PAYABLE").await?;
    let entry = JournalEntry {
        id: None,
        date: input.payment_date,
        description: format!("Bill Payment - {vendor}"),
        lines: vec![JournalLine::debit(ap_account, input.amount_cents), JournalLine::credit(input.payment_account_id, input.amount_cents)],
        currency: None,
    };
    let entry_id = post_entry_in_tx(tx, &entry, EntrySource::BillPayment(payment_id)).await?;

    sqlx::query("UPDATE bill_payments SET journal_entry_id = ?1 WHERE id = ?2")
        .bind(entry_id)
        .bind(payment_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;

    let new_paid = paid + input.amount_cents;
    sqlx::query("UPDATE bills SET amount_paid_cents = ?1, status = ?2 WHERE id = ?3")
        .bind(new_paid)
        .bind(if new_paid == total { "PAID" } else { "PARTIAL" })
        .bind(input.bill_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(())
}


// ------------------------------------------------------------------------------------------------
// Expenses & checks
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitLineInput {
    pub account_id: i64,
    pub amount_cents: i64,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseInput {
    /// `EXPENSE` (paid by bank or card) or `CHECK` (written from a bank account).
    pub kind: String,
    pub payee_name: Option<String>,
    pub payment_account_id: i64,
    pub expense_date: NaiveDate,
    pub check_number: Option<String>,
    pub payment_method: Option<String>,
    pub memo: Option<String>,
    pub lines: Vec<SplitLineInput>,
    /// Set when paid in a foreign currency; line amounts are then in that currency.
    #[serde(default)]
    pub currency: Option<ForeignCurrency>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseView {
    pub id: i64,
    pub kind: String,
    pub payee_name: Option<String>,
    pub payment_account_id: i64,
    pub expense_date: String,
    pub check_number: Option<String>,
    pub payment_method: Option<String>,
    pub memo: Option<String>,
    pub total_cents: i64,
    pub line_count: i64,
    pub category_account_id: Option<i64>,
    pub journal_entry_id: Option<i64>,
    pub is_voided: bool,
    /// `None` for home-currency expenses; `total_cents` is then already home currency.
    pub currency_code: Option<String>,
    pub exchange_rate_e6: Option<i64>,
    pub home_total_cents: i64,
}

const EXPENSE_SELECT: &str = "
    SELECT x.id, x.kind, v.name AS payee_name, x.payment_account_id, x.expense_date, x.check_number,
           x.payment_method, x.memo, x.total_cents, x.currency_code, x.exchange_rate_e6,
           COALESCE(x.home_total_cents, x.total_cents) AS home_total_cents,
           (SELECT COUNT(*) FROM expense_lines WHERE expense_id = x.id) AS line_count,
           (SELECT CASE WHEN COUNT(DISTINCT account_id) = 1 THEN MAX(account_id) END
            FROM expense_lines WHERE expense_id = x.id) AS category_account_id,
           x.journal_entry_id, x.is_voided
    FROM expenses x LEFT JOIN vendors v ON v.id = x.vendor_id";

/// Records an expense or check: Dr each split account / Cr the payment account, atomically.
pub async fn post_expense(pool: &SqlitePool, input: ExpenseInput) -> Result<ExpenseView, String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let (id, _) = match write_expense(&mut tx, &input).await {
        Ok(ids) => ids,
        Err(err) => return Err(rollback(tx, err).await),
    };
    tx.commit().await.map_err(db_err)?;
    sqlx::query_as::<_, ExpenseView>(&format!("{EXPENSE_SELECT} WHERE x.id = ?1"))
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

/// Writes and posts an expense inside the caller's transaction; returns (expense id, entry id).
/// Bank feed approvals use this so an approved outflow is a real Expense document.
pub(crate) async fn write_expense(tx: &mut Transaction<'_, Sqlite>, input: &ExpenseInput) -> Result<(i64, i64), String> {
    let (kind, source): (&str, fn(i64) -> EntrySource) = match input.kind.as_str() {
        "EXPENSE" => ("EXPENSE", EntrySource::Expense),
        "CHECK" => ("CHECK", EntrySource::Check),
        other => return Err(format!("EXP_BAD_KIND: unknown expense kind '{other}'")),
    };
    if input.lines.is_empty() {
        return Err("EXP_NO_ITEMS: add at least one split line".into());
    }
    let total = sum_split_lines(input.lines.iter().map(|l| l.amount_cents), "EXP")?;
    if let Some(method) = clean(input.payment_method.as_deref()) {
        if !crate::invoices::PAYMENT_METHODS.contains(&method) {
            return Err(format!("EXP_BAD_METHOD: unknown payment method '{method}'"));
        }
    }
    let account_type: Option<String> = sqlx::query_scalar("SELECT account_type FROM chart_of_accounts WHERE id = ?1")
        .bind(input.payment_account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
    match (kind, account_type.as_deref()) {
        ("CHECK", Some("BANK")) | ("EXPENSE", Some("BANK")) | ("EXPENSE", Some("CREDIT_CARD")) => {}
        ("CHECK", _) => return Err("EXP_BAD_PAYMENT_ACCOUNT: checks are written from a Bank account".into()),
        _ => return Err("EXP_BAD_PAYMENT_ACCOUNT: pay expenses from a Bank or Credit Card account".into()),
    }
    let vendor_id = match clean(input.payee_name.as_deref()) {
        Some(name) => Some(find_or_create_named(tx, Party::Vendor, name).await?),
        None => None,
    };
    let check_number = if kind == "CHECK" { clean(input.check_number.as_deref()) } else { None };

    // Journal lines in the document's currency; the ledger converts foreign ones to home currency.
    let mut lines: Vec<JournalLine> = input
        .lines
        .iter()
        .map(|line| JournalLine::debit(line.account_id, line.amount_cents).with_memo(line.memo.as_deref()))
        .collect();
    lines.push(JournalLine::credit(input.payment_account_id, total));
    let home_total = match &input.currency {
        Some(currency) => {
            currency.validate()?;
            convert_lines(&lines, currency.exchange_rate_e6)?.last().map(|l| l.1)
        }
        None => None,
    };

    let expense_id: i64 = sqlx::query_scalar(
        "INSERT INTO expenses
            (kind, vendor_id, payment_account_id, expense_date, check_number, payment_method, memo, total_cents,
             currency_code, exchange_rate_e6, home_total_cents)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) RETURNING id",
    )
    .bind(kind)
    .bind(vendor_id)
    .bind(input.payment_account_id)
    .bind(input.expense_date)
    .bind(check_number)
    .bind(clean(input.payment_method.as_deref()))
    .bind(clean(input.memo.as_deref()))
    .bind(total)
    .bind(input.currency.as_ref().map(|c| c.code.as_str()))
    .bind(input.currency.as_ref().map(|c| c.exchange_rate_e6))
    .bind(home_total)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    for (idx, line) in input.lines.iter().enumerate() {
        sqlx::query(
            "INSERT INTO expense_lines (expense_id, line_number, account_id, amount_cents, memo) VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(expense_id)
        .bind(idx as i64 + 1)
        .bind(line.account_id)
        .bind(line.amount_cents)
        .bind(clean(line.memo.as_deref()))
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("{} (split line {})", db_err(e), idx + 1))?;
    }

    let payee = clean(input.payee_name.as_deref()).map(|p| format!(" - {p}")).unwrap_or_default();
    let label = match (kind, check_number) {
        ("CHECK", Some(n)) => format!("Check #{n}{payee}"),
        ("CHECK", None) => format!("Check{payee}"),
        _ => format!("Expense{payee}"),
    };
    let description = match clean(input.memo.as_deref()) {
        Some(m) if payee.is_empty() => format!("{label}: {m}"),
        _ => label,
    };
    let entry = JournalEntry { id: None, date: input.expense_date, description, lines, currency: input.currency.clone() };
    let entry_id = post_entry_in_tx(tx, &entry, source(expense_id)).await?;
    sqlx::query("UPDATE expenses SET journal_entry_id = ?1 WHERE id = ?2")
        .bind(entry_id)
        .bind(expense_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok((expense_id, entry_id))
}

pub async fn list_expenses(pool: &SqlitePool) -> Result<Vec<ExpenseView>, String> {
    sqlx::query_as::<_, ExpenseView>(&format!("{EXPENSE_SELECT} ORDER BY x.expense_date DESC, x.id DESC"))
        .fetch_all(pool)
        .await
        .map_err(db_err)
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
    use crate::statements::balance_sheet;

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

    fn bill(vendor: &str, lines: Vec<(i64, i64)>) -> BillInput {
        BillInput {
            vendor_name: vendor.into(),
            bill_number: Some("INV-778".into()),
            bill_date: d(1),
            due_date: d(31),
            memo: None,
            total_cents: lines.iter().map(|(_, cents)| cents).sum(),
            lines: lines.into_iter().map(|(account, cents)| BillLineInput{ account_id: account, amount_cents: cents, memo: Some("Line".into()), quantity_e4: None, rate_cents: None, product_service_id: None }).collect(),
        }
    }

    fn pay(bill_id: i64, account: i64, cents: i64) -> BillPaymentInput {
        BillPaymentInput { bill_id, payment_account_id: account, payment_date: d(10), amount_cents: cents, reference: Some("1001".into()) }
    }

    #[tokio::test]
    async fn bill_accrues_payable_and_payments_relieve_it() {
        let pool = test_pool().await;
        let (ap, checking, card, rent, utilities) = (
            account_id(&pool, "2000").await,
            account_id(&pool, "1010").await,
            account_id(&pool, "2100").await,
            account_id(&pool, "6300").await,
            account_id(&pool, "6500").await,
        );

        let posted = post_vendor_bill(&pool, bill("Landlord LLC", vec![(rent, 150_000), (utilities, 12_345)])).await.expect("bill posts");
        assert_eq!((posted.status.as_str(), posted.total_cents, posted.line_count), ("UNPAID", 162_345, 2));
        assert_eq!(balance(&pool, ap).await, 162_345);
        assert_eq!(balance(&pool, rent).await, 150_000);
        assert_eq!(balance(&pool, utilities).await, 12_345);

        let partial = pay_vendor_bill(&pool, pay(posted.id, checking, 100_000)).await.expect("partial payment");
        assert_eq!((partial.status.as_str(), partial.balance_cents), ("PARTIAL", 62_345));
        let err = pay_vendor_bill(&pool, pay(posted.id, checking, 62_346)).await.expect_err("overpayment");
        assert!(err.starts_with("BILL_OVERPAYMENT"), "{err}");

        let paid = pay_vendor_bill(&pool, pay(posted.id, card, 62_345)).await.expect("final payment by card");
        assert_eq!((paid.status.as_str(), paid.balance_cents), ("PAID", 0));
        assert_eq!(balance(&pool, ap).await, 0);
        assert_eq!(balance(&pool, checking).await, -100_000);
        assert_eq!(balance(&pool, card).await, 62_345);

        let vendors = list_vendors(&pool).await.unwrap();
        assert_eq!((vendors.len(), vendors[0].open_balance_cents), (1, 0));
        assert!(balance_sheet(&pool, d(31)).await.unwrap().is_balanced);
    }

    #[tokio::test]
    async fn bill_rejects_bad_categories_and_rolls_back() {
        let pool = test_pool().await;
        let (rent, sales, ar) = (account_id(&pool, "6300").await, account_id(&pool, "4000").await, account_id(&pool, "1200").await);
        for bad in [sales, ar] {
            let err = post_vendor_bill(&pool, bill("Oops Inc", vec![(rent, 1_000), (bad, 500)])).await.expect_err("bad category");
            assert!(err.starts_with("BILL_BAD_CATEGORY: line 2"), "{err}");
        }
        let (bills, entries, vendors): (i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT COUNT(*) FROM bills), (SELECT COUNT(*) FROM journal_entries), (SELECT COUNT(*) FROM vendors)",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!((bills, entries, vendors), (0, 0, 0));

        let mut early = bill("Late Co", vec![(rent, 1_000)]);
        early.due_date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        assert!(post_vendor_bill(&pool, early).await.unwrap_err().starts_with("BILL_DUE_BEFORE_DATE"));
    }

    #[tokio::test]
    async fn split_bill_must_match_total_and_debits_each_account_once() {
        let pool = test_pool().await;
        let (ap, rent, utilities) = (account_id(&pool, "2000").await, account_id(&pool, "6300").await, account_id(&pool, "6500").await);

        let mut short = bill("Landlord LLC", vec![(rent, 100_000), (utilities, 20_000)]);
        short.total_cents = 125_000;
        let err = post_vendor_bill(&pool, short).await.expect_err("split must match total");
        assert!(err.starts_with("BILL_SPLIT_MISMATCH") && err.contains("$50.00 unallocated"), "{err}");

        // Two splits to the same account collapse into one debit line on the journal entry.
        let posted = post_vendor_bill(&pool, bill("Landlord LLC", vec![(rent, 100_000), (utilities, 20_000), (rent, 5_000)])).await.unwrap();
        let lines: Vec<(i64, i64, i64)> = sqlx::query_as(
            "SELECT account_id, debit_cents, credit_cents FROM journal_lines WHERE journal_entry_id = ?1 ORDER BY line_number",
        )
        .bind(posted.journal_entry_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(lines, vec![(rent, 105_000, 0), (utilities, 20_000, 0), (ap, 0, 125_000)]);
        assert_eq!((posted.line_count, posted.category_account_id), (3, None));
    }

    #[tokio::test]
    async fn expenses_and_checks_credit_the_payment_account() {
        let pool = test_pool().await;
        let (checking, card, rent, fuel, ar) =
            (account_id(&pool, "1010").await, account_id(&pool, "2100").await, account_id(&pool, "6300").await, account_id(&pool, "6100").await, account_id(&pool, "1200").await);
        let input = |kind: &str, account: i64, lines: Vec<(i64, i64)>| ExpenseInput {
            kind: kind.into(),
            payee_name: Some("Shell".into()),
            payment_account_id: account,
            expense_date: d(5),
            check_number: Some("1042".into()),
            payment_method: None,
            memo: None,
            lines: lines.into_iter().map(|(account_id, amount_cents)| SplitLineInput { account_id, amount_cents, memo: None }).collect(),
            currency: None,
        };

        let check = post_expense(&pool, input("CHECK", checking, vec![(rent, 90_000), (fuel, 4_000)])).await.expect("check posts");
        assert_eq!((check.kind.as_str(), check.check_number.as_deref(), check.total_cents), ("CHECK", Some("1042"), 94_000));
        let card_expense = post_expense(&pool, input("EXPENSE", card, vec![(fuel, 6_000)])).await.expect("card expense posts");
        assert_eq!(card_expense.check_number, None);

        assert_eq!(balance(&pool, checking).await, -94_000);
        assert_eq!(balance(&pool, card).await, 6_000);
        assert_eq!(balance(&pool, fuel).await, 10_000);

        let err = post_expense(&pool, input("CHECK", card, vec![(fuel, 100)])).await.expect_err("checks need a bank");
        assert!(err.starts_with("EXP_BAD_PAYMENT_ACCOUNT"), "{err}");
        let err = post_expense(&pool, input("EXPENSE", checking, vec![(ar, 100)])).await.expect_err("A/R needs a customer");
        assert!(err.starts_with("EXP_BAD_CATEGORY"), "{err}");
        assert_eq!(list_expenses(&pool).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn pay_bills_is_all_or_nothing() {
        let pool = test_pool().await;
        let (checking, rent) = (account_id(&pool, "1010").await, account_id(&pool, "6300").await);
        let a = post_vendor_bill(&pool, bill("A Co", vec![(rent, 10_000)])).await.unwrap();
        let b = post_vendor_bill(&pool, bill("B Co", vec![(rent, 20_000)])).await.unwrap();

        let err = pay_vendor_bills(&pool, vec![pay(a.id, checking, 10_000), pay(b.id, checking, 99_999)]).await.expect_err("second overpays");
        assert!(err.starts_with("BILL_OVERPAYMENT"), "{err}");
        assert_eq!(balance(&pool, checking).await, 0, "first payment rolled back too");

        assert_eq!(pay_vendor_bills(&pool, vec![pay(a.id, checking, 10_000), pay(b.id, checking, 5_000)]).await.unwrap(), 2);
        assert_eq!(balance(&pool, checking).await, -15_000);
        assert_eq!(list_bill_payments(&pool, b.id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn posted_bills_are_immutable_at_the_database_layer() {
        let pool = test_pool().await;
        let rent = account_id(&pool, "6300").await;
        let posted = post_vendor_bill(&pool, bill("Landlord LLC", vec![(rent, 5_000)])).await.unwrap();
        let err = sqlx::query("UPDATE bills SET total_cents = 1 WHERE id = ?1").bind(posted.id).execute(&pool).await.unwrap_err();
        assert!(err.to_string().contains("BILL_IMMUTABLE"), "{err}");
        let err = sqlx::query("DELETE FROM bill_lines WHERE bill_id = ?1").bind(posted.id).execute(&pool).await.unwrap_err();
        assert!(err.to_string().contains("BILL_IMMUTABLE"), "{err}");
    }

    #[tokio::test]
    async fn foreign_expense_credits_the_bank_in_home_currency() {
        let pool = test_pool().await;
        let (checking, travel, meals) = (account_id(&pool, "1010").await, account_id(&pool, "6450").await, account_id(&pool, "6200").await);
        let posted = post_expense(
            &pool,
            ExpenseInput {
                kind: "EXPENSE".into(),
                payee_name: Some("Hotel Tokyo".into()),
                payment_account_id: checking,
                expense_date: d(3),
                check_number: None,
                payment_method: Some("CREDIT_CARD".into()),
                memo: None,
                lines: vec![
                    SplitLineInput { account_id: travel, amount_cents: 3_200_000, memo: Some("2 nights".into()) },
                    SplitLineInput { account_id: meals, amount_cents: 450_050, memo: None },
                ],
                currency: Some(ForeignCurrency { code: "JPY".into(), exchange_rate_e6: 6_667 }),
            },
        )
        .await
        .expect("foreign expense posts");
        assert_eq!((posted.currency_code.as_deref(), posted.total_cents), (Some("JPY"), 3_650_050));
        let (t, m, c) = (balance(&pool, travel).await, balance(&pool, meals).await, balance(&pool, checking).await);
        // ¥3,200,000 → $213.344 and ¥450,050 → $30.0048 round to $243.34, but the ¥3,650,050 total
        // converts to $243.35; the $0.01 difference lands on the largest debit (travel).
        assert_eq!((t, m), (21_335, 3_000));
        assert_eq!(c, -(t + m), "the bank credit equals the converted debits");
        assert_eq!(posted.home_total_cents, t + m);
    }

    #[tokio::test]
    async fn bill_lines_can_be_priced_by_quantity_and_rate() {
        let pool = test_pool().await;
        let (ap, supplies) = (account_id(&pool, "2000").await, account_id(&pool, "6250").await);
        let priced = |q: i64, r: i64, amount: i64| BillLineInput{ account_id: supplies, amount_cents: amount, memo: Some("Paper".into()), quantity_e4: Some(q), rate_cents: Some(r), product_service_id: None };
        let mut input = bill("Staples", vec![]);
        input.lines = vec![priced(25_000, 1_299, 3_248)];
        input.total_cents = 3_248;
        post_vendor_bill(&pool, input.clone()).await.expect("2.5 × $12.99 = $32.475 → $32.48");
        assert_eq!(balance(&pool, ap).await, 3_248);

        input.lines = vec![priced(25_000, 1_299, 3_247)];
        input.total_cents = 3_247;
        assert!(post_vendor_bill(&pool, input.clone()).await.unwrap_err().starts_with("BILL_LINE_MISMATCH"));
        input.lines = vec![BillLineInput { quantity_e4: Some(10_000), rate_cents: None, ..priced(0, 0, 3_247) }];
        assert!(post_vendor_bill(&pool, input).await.unwrap_err().starts_with("BILL_LINE_INCOMPLETE"));
    }
}
