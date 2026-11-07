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
    pub costs: Vec<CostRef>,
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
    if input.costs.is_empty() {
        return Err("COST_NOTHING_TO_BILL: choose at least one cost to bill".into());
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
    let income = if settings.as_income {
        Some(settings.income_account_id.ok_or("COST_NO_INCOME_ACCOUNT: choose the income account for reimbursed expenses in Preferences › Time & Expenses")?)
    } else {
        None
    };

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
        let account = income.unwrap_or(found.account_id);
        if income.is_none() {
            reimbursed.push(found.account_id);
        }
        lines.push(InvoiceLineInput {
            description: format!("{what} ({}{from})", found.date),
            quantity_e4: QUANTITY_SCALE,
            rate_cents: found.amount_cents,
            income_account_id: account,
            product_service_id: None,
            class_id: None,
            is_taxable: false,
        });
        let this_markup = markup_cents(found.amount_cents, markup).filter(|m| (0..=MAX_LINE_CENTS).contains(m)).ok_or("COST_MARKUP_TOO_LARGE: the markup is out of range")?;
        markup_total = markup_total.checked_add(this_markup).ok_or("COST_MARKUP_TOO_LARGE: the markup is out of range")?;
        links.push((column, cost.line_id, found.amount_cents, this_markup));
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
            is_taxable: false,
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

/// An edited invoice keeps the costs the original billed.
pub(crate) async fn follow_replaced_invoice(tx: &mut Transaction<'_, Sqlite>, old_invoice: i64, new_invoice: i64) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO billed_costs (expense_line_id, bill_line_id, invoice_id, amount_cents, markup_cents)
         SELECT expense_line_id, bill_line_id, ?2, amount_cents, markup_cents FROM billed_costs WHERE invoice_id = ?1",
    )
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
