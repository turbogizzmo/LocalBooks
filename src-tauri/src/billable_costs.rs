//! Billable expenses: costs on checks, expenses and bills marked billable to a customer or job,
//! invoiced to them from Sales › Bill Costs.
//!
//! A cost is billable when its line has a customer and is marked billable (new lines follow
//! Preferences › Time & Expenses › "Mark all expenses as billable"). Billing writes one invoice:
//!
//! - one line per cost, for its amount, crediting the cost's own expense account (the
//!   reimbursement reduces the expense) or, when Preferences say to count reimbursed expenses as
//!   income, the income account chosen there;
//! - one Markup line when a markup percent is given (the default comes from Preferences), crediting
//!   the markup income account. Each cost's markup is round_half_away(amount × percent), and the
//!   line is their sum.
//!
//! Billable **time** (timesheet activities marked billable, with a customer and an hourly rate)
//! goes on the same invoice: one line per activity for its service item, at the hours and rate, and
//! `billed_time` records it. When reimbursed expenses count as income, each cost's expense account
//! may name its own income account (`chart_of_accounts.reimbursement_income_account_id`); the rest
//! use the one in Preferences.
//!
//! `billed_costs` records which invoice billed each cost; rows are only ever inserted. A cost
//! whose invoice is voided can be billed again, and an edited invoice or expense carries its
//! links to the replacement. Home-currency costs only.

use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use chrono::NaiveDate;

use crate::invoices::{InvoiceInput, InvoiceLineInput, InvoiceSummary};
use crate::ledger::{db_err, mul_div_round, rollback, MAX_LINE_CENTS, QUANTITY_SCALE};
use crate::preferences::tax_time;

/// A billable cost not yet on a live invoice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UnbilledCost {
    /// `EXPENSE` (a check or expense line) or `BILL` (a bill line).
    pub source: String,
    pub line_id: i64,
    pub date: String,
    pub vendor_name: Option<String>,
    pub account_id: i64,
    pub account_name: String,
    pub memo: Option<String>,
    pub amount_cents: i64,
    pub customer_id: i64,
    pub customer_name: String,
}

/// Billable lines of live, home-currency expenses and bills, not on a live invoice.
const UNBILLED: &str = "
    SELECT 'EXPENSE' AS source, l.id AS line_id, x.expense_date AS date, v.name AS vendor_name, l.account_id, a.full_name AS account_name,
           NULLIF(l.memo, '') AS memo, l.amount_cents, l.customer_id, c.name AS customer_name
    FROM expense_lines l JOIN expenses x ON x.id = l.expense_id JOIN journal_entries e ON e.id = x.journal_entry_id
    JOIN v_chart_of_accounts a ON a.id = l.account_id JOIN customers c ON c.id = l.customer_id LEFT JOIN vendors v ON v.id = x.vendor_id
    WHERE l.is_billable = 1 AND e.is_voided = 0 AND x.is_credit = 0 AND x.currency_code IS NULL
      AND NOT EXISTS (SELECT 1 FROM billed_costs b JOIN invoices i ON i.id = b.invoice_id WHERE b.expense_line_id = l.id AND i.is_voided = 0)
    UNION ALL
    SELECT 'BILL', l.id, b.bill_date, v.name, l.account_id, a.full_name, NULLIF(l.memo, ''), l.amount_cents, l.customer_id, c.name
    FROM bill_lines l JOIN bills b ON b.id = l.bill_id JOIN journal_entries e ON e.id = b.journal_entry_id
    JOIN v_chart_of_accounts a ON a.id = l.account_id JOIN customers c ON c.id = l.customer_id JOIN vendors v ON v.id = b.vendor_id
    WHERE l.is_billable = 1 AND e.is_voided = 0
      AND NOT EXISTS (SELECT 1 FROM billed_costs bc JOIN invoices i ON i.id = bc.invoice_id WHERE bc.bill_line_id = l.id AND i.is_voided = 0)";

