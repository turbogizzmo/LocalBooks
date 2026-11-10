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
//! The same costs and time can be added from the invoice form (**Add billable costs…**): the form
//! posts its own lines plus the chosen costs ([`OnInvoice`]), and [`write_invoice_with_costs`]
//! appends the billing lines worked out here, so both paths bill alike.
//!
//! `billed_costs` records which invoice billed each cost; rows are only ever inserted. A cost
//! whose invoice is voided can be billed again, and an edited invoice or expense carries its
//! links to the replacement.
//!
//! A cost on an expense in another currency is billed at its home-currency amount, converted at the
//! expense's own exchange rate as its entry was (`ledger::to_home_cents`), so the reimbursement
//! takes off what the expense put on; the list shows the original amount beside it. Invoices are in
//! the home currency.

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
    /// In the home currency (converted for a cost in another currency).
    pub amount_cents: i64,
    pub customer_id: i64,
    pub customer_name: String,
    /// For a cost in another currency: its currency, rate and amount in that currency.
    pub currency_code: Option<String>,
    pub exchange_rate_e6: Option<i64>,
    #[sqlx(default)]
    pub foreign_amount_cents: Option<i64>,
}

/// A cost in another currency at its home amount, converted as its expense's entry was.
fn in_home_currency(mut cost: UnbilledCost) -> Result<UnbilledCost, String> {
    if let (Some(_), Some(rate)) = (&cost.currency_code, cost.exchange_rate_e6) {
        let home = crate::ledger::to_home_cents(cost.amount_cents, rate).filter(|h| *h > 0).ok_or("COST_FX: the cost doesn't convert to the home currency")?;
        cost.foreign_amount_cents = Some(cost.amount_cents);
        cost.amount_cents = home;
    }
    Ok(cost)
}

/// Billable lines of live expenses (in any currency) and bills, not on a live invoice.
const UNBILLED: &str = "
    SELECT 'EXPENSE' AS source, l.id AS line_id, x.expense_date AS date, v.name AS vendor_name, l.account_id, a.full_name AS account_name,
           NULLIF(l.memo, '') AS memo, l.amount_cents, l.customer_id, c.name AS customer_name, x.currency_code, x.exchange_rate_e6
    FROM expense_lines l JOIN expenses x ON x.id = l.expense_id JOIN journal_entries e ON e.id = x.journal_entry_id
    JOIN v_chart_of_accounts a ON a.id = l.account_id JOIN customers c ON c.id = l.customer_id LEFT JOIN vendors v ON v.id = x.vendor_id
    WHERE l.is_billable = 1 AND e.is_voided = 0 AND x.is_credit = 0
      AND NOT EXISTS (SELECT 1 FROM billed_costs b JOIN invoices i ON i.id = b.invoice_id WHERE b.expense_line_id = l.id AND i.is_voided = 0)
    UNION ALL
    SELECT 'BILL', l.id, b.bill_date, v.name, l.account_id, a.full_name, NULLIF(l.memo, ''), l.amount_cents, l.customer_id, c.name, NULL, NULL
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
    /// Hours × 10,000, rounded as the invoice line's quantity is.
    pub hours_e4: i64,
    pub hourly_rate_cents: i64,
    /// round_half_away(hours × rate).
    pub amount_cents: i64,
    pub customer_id: i64,
    pub customer_name: String,
    /// The service item is taxable, so the invoice line is (the invoice form's total counts it).
    pub is_taxable: bool,
}

const UNBILLED_TIME: &str = "
    SELECT t.id, t.activity_date AS date, m.name AS employee_name, t.product_service_id, p.name AS service_name, NULLIF(t.description, '') AS description,
           t.minutes, 0 AS hours_e4, t.hourly_rate_cents, 0 AS amount_cents, t.customer_id, c.name AS customer_name,
           COALESCE(p.is_taxable, 0) AS is_taxable
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
        r.hours_e4 = hours_e4(r.minutes).ok_or("TIME_AMOUNT: the hours are out of range")?;
        r.amount_cents = mul_div_round(r.hours_e4, r.hourly_rate_cents, QUANTITY_SCALE).ok_or("TIME_AMOUNT: the amount is out of range")?;
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
        .map_err(db_err)?
        .into_iter()
        .map(in_home_currency)
        .collect()
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

/// The billing defaults and the markup to use: `markup_text` as typed, or the default when blank.
pub async fn billing_settings(pool: &SqlitePool, markup_text: Option<&str>) -> Result<(Defaults, i64), String> {
    let settings = defaults(pool).await?;
    let markup = match markup_text.map(str::trim).filter(|m| !m.is_empty()) {
        Some(text) => tax_time::parse_markup(text)?,
        None => settings.markup_milli_percent,
    };
    Ok((settings, markup))
}

/// Bills costs on a new invoice to their customer or job; the invoice posts and the costs are
/// marked billed, together or not at all.
pub async fn bill_costs(pool: &SqlitePool, input: BillCostsInput) -> Result<InvoiceSummary, String> {
    if input.costs.is_empty() && input.time.is_empty() {
        return Err("COST_NOTHING_TO_BILL: choose at least one cost or time entry to bill".into());
    }
    let (settings, markup) = billing_settings(pool, input.markup_percent.as_deref()).await?;
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
    let invoice = InvoiceInput {
        customer_name: customer,
        issue_date: input.issue_date,
        due_date: input.due_date,
        lines: Vec::new(),
        currency: None,
        tax_rate_id: input.tax_rate_id,
        discount_cents: 0,
        details: Default::default(),
    };
    let chosen = OnInvoice { costs: input.costs.clone(), time: input.time.clone(), markup_percent: None };
    write_with_billing(tx, &invoice, input.customer_id, &chosen, settings, markup).await
}

