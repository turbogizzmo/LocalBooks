//! Expenses: vendors, bills (accrual: Dr each split account / Cr Accounts Payable), bill payments
//! (Dr Accounts Payable / Cr bank or credit card), and expenses & checks (cash basis: Dr each split
//! account / Cr the bank or credit card), and credit card credits (the reverse of a card charge: Dr the
//! card / Cr each split account). Bills support partial payments like invoices.

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
    #[serde(default)]
    pub class_id: Option<i64>,
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
                          WHERE vendor_id = v.id AND status IN ('UNPAID', 'PARTIAL') AND is_voided = 0), 0)
                - COALESCE((SELECT SUM(remaining_cents) FROM v_vendor_credit_balances WHERE vendor_id = v.id), 0) AS open_balance_cents
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
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let bill_id = match write_bill(&mut tx, &input).await {
        Ok(id) => id,
        Err(err) => return Err(rollback(tx, err).await),
    };
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    get_bill(pool, bill_id).await
}

pub(crate) async fn write_bill(tx: &mut Transaction<'_, Sqlite>, input: &BillInput) -> Result<i64, String> {
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

    let mut debits: Vec<((i64, Option<i64>), i64)> = Vec::new();
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
        if let Some(class) = line.class_id {
            crate::classes::check_usable(tx, class, &format!("BILL: line {n}")).await?;
        }

        sqlx::query(
            "INSERT INTO bill_lines (bill_id, line_number, memo, account_id, amount_cents, quantity_e4, rate_cents, product_service_id, class_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(bill_id)
        .bind(n as i64)
        .bind(clean(line.memo.as_deref()).unwrap_or(""))
        .bind(line.account_id)
        .bind(line.amount_cents)
        .bind(line.quantity_e4)
        .bind(line.rate_cents)
        .bind(line.product_service_id)
        .bind(line.class_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
        // One debit per distinct account and class, in first-appearance order.
        let key = (line.account_id, line.class_id);
        match debits.iter_mut().find(|(k, _)| *k == key) {
            Some((_, sum)) => *sum += line.amount_cents,
            None => debits.push((key, line.amount_cents)),
        }
    }

    let ap_account = system_account(tx, "ACCOUNTS_PAYABLE").await?;
    let mut lines: Vec<JournalLine> =
        debits.into_iter().map(|((account, class), cents)| JournalLine::debit(account, cents).with_class(class)).collect();
    lines.push(JournalLine::credit(ap_account, total));
    let receipt = crate::inventory::plan_receipt(tx, &received).await?;
    lines.extend(receipt.lines.iter().cloned());
    let reference = clean(input.bill_number.as_deref()).map(|n| format!(" #{n}")).unwrap_or_default();
    let entry = JournalEntry { id: None, date: input.bill_date, description: format!("Bill{reference} - {vendor_name}"), lines, currency: None };
    let entry_id = post_entry_in_tx(tx, &entry, EntrySource::Bill(bill_id)).await?;
    receipt.record(tx, input.bill_date, entry_id).await?;

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
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    for input in &inputs {
        if let Err(err) = write_bill_payment(&mut tx, input, "STANDARD").await {
            return Err(rollback(tx, err).await);
        }
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
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
    /// `STANDARD`, or `ACH` when paid by an ACH file.
    pub kind: String,
}

pub async fn list_bill_payments(pool: &SqlitePool, bill_id: i64) -> Result<Vec<BillPaymentView>, String> {
    sqlx::query_as::<_, BillPaymentView>(
        "SELECT id, payment_date, amount_cents, payment_account_id, reference, journal_entry_id, is_voided, kind
         FROM bill_payments WHERE bill_id = ?1 ORDER BY payment_date, id",
    )
    .bind(bill_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// Records one bill payment of `kind` (`STANDARD`, or `ACH` from an ACH file) and posts it. Returns its id.
pub(crate) async fn write_bill_payment(tx: &mut Transaction<'_, Sqlite>, input: &BillPaymentInput, kind: &str) -> Result<i64, String> {
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
        "INSERT INTO bill_payments (bill_id, payment_date, amount_cents, payment_account_id, reference, kind)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING id",
    )
    .bind(input.bill_id)
    .bind(input.payment_date)
    .bind(input.amount_cents)
    .bind(input.payment_account_id)
    .bind(clean(input.reference.as_deref()))
    .bind(kind)
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
    Ok(payment_id)
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
    #[serde(default)]
    pub class_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpenseInput {
    /// `EXPENSE` (paid by bank or card), `CHECK` (written from a bank account) or `CARD_CREDIT`
    /// (a refund or credit from a vendor onto a credit card).
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
    /// `EXPENSE`, `CHECK` or `CARD_CREDIT`. A card credit's total is positive, like a charge's.
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
    SELECT x.id, CASE WHEN x.is_credit = 1 THEN 'CARD_CREDIT' ELSE x.kind END AS kind, v.name AS payee_name, x.payment_account_id, x.expense_date, x.check_number,
           x.payment_method, x.memo, x.total_cents, x.currency_code, x.exchange_rate_e6,
           COALESCE(x.home_total_cents, x.total_cents) AS home_total_cents,
           (SELECT COUNT(*) FROM expense_lines WHERE expense_id = x.id) AS line_count,
           (SELECT CASE WHEN COUNT(DISTINCT account_id) = 1 THEN MAX(account_id) END
            FROM expense_lines WHERE expense_id = x.id) AS category_account_id,
           x.journal_entry_id, x.is_voided
    FROM expenses x LEFT JOIN vendors v ON v.id = x.vendor_id";

/// Records an expense or check (Dr each split account / Cr the payment account) or a card credit
/// (Dr the card / Cr each split account), atomically.
pub async fn post_expense(pool: &SqlitePool, input: ExpenseInput) -> Result<ExpenseView, String> {
    let mut tx = crate::ledger::begin_write(&pool).await.map_err(db_err)?;
    let (id, _) = match write_expense(&mut tx, &input).await {
        Ok(ids) => ids,
        Err(err) => return Err(rollback(tx, err).await),
    };
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    sqlx::query_as::<_, ExpenseView>(&format!("{EXPENSE_SELECT} WHERE x.id = ?1"))
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(db_err)
}

/// Writes and posts an expense inside the caller's transaction; returns (expense id, entry id).
/// Bank feed approvals use this so an approved outflow is a real Expense document.
pub(crate) async fn write_expense(tx: &mut Transaction<'_, Sqlite>, input: &ExpenseInput) -> Result<(i64, i64), String> {
    // A card credit is stored as an EXPENSE flagged as a credit and posts the other way round.
    let (kind, is_credit, source): (&str, bool, fn(i64) -> EntrySource) = match input.kind.as_str() {
        "EXPENSE" => ("EXPENSE", false, EntrySource::Expense),
        "CARD_CREDIT" => ("EXPENSE", true, EntrySource::Expense),
        "CHECK" => ("CHECK", false, EntrySource::Check),
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
    match (kind, is_credit, account_type.as_deref()) {
        ("CHECK", _, Some("BANK")) | ("EXPENSE", false, Some("BANK" | "CREDIT_CARD")) | ("EXPENSE", true, Some("CREDIT_CARD")) => {}
        ("CHECK", _, _) => return Err("EXP_BAD_PAYMENT_ACCOUNT: checks are written from a Bank account".into()),
        (_, true, _) => return Err("EXP_BAD_PAYMENT_ACCOUNT: a card credit goes to a Credit Card account".into()),
        _ => return Err("EXP_BAD_PAYMENT_ACCOUNT: pay expenses from a Bank or Credit Card account".into()),
    }
    let vendor_id = match clean(input.payee_name.as_deref()) {
        Some(name) => Some(find_or_create_named(tx, Party::Vendor, name).await?),
        None => None,
    };
    let check_number = if kind == "CHECK" { clean(input.check_number.as_deref()) } else { None };

    // Journal lines in the document's currency; the ledger converts foreign ones to home currency.
    // A charge debits each split and credits the payment account; a card credit is the reverse.
    let split = |account, cents| if is_credit { JournalLine::credit(account, cents) } else { JournalLine::debit(account, cents) };
    let mut lines: Vec<JournalLine> =
        input.lines.iter().map(|line| split(line.account_id, line.amount_cents).with_memo(line.memo.as_deref()).with_class(line.class_id)).collect();
    lines.push(if is_credit { JournalLine::debit(input.payment_account_id, total) } else { JournalLine::credit(input.payment_account_id, total) });
    let home_total = match &input.currency {
        Some(currency) => {
            currency.validate()?;
            convert_lines(&lines, currency.exchange_rate_e6)?.last().map(|l| l.0 + l.1)
        }
        None => None,
    };

    let expense_id: i64 = sqlx::query_scalar(
        "INSERT INTO expenses
            (kind, vendor_id, payment_account_id, expense_date, check_number, payment_method, memo, total_cents,
             currency_code, exchange_rate_e6, home_total_cents, is_credit)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) RETURNING id",
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
    .bind(is_credit)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    for (idx, line) in input.lines.iter().enumerate() {
        sqlx::query(
            "INSERT INTO expense_lines (expense_id, line_number, account_id, amount_cents, memo, class_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(expense_id)
        .bind(idx as i64 + 1)
        .bind(line.account_id)
        .bind(line.amount_cents)
        .bind(clean(line.memo.as_deref()))
        .bind(line.class_id)
        .execute(&mut **tx)
        .await
        .map_err(|e| format!("{} (split line {})", db_err(e), idx + 1))?;
    }

    let payee = clean(input.payee_name.as_deref()).map(|p| format!(" - {p}")).unwrap_or_default();
    let label = match (kind, check_number) {
        ("CHECK", Some(n)) => format!("Check #{n}{payee}"),
        ("CHECK", None) => format!("Check{payee}"),
        _ if is_credit => format!("Credit Card Credit{payee}"),
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
mod tests;