/// Billable time not yet on a live invoice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UnbilledTime {
    pub id: i64,
    pub date: String,
    pub employee_name: String,
    pub product_service_id: Option<i64>,
    pub service_name: Option<String>,
    pub description: Option<String>,
    pub minutes: i64,
    pub hourly_rate_cents: i64,
    /// round_half_away(hours × rate).
    pub amount_cents: i64,
    pub customer_id: i64,
    pub customer_name: String,
}

const UNBILLED_TIME: &str = "
    SELECT t.id, t.activity_date AS date, m.name AS employee_name, t.product_service_id, p.name AS service_name, NULLIF(t.description, '') AS description,
           t.minutes, t.hourly_rate_cents, 0 AS amount_cents, t.customer_id, c.name AS customer_name
    FROM time_activities t JOIN employees m ON m.id = t.employee_id JOIN customers c ON c.id = t.customer_id
    LEFT JOIN products_services p ON p.id = t.product_service_id
    WHERE t.billable = 1 AND t.hourly_rate_cents IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM billed_time b JOIN invoices i ON i.id = b.invoice_id WHERE b.time_activity_id = t.id AND i.is_voided = 0)";

/// Hours (× 10,000) for `minutes`, rounded half away from zero.
fn hours_e4(minutes: i64) -> Option<i64> {
    mul_div_round(minutes, QUANTITY_SCALE, 60)
}

/// Unbilled billable time, for one customer or job or for all, oldest first.
pub async fn list_unbilled_time(pool: &SqlitePool, customer_id: Option<i64>) -> Result<Vec<UnbilledTime>, String> {
    let mut rows: Vec<UnbilledTime> =
        sqlx::query_as(&format!("SELECT * FROM ({UNBILLED_TIME}) WHERE ?1 IS NULL OR customer_id = ?1 ORDER BY customer_name COLLATE NOCASE, date, id"))
            .bind(customer_id)
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
    for r in &mut rows {
        r.amount_cents = hours_e4(r.minutes).and_then(|h| mul_div_round(h, r.hourly_rate_cents, QUANTITY_SCALE)).ok_or("TIME_AMOUNT: the amount is out of range")?;
    }
    Ok(rows)
}

