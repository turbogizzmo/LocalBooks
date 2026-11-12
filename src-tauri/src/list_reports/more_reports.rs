//! Report presets the desktop product has that the Report Center lacked (T36): Budget Overview,
//! Sales Tax Revenue Summary, Memorized Transaction Listing, To Do Notes, Sales by Ship To
//! Address, and Realized / Unrealized Gains & Losses. Each reads what an existing screen or report
//! already keeps, so its figures match: the budget as entered, the Sales Tax Liability report's
//! sales, the income each sale posted, and the Exchange Gain or Loss account.

use chrono::{Datelike, NaiveDate};
use serde_json::json;
use sqlx::SqlitePool;

use super::{done, head, labeled, plural, row, section};
use crate::accountant_reports::AccountantReport;
use crate::custom_reports::{type_label, ColumnHead, ColumnKind, RowKind};
use crate::ledger::{db_err, DISPLAY_SOURCE_TYPE};

fn sum(values: impl IntoIterator<Item = i64>) -> Result<i64, String> {
    values.into_iter().try_fold(0i64, |a, v| a.checked_add(v)).ok_or_else(|| "RPT_OVERFLOW: amount out of range".to_string())
}

/// "Jan 2026" for the month starting `first`.
fn month_label(first: NaiveDate) -> String {
    const NAMES: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {}", NAMES[first.month0() as usize], first.year())
}

/// The first day of the month `n` months after `first`.
fn plus_months(first: NaiveDate, n: u32) -> NaiveDate {
    let total = first.year() * 12 + first.month0() as i32 + n as i32;
    NaiveDate::from_ymd_opt(total.div_euclid(12), total.rem_euclid(12) as u32 + 1, 1).expect("a valid month")
}

// ------------------------------------------------------------------------------------------------
// Budget Overview
// ------------------------------------------------------------------------------------------------

