//! Item receipts: goods received before the vendor's bill (T12, design in
//! `docs/plans/item-receipts.md`).
//!
//! Receiving posts Dr each line's account (a tracked item's Inventory Asset, or an expense account
//! for freight or a service) / Cr **Items Received Not Billed**, a system current liability, and
//! brings tracked stock in at the receipt's cost. Nothing reaches Accounts Payable, so Pay Bills,
//! A/P aging and vendor balances only ever hold real bills; A/P Aging shows the received-not-billed
//! total underneath.
//!
//! **Enter Bill for Received Items** posts a bill alongside the receipt (the receipt is not replaced,
//! so its stock never moves twice): Dr Items Received Not Billed for the receipt's amount / Cr A/P
//! for the bill's. A line billed at a different price moves the difference to the line's expense
//! account, or for tracked stock to Inventory Asset for the units still on hand and to Cost of Goods
//! Sold for the units already sold.
//!
//! A receipt is voided through its journal entry, which moves its stock back; that is refused while a
//! live bill uses it (void the bill first). Voiding the bill makes the receipt open again.
//! Receipts may be against a purchase order; a receipt line points at the order's line, and the order
//! closes once every line has been received in full (it reopens if a receipt is voided).

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{
    self, db_err, format_cents, mul_div_round, post_entry_in_tx, rollback, EntrySource, JournalEntry, JournalLine, MAX_LINE_CENTS, QUANTITY_SCALE,
};
use crate::money::Cents;