/// Sets the income account reimbursements of `expense_account_id` go to (None: the one in
/// Preferences › Time & Expenses).
pub async fn set_reimbursement_account(pool: &SqlitePool, expense_account_id: i64, income_account_id: Option<i64>) -> Result<(), String> {
    let class: Option<String> = sqlx::query_scalar("SELECT classification FROM v_chart_of_accounts WHERE id = ?1").bind(expense_account_id).fetch_optional(pool).await.map_err(db_err)?;
    match class.as_deref() {
        None => return Err("ACCOUNT_NOT_FOUND: that account does not exist".into()),
        Some("EXPENSE") => {}
        Some(_) => return Err("COST_REIMBURSE_NOT_EXPENSE: only an expense account's reimbursements can go to an income account".into()),
    }
    if let Some(income) = income_account_id {
        let class: Option<String> = sqlx::query_scalar("SELECT classification FROM v_chart_of_accounts WHERE id = ?1").bind(income).fetch_optional(pool).await.map_err(db_err)?;
        if class.as_deref() != Some("INCOME") {
            return Err("COST_REIMBURSE_NOT_INCOME: choose an income account for the reimbursements".into());
        }
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result = sqlx::query("UPDATE chart_of_accounts SET reimbursement_income_account_id = ?1 WHERE id = ?2")
        .bind(income_account_id)
        .bind(expense_account_id)
        .execute(&mut *tx)
        .await
        .map_err(db_err);
    match result {
        Ok(_) => crate::ledger::commit_write(tx).await.map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// Unbilled costs, for one customer or job or for all, oldest first.
pub async fn list_unbilled(pool: &SqlitePool, customer_id: Option<i64>) -> Result<Vec<UnbilledCost>, String> {
    sqlx::query_as::<_, UnbilledCost>(&format!("SELECT * FROM ({UNBILLED}) WHERE ?1 IS NULL OR customer_id = ?1 ORDER BY customer_name COLLATE NOCASE, date, source, line_id"))
        .bind(customer_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostRef {
    pub source: String,
    pub line_id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BillCostsInput {
    pub customer_id: i64,
    #[serde(default)]
    pub costs: Vec<CostRef>,
    /// Billable time activities to bill too (ids).
    #[serde(default)]
    pub time: Vec<i64>,
    pub issue_date: NaiveDate,
    pub due_date: NaiveDate,
    /// Percent, such as `15` or `12.5`; left out, the default in Preferences › Time & Expenses.
    #[serde(default)]
    pub markup_percent: Option<String>,
    #[serde(default)]
    pub tax_rate_id: Option<i64>,
}

/// The billing defaults from Preferences › Time & Expenses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Defaults {
    pub markup_milli_percent: i64,
    pub markup_account_id: Option<i64>,
    pub as_income: bool,
    pub income_account_id: Option<i64>,
}

pub async fn defaults(pool: &SqlitePool) -> Result<Defaults, String> {
    use crate::preferences::{flag, get};
    Ok(Defaults {
        markup_milli_percent: tax_time::parse_markup(&get(pool, tax_time::MARKUP_PERCENT).await?)?,
        markup_account_id: get(pool, tax_time::MARKUP_ACCOUNT).await?.parse().ok(),
        as_income: flag(pool, tax_time::REIMBURSED_AS_INCOME).await?,
        income_account_id: get(pool, tax_time::REIMBURSED_ACCOUNT).await?.parse().ok(),
    })
}

/// round_half_away(amount × milli_percent / 100,000).
pub fn markup_cents(amount_cents: i64, milli_percent: i64) -> Option<i64> {
    mul_div_round(amount_cents, milli_percent, 100_000)
}

/// Bills costs on a new invoice to their customer or job; the invoice posts and the costs are
/// marked billed, together or not at all.
pub async fn bill_costs(pool: &SqlitePool, input: BillCostsInput) -> Result<InvoiceSummary, String> {
    if input.costs.is_empty() && input.time.is_empty() {
        return Err("COST_NOTHING_TO_BILL: choose at least one cost or time entry to bill".into());
    }
    let settings = defaults(pool).await?;
    let markup = match input.markup_percent.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        Some(text) => tax_time::parse_markup(text)?,
        None => settings.markup_milli_percent,
    };
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let invoice_id = match write_billing(&mut tx, &input, &settings, markup).await {
        Ok(id) => id,
        Err(err) => return Err(rollback(tx, err).await),
    };
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    crate::invoices::get_summary(pool, invoice_id).await
}

async fn write_billing(tx: &mut Transaction<'_, Sqlite>, input: &BillCostsInput, settings: &Defaults, markup: i64) -> Result<i64, String> {
    let customer: String = sqlx::query_scalar("SELECT name FROM customers WHERE id = ?1")
        .bind(input.customer_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?
        .ok_or("COST_UNKNOWN_CUSTOMER: that customer or job does not exist")?;

    let mut lines = Vec::with_capacity(input.costs.len() + 1);
    let mut links = Vec::with_capacity(input.costs.len());
    let mut reimbursed = Vec::new();
    let mut markup_total = 0i64;
    for (n, cost) in input.costs.iter().enumerate() {
        let column = match cost.source.as_str() {
            "EXPENSE" => "expense_line_id",
            "BILL" => "bill_line_id",
            other => return Err(format!("COST_BAD_SOURCE: '{other}' is not a cost source")),
        };
        let found: Option<UnbilledCost> = sqlx::query_as(&format!("SELECT * FROM ({UNBILLED}) WHERE source = ?1 AND line_id = ?2"))
            .bind(&cost.source)
            .bind(cost.line_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
        let Some(found) = found else {
            return Err(format!("COST_NOT_BILLABLE: cost {} is not billable, or is already on an invoice", n + 1));
        };
        if found.customer_id != input.customer_id {
            return Err(format!("COST_OTHER_CUSTOMER: the cost dated {} is for a different customer or job", found.date));
        }
        let what = found.memo.clone().unwrap_or_else(|| found.account_name.clone());
        let from = found.vendor_name.as_deref().map(|v| format!(", {v}")).unwrap_or_default();
        // Counted as income: the expense account's own income account, else the one in Preferences.
        let account = if settings.as_income {
            let own: Option<i64> = sqlx::query_scalar("SELECT reimbursement_income_account_id FROM chart_of_accounts WHERE id = ?1")
                .bind(found.account_id)
                .fetch_one(&mut **tx)
                .await
                .map_err(db_err)?;
            own.or(settings.income_account_id)
                .ok_or("COST_NO_INCOME_ACCOUNT: choose the income account for reimbursed expenses in Preferences › Time & Expenses, or on the expense account")?
        } else {
            reimbursed.push(found.account_id);
            found.account_id
        };
        lines.push(InvoiceLineInput {
            description: format!("{what} ({}{from})", found.date),
            quantity_e4: QUANTITY_SCALE,
            rate_cents: found.amount_cents,
            income_account_id: account,
            product_service_id: None,
            class_id: None,
            is_taxable: false, service_date: None,
        });
        let this_markup = markup_cents(found.amount_cents, markup).filter(|m| (0..=MAX_LINE_CENTS).contains(m)).ok_or("COST_MARKUP_TOO_LARGE: the markup is out of range")?;
        markup_total = markup_total.checked_add(this_markup).ok_or("COST_MARKUP_TOO_LARGE: the markup is out of range")?;
        links.push((column, cost.line_id, found.amount_cents, this_markup));
    }
    let mut time_links = Vec::with_capacity(input.time.len());
    for &activity in &input.time {
        let found: Option<UnbilledTime> = sqlx::query_as(&format!("SELECT * FROM ({UNBILLED_TIME}) WHERE id = ?1")).bind(activity).fetch_optional(&mut **tx).await.map_err(db_err)?;
        let Some(found) = found else {
            return Err("TIME_NOT_BILLABLE: a time entry is not billable, or is already on an invoice".into());
        };
        if found.customer_id != input.customer_id {
            return Err(format!("TIME_OTHER_CUSTOMER: the time dated {} is for a different customer or job", found.date));
        }
        let item = found.product_service_id.ok_or_else(|| format!("TIME_NO_ITEM: the time dated {} has no service item; choose one on the timesheet so the invoice knows its income account", found.date))?;
        let (income_account_id, is_taxable): (i64, bool) =
            sqlx::query_as("SELECT income_account_id, is_taxable FROM products_services WHERE id = ?1").bind(item).fetch_one(&mut **tx).await.map_err(db_err)?;
        let hours = hours_e4(found.minutes).ok_or("TIME_AMOUNT: the hours are out of range")?;
        let amount = mul_div_round(hours, found.hourly_rate_cents, QUANTITY_SCALE).ok_or("TIME_AMOUNT: the amount is out of range")?;
        let what = found.description.clone().or(found.service_name.clone()).unwrap_or_else(|| "Time".into());
        lines.push(InvoiceLineInput {
            description: format!("{what} ({}, {})", found.date, found.employee_name),
            quantity_e4: hours,
            rate_cents: found.hourly_rate_cents,
            income_account_id,
            product_service_id: Some(item),
            class_id: None,
            is_taxable,
            service_date: NaiveDate::parse_from_str(&found.date, "%Y-%m-%d").ok(),
        });
        time_links.push((activity, amount));
    }
    if markup_total > 0 {
        let account = settings.markup_account_id.ok_or("COST_NO_MARKUP_ACCOUNT: choose the markup account in Preferences › Time & Expenses")?;
        lines.push(InvoiceLineInput {
            description: format!("Markup ({}%)", crate::finance_charges::format_rate(markup)),
            quantity_e4: QUANTITY_SCALE,
            rate_cents: markup_total,
            income_account_id: account,
            product_service_id: None,
            class_id: None,
            is_taxable: false, service_date: None,
        });
    }

    let invoice = InvoiceInput {
        customer_name: customer,
        issue_date: input.issue_date,
        due_date: input.due_date,
        lines,
        currency: None,
        tax_rate_id: input.tax_rate_id,
        discount_cents: 0,
        details: Default::default(),
    };
    let invoice_id = crate::invoices::write_invoice_reimbursing(tx, &invoice, &Default::default(), &reimbursed).await?;
    for (activity, amount) in time_links {
        sqlx::query("INSERT INTO billed_time (time_activity_id, invoice_id, amount_cents) VALUES (?1, ?2, ?3)")
            .bind(activity)
            .bind(invoice_id)
            .bind(amount)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
    }
    for (column, line_id, amount, markup) in links {
        sqlx::query(&format!("INSERT INTO billed_costs ({column}, invoice_id, amount_cents, markup_cents) VALUES (?1, ?2, ?3, ?4)"))
            .bind(line_id)
            .bind(invoice_id)
            .bind(amount)
            .bind(markup)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
    }
    Ok(invoice_id)
}

/// Expense accounts an invoice credits for reimbursed costs, so an edit of it may keep them.
pub(crate) async fn reimbursed_accounts(tx: &mut Transaction<'_, Sqlite>, invoice_id: i64) -> Result<Vec<i64>, String> {
    sqlx::query_scalar(
        "SELECT l.account_id FROM billed_costs b JOIN expense_lines l ON l.id = b.expense_line_id WHERE b.invoice_id = ?1
         UNION SELECT l.account_id FROM billed_costs b JOIN bill_lines l ON l.id = b.bill_line_id WHERE b.invoice_id = ?1",
    )
    .bind(invoice_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)
}

/// An edited invoice keeps the costs and time the original billed.
pub(crate) async fn follow_replaced_invoice(tx: &mut Transaction<'_, Sqlite>, old_invoice: i64, new_invoice: i64) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO billed_costs (expense_line_id, bill_line_id, invoice_id, amount_cents, markup_cents)
         SELECT expense_line_id, bill_line_id, ?2, amount_cents, markup_cents FROM billed_costs WHERE invoice_id = ?1",
    )
    .bind(old_invoice)
    .bind(new_invoice)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    sqlx::query("INSERT INTO billed_time (time_activity_id, invoice_id, amount_cents) SELECT time_activity_id, ?2, amount_cents FROM billed_time WHERE invoice_id = ?1")
        .bind(old_invoice)
        .bind(new_invoice)
        .execute(&mut **tx)
        .await
        .map(|_| ())
        .map_err(db_err)
}

/// An edited expense or bill: a cost already billed stays billed on its replacement line (same line
/// number, same customer), so it can't be billed twice.
pub(crate) async fn follow_replaced_lines(tx: &mut Transaction<'_, Sqlite>, table: &str, old_id: i64, new_id: i64) -> Result<(), String> {
    let (lines, parent, column) = match table {
        "expenses" => ("expense_lines", "expense_id", "expense_line_id"),
        "bills" => ("bill_lines", "bill_id", "bill_line_id"),
        _ => return Ok(()),
    };
    sqlx::query(&format!(
        "INSERT INTO billed_costs ({column}, invoice_id, amount_cents, markup_cents)
         SELECT n.id, b.invoice_id, b.amount_cents, b.markup_cents
         FROM billed_costs b JOIN {lines} o ON o.id = b.{column} JOIN invoices i ON i.id = b.invoice_id
         JOIN {lines} n ON n.{parent} = ?2 AND n.line_number = o.line_number AND n.customer_id IS o.customer_id
         WHERE o.{parent} = ?1 AND i.is_voided = 0"
    ))
    .bind(old_id)
    .bind(new_id)
    .execute(&mut **tx)
    .await
    .map(|_| ())
    .map_err(db_err)
}

#[cfg(test)]
mod tests;