/// Billable costs and time chosen on the invoice form, added as lines when the invoice saves.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnInvoice {
    #[serde(default)]
    pub costs: Vec<CostRef>,
    #[serde(default)]
    pub time: Vec<i64>,
    /// Percent; left out or blank, the default in Preferences › Time & Expenses.
    #[serde(default)]
    pub markup_percent: Option<String>,
}

impl OnInvoice {
    pub fn is_empty(&self) -> bool {
        self.costs.is_empty() && self.time.is_empty()
    }
}

/// Writes the invoice on the form with the chosen costs and time added after its own lines, and
/// marks them billed by it, in the caller's transaction. `settings` and `markup` come from
/// [`billing_settings`], read before the transaction. Nothing chosen: an ordinary invoice.
pub(crate) async fn write_invoice_with_costs(tx: &mut Transaction<'_, Sqlite>, input: &InvoiceInput, chosen: &OnInvoice, settings: &Defaults, markup: i64) -> Result<i64, String> {
    if chosen.is_empty() {
        return crate::invoices::write_invoice(tx, input).await;
    }
    if input.currency.is_some() {
        return Err("COST_FOREIGN_INVOICE: billable costs go on invoices in the home currency".into());
    }
    let name = crate::sales::job_name(input.customer_name.trim())?;
    let customer_id: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = ?1 COLLATE NOCASE")
        .bind(&name)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?
        .ok_or("COST_UNKNOWN_CUSTOMER: billable costs belong to a customer or job on your list; choose it on the invoice")?;
    write_with_billing(tx, input, customer_id, chosen, settings, markup).await
}

/// The one billing path, for Bill Costs and the invoice form alike: works out a line per cost and
/// time entry (and the markup line), writes the invoice with them after `input`'s own lines, and
/// records what it billed.
async fn write_with_billing(tx: &mut Transaction<'_, Sqlite>, input: &InvoiceInput, customer_id: i64, chosen: &OnInvoice, settings: &Defaults, markup: i64) -> Result<i64, String> {
    let mut lines = Vec::with_capacity(chosen.costs.len() + chosen.time.len() + 1);
    let mut links = Vec::with_capacity(chosen.costs.len());
    let mut reimbursed = Vec::new();
    let mut markup_total = 0i64;
    // Each expense account's own income account for reimbursements, read once for the whole billing.
    let own_income: std::collections::HashMap<i64, i64> =
        sqlx::query_as("SELECT id, reimbursement_income_account_id FROM chart_of_accounts WHERE reimbursement_income_account_id IS NOT NULL")
            .fetch_all(&mut **tx)
            .await
            .map_err(db_err)?
            .into_iter()
            .collect();
    for (n, cost) in chosen.costs.iter().enumerate() {
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
        let found = in_home_currency(found)?;
        // A cost in another currency says what it was in that currency.
        let foreign = match (&found.currency_code, found.foreign_amount_cents) {
            (Some(code), Some(amount)) => format!(", {code} {}", crate::export_engine::plain_amount(amount)),
            _ => String::new(),
        };
        if found.customer_id != customer_id {
            return Err(format!("COST_OTHER_CUSTOMER: the cost dated {} is for a different customer or job", found.date));
        }
        let what = found.memo.clone().unwrap_or_else(|| found.account_name.clone());
        let from = found.vendor_name.as_deref().map(|v| format!(", {v}")).unwrap_or_default();
        // Counted as income: the expense account's own income account, else the one in Preferences.
        let account = if settings.as_income {
            own_income.get(&found.account_id).copied().or(settings.income_account_id)
                .ok_or("COST_NO_INCOME_ACCOUNT: choose the income account for reimbursed expenses in Preferences › Time & Expenses, or on the expense account")?
        } else {
            reimbursed.push(found.account_id);
            found.account_id
        };
        lines.push(InvoiceLineInput {
            description: format!("{what} ({}{from}{foreign})", found.date),
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
    let mut time_links = Vec::with_capacity(chosen.time.len());
    for &activity in &chosen.time {
        let found: Option<UnbilledTime> = sqlx::query_as(&format!("SELECT * FROM ({UNBILLED_TIME}) WHERE id = ?1")).bind(activity).fetch_optional(&mut **tx).await.map_err(db_err)?;
        let Some(found) = found else {
            return Err("TIME_NOT_BILLABLE: a time entry is not billable, or is already on an invoice".into());
        };
        if found.customer_id != customer_id {
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

    // Only the reimbursement lines may credit an expense account; the form's own lines are refused
    // there as on any invoice.
    if let Some(n) = input.lines.iter().position(|l| reimbursed.contains(&l.income_account_id)) {
        return Err(format!("INV_NOT_INCOME_ACCOUNT: line {} must use an Income account", n + 1));
    }
    let mut invoice = input.clone();
    invoice.lines.extend(lines);
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
/// The expense accounts an edited invoice's lines may credit: for an invoice that billed costs,
/// any active expense account (a reimbursed line can be moved to another expense account, say to
/// correct where the cost was booked); for any other invoice, none.
pub(crate) async fn reimbursed_accounts(tx: &mut Transaction<'_, Sqlite>, invoice_id: i64) -> Result<Vec<i64>, String> {
    sqlx::query_scalar(
        "SELECT a.id FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type
         WHERE t.classification = 'EXPENSE' AND a.is_active = 1
           AND EXISTS (SELECT 1 FROM billed_costs b WHERE b.invoice_id = ?1)",
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