/// The clearing account receipts post to.
pub const CLEARING_NAME: &str = "Items Received Not Billed";
const CLEARING_TYPE: &str = "OTHER_CURRENT_LIABILITY";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptLineInput {
    /// The purchase order line received (when the receipt is against an order).
    #[serde(default)]
    pub purchase_order_line_id: Option<i64>,
    pub account_id: i64,
    #[serde(default)]
    pub product_service_id: Option<i64>,
    #[serde(default)]
    pub description: Option<String>,
    /// Quantity × 10,000 and rate, or neither (then `amount_cents` alone).
    #[serde(default)]
    pub quantity_e4: Option<i64>,
    #[serde(default)]
    pub rate_cents: Option<Cents>,
    pub amount_cents: Cents,
    #[serde(default)]
    pub class_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptInput {
    pub vendor_id: i64,
    #[serde(default)]
    pub purchase_order_id: Option<i64>,
    pub receipt_date: NaiveDate,
    /// The vendor's packing slip or delivery number.
    #[serde(default)]
    pub reference: Option<String>,
    #[serde(default)]
    pub memo: Option<String>,
    pub lines: Vec<ReceiptLineInput>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptView {
    pub id: i64,
    pub receipt_number: i64,
    pub vendor_id: i64,
    pub vendor_name: String,
    pub purchase_order_id: Option<i64>,
    pub po_number: Option<i64>,
    pub receipt_date: String,
    pub reference: Option<String>,
    pub memo: Option<String>,
    pub total_cents: i64,
    pub journal_entry_id: i64,
    pub is_voided: bool,
    /// The live bill entered for it, if any.
    pub bill_id: Option<i64>,
    pub bill_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptLineView {
    pub id: i64,
    pub line_number: i64,
    pub purchase_order_line_id: Option<i64>,
    pub account_id: i64,
    pub product_service_id: Option<i64>,
    pub product_name: Option<String>,
    pub description: String,
    pub quantity_e4: Option<i64>,
    pub rate_cents: Option<i64>,
    pub amount_cents: i64,
    pub class_id: Option<i64>,
    /// The customer or job its purchase order line is for; it carries to the bill.
    pub customer_id: Option<i64>,
    pub customer_name: Option<String>,
}

/// A purchase order line with what live receipts have received of it.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OrderLineReceiving {
    pub id: i64,
    pub line_number: i64,
    pub account_id: i64,
    pub product_service_id: Option<i64>,
    pub product_name: Option<String>,
    pub description: Option<String>,
    pub quantity_e4: Option<i64>,
    pub rate_cents: Option<i64>,
    pub amount_cents: i64,
    pub class_id: Option<i64>,
    pub customer_id: Option<i64>,
    pub customer_name: Option<String>,
    pub received_quantity_e4: i64,
    pub received_amount_cents: i64,
}

const RECEIPT_SELECT: &str = "
    SELECT r.id, r.receipt_number, r.vendor_id, v.name AS vendor_name, r.purchase_order_id, o.po_number, r.receipt_date,
           r.reference, r.memo, r.total_cents, r.journal_entry_id, e.is_voided,
           (SELECT b.id FROM item_receipt_bills rb JOIN bills b ON b.id = rb.bill_id JOIN journal_entries be ON be.id = b.journal_entry_id
            WHERE rb.item_receipt_id = r.id AND be.is_voided = 0 ORDER BY rb.id DESC LIMIT 1) AS bill_id,
           (SELECT b.bill_number FROM item_receipt_bills rb JOIN bills b ON b.id = rb.bill_id JOIN journal_entries be ON be.id = b.journal_entry_id
            WHERE rb.item_receipt_id = r.id AND be.is_voided = 0 ORDER BY rb.id DESC LIMIT 1) AS bill_number
    FROM item_receipts r
    JOIN vendors v ON v.id = r.vendor_id
    JOIN journal_entries e ON e.id = r.journal_entry_id
    LEFT JOIN purchase_orders o ON o.id = r.purchase_order_id";

/// What live (not voided) receipts have received of each line of an order.
const RECEIVED_BY_LINE: &str = "
    SELECT l.purchase_order_line_id AS po_line_id, COALESCE(SUM(l.quantity_e4), 0) AS qty, SUM(l.amount_cents) AS amount
    FROM item_receipt_lines l JOIN item_receipts r ON r.id = l.item_receipt_id JOIN journal_entries e ON e.id = r.journal_entry_id
    WHERE e.is_voided = 0 AND l.purchase_order_line_id IS NOT NULL
    GROUP BY l.purchase_order_line_id";

fn clean(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|t| !t.is_empty())
}

async fn run<T>(pool: &SqlitePool, work: impl for<'t> AsyncFnOnce(&mut Transaction<'t, Sqlite>) -> Result<T, String>) -> Result<T, String> {
    let mut tx = ledger::begin_write(pool).await.map_err(db_err)?;
    match work(&mut tx).await {
        Ok(value) => ledger::commit_write(tx).await.map(|()| value).map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// The Items Received Not Billed account, created (as a system account) the first time it's needed.
pub(crate) async fn clearing_account(tx: &mut Transaction<'_, Sqlite>) -> Result<i64, String> {
    let existing: Option<i64> =
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = ?1 AND account_type = ?2 AND is_system = 1 ORDER BY id LIMIT 1")
            .bind(CLEARING_NAME)
            .bind(CLEARING_TYPE)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
    if let Some(id) = existing {
        return Ok(id);
    }
    // Numbered like the chart's other accounts (four or five digits), unless the number is taken.
    sqlx::query_scalar(
        "INSERT INTO chart_of_accounts (account_number, name, account_type, description, is_system)
         SELECT CASE WHEN EXISTS (SELECT 1 FROM chart_of_accounts WHERE account_number = n.number) THEN NULL ELSE n.number END, ?1, ?2,
                'Goods received from vendors whose bills have not been entered yet', 1
         FROM (SELECT CASE WHEN EXISTS (SELECT 1 FROM chart_of_accounts WHERE length(account_number) = 5 AND account_number GLOB '[0-9]*')
                           THEN '20500' ELSE '2050' END AS number) n
         RETURNING id",
    )
    .bind(CLEARING_NAME)
    .bind(CLEARING_TYPE)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)
}

/// The account a receipt or its bill line may post to: an active expense or asset account that isn't
/// a system account (the same rule as bill lines).
async fn check_line_account(tx: &mut Transaction<'_, Sqlite>, account_id: i64, n: usize) -> Result<(), String> {
    let category: Option<(String, String, bool, bool)> = sqlx::query_as(
        "SELECT t.classification, a.account_type, a.is_active, a.is_system
         FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1",
    )
    .bind(account_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    match category {
        None => Err(format!("RECEIPT_UNKNOWN_ACCOUNT: line {n} references an account that does not exist")),
        Some((_, _, false, _)) => Err(format!("RECEIPT_INACTIVE_ACCOUNT: line {n} uses an inactive account")),
        Some((class, kind, _, is_system))
            if is_system || !(class == "EXPENSE" || matches!(kind.as_str(), "FIXED_ASSET" | "OTHER_ASSET" | "OTHER_CURRENT_ASSET")) =>
        {
            Err(format!("RECEIPT_BAD_CATEGORY: line {n} must use an expense or asset account"))
        }
        Some(_) => Ok(()),
    }
}

/// Receive Items: records goods received from a vendor, against one of its open purchase orders or
/// on their own. Posts Dr each line's account / Cr Items Received Not Billed, and brings tracked
/// stock in. Receiving more than was ordered is allowed (the screen warns first).
pub async fn receive_items(pool: &SqlitePool, input: ReceiptInput) -> Result<ReceiptView, String> {
    if input.lines.is_empty() {
        return Err("RECEIPT_NO_LINES: receive at least one line".into());
    }
    let mut total: i64 = 0;
    for (idx, line) in input.lines.iter().enumerate() {
        let n = idx + 1;
        if !(1..=MAX_LINE_CENTS).contains(&line.amount_cents.get()) {
            return Err(format!("RECEIPT_BAD_AMOUNT: line {n} needs an amount above zero"));
        }
        match (line.quantity_e4, line.rate_cents) {
            (None, None) => {}
            (Some(q), Some(r)) if q > 0 && r >= Cents::ZERO => {}
            _ => return Err(format!("RECEIPT_BAD_QUANTITY: line {n} needs a quantity above zero and a rate, or neither")),
        }
        total = total
            .checked_add(line.amount_cents.get())
            .filter(|t| *t <= MAX_LINE_CENTS)
            .ok_or_else(|| format!("RECEIPT_TOO_LARGE: the receipt exceeds {}", format_cents(MAX_LINE_CENTS)))?;
    }
    let id = run(pool, async |tx| write_receipt(tx, &input, total).await).await?;
    receipt(pool, id).await
}

async fn write_receipt(tx: &mut Transaction<'_, Sqlite>, input: &ReceiptInput, total: i64) -> Result<i64, String> {
    let vendor: String = sqlx::query_scalar("SELECT name FROM vendors WHERE id = ?1")
        .bind(input.vendor_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?
        .ok_or("RECEIPT_NO_VENDOR: that vendor does not exist")?;
    if let Some(order) = input.purchase_order_id {
        let head: Option<(i64, String, i64)> = sqlx::query_as("SELECT vendor_id, status, po_number FROM purchase_orders WHERE id = ?1")
            .bind(order)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
        let (order_vendor, status, number) = head.ok_or("RECEIPT_NO_ORDER: that purchase order does not exist")?;
        if order_vendor != input.vendor_id {
            return Err(format!("RECEIPT_OTHER_VENDOR: purchase order #{number} is for another vendor"));
        }
        if status != "OPEN" {
            return Err(format!("RECEIPT_ORDER_NOT_OPEN: purchase order #{number} is {}; only an open order is received", status.to_lowercase()));
        }
    }

    let mut debits: Vec<((i64, Option<i64>), i64)> = Vec::new();
    let mut received: Vec<(i64, i64, i64)> = Vec::new();
    for (idx, line) in input.lines.iter().enumerate() {
        let n = idx + 1;
        check_line_account(tx, line.account_id, n).await?;
        if let Some(po_line) = line.purchase_order_line_id {
            let on_order: Option<i64> = sqlx::query_scalar("SELECT purchase_order_id FROM purchase_order_lines WHERE id = ?1")
                .bind(po_line)
                .fetch_optional(&mut **tx)
                .await
                .map_err(db_err)?;
            if on_order.is_none() || on_order != input.purchase_order_id {
                return Err(format!("RECEIPT_BAD_ORDER_LINE: line {n} is not a line of this purchase order"));
            }
        }
        if let Some(product) = line.product_service_id {
            if let Some(item) = crate::inventory::check_purchase_line(tx, product, line.account_id, line.quantity_e4, n).await? {
                received.push((item, line.quantity_e4.unwrap_or_default(), line.amount_cents.get()));
            }
        }
        if let Some(class) = line.class_id {
            crate::classes::check_usable(tx, class, &format!("RECEIPT: line {n}")).await?;
        }
        let key = (line.account_id, line.class_id);
        match debits.iter_mut().find(|(k, _)| *k == key) {
            Some((_, sum)) => *sum = sum.checked_add(line.amount_cents.get()).ok_or("RECEIPT_TOO_LARGE: the receipt is too large")?,
            None => debits.push((key, line.amount_cents.get())),
        }
    }

    let clearing = clearing_account(tx).await?;
    let number: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(receipt_number), 0) + 1 FROM item_receipts").fetch_one(&mut **tx).await.map_err(db_err)?;
    let mut lines: Vec<JournalLine> = debits.into_iter().map(|((account, class), cents)| JournalLine::debit(account, cents).with_class(class)).collect();
    lines.push(JournalLine::credit(clearing, total));
    let stock = crate::inventory::plan_receipt(tx, &received).await?;
    lines.extend(stock.lines.iter().cloned());
    let reference = clean(input.reference.as_deref()).map(|r| format!(" ({r})")).unwrap_or_default();
    let entry =
        JournalEntry { id: None, date: input.receipt_date, description: format!("Item Receipt #{number}{reference} - {vendor}"), lines, currency: None };
    let entry_id = post_entry_in_tx(tx, &entry, EntrySource::Journal).await?;
    stock.record(tx, input.receipt_date, entry_id).await?;

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO item_receipts (receipt_number, vendor_id, purchase_order_id, receipt_date, reference, memo, total_cents, journal_entry_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING id",
    )
    .bind(number)
    .bind(input.vendor_id)
    .bind(input.purchase_order_id)
    .bind(input.receipt_date)
    .bind(clean(input.reference.as_deref()))
    .bind(clean(input.memo.as_deref()))
    .bind(total)
    .bind(entry_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    for (idx, line) in input.lines.iter().enumerate() {
        sqlx::query(
            "INSERT INTO item_receipt_lines (item_receipt_id, line_number, purchase_order_line_id, account_id, product_service_id, description, quantity_e4, rate_cents, amount_cents, class_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )
        .bind(id)
        .bind(idx as i64 + 1)
        .bind(line.purchase_order_line_id)
        .bind(line.account_id)
        .bind(line.product_service_id)
        .bind(clean(line.description.as_deref()).unwrap_or(""))
        .bind(line.quantity_e4)
        .bind(line.rate_cents)
        .bind(line.amount_cents)
        .bind(line.class_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    if let Some(order) = input.purchase_order_id {
        close_if_received(tx, order).await?;
    }
    Ok(id)
}

/// Whether every line of an order has been received in full (by quantity, or by amount for a line
/// without one), on receipts that aren't voided. An order with no lines is not.
pub(crate) async fn received_in_full(conn: &mut sqlx::SqliteConnection, order: i64) -> Result<bool, String> {
    sqlx::query_scalar(&format!(
        "SELECT EXISTS (SELECT 1 FROM {RECEIPT_LINES_OF_ORDER}) AND NOT EXISTS (
             SELECT 1 FROM purchase_order_lines p LEFT JOIN ({RECEIVED_BY_LINE}) r ON r.po_line_id = p.id
             WHERE p.purchase_order_id = ?1
               AND (CASE WHEN p.quantity_e4 IS NOT NULL THEN COALESCE(r.qty, 0) < p.quantity_e4 ELSE COALESCE(r.amount, 0) < p.amount_cents END))"
    ))
    .bind(order)
    .fetch_one(conn)
    .await
    .map_err(db_err)
}

/// The receipt lines received against order `?1` (receipts not voided).
const RECEIPT_LINES_OF_ORDER: &str = "item_receipt_lines l JOIN item_receipts r ON r.id = l.item_receipt_id JOIN journal_entries e ON e.id = r.journal_entry_id
    WHERE r.purchase_order_id = ?1 AND e.is_voided = 0";

/// Closes an open order once every line has been received in full (by quantity, or by amount for a
/// line without one).
async fn close_if_received(tx: &mut Transaction<'_, Sqlite>, order: i64) -> Result<(), String> {
    sqlx::query(&format!(
        "UPDATE purchase_orders SET status = 'CLOSED'
         WHERE id = ?1 AND status = 'OPEN' AND NOT EXISTS (
             SELECT 1 FROM purchase_order_lines p LEFT JOIN ({RECEIVED_BY_LINE}) r ON r.po_line_id = p.id
             WHERE p.purchase_order_id = ?1
               AND (CASE WHEN p.quantity_e4 IS NOT NULL THEN COALESCE(r.qty, 0) < p.quantity_e4 ELSE COALESCE(r.amount, 0) < p.amount_cents END))"
    ))
    .bind(order)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

/// Called by [`ledger`] while voiding a journal entry: refuses voiding a receipt a live bill uses,
/// and reopens the receipt's purchase order (closed when it was received in full).
pub(crate) async fn before_void(tx: &mut Transaction<'_, Sqlite>, journal_entry_id: i64) -> Result<(), String> {
    let receipt: Option<(i64, i64, Option<i64>)> =
        sqlx::query_as("SELECT id, receipt_number, purchase_order_id FROM item_receipts WHERE journal_entry_id = ?1")
            .bind(journal_entry_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
    let Some((id, number, order)) = receipt else { return Ok(()) };
    let billed: Option<String> = sqlx::query_scalar(
        "SELECT COALESCE(b.bill_number, '') FROM item_receipt_bills rb JOIN bills b ON b.id = rb.bill_id JOIN journal_entries e ON e.id = b.journal_entry_id
         WHERE rb.item_receipt_id = ?1 AND e.is_voided = 0 LIMIT 1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    if let Some(bill) = billed {
        let which = if bill.is_empty() { String::new() } else { format!(" #{bill}") };
        return Err(format!("VOID_RECEIPT_BILLED: item receipt #{number} has been billed (bill{which}); void the bill first"));
    }
    if let Some(order) = order {
        sqlx::query("UPDATE purchase_orders SET status = 'OPEN' WHERE id = ?1 AND status = 'CLOSED'").bind(order).execute(&mut **tx).await.map_err(db_err)?;
    }
    Ok(())
}

/// One receipt.
pub async fn receipt(pool: &SqlitePool, id: i64) -> Result<ReceiptView, String> {
    sqlx::query_as(&format!("{RECEIPT_SELECT} WHERE r.id = ?1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| "RECEIPT_NOT_FOUND: that item receipt does not exist".into())
}

/// Receipts, newest first: every one, or only those still waiting for their bill (`open_only`).
pub async fn list_receipts(pool: &SqlitePool, open_only: bool) -> Result<Vec<ReceiptView>, String> {
    sqlx::query_as(&format!(
        "SELECT * FROM ({RECEIPT_SELECT}) WHERE NOT ?1 OR (is_voided = 0 AND bill_id IS NULL) ORDER BY receipt_date DESC, receipt_number DESC"
    ))
    .bind(open_only)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn receipt_lines(pool: &SqlitePool, id: i64) -> Result<Vec<ReceiptLineView>, String> {
    sqlx::query_as(
        "SELECT l.id, l.line_number, l.purchase_order_line_id, l.account_id, l.product_service_id, p.name AS product_name, l.description,
                l.quantity_e4, l.rate_cents, l.amount_cents, l.class_id, o.customer_id, c.name AS customer_name
         FROM item_receipt_lines l LEFT JOIN products_services p ON p.id = l.product_service_id
         LEFT JOIN purchase_order_lines o ON o.id = l.purchase_order_line_id LEFT JOIN customers c ON c.id = o.customer_id
         WHERE l.item_receipt_id = ?1 ORDER BY l.line_number",
    )
    .bind(id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// A purchase order's lines with what has been received of each.
pub async fn order_receiving(pool: &SqlitePool, purchase_order_id: i64) -> Result<Vec<OrderLineReceiving>, String> {
    sqlx::query_as(&format!(
        "SELECT p.id, p.line_number, p.account_id, p.product_service_id, ps.name AS product_name, p.description, p.quantity_e4, p.rate_cents, p.amount_cents,
                p.class_id, p.customer_id, c.name AS customer_name, COALESCE(r.qty, 0) AS received_quantity_e4, COALESCE(r.amount, 0) AS received_amount_cents
         FROM purchase_order_lines p LEFT JOIN products_services ps ON ps.id = p.product_service_id
         LEFT JOIN customers c ON c.id = p.customer_id
         LEFT JOIN ({RECEIVED_BY_LINE}) r ON r.po_line_id = p.id
         WHERE p.purchase_order_id = ?1 ORDER BY p.line_number"
    ))
    .bind(purchase_order_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

/// The total received and not yet billed, as of a date (for A/P Aging): what live receipts dated on
/// or before it hold, less what their live bills dated on or before it cleared.
pub async fn received_not_billed(pool: &SqlitePool, as_of: NaiveDate) -> Result<i64, String> {
    sqlx::query_scalar(
        "SELECT COALESCE(SUM(l.credit_cents - l.debit_cents), 0)
         FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
         JOIN chart_of_accounts a ON a.id = l.account_id
         WHERE a.name = ?1 AND a.account_type = ?2 AND a.is_system = 1 AND e.status = 'POSTED' AND e.entry_date <= ?3",
    )
    .bind(CLEARING_NAME)
    .bind(CLEARING_TYPE)
    .bind(as_of)
    .fetch_one(pool)
    .await
    .map_err(db_err)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptBillLine {
    pub receipt_line_id: i64,
    /// What the vendor billed for the line (the receipt's amount when unchanged).
    pub amount_cents: Cents,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptBillInput {
    pub item_receipt_id: i64,
    #[serde(default)]
    pub bill_number: Option<String>,
    pub bill_date: NaiveDate,
    pub due_date: NaiveDate,
    #[serde(default)]
    pub terms_id: Option<i64>,
    #[serde(default)]
    pub memo: Option<String>,
    /// Every line of the receipt, with its billed amount.
    pub lines: Vec<ReceiptBillLine>,
}

/// Enter Bill for Received Items: posts the vendor's bill for a receipt, alongside it. Returns the
/// bill id. Dr Items Received Not Billed (the receipt's amount) / Cr A/P (the bill's), with each
/// line's price difference posted as the module notes say.
pub async fn bill_received_items(pool: &SqlitePool, input: ReceiptBillInput) -> Result<i64, String> {
    if input.due_date < input.bill_date {
        return Err("BILL_DUE_BEFORE_DATE: due date cannot be earlier than the bill date".into());
    }
    run(pool, async |tx| write_receipt_bill(tx, &input).await).await
}

async fn write_receipt_bill(tx: &mut Transaction<'_, Sqlite>, input: &ReceiptBillInput) -> Result<i64, String> {
    let head: Option<(i64, i64, String, i64, bool, Option<i64>)> = sqlx::query_as(
        "SELECT r.receipt_number, r.vendor_id, v.name, r.total_cents, e.is_voided, r.purchase_order_id
         FROM item_receipts r JOIN vendors v ON v.id = r.vendor_id JOIN journal_entries e ON e.id = r.journal_entry_id WHERE r.id = ?1",
    )
    .bind(input.item_receipt_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let (number, vendor_id, vendor, receipt_total, voided, order) = head.ok_or("RECEIPT_NOT_FOUND: that item receipt does not exist")?;
    if voided {
        return Err(format!("RECEIPT_VOIDED: item receipt #{number} is voided"));
    }
    let billed: Option<i64> = sqlx::query_scalar(
        "SELECT rb.bill_id FROM item_receipt_bills rb JOIN bills b ON b.id = rb.bill_id JOIN journal_entries e ON e.id = b.journal_entry_id
         WHERE rb.item_receipt_id = ?1 AND e.is_voided = 0 LIMIT 1",
    )
    .bind(input.item_receipt_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    if billed.is_some() {
        return Err(format!("RECEIPT_BILLED: item receipt #{number} already has its bill"));
    }
    if let Some(terms) = input.terms_id {
        crate::profile_lists::check_terms(tx, terms, None, "BILL").await?;
    }
    // A line's customer or job is its purchase order line's: order lines are never edited, and a
    // merge moves the job on the order line to the one kept.
    type LineRow = (i64, i64, i64, Option<i64>, String, Option<i64>, i64, Option<i64>, Option<i64>, Option<i64>);
    let lines: Vec<LineRow> = sqlx::query_as(
        "SELECT l.id, l.line_number, l.account_id, l.product_service_id, l.description, l.quantity_e4, l.amount_cents, l.class_id, l.purchase_order_line_id, o.customer_id
         FROM item_receipt_lines l LEFT JOIN purchase_order_lines o ON o.id = l.purchase_order_line_id
         WHERE l.item_receipt_id = ?1 ORDER BY l.line_number",
    )
    .bind(input.item_receipt_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    if input.lines.len() != lines.len() || lines.iter().any(|l| !input.lines.iter().any(|b| b.receipt_line_id == l.0)) {
        return Err("RECEIPT_BILL_LINES: bill every line of the receipt once".into());
    }

    let mut total: i64 = 0;
    let mut billed_lines = Vec::with_capacity(lines.len());
    for (id, n, account, product, description, quantity, received, class, _, customer) in &lines {
        let amount = input.lines.iter().find(|b| b.receipt_line_id == *id).map(|b| b.amount_cents.get()).unwrap_or(*received);
        if !(1..=MAX_LINE_CENTS).contains(&amount) {
            return Err(format!("RECEIPT_BAD_AMOUNT: line {n} needs an amount above zero"));
        }
        total = total.checked_add(amount).filter(|t| *t <= MAX_LINE_CENTS).ok_or("RECEIPT_TOO_LARGE: the bill is too large")?;
        billed_lines.push((*n, *account, *product, description.clone(), *quantity, *received, amount, *class, *customer));
    }
    // The order's discounts and charges, after the lines received from their order lines (T21).
    let splits: Vec<i64> = billed_lines.iter().map(|l| l.6).collect();
    let po_lines: Vec<Option<i64>> = lines.iter().map(|l| l.8).collect();
    let form = crate::purchase_lines::fit_carried(carried_form_lines(tx, order, &po_lines).await?, &splits);
    let (form_amounts, total) = crate::purchase_lines::lay_out(&splits, &form, "BILL")?;
    crate::purchase_lines::check(tx, &form, "BILL").await?;

    let bill_id: i64 = sqlx::query_scalar(
        "INSERT INTO bills (vendor_id, bill_number, bill_date, due_date, memo, total_cents, status, terms_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'DRAFT', ?7) RETURNING id",
    )
    .bind(vendor_id)
    .bind(clean(input.bill_number.as_deref()))
    .bind(input.bill_date)
    .bind(input.due_date)
    .bind(clean(input.memo.as_deref()).map(str::to_string).unwrap_or_else(|| format!("Item receipt #{number}")))
    .bind(total)
    .bind(input.terms_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    crate::purchase_lines::write(tx, crate::purchase_lines::PurchaseDoc::Bill, bill_id, &form, &form_amounts).await?;
    let clearing = clearing_account(tx).await?;
    let ap: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'ACCOUNTS_PAYABLE' AND is_system = 1 ORDER BY id LIMIT 1")
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?
        .ok_or("COA_SYSTEM_ACCOUNT_MISSING: no system Accounts Payable account")?;
    let mut journal = vec![JournalLine::debit(clearing, receipt_total)];
    let mut differences = Vec::new();
    for (n, account, product, description, quantity, received, amount, class, customer) in &billed_lines {
        // The bill line keeps quantity × rate only when the billed amount still equals it.
        let rate = quantity.and_then(|q| mul_div_round(*amount, QUANTITY_SCALE, q)).filter(|r| *r > 0);
        let priced = match (quantity, rate) {
            (Some(q), Some(r)) if mul_div_round(*q, r, QUANTITY_SCALE) == Some(*amount) => Some((*q, r)),
            _ => None,
        };
        // Billable by the company's preference, as on a bill entered by hand.
        let billable = crate::expenses::is_billable(tx, None, *customer).await?;
        sqlx::query(
            "INSERT INTO bill_lines (bill_id, line_number, memo, account_id, amount_cents, quantity_e4, rate_cents, product_service_id, class_id, customer_id, is_billable)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )
        .bind(bill_id)
        .bind(n)
        .bind(description)
        .bind(account)
        .bind(amount)
        .bind(priced.map(|p| p.0))
        .bind(priced.map(|p| p.1))
        .bind(product)
        .bind(class)
        .bind(customer)
        .bind(billable)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
        let diff = amount - received;
        if diff == 0 {
            continue;
        }
        let stock = match product {
            Some(p) => crate::inventory::plan_price_difference(tx, *p, quantity.unwrap_or_default(), diff, *class).await?,
            None => None,
        };
        match stock {
            Some(plan) => {
                journal.extend(plan.lines.iter().cloned());
                differences.push(plan);
            }
            None if diff > 0 => journal.push(JournalLine::debit(*account, diff).with_class(*class)),
            None => journal.push(JournalLine::credit(*account, -diff).with_class(*class)),
        }
    }
    journal.extend(crate::purchase_lines::journal_lines(&form, &form_amounts, true));
    journal.push(JournalLine::credit(ap, total));
    let reference = clean(input.bill_number.as_deref()).map(|n| format!(" #{n}")).unwrap_or_default();
    let entry = JournalEntry {
        id: None,
        date: input.bill_date,
        description: format!("Bill{reference} - {vendor} (item receipt #{number})"),
        lines: journal,
        currency: None,
    };
    let entry_id = post_entry_in_tx(tx, &entry, EntrySource::Bill(bill_id)).await?;
    for plan in &differences {
        plan.record(tx, input.bill_date, entry_id).await?;
    }
    sqlx::query("UPDATE bills SET status = 'UNPAID', journal_entry_id = ?1 WHERE id = ?2")
        .bind(entry_id)
        .bind(bill_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    sqlx::query("INSERT INTO item_receipt_bills (item_receipt_id, bill_id) VALUES (?1, ?2)")
        .bind(input.item_receipt_id)
        .bind(bill_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    Ok(bill_id)
}

/// The purchase order's discounts and charges a receipt's bill takes, placed after the receipt lines
/// received from their order lines (`po_lines`, in receipt line order). A percent line applies to
/// every bill's line for that order line. A fixed amount not yet on a live bill is placed after each
/// receipt line from its order line, tagged with its `source`; [`crate::purchase_lines::fit_carried`]
/// keeps it once, after the first line that can take it, or holds it back for a later bill.
async fn carried_form_lines(
    tx: &mut Transaction<'_, Sqlite>,
    order: Option<i64>,
    po_lines: &[Option<i64>],
) -> Result<Vec<crate::purchase_lines::CarriedLine>, String> {
    let Some(order) = order else { return Ok(Vec::new()) };
    let views: Vec<crate::purchase_lines::PurchaseFormLineView> = sqlx::query_as(
        "SELECT after_line, kind, description, product_service_id, percent_ppm, amount_cents, account_id, class_id
         FROM purchase_form_lines WHERE purchase_order_id = ?1 ORDER BY after_line",
    )
    .bind(order)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    if views.is_empty() {
        return Ok(Vec::new());
    }
    let numbers: Vec<(i64, i64)> = sqlx::query_as("SELECT id, line_number FROM purchase_order_lines WHERE purchase_order_id = ?1")
        .bind(order)
        .fetch_all(&mut **tx)
        .await
        .map_err(db_err)?;
    let inputs = crate::purchase_lines::inputs_of(&views);
    let mut out = Vec::new();
    for (i, po_line) in po_lines.iter().enumerate() {
        let Some(po_line) = *po_line else { continue };
        let Some(&(_, number)) = numbers.iter().find(|(id, _)| *id == po_line) else { continue };
        for (source, (view, input)) in views.iter().zip(&inputs).enumerate() {
            if view.after_line != number {
                continue;
            }
            if input.percent_ppm.is_none() {
                // Already on a live bill for this order line (an earlier part shipment).
                let carried: bool = sqlx::query_scalar(
                    "SELECT EXISTS (
                         SELECT 1 FROM purchase_form_lines f
                         JOIN item_receipt_bills rb ON rb.bill_id = f.bill_id
                         JOIN bills b ON b.id = f.bill_id
                         JOIN journal_entries e ON e.id = b.journal_entry_id
                         JOIN item_receipt_lines l ON l.item_receipt_id = rb.item_receipt_id AND l.line_number = f.after_line
                         WHERE e.is_voided = 0 AND l.purchase_order_line_id = ?1 AND f.kind = ?2 AND f.percent_ppm IS NULL
                           AND f.amount_cents = ?3 AND f.account_id = ?4)",
                )
                .bind(po_line)
                .bind(&view.kind)
                .bind(view.amount_cents)
                .bind(view.account_id)
                .fetch_one(&mut **tx)
                .await
                .map_err(db_err)?;
                if carried {
                    continue;
                }
            }
            // A fixed one after each receipt line from its order line; `fit_carried` keeps one.
            out.push(crate::purchase_lines::CarriedLine { line: crate::purchase_lines::PurchaseFormLineInput { after_line: i + 1, ..input.clone() }, source });
        }
    }
    Ok(out)
}

/// What Enter Bill for Received Items shows it will take from the receipt's purchase order: the
/// discounts and charges [`carried_form_lines`] places, before the billed amounts are known (the
/// dialog fits them to its amounts as [`crate::purchase_lines::fit_carried`] does).
pub async fn receipt_bill_form_lines(pool: &SqlitePool, item_receipt_id: i64) -> Result<Vec<crate::purchase_lines::CarriedLine>, String> {
    let mut tx = pool.begin().await.map_err(db_err)?;
    let order: Option<Option<i64>> =
        sqlx::query_scalar("SELECT purchase_order_id FROM item_receipts WHERE id = ?1").bind(item_receipt_id).fetch_optional(&mut *tx).await.map_err(db_err)?;
    let order = order.ok_or("RECEIPT_NOT_FOUND: that item receipt does not exist")?;
    let po_lines: Vec<Option<i64>> =
        sqlx::query_scalar("SELECT purchase_order_line_id FROM item_receipt_lines WHERE item_receipt_id = ?1 ORDER BY line_number")
            .bind(item_receipt_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(db_err)?;
    let lines = carried_form_lines(&mut tx, order, &po_lines).await?;
    tx.rollback().await.map_err(db_err)?;
    Ok(lines)
}

/// Whether a bill was entered for an item receipt (such a bill is voided and entered again, not
/// edited: an edit would bring its stock in a second time).
pub(crate) async fn is_receipt_bill(tx: &mut Transaction<'_, Sqlite>, bill_id: i64) -> Result<bool, String> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM item_receipt_bills WHERE bill_id = ?1)").bind(bill_id).fetch_one(&mut **tx).await.map_err(db_err)
}

/// Whether a purchase order has live receipts (then it's billed through them, not as a whole).
pub(crate) async fn order_has_receipts(tx: &mut Transaction<'_, Sqlite>, purchase_order_id: i64) -> Result<bool, String> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM item_receipts r JOIN journal_entries e ON e.id = r.journal_entry_id WHERE r.purchase_order_id = ?1 AND e.is_voided = 0)",
    )
    .bind(purchase_order_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)
}

#[cfg(test)]
mod tests;