/// Every budget for the fiscal year holding `as_of`: each account's amount by month and its total,
/// income first, then expenses, with net income, as the budget was entered (Accounting › Budgets).
pub(super) async fn budget_overview(pool: &SqlitePool, as_of: NaiveDate) -> Result<AccountantReport, String> {
    let (fy_start, _) = crate::preferences::fiscal_year_bounds(as_of, crate::preferences::fiscal_start_month(pool).await?);
    let budgets: Vec<(i64, String)> = sqlx::query_as("SELECT id, name FROM budgets WHERE fiscal_year_start = ?1 ORDER BY name COLLATE NOCASE")
        .bind(fy_start)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    let months: Vec<NaiveDate> = (0..12).map(|n| plus_months(fy_start, n)).collect();
    let mut columns: Vec<ColumnHead> = vec![head("Account", ColumnKind::Text)];
    columns.extend(months.iter().map(|m| head(&month_label(*m), ColumnKind::Money)));
    columns.push(head("Total", ColumnKind::Money));
    let w = columns.len();
    let mut rows = Vec::new();
    let mut lines = 0;
    for (budget_id, name) in &budgets {
        rows.push(section(0, w, name));
        let amounts: Vec<(i64, String, String, i64, i64)> = sqlx::query_as(
            "SELECT a.id, a.full_name, a.classification, b.period, b.amount_cents
             FROM budget_amounts b JOIN v_chart_of_accounts a ON a.id = b.account_id
             WHERE b.budget_id = ?1
             ORDER BY CASE a.classification WHEN 'INCOME' THEN 0 WHEN 'EXPENSE' THEN 1 ELSE 2 END, a.full_name COLLATE NOCASE, b.period",
        )
        .bind(budget_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
        // Net income by month: income less expenses.
        let mut net = [0i64; 12];
        let mut i = 0;
        while i < amounts.len() {
            let (account, ref label, ref class, ..) = amounts[i];
            let mut by_month = [0i64; 12];
            while i < amounts.len() && amounts[i].0 == account {
                by_month[(amounts[i].3 - 1) as usize] = amounts[i].4;
                i += 1;
            }
            // Net income counts income and expenses only; a balance-sheet account's budget is listed but
            // left out of it.
            let sign = match class.as_str() {
                "INCOME" => 1,
                "EXPENSE" => -1,
                _ => 0,
            };
            for (n, v) in net.iter_mut().zip(by_month) {
                *n = v.checked_mul(sign).and_then(|v| n.checked_add(v)).ok_or("RPT_OVERFLOW: amount out of range")?;
            }
            let mut cells = vec![json!(label)];
            cells.extend(by_month.iter().map(|v| json!(v)));
            cells.push(json!(sum(by_month)?));
            rows.push(row(RowKind::Detail, 1, cells));
            lines += 1;
        }
        let mut cells = vec![json!(format!("Net Income · {name}"))];
        cells.extend(net.iter().map(|v| json!(v)));
        cells.push(json!(sum(net)?));
        rows.push(row(RowKind::Total, 0, cells));
    }
    let notes = vec![if budgets.is_empty() {
        format!("No budget for the fiscal year starting {fy_start}. Set one up in Accounting › Budgets.")
    } else {
        format!("{} for the fiscal year starting {fy_start}, as entered in Accounting › Budgets.", plural(budgets.len(), "budget", "budgets"))
    }];
    let fy_end = plus_months(fy_start, 12).pred_opt().unwrap_or(as_of);
    Ok(done(Some(fy_start), fy_end, columns, rows, lines, notes))
}

// ------------------------------------------------------------------------------------------------
// Sales Tax Revenue Summary
// ------------------------------------------------------------------------------------------------

/// Per tax rate: total sales, non-taxable and taxable sales, the same figures the Sales Tax
/// Liability report reports (it is built from that report).
pub(super) async fn sales_tax_revenue(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<AccountantReport, String> {
    let liability = crate::sales_tax::tax_liability(pool, start, end).await?;
    let columns = vec![
        head("Tax", ColumnKind::Text),
        head("Total Sales", ColumnKind::Money),
        head("Non-Taxable Sales", ColumnKind::Money),
        head("Taxable Sales", ColumnKind::Money),
    ];
    let mut rows = Vec::new();
    let mut totals = [0i64; 3];
    for r in &liability.rows {
        let total = r.taxable_sales_cents.checked_add(r.non_taxable_sales_cents).ok_or("RPT_OVERFLOW: amount out of range")?;
        for (t, v) in totals.iter_mut().zip([total, r.non_taxable_sales_cents, r.taxable_sales_cents]) {
            *t = t.checked_add(v).ok_or("RPT_OVERFLOW: amount out of range")?;
        }
        rows.push(row(RowKind::Detail, 0, vec![json!(r.name), json!(total), json!(r.non_taxable_sales_cents), json!(r.taxable_sales_cents)]));
    }
    let count = rows.len();
    rows.push(row(RowKind::Total, 0, vec![json!("TOTAL"), json!(totals[0]), json!(totals[1]), json!(totals[2])]));
    let notes = vec!["The sales the Sales Tax Liability report counts, by tax rate. Sales with no rate are non-taxable.".to_string()];
    Ok(done(Some(start), end, columns, rows, count, notes))
}

// ------------------------------------------------------------------------------------------------
// Memorized Transaction Listing
// ------------------------------------------------------------------------------------------------

/// Every memorized (recurring) transaction: name, type, how often, the next date and when it ends.
pub(super) async fn memorized_transactions(pool: &SqlitePool, today: NaiveDate) -> Result<AccountantReport, String> {
    type ListRow = (String, String, String, i64, String, Option<String>, bool);
    let list: Vec<ListRow> =
        sqlx::query_as("SELECT name, kind, frequency, every, next_date, end_date, is_active FROM recurring_templates ORDER BY name COLLATE NOCASE")
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
    let columns = vec![
        head("Name", ColumnKind::Text),
        head("Type", ColumnKind::Text),
        head("How Often", ColumnKind::Text),
        head("Next Date", ColumnKind::Date),
        head("Ends", ColumnKind::Date),
        head("Status", ColumnKind::Text),
    ];
    fn kind_label(k: &str) -> &str {
        match k {
            "JOURNAL" => "Journal Entry",
            "EXPENSE" => "Expense",
            "BILL" => "Bill",
            "INVOICE" => "Invoice",
            other => other,
        }
    }
    let rows = list
        .iter()
        .map(|(name, kind, frequency, every, next, ends, active)| {
            let unit = match frequency.as_str() {
                "WEEKLY" => ("week", "weeks"),
                "MONTHLY" => ("month", "months"),
                "QUARTERLY" => ("quarter", "quarters"),
                _ => ("year", "years"),
            };
            let how = if *every == 1 { format!("Every {}", unit.0) } else { format!("Every {every} {}", unit.1) };
            row(
                RowKind::Detail,
                0,
                vec![json!(name), json!(kind_label(kind)), json!(how), json!(next), json!(ends), json!(if *active { "Active" } else { "Paused" })],
            )
        })
        .collect();
    let notes = vec![format!("{} in Accounting › Recurring Transactions.", plural(list.len(), "memorized transaction", "memorized transactions"))];
    Ok(done(None, today, columns, rows, list.len(), notes))
}

// ------------------------------------------------------------------------------------------------
// To Do Notes
// ------------------------------------------------------------------------------------------------

/// Every to-do, open ones first by due date: done or not, due date, what it says, who or what it is
/// about, and who it is assigned to.
pub(super) async fn todo_notes(pool: &SqlitePool, today: NaiveDate) -> Result<AccountantReport, String> {
    type ListRow = (bool, Option<String>, String, String, Option<String>, Option<String>);
    let list: Vec<ListRow> = sqlx::query_as(
        "SELECT t.is_done, t.due_date, t.title, t.note,
                COALESCE(c.name, v.name, e.name,
                         CASE t.link_kind WHEN 'INVOICE' THEN 'Invoice ' || (SELECT invoice_number FROM invoices WHERE id = t.link_id)
                                          WHEN 'BILL' THEN 'Bill ' || COALESCE((SELECT bill_number FROM bills WHERE id = t.link_id), '') END),
                u.display_name
         FROM todos t
         LEFT JOIN customers c ON c.id = t.customer_id
         LEFT JOIN vendors v ON v.id = t.vendor_id
         LEFT JOIN employees e ON e.id = t.employee_id
         LEFT JOIN users u ON u.id = t.assigned_user_id
         ORDER BY t.is_done, t.due_date IS NULL, t.due_date, t.id",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let columns = vec![
        head("Done", ColumnKind::Text),
        head("Due", ColumnKind::Date),
        head("To Do", ColumnKind::Text),
        head("Note", ColumnKind::Text),
        head("About", ColumnKind::Text),
        head("Assigned To", ColumnKind::Text),
    ];
    let open = list.iter().filter(|t| !t.0).count();
    let rows = list
        .iter()
        .map(|(is_done, due, title, note, about, user)| {
            row(RowKind::Detail, 0, vec![json!(if *is_done { "✓" } else { "" }), json!(due), json!(title), json!(note), json!(about), json!(user)])
        })
        .collect();
    let notes = vec![format!("{} open, {} done.", plural(open, "to-do", "to-dos"), list.len() - open)];
    Ok(done(None, today, columns, rows, list.len(), notes))
}

// ------------------------------------------------------------------------------------------------
// Sales by Ship To Address
// ------------------------------------------------------------------------------------------------

/// Invoices and sales receipts in the range, grouped by their Ship To address. Each sale's amount
/// is the income it posted (in home currency, after discounts, before tax), as Profit & Loss
/// counts it.
pub(super) async fn sales_by_ship_to(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<AccountantReport, String> {
    type SaleRow = (Option<String>, String, String, Option<String>, Option<String>, i64, i64);
    let sales: Vec<SaleRow> = sqlx::query_as(&format!(
        "SELECT ship_to, source_type, entry_date, num, name, journal_entry_id, income FROM (
            SELECT NULLIF(trim(i.ship_to), '') AS ship_to, {DISPLAY_SOURCE_TYPE} AS source_type, e.entry_date, CAST(i.invoice_number AS TEXT) AS num,
                   c.name, e.id AS journal_entry_id,
                   (SELECT COALESCE(SUM(l.credit_cents - l.debit_cents), 0) FROM journal_lines l JOIN v_chart_of_accounts a ON a.id = l.account_id
                    WHERE l.journal_entry_id = e.id AND a.classification = 'INCOME') AS income
            FROM invoices i JOIN journal_entries e ON e.id = i.journal_entry_id JOIN customers c ON c.id = i.customer_id
            WHERE i.is_voided = 0 AND e.status = 'POSTED' AND e.entry_date BETWEEN ?1 AND ?2
            UNION ALL
            SELECT NULLIF(trim(r.ship_to), ''), {DISPLAY_SOURCE_TYPE}, e.entry_date, CAST(r.receipt_number AS TEXT), c.name, e.id,
                   (SELECT COALESCE(SUM(l.credit_cents - l.debit_cents), 0) FROM journal_lines l JOIN v_chart_of_accounts a ON a.id = l.account_id
                    WHERE l.journal_entry_id = e.id AND a.classification = 'INCOME')
            FROM sales_receipts r JOIN journal_entries e ON e.id = r.journal_entry_id LEFT JOIN customers c ON c.id = r.customer_id
            WHERE r.is_voided = 0 AND e.status = 'POSTED' AND e.entry_date BETWEEN ?1 AND ?2
         ) ORDER BY ship_to IS NULL, ship_to COLLATE NOCASE, entry_date, num"
    ))
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let columns = vec![
        head("Type", ColumnKind::Text),
        head("Date", ColumnKind::Date),
        head("Num", ColumnKind::Text),
        head("Customer", ColumnKind::Text),
        head("Amount", ColumnKind::Money),
    ];
    let w = columns.len();
    let mut rows = Vec::new();
    let mut total = 0i64;
    let mut i = 0;
    while i < sales.len() {
        let address = sales[i].0.clone();
        let label = address.clone().map_or_else(|| "(No ship-to address)".to_string(), |a| a.replace('\n', ", "));
        rows.push(section(0, w, &label));
        let mut sub = 0i64;
        while i < sales.len() && sales[i].0 == address {
            let (_, source, date, num, name, entry, income) = &sales[i];
            let mut r = row(RowKind::Detail, 1, vec![json!(type_label(source)), json!(date), json!(num), json!(name), json!(income)]);
            r.journal_entry_id = Some(*entry);
            rows.push(r);
            sub = sub.checked_add(*income).ok_or("RPT_OVERFLOW: amount out of range")?;
            i += 1;
        }
        rows.push(labeled(RowKind::Subtotal, 0, w, &format!("Total {label}"), &[json!(sub)]));
        total = total.checked_add(sub).ok_or("RPT_OVERFLOW: amount out of range")?;
    }
    if !sales.is_empty() {
        rows.push(labeled(RowKind::Total, 0, w, "TOTAL", &[json!(total)]));
    }
    let notes = vec!["Invoices and sales receipts by their Ship To address; each amount is the income it posted, before sales tax.".to_string()];
    Ok(done(Some(start), end, columns, rows, sales.len(), notes))
}

// ------------------------------------------------------------------------------------------------
// Realized / Unrealized Gains & Losses
// ------------------------------------------------------------------------------------------------

/// What posted to a currency gain account in the range, by transaction. `unrealized` = false: the
/// Exchange Gain or Loss account, where gains and losses are realized when foreign-currency
/// invoices and bills are paid; true: Unrealized Exchange Gain or Loss, where the home currency
/// adjustments (Accounting › Currency Revaluation) and their reversals post. A gain is positive.
pub(super) async fn currency_gains(pool: &SqlitePool, start: NaiveDate, end: NaiveDate, unrealized: bool) -> Result<AccountantReport, String> {
    let account = if unrealized { "Unrealized Exchange Gain or Loss" } else { "Exchange Gain or Loss" };
    type LineRow = (i64, String, String, i64, Option<String>, Option<String>, i64);
    let lines: Vec<LineRow> = sqlx::query_as(&format!(
        "SELECT e.id, {DISPLAY_SOURCE_TYPE}, e.entry_date, e.entry_number, MAX(p.party_name), COALESCE(MAX(l.memo), e.memo),
                SUM(l.credit_cents - l.debit_cents)
         FROM journal_lines l
         JOIN journal_entries e ON e.id = l.journal_entry_id
         JOIN chart_of_accounts a ON a.id = l.account_id
         LEFT JOIN v_entry_party p ON p.journal_entry_id = e.id
         WHERE a.parent_id IS NULL AND a.name = ?3 COLLATE NOCASE
           AND e.status = 'POSTED' AND e.entry_date BETWEEN ?1 AND ?2
         GROUP BY e.id
         ORDER BY e.entry_date, e.entry_number"
    ))
    .bind(start)
    .bind(end)
    .bind(account)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let columns = vec![
        head("Type", ColumnKind::Text),
        head("Date", ColumnKind::Date),
        head("Entry", ColumnKind::Text),
        head("Name", ColumnKind::Text),
        head("Memo", ColumnKind::Text),
        head("Gain (Loss)", ColumnKind::Money),
    ];
    let w = columns.len();
    let mut rows: Vec<_> = lines
        .iter()
        .map(|(entry, source, date, number, name, memo, gain)| {
            let mut r = row(RowKind::Detail, 0, vec![json!(type_label(source)), json!(date), json!(number.to_string()), json!(name), json!(memo), json!(gain)]);
            r.journal_entry_id = Some(*entry);
            r
        })
        .collect();
    let total = sum(lines.iter().map(|l| l.6))?;
    rows.push(labeled(RowKind::Total, 0, w, "TOTAL", &[json!(total)]));
    let what = if unrealized {
        "Home currency adjustments (Accounting › Currency Revaluation) and their reversals, as posted to Unrealized Exchange Gain or Loss."
    } else {
        "Gains and losses realized on foreign-currency payments, as posted to Exchange Gain or Loss."
    };
    Ok(done(Some(start), end, columns, rows, lines.len(), vec![what.to_string()]))
}
