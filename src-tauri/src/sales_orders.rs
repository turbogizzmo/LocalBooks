//! Sales orders (gaps row T69, design in `docs/plans/sales-orders.md`, migration 0153).
//!
//! A sales order records what a customer ordered before it is shipped and billed. It posts nothing.
//! Invoices are made from it, whole or in parts ([`invoice_from_order`]); each takes quantities off
//! the order's lines in the same write transaction as the invoice posts, through link rows in
//! `sales_order_invoiced`. Voiding such an invoice gives its quantities back (a trigger on
//! `invoices.is_voided`), and an edited invoice takes the links over ([`follow_replaced_invoice`]).
//!
//! What is left on open order lines is **on sales order** for its item; on hand less that is
//! **available**, and the part of a line that available stock can't cover is **backordered**
//! ([`open_lines`]). Nothing in stock moves until the invoice.

use std::collections::{BTreeMap, HashMap};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::invoices::{validate_line, InvoiceInput};
use crate::ledger::{db_err, format_cents, rollback, MAX_LINE_CENTS};
use crate::money::Cents;
use crate::sales::{find_or_create_named, Party, SalesLineInput};
use crate::sales_header::SalesHeader;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesOrderLineInput {
    /// An existing line being kept (editing an order); `None` for a new line.
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(flatten)]
    pub line: SalesLineInput,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesOrderInput {
    pub customer_name: String,
    /// The number typed in the form's number box; `None` takes the next one; ignored when an order is changed, which keeps its number (`doc_numbers`).
    #[serde(default)]
    pub order_number: Option<i64>,
    pub order_date: NaiveDate,
    #[serde(default)]
    pub expected_date: Option<NaiveDate>,
    #[serde(default)]
    pub memo: Option<String>,
    pub lines: Vec<SalesOrderLineInput>,
    #[serde(default)]
    pub tax_rate_id: Option<i64>,
    #[serde(default)]
    pub sales_rep_id: Option<i64>,
    #[serde(default)]
    pub ship_method_id: Option<i64>,
    #[serde(default)]
    pub customer_message_id: Option<i64>,
    /// Bill To, Ship To, P.O. number and FOB; addresses left out come from the customer.
    #[serde(flatten, default)]
    pub header: SalesHeader,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SalesOrderSummary {
    pub id: i64,
    pub order_number: i64,
    pub customer_id: i64,
    pub customer_name: String,
    pub order_date: NaiveDate,
    pub expected_date: Option<NaiveDate>,
    pub memo: Option<String>,
    pub po_number: Option<String>,
    pub is_closed: bool,
    /// `OPEN` (nothing invoiced), `PARTLY_INVOICED` or `CLOSED` (nothing left to invoice).
    pub status: String,
    /// Ordered amount, before tax.
    pub total_cents: i64,
    /// What is left to invoice, at the order's rates.
    pub open_cents: i64,
    pub line_count: i64,
    /// Invoices were made from it (voided ones too), so it can be closed but not deleted.
    pub has_invoices: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct SalesOrderLine {
    pub id: i64,
    pub line_number: i64,
    pub product_service_id: Option<i64>,
    pub item_name: Option<String>,
    pub description: String,
    pub quantity_e4: i64,
    pub rate_cents: i64,
    pub amount_cents: i64,
    pub income_account_id: i64,
    pub class_id: Option<i64>,
    pub is_taxable: bool,
    pub invoiced_qty_e4: i64,
    pub closed_qty_e4: i64,
    /// Ordered less invoiced and closed.
    pub left_qty_e4: i64,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OrderInvoice {
    pub invoice_id: i64,
    pub invoice_number: i64,
    pub issue_date: NaiveDate,
    pub is_voided: bool,
    pub quantity_e4: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesOrderDetail {
    #[serde(flatten)]
    pub summary: SalesOrderSummary,
    pub tax_rate_id: Option<i64>,
    pub sales_rep_id: Option<i64>,
    pub ship_method_id: Option<i64>,
    pub customer_message_id: Option<i64>,
    #[serde(flatten)]
    pub header: SalesHeader,
    pub lines: Vec<SalesOrderLine>,
    /// The invoices made from this order, newest first.
    pub invoices: Vec<OrderInvoice>,
}

const SUMMARY_SELECT: &str = "
    SELECT o.id, o.order_number, o.customer_id, c.name AS customer_name, o.order_date, o.expected_date, o.memo, o.po_number,
           o.is_closed = 1 AS is_closed,
           CASE WHEN o.is_closed = 1 OR NOT EXISTS (SELECT 1 FROM sales_order_lines l WHERE l.sales_order_id = o.id AND l.quantity_e4 > l.invoiced_qty_e4 + l.closed_qty_e4) THEN 'CLOSED'
                WHEN EXISTS (SELECT 1 FROM sales_order_lines l WHERE l.sales_order_id = o.id AND l.invoiced_qty_e4 > 0) THEN 'PARTLY_INVOICED'
                ELSE 'OPEN' END AS status,
           COALESCE((SELECT SUM(l.amount_cents) FROM sales_order_lines l WHERE l.sales_order_id = o.id), 0) AS total_cents,
           CASE WHEN o.is_closed = 1 THEN 0 ELSE
               COALESCE((SELECT SUM((((l.quantity_e4 - l.invoiced_qty_e4 - l.closed_qty_e4) * l.rate_cents) + 5000) / 10000)
                         FROM sales_order_lines l WHERE l.sales_order_id = o.id), 0) END AS open_cents,
           (SELECT COUNT(*) FROM sales_order_lines l WHERE l.sales_order_id = o.id) AS line_count,
           EXISTS (SELECT 1 FROM sales_order_invoiced k JOIN sales_order_lines l ON l.id = k.sales_order_line_id WHERE l.sales_order_id = o.id) AS has_invoices
    FROM sales_orders o JOIN customers c ON c.id = o.customer_id";

/// Every sales order, newest first; `open_only` leaves out closed ones.
pub async fn list(pool: &SqlitePool, open_only: bool) -> Result<Vec<SalesOrderSummary>, String> {
    let filter = if open_only { "WHERE status <> 'CLOSED'" } else { "" };
    sqlx::query_as(&format!("SELECT * FROM ({SUMMARY_SELECT}) {filter} ORDER BY order_number DESC")).fetch_all(pool).await.map_err(db_err)
}

async fn summary(conn: &mut sqlx::SqliteConnection, id: i64) -> Result<SalesOrderSummary, String> {
    sqlx::query_as(&format!("{SUMMARY_SELECT} WHERE o.id = ?1"))
        .bind(id)
        .fetch_optional(conn)
        .await
        .map_err(db_err)?
        .ok_or_else(|| "SO_NOT_FOUND: that sales order does not exist".to_string())
}

pub async fn detail(pool: &SqlitePool, id: i64) -> Result<SalesOrderDetail, String> {
    let mut conn = pool.acquire().await.map_err(db_err)?;
    let summary = summary(&mut conn, id).await?;
    type Head = (Option<i64>, Option<i64>, Option<i64>, Option<i64>, Option<String>, Option<String>, Option<String>, Option<String>);
    let (tax_rate_id, sales_rep_id, ship_method_id, customer_message_id, bill_to, ship_to, po_number, fob): Head = sqlx::query_as(
        "SELECT tax_rate_id, sales_rep_id, ship_method_id, customer_message_id, bill_to, ship_to, po_number, fob FROM sales_orders WHERE id = ?1",
    )
    .bind(id)
    .fetch_one(&mut *conn)
    .await
    .map_err(db_err)?;
    let lines = order_lines(&mut conn, id).await?;
    let invoices: Vec<OrderInvoice> = sqlx::query_as(
        "SELECT i.id AS invoice_id, i.invoice_number, i.issue_date, i.is_voided = 1 AS is_voided, SUM(k.quantity_e4) AS quantity_e4
         FROM sales_order_invoiced k JOIN sales_order_lines l ON l.id = k.sales_order_line_id JOIN invoices i ON i.id = k.invoice_id
         WHERE l.sales_order_id = ?1 GROUP BY i.id ORDER BY i.id DESC",
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;
    Ok(SalesOrderDetail {
        summary,
        tax_rate_id,
        sales_rep_id,
        ship_method_id,
        customer_message_id,
        header: SalesHeader { bill_to, ship_to, po_number, fob },
        lines,
        invoices,
    })
}

/// The order's rep, ship via, message and addresses, as a form prints them.
pub async fn details_view(pool: &SqlitePool, id: i64) -> Result<crate::sales_lists::SalesDetailsView, String> {
    type Row = (Option<i64>, Option<String>, Option<String>, Option<i64>, Option<String>, Option<i64>, Option<String>);
    let row: Option<Row> = sqlx::query_as(
        "SELECT o.sales_rep_id, r.initials, r.name, o.ship_method_id, s.name, o.customer_message_id, m.name
         FROM sales_orders o LEFT JOIN sales_reps r ON r.id = o.sales_rep_id LEFT JOIN ship_methods s ON s.id = o.ship_method_id
         LEFT JOIN profile_entries m ON m.id = o.customer_message_id WHERE o.id = ?1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let (sales_rep_id, sales_rep_initials, sales_rep_name, ship_method_id, ship_method_name, customer_message_id, customer_message) =
        row.ok_or("SO_NOT_FOUND: that sales order does not exist")?;
    Ok(crate::sales_lists::SalesDetailsView {
        sales_rep_id,
        sales_rep_initials,
        sales_rep_name,
        ship_method_id,
        ship_method_name,
        customer_message_id,
        customer_message,
        header: crate::sales_header::read(pool, "sales_orders", id).await?,
        ..Default::default()
    })
}

async fn order_lines(conn: &mut sqlx::SqliteConnection, id: i64) -> Result<Vec<SalesOrderLine>, String> {
    sqlx::query_as(
        "SELECT l.id, l.line_number, l.product_service_id, p.name AS item_name, l.description, l.quantity_e4, l.rate_cents, l.amount_cents,
                l.income_account_id, l.class_id, l.is_taxable = 1 AS is_taxable, l.invoiced_qty_e4, l.closed_qty_e4,
                l.quantity_e4 - l.invoiced_qty_e4 - l.closed_qty_e4 AS left_qty_e4
         FROM sales_order_lines l LEFT JOIN products_services p ON p.id = l.product_service_id
         WHERE l.sales_order_id = ?1 ORDER BY l.line_number",
    )
    .bind(id)
    .fetch_all(conn)
    .await
    .map_err(db_err)
}

/// Saves a new sales order. Nothing posts.
pub async fn create(pool: &SqlitePool, input: &SalesOrderInput) -> Result<SalesOrderSummary, String> {
    run(pool, async |tx| {
        let customer_id = find_or_create_named(tx, Party::Customer, &input.customer_name).await?;
        let number = crate::doc_numbers::take_number(tx, crate::doc_numbers::DocKind::SalesOrder, input.order_number).await?;
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO sales_orders (order_number, customer_id, order_date)
             VALUES (?3, ?1, ?2) RETURNING id",
        )
        .bind(customer_id)
        .bind(input.order_date)
        .bind(number)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)?;
        write(tx, id, input).await?;
        Ok(id)
    })
    .await
}

/// A sales order made from an estimate, and what of the estimate it couldn't hold.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderFromEstimate {
    pub order: SalesOrderSummary,
    /// Lines left on the estimate: a discount, percent charges, subtotals and lines priced at 0.00
    /// go on the invoice instead.
    pub left_out: Vec<String>,
}

/// Makes a sales order from an estimate that is pending or accepted and hasn't become an invoice
/// or another order: its customer, item lines, tax rate, rep, ship via, message and addresses.
/// The order keeps the estimate it came from (`estimate_id`), and the estimate can then only be
/// invoiced through the order.
pub async fn create_from_estimate(pool: &SqlitePool, estimate_id: i64, order_date: NaiveDate) -> Result<OrderFromEstimate, String> {
    if !crate::preferences::flag(pool, crate::sales_prefs::SALES_ORDERS).await? {
        return Err("SO_OFF: sales orders are turned off (Preferences › Sales & Customers)".into());
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    match write_from_estimate(&mut tx, estimate_id, order_date).await {
        Ok((id, left_out)) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            let mut conn = pool.acquire().await.map_err(db_err)?;
            Ok(OrderFromEstimate { order: summary(&mut conn, id).await?, left_out })
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

async fn write_from_estimate(tx: &mut Transaction<'_, Sqlite>, estimate_id: i64, order_date: NaiveDate) -> Result<(i64, Vec<String>), String> {
    type Head = (String, String, Option<String>, Option<i64>, i64);
    let head: Option<Head> = sqlx::query_as(
        "SELECT c.name, e.status, e.memo, e.tax_rate_id, e.discount_cents FROM estimates e JOIN customers c ON c.id = e.customer_id WHERE e.id = ?1",
    )
    .bind(estimate_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(db_err)?;
    let (customer_name, status, memo, tax_rate_id, discount_cents) = head.ok_or("EST_NOT_FOUND: that estimate does not exist")?;
    match status.as_str() {
        "CONVERTED" => return Err("EST_CONVERTED: this estimate has already become an invoice".into()),
        "REJECTED" => return Err("EST_REJECTED: a rejected estimate can't become an order; mark it pending or accepted first".into()),
        _ => {}
    }
    let ordered: Option<i64> =
        sqlx::query_scalar("SELECT order_number FROM sales_orders WHERE estimate_id = ?1").bind(estimate_id).fetch_optional(&mut **tx).await.map_err(db_err)?;
    if let Some(number) = ordered {
        return Err(format!("EST_ORDERED: this estimate is already sales order #{number}"));
    }
    type Line = (Option<i64>, String, i64, i64, i64, Option<i64>, bool, Option<i64>, i64);
    let rows: Vec<Line> = sqlx::query_as(
        "SELECT product_service_id, description, quantity_e4, rate_cents, income_account_id, class_id, is_taxable, percent_ppm, line_number
         FROM estimate_lines WHERE estimate_id = ?1 ORDER BY line_number",
    )
    .bind(estimate_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    let mut left_out = Vec::new();
    let mut lines = Vec::new();
    for (product_service_id, description, quantity_e4, rate_cents, income_account_id, class_id, is_taxable, percent, number) in rows {
        if percent.is_some() || rate_cents <= 0 {
            left_out.push(format!("line {number} ({})", if percent.is_some() { "a percent charge" } else { "priced at 0.00" }));
            continue;
        }
        lines.push(SalesOrderLineInput {
            id: None,
            line: SalesLineInput {
                product_service_id,
                description,
                quantity_e4,
                rate_cents: Cents(rate_cents),
                income_account_id,
                class_id,
                is_taxable,
                service_date: None,
            },
        });
    }
    let form = crate::item_types::form_lines_of(&mut **tx, crate::sales_lists::SalesDoc::Estimate, estimate_id).await?;
    if !form.is_empty() {
        left_out.push(format!("{} subtotal or other form line{}", form.len(), if form.len() == 1 { "" } else { "s" }));
    }
    if discount_cents > 0 {
        left_out.push(format!("the {} discount", format_cents(discount_cents)));
    }
    if lines.is_empty() {
        return Err("SO_NO_LINES: the estimate has no item lines priced above zero to order".into());
    }
    let details = crate::sales_lists::details_in_tx(tx, crate::sales_lists::SalesDoc::Estimate, estimate_id).await?;
    let input = SalesOrderInput {
        order_number: None,
        customer_name,
        order_date,
        expected_date: None,
        memo,
        lines,
        tax_rate_id,
        sales_rep_id: details.sales_rep_id,
        ship_method_id: details.ship_method_id,
        customer_message_id: details.customer_message_id,
        header: details.header,
    };
    let customer_id = find_or_create_named(tx, Party::Customer, &input.customer_name).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO sales_orders (order_number, customer_id, order_date, estimate_id)
         VALUES (?4, ?1, ?2, ?3) RETURNING id",
    )
    .bind(customer_id)
    .bind(order_date)
    .bind(estimate_id)
    .bind(crate::doc_numbers::next_number(tx, crate::doc_numbers::DocKind::SalesOrder).await?)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    write(tx, id, &input).await?;
    Ok((id, left_out))
}

/// Changes a sales order. Lines already invoiced stay (with at least what was invoiced); an order
/// closed by hand has to be reopened first.
pub async fn update(pool: &SqlitePool, id: i64, input: &SalesOrderInput) -> Result<SalesOrderSummary, String> {
    run(pool, async |tx| {
        let closed: Option<bool> =
            sqlx::query_scalar("SELECT is_closed = 1 FROM sales_orders WHERE id = ?1").bind(id).fetch_optional(&mut **tx).await.map_err(db_err)?;
        match closed {
            None => return Err("SO_NOT_FOUND: that sales order does not exist".into()),
            Some(true) => return Err("SO_CLOSED: reopen the sales order to change it".into()),
            Some(false) => {}
        }
        write(tx, id, input).await?;
        Ok(id)
    })
    .await
}

async fn run(pool: &SqlitePool, work: impl for<'t> AsyncFnOnce(&mut Transaction<'t, Sqlite>) -> Result<i64, String>) -> Result<SalesOrderSummary, String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    match work(&mut tx).await {
        Ok(id) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            let mut conn = pool.acquire().await.map_err(db_err)?;
            summary(&mut conn, id).await
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// Writes the header and lines of order `id` (its row exists).
async fn write(tx: &mut Transaction<'_, Sqlite>, id: i64, input: &SalesOrderInput) -> Result<(), String> {
    if input.lines.is_empty() {
        return Err("SO_NO_LINES: a sales order needs at least one line".into());
    }
    if input.expected_date.is_some_and(|d| d < input.order_date) {
        return Err("SO_EXPECTED_BEFORE_DATE: the expected date can't be before the order date".into());
    }
    let customer_id = find_or_create_named(tx, Party::Customer, &input.customer_name).await?;
    let current: Vec<(i64, i64, i64, i64)> =
        sqlx::query_as("SELECT id, invoiced_qty_e4, closed_qty_e4, line_number FROM sales_order_lines WHERE sales_order_id = ?1")
            .bind(id)
            .fetch_all(&mut **tx)
            .await
            .map_err(db_err)?;
    let invoiced = current.iter().any(|(_, invoiced, _, _)| *invoiced > 0);
    let old_customer: i64 = sqlx::query_scalar("SELECT customer_id FROM sales_orders WHERE id = ?1").bind(id).fetch_one(&mut **tx).await.map_err(db_err)?;
    if invoiced && old_customer != customer_id {
        return Err("SO_INVOICED_CUSTOMER: part of this order is invoiced, so its customer can't change".into());
    }
    let mut total: i64 = 0;
    let mut amounts = Vec::with_capacity(input.lines.len());
    for (idx, l) in input.lines.iter().enumerate() {
        let line = &l.line;
        // Priced above zero, as on invoices: every order line has to be invoiceable.
        let amount = validate_line(line.quantity_e4, line.rate_cents.get(), idx + 1, "SO")?;
        total = total
            .checked_add(amount)
            .filter(|t| *t <= MAX_LINE_CENTS)
            .ok_or_else(|| format!("SO_AMOUNT_TOO_LARGE: the order total exceeds {}", format_cents(MAX_LINE_CENTS)))?;
        amounts.push(amount);
        let classification: Option<String> =
            sqlx::query_scalar("SELECT t.classification FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1")
                .bind(line.income_account_id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(db_err)?;
        if classification.as_deref() != Some("INCOME") {
            return Err(format!("SO_NOT_INCOME_ACCOUNT: line {} must use an Income account", idx + 1));
        }
        if let Some(class) = line.class_id {
            crate::classes::check_usable(tx, class, &format!("SO: line {}", idx + 1)).await?;
        }
    }
    if let Some(rate) = input.tax_rate_id {
        crate::sales_tax::document_tax(tx, Some(rate), 0, "SO").await?;
    }
    sqlx::query(
        "UPDATE sales_orders SET customer_id = ?1, order_date = ?2, expected_date = ?3, memo = ?4, tax_rate_id = ?5,
                sales_rep_id = ?6, ship_method_id = ?7, customer_message_id = ?8 WHERE id = ?9",
    )
    .bind(customer_id)
    .bind(input.order_date)
    .bind(input.expected_date)
    .bind(input.memo.as_deref().map(str::trim).filter(|m| !m.is_empty()))
    .bind(input.tax_rate_id)
    .bind(input.sales_rep_id)
    .bind(input.ship_method_id)
    .bind(input.customer_message_id)
    .bind(id)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    crate::sales_header::write(tx, "sales_orders", id, &input.header).await?;

    // Lines: kept ones are updated, dropped ones deleted (a trigger refuses an invoiced one), new
    // ones added. Line numbers move out of the way first so the new order can take them.
    let kept: Vec<i64> = input.lines.iter().filter_map(|l| l.id).collect();
    if kept.iter().collect::<std::collections::HashSet<_>>().len() != kept.len() {
        return Err("SO_LINE_TWICE: an order line appears twice".into());
    }
    for (line_id, invoiced, _, _) in &current {
        if !kept.contains(line_id) {
            if *invoiced > 0 {
                return Err("SO_LINE_INVOICED: a line that has been invoiced can't be removed".into());
            }
            sqlx::query("DELETE FROM sales_order_lines WHERE id = ?1").bind(line_id).execute(&mut **tx).await.map_err(db_err)?;
        }
    }
    sqlx::query("UPDATE sales_order_lines SET line_number = line_number + 1000000 WHERE sales_order_id = ?1")
        .bind(id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    for (idx, (l, amount)) in input.lines.iter().zip(amounts).enumerate() {
        let line = &l.line;
        let number = i64::try_from(idx + 1).map_err(|_| "SO_TOO_MANY_LINES: too many lines".to_string())?;
        match l.id {
            Some(line_id) => {
                let Some((_, invoiced, closed, _)) = current.iter().find(|(c, ..)| *c == line_id) else {
                    return Err(format!("SO_LINE_NOT_FOUND: line {} is not on this order", idx + 1));
                };
                if line.quantity_e4 < invoiced + closed {
                    return Err(format!(
                        "SO_BELOW_INVOICED: line {} can't be less than the {} already invoiced",
                        idx + 1,
                        crate::forms::format_quantity(*invoiced)
                    ));
                }
                let product: Option<i64> = sqlx::query_scalar("SELECT product_service_id FROM sales_order_lines WHERE id = ?1")
                    .bind(line_id)
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(db_err)?;
                if *invoiced > 0 && product != line.product_service_id {
                    return Err(format!("SO_INVOICED_ITEM: line {} has been invoiced, so its item can't change", idx + 1));
                }
                sqlx::query(
                    "UPDATE sales_order_lines SET line_number = ?1, product_service_id = ?2, description = ?3, quantity_e4 = ?4, rate_cents = ?5,
                            amount_cents = ?6, income_account_id = ?7, class_id = ?8, is_taxable = ?9 WHERE id = ?10",
                )
                .bind(number)
                .bind(line.product_service_id)
                .bind(line.description.trim())
                .bind(line.quantity_e4)
                .bind(line.rate_cents)
                .bind(amount)
                .bind(line.income_account_id)
                .bind(line.class_id)
                .bind(line.is_taxable && input.tax_rate_id.is_some())
                .bind(line_id)
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
            }
            None => {
                sqlx::query(
                    "INSERT INTO sales_order_lines (sales_order_id, line_number, product_service_id, description, quantity_e4, rate_cents, amount_cents,
                                                    income_account_id, class_id, is_taxable)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                )
                .bind(id)
                .bind(number)
                .bind(line.product_service_id)
                .bind(line.description.trim())
                .bind(line.quantity_e4)
                .bind(line.rate_cents)
                .bind(amount)
                .bind(line.income_account_id)
                .bind(line.class_id)
                .bind(line.is_taxable && input.tax_rate_id.is_some())
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
            }
        }
    }
    Ok(())
}

/// Closes an order by hand: what is left on its lines won't be invoiced.
pub async fn close(pool: &SqlitePool, id: i64) -> Result<SalesOrderSummary, String> {
    run(pool, async |tx| {
        let done = sqlx::query("UPDATE sales_orders SET is_closed = 1 WHERE id = ?1 AND is_closed = 0").bind(id).execute(&mut **tx).await.map_err(db_err)?;
        if done.rows_affected() != 1 {
            return Err("SO_CLOSED: that sales order is already closed or does not exist".into());
        }
        sqlx::query("UPDATE sales_order_lines SET closed_qty_e4 = quantity_e4 - invoiced_qty_e4 WHERE sales_order_id = ?1")
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        Ok(id)
    })
    .await
}

/// Reopens an order closed by hand.
pub async fn reopen(pool: &SqlitePool, id: i64) -> Result<SalesOrderSummary, String> {
    run(pool, async |tx| {
        let done = sqlx::query("UPDATE sales_orders SET is_closed = 0 WHERE id = ?1 AND is_closed = 1").bind(id).execute(&mut **tx).await.map_err(db_err)?;
        if done.rows_affected() != 1 {
            return Err("SO_NOT_CLOSED: that sales order isn't closed".into());
        }
        sqlx::query("UPDATE sales_order_lines SET closed_qty_e4 = 0 WHERE sales_order_id = ?1").bind(id).execute(&mut **tx).await.map_err(db_err)?;
        Ok(id)
    })
    .await
}

/// Deletes an order that nothing has been invoiced from (it never posted); others can only be closed.
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let linked: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sales_order_invoiced k JOIN sales_order_lines l ON l.id = k.sales_order_line_id WHERE l.sales_order_id = ?1)",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    if linked {
        return Err(rollback(tx, "SO_HAS_INVOICES: invoices were made from this order; close it instead".into()).await);
    }
    let done = sqlx::query("DELETE FROM sales_orders WHERE id = ?1").bind(id).execute(&mut *tx).await.map_err(db_err)?;
    if done.rows_affected() != 1 {
        return Err(rollback(tx, "SO_NOT_FOUND: that sales order does not exist".into()).await);
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)
}

/// Which order line an invoice line takes from: `invoice_line` is its index in the invoice's lines.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Take {
    pub order_line_id: i64,
    pub invoice_line: usize,
}

/// Posts an invoice made from sales order `order_id` and takes its quantities off the order, in
/// one write transaction. The invoice may carry other lines too (freight, a discount); each line in
/// `takes` must be for the order line's item and no more than is left on it.
pub async fn invoice_from_order(pool: &SqlitePool, order_id: i64, input: &InvoiceInput, takes: &[Take]) -> Result<crate::invoices::InvoiceSummary, String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    match write_invoice(&mut tx, order_id, input, takes).await {
        Ok(invoice_id) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            crate::invoices::get_summary(pool, invoice_id).await
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

async fn write_invoice(tx: &mut Transaction<'_, Sqlite>, order_id: i64, input: &InvoiceInput, takes: &[Take]) -> Result<i64, String> {
    if takes.is_empty() {
        return Err("SO_NOTHING_TAKEN: choose at least one order line to invoice".into());
    }
    if input.currency.is_some() {
        return Err("SO_CURRENCY: a sales order is invoiced in the home currency".into());
    }
    let order: Option<(i64, bool)> =
        sqlx::query_as("SELECT customer_id, is_closed = 1 FROM sales_orders WHERE id = ?1").bind(order_id).fetch_optional(&mut **tx).await.map_err(db_err)?;
    let (order_customer, closed) = order.ok_or("SO_NOT_FOUND: that sales order does not exist")?;
    if closed {
        return Err("SO_CLOSED: that sales order is closed".into());
    }
    let customer_id = find_or_create_named(tx, Party::Customer, &input.customer_name).await?;
    if !customer_of(tx, customer_id, order_customer).await? {
        return Err("SO_OTHER_CUSTOMER: invoice the order's customer, or one of its jobs".into());
    }
    // How much each order line gives, adding up invoice lines that take from the same one.
    let mut taken: BTreeMap<i64, i64> = BTreeMap::new();
    let mut used = vec![false; input.lines.len()];
    for take in takes {
        let line = input.lines.get(take.invoice_line).ok_or_else(|| format!("SO_BAD_TAKE: the invoice has no line {}", take.invoice_line + 1))?;
        if std::mem::replace(&mut used[take.invoice_line], true) {
            return Err(format!("SO_BAD_TAKE: invoice line {} takes from the order twice", take.invoice_line + 1));
        }
        let row: Option<(i64, Option<i64>)> = sqlx::query_as("SELECT sales_order_id, product_service_id FROM sales_order_lines WHERE id = ?1")
            .bind(take.order_line_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
        match row {
            Some((order, product)) if order == order_id => {
                if product != line.product_service_id {
                    return Err(format!("SO_ITEM_MISMATCH: invoice line {} is for a different item than its order line", take.invoice_line + 1));
                }
            }
            _ => return Err(format!("SO_BAD_TAKE: invoice line {} names a line that isn't on this order", take.invoice_line + 1)),
        }
        let sum = taken.entry(take.order_line_id).or_insert(0);
        *sum = sum.checked_add(line.quantity_e4).ok_or("SO_OVER_INVOICED: quantity out of range")?;
    }
    for (line_id, qty) in &taken {
        let (number, left): (i64, i64) =
            sqlx::query_as("SELECT line_number, quantity_e4 - invoiced_qty_e4 - closed_qty_e4 FROM sales_order_lines WHERE id = ?1")
                .bind(line_id)
                .fetch_one(&mut **tx)
                .await
                .map_err(db_err)?;
        if *qty > left {
            return Err(format!(
                "SO_OVER_INVOICED: order line {number} has {} left to invoice; change the order to ship more",
                crate::forms::format_quantity(left)
            ));
        }
    }
    // The invoice carries its own details (the form fills them from the order).
    let invoice_id = crate::invoices::write_invoice_carrying(tx, input, &crate::sales_lists::SalesDetails::default()).await?;
    for (line_id, qty) in taken {
        sqlx::query("INSERT INTO sales_order_invoiced (sales_order_line_id, invoice_id, quantity_e4) VALUES (?1, ?2, ?3)")
            .bind(line_id)
            .bind(invoice_id)
            .bind(qty)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
    }
    Ok(invoice_id)
}

/// `customer` is `order_customer` or one of its jobs.
async fn customer_of(tx: &mut Transaction<'_, Sqlite>, customer: i64, order_customer: i64) -> Result<bool, String> {
    sqlx::query_scalar(
        "WITH RECURSIVE up(id) AS (SELECT ?1 UNION SELECT c.parent_id FROM customers c JOIN up ON c.id = up.id WHERE c.parent_id IS NOT NULL)
         SELECT EXISTS (SELECT 1 FROM up WHERE id = ?2)",
    )
    .bind(customer)
    .bind(order_customer)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)
}

/// An edited invoice (`edits::replace_invoice`) replaces `old` with `new`: the order lines the old
/// one took from are taken by the new one, with the same quantities. Runs after the old invoice was
/// voided, which gave the quantities back.
///
/// The new invoice keeps a link only as far as it still sells that item to the order's customer
/// (or one of its jobs): an edit that moves the invoice to someone else, drops the item or ships
/// fewer leaves the rest open on the order again.
pub(crate) async fn follow_replaced_invoice(tx: &mut Transaction<'_, Sqlite>, old: i64, new: i64) -> Result<(), String> {
    let links: Vec<(i64, i64, Option<i64>, i64)> = sqlx::query_as(
        "SELECT k.sales_order_line_id, k.quantity_e4, l.product_service_id, o.customer_id
         FROM sales_order_invoiced k JOIN sales_order_lines l ON l.id = k.sales_order_line_id JOIN sales_orders o ON o.id = l.sales_order_id
         WHERE k.invoice_id = ?1 AND k.is_reversed = 1 ORDER BY k.id",
    )
    .bind(old)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?;
    if links.is_empty() {
        return Ok(());
    }
    let customer: Option<i64> = sqlx::query_scalar("SELECT customer_id FROM invoices WHERE id = ?1").bind(new).fetch_one(&mut **tx).await.map_err(db_err)?;
    // What the edited invoice sells of each item, handed out to the links in order.
    let sold: Vec<(Option<i64>, i64)> =
        sqlx::query_as("SELECT product_service_id, SUM(quantity_e4) FROM invoice_items WHERE invoice_id = ?1 GROUP BY product_service_id")
            .bind(new)
            .fetch_all(&mut **tx)
            .await
            .map_err(db_err)?;
    let mut left: HashMap<Option<i64>, i64> = sold.into_iter().collect();
    for (line_id, qty, product, order_customer) in links {
        let ours = match customer {
            Some(c) => customer_of(tx, c, order_customer).await?,
            None => false,
        };
        let available = left.get(&product).copied().unwrap_or(0);
        let qty = if ours { qty.min(available) } else { 0 };
        if qty <= 0 {
            continue;
        }
        left.insert(product, available - qty);
        sqlx::query("INSERT INTO sales_order_invoiced (sales_order_line_id, invoice_id, quantity_e4) VALUES (?1, ?2, ?3)")
            .bind(line_id)
            .bind(new)
            .bind(qty)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Open lines, backorders and the reports
// ------------------------------------------------------------------------------------------------

/// An open order line, with what stock can cover it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenLine {
    pub order_id: i64,
    pub order_number: i64,
    pub order_date: NaiveDate,
    pub expected_date: Option<NaiveDate>,
    pub customer_id: i64,
    pub customer_name: String,
    pub line_id: i64,
    pub product_service_id: Option<i64>,
    pub item_name: Option<String>,
    pub description: String,
    pub quantity_e4: i64,
    pub invoiced_qty_e4: i64,
    pub left_qty_e4: i64,
    pub rate_cents: i64,
    /// Left × rate.
    pub open_cents: i64,
    /// The part of what is left that stock on hand can't cover, oldest orders served first.
    pub backordered_qty_e4: i64,
}

/// Stock for an item with open orders.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemStock {
    pub product_service_id: i64,
    pub item_name: String,
    /// `None` for an item that isn't tracked in inventory.
    pub on_hand_e4: Option<i64>,
    pub on_sales_order_e4: i64,
    /// On hand less on sales order.
    pub available_e4: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenSalesOrders {
    pub lines: Vec<OpenLine>,
    pub items: Vec<ItemStock>,
    pub total_open_cents: i64,
}

/// Every open order line (Open Sales Orders by Customer and by Item read this), oldest order first.
pub async fn open_lines(pool: &SqlitePool) -> Result<OpenSalesOrders, String> {
    type Row = (i64, i64, NaiveDate, Option<NaiveDate>, i64, String, i64, Option<i64>, Option<String>, String, i64, i64, i64, i64, Option<i64>);
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT o.id, o.order_number, o.order_date, o.expected_date, o.customer_id, c.name, l.id, l.product_service_id, p.name, l.description,
                l.quantity_e4, l.invoiced_qty_e4, l.quantity_e4 - l.invoiced_qty_e4 - l.closed_qty_e4, l.rate_cents, i.qty_on_hand_e4
         FROM sales_order_lines l JOIN sales_orders o ON o.id = l.sales_order_id JOIN customers c ON c.id = o.customer_id
         LEFT JOIN products_services p ON p.id = l.product_service_id LEFT JOIN inventory_items i ON i.product_id = l.product_service_id
         WHERE o.is_closed = 0 AND l.quantity_e4 > l.invoiced_qty_e4 + l.closed_qty_e4
         ORDER BY o.order_date, o.order_number, l.line_number",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let overflow = || "SO_OVERFLOW: amount out of range".to_string();
    // What on-hand stock is still free to cover the next order, per tracked item.
    let mut free: HashMap<i64, i64> = HashMap::new();
    let mut items: BTreeMap<i64, ItemStock> = BTreeMap::new();
    let mut lines = Vec::with_capacity(rows.len());
    let mut total_open_cents: i64 = 0;
    for (
        order_id,
        order_number,
        order_date,
        expected_date,
        customer_id,
        customer_name,
        line_id,
        product,
        item_name,
        description,
        quantity_e4,
        invoiced,
        left,
        rate_cents,
        on_hand,
    ) in rows
    {
        let open_cents = crate::ledger::quantity_amount(left, rate_cents).ok_or_else(overflow)?;
        total_open_cents = total_open_cents.checked_add(open_cents).ok_or_else(overflow)?;
        let mut backordered = 0;
        if let Some(product) = product {
            let stock = items.entry(product).or_insert_with(|| ItemStock {
                product_service_id: product,
                item_name: item_name.clone().unwrap_or_default(),
                on_hand_e4: on_hand,
                on_sales_order_e4: 0,
                available_e4: on_hand,
            });
            stock.on_sales_order_e4 = stock.on_sales_order_e4.checked_add(left).ok_or_else(overflow)?;
            if let Some(on_hand) = on_hand {
                stock.available_e4 = Some(on_hand.checked_sub(stock.on_sales_order_e4).ok_or_else(overflow)?);
                let free = free.entry(product).or_insert_with(|| on_hand.max(0));
                let covered = left.min(*free);
                *free -= covered;
                backordered = left - covered;
            }
        }
        lines.push(OpenLine {
            order_id,
            order_number,
            order_date,
            expected_date,
            customer_id,
            customer_name,
            line_id,
            product_service_id: product,
            item_name,
            description,
            quantity_e4,
            invoiced_qty_e4: invoiced,
            left_qty_e4: left,
            rate_cents,
            open_cents,
            backordered_qty_e4: backordered,
        });
    }
    Ok(OpenSalesOrders { lines, items: items.into_values().collect(), total_open_cents })
}

/// Pending Sales: what customers have ordered and not been invoiced for, by customer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingSalesRow {
    pub customer_id: i64,
    pub customer_name: String,
    pub orders: i64,
    pub open_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingSales {
    pub rows: Vec<PendingSalesRow>,
    pub total_cents: i64,
}

pub async fn pending_sales(pool: &SqlitePool) -> Result<PendingSales, String> {
    let open = open_lines(pool).await?;
    let mut by_customer: BTreeMap<(String, i64), (Vec<i64>, i64)> = BTreeMap::new();
    for line in &open.lines {
        let entry = by_customer.entry((line.customer_name.to_lowercase(), line.customer_id)).or_insert_with(|| (Vec::new(), 0));
        if !entry.0.contains(&line.order_id) {
            entry.0.push(line.order_id);
        }
        entry.1 = entry.1.checked_add(line.open_cents).ok_or("SO_OVERFLOW: amount out of range")?;
    }
    let names: HashMap<i64, String> = open.lines.iter().map(|l| (l.customer_id, l.customer_name.clone())).collect();
    let rows = by_customer
        .into_iter()
        .map(|((_, customer_id), (orders, open_cents))| PendingSalesRow {
            customer_id,
            customer_name: names.get(&customer_id).cloned().unwrap_or_default(),
            orders: i64::try_from(orders.len()).unwrap_or(i64::MAX),
            open_cents,
        })
        .collect();
    Ok(PendingSales { rows, total_cents: open.total_open_cents })
}

/// On sales order for each item (Stock Status and the information panel).
pub async fn on_sales_order(conn: &mut sqlx::SqliteConnection) -> Result<HashMap<i64, i64>, String> {
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT l.product_service_id, SUM(l.quantity_e4 - l.invoiced_qty_e4 - l.closed_qty_e4)
         FROM sales_order_lines l JOIN sales_orders o ON o.id = l.sales_order_id
         WHERE o.is_closed = 0 AND l.product_service_id IS NOT NULL GROUP BY l.product_service_id",
    )
    .fetch_all(conn)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().collect())
}

#[cfg(test)]
mod tests;
