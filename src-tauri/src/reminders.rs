//! The To Do list and Reminders.
//!
//! A to-do is a task with a title, a note and an optional due date, about at most one customer,
//! vendor or employee and optionally one invoice or bill. With users on it can be assigned to one of
//! them. Admins and Standard users add, edit and complete any to-do; a View-only user may only tick
//! off (or reopen) the to-dos assigned to them.
//!
//! Reminders are a summary of what needs attention as of a day, one group per category, each with
//! its items: overdue invoices, bills due soon, to-dos due, recurring transactions due to be entered,
//! the Email later queue, upcoming federal tax dates, and a backup when the last one is too old.
//! Which categories show and how many days each looks ahead are the viewer's own settings
//! (`reminder_settings`, one row per user, or user 0 in a file without users).
//!
//! LocalBooks has no "print later" queue and no inventory reorder points, so QuickBooks' "Forms to
//! print" and "Inventory to reorder" reminders have nothing to read and are not offered.

use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::db_err;
use crate::users::Role;

/// The longest look-ahead (or overdue limit) a category accepts.
pub const MAX_DAYS: i64 = 365;

// ------------------------------------------------------------------------------------------------
// Who is asking
// ------------------------------------------------------------------------------------------------

/// The signed-in user, or nobody in a file without users.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Viewer {
    pub user_id: Option<i64>,
    pub role: Option<Role>,
}

impl Viewer {
    /// Whose reminder settings apply: the user's, or 0 in a file without users.
    fn settings_key(self) -> i64 {
        self.user_id.unwrap_or(0)
    }
}

// ------------------------------------------------------------------------------------------------
// Settings
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub show: bool,
    /// Days ahead to look (bills, to-dos, recurring, tax dates), days past due before an invoice
    /// shows (overdue invoices), or days since the last backup before it is due (backup).
    pub days: i64,
}

const fn shown(days: i64) -> Category {
    Category { show: true, days }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReminderSettings {
    /// Show the Reminders window when the company opens.
    pub show_at_startup: bool,
    pub overdue_invoices: Category,
    pub bills_due: Category,
    pub todos: Category,
    pub recurring: Category,
    pub email_queue: Category,
    pub tax_dates: Category,
    pub backup: Category,
}

impl Default for ReminderSettings {
    fn default() -> Self {
        ReminderSettings {
            show_at_startup: false,
            overdue_invoices: shown(0),
            bills_due: shown(10),
            todos: shown(0),
            recurring: shown(0),
            email_queue: shown(0),
            tax_dates: shown(30),
            backup: shown(7),
        }
    }
}

impl ReminderSettings {
    fn categories(&self) -> [(&'static str, Category); 7] {
        [
            ("overdue invoices", self.overdue_invoices),
            ("bills due", self.bills_due),
            ("to-dos", self.todos),
            ("recurring transactions", self.recurring),
            ("email later", self.email_queue),
            ("tax dates", self.tax_dates),
            ("backup", self.backup),
        ]
    }

    pub fn validate(&self) -> Result<(), String> {
        for (name, category) in self.categories() {
            if !(0..=MAX_DAYS).contains(&category.days) {
                return Err(format!("REMINDER_BAD_DAYS: days for {name} must be 0 to {MAX_DAYS}"));
            }
        }
        if self.backup.days < 1 {
            return Err("REMINDER_BAD_DAYS: a backup is due after at least 1 day".into());
        }
        Ok(())
    }
}

pub async fn settings(pool: &SqlitePool, viewer: Viewer) -> Result<ReminderSettings, String> {
    let saved: Option<String> = sqlx::query_scalar("SELECT settings FROM reminder_settings WHERE user_id = ?1")
        .bind(viewer.settings_key())
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    // Settings saved by another version that no longer read fall back to the defaults.
    Ok(saved.and_then(|json| serde_json::from_str(&json).ok()).unwrap_or_default())
}

pub async fn save_settings(pool: &SqlitePool, viewer: Viewer, settings: &ReminderSettings) -> Result<ReminderSettings, String> {
    settings.validate()?;
    let json = serde_json::to_string(settings).map_err(|e| format!("REMINDER_SETTINGS: {e}"))?;
    sqlx::query(
        "INSERT INTO reminder_settings (user_id, settings) VALUES (?1, ?2)
         ON CONFLICT (user_id) DO UPDATE SET settings = excluded.settings, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(viewer.settings_key())
    .bind(json)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(*settings)
}

// ------------------------------------------------------------------------------------------------
// To-dos
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoInput {
    pub title: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub due_date: Option<NaiveDate>,
    #[serde(default)]
    pub customer_id: Option<i64>,
    #[serde(default)]
    pub vendor_id: Option<i64>,
    #[serde(default)]
    pub employee_id: Option<i64>,
    /// `INVOICE` or `BILL`.
    #[serde(default)]
    pub link_kind: Option<String>,
    #[serde(default)]
    pub link_id: Option<i64>,
    #[serde(default)]
    pub assigned_user_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct TodoView {
    pub id: i64,
    pub title: String,
    pub note: String,
    pub due_date: Option<String>,
    pub customer_id: Option<i64>,
    pub customer_name: Option<String>,
    pub vendor_id: Option<i64>,
    pub vendor_name: Option<String>,
    pub employee_id: Option<i64>,
    pub employee_name: Option<String>,
    pub link_kind: Option<String>,
    pub link_id: Option<i64>,
    /// "Invoice 1042" or "Bill INV-7 · Acme"; None when the linked document is gone.
    pub link_label: Option<String>,
    /// The linked document's journal entry, to open it.
    pub link_entry_id: Option<i64>,
    pub assigned_user_id: Option<i64>,
    pub assigned_name: Option<String>,
    pub is_done: bool,
    pub done_at: Option<String>,
    pub created_at: String,
}

const TODO_SELECT: &str = "
    SELECT t.id, t.title, t.note, t.due_date,
           t.customer_id, c.name AS customer_name, t.vendor_id, v.name AS vendor_name, t.employee_id, e.name AS employee_name,
           t.link_kind, t.link_id,
           CASE t.link_kind
               WHEN 'INVOICE' THEN (SELECT 'Invoice ' || i.invoice_number FROM invoices i WHERE i.id = t.link_id)
               WHEN 'BILL' THEN (SELECT 'Bill' || COALESCE(' ' || NULLIF(b.bill_number, ''), '') || ' · ' || bv.name
                                 FROM bills b JOIN vendors bv ON bv.id = b.vendor_id WHERE b.id = t.link_id)
           END AS link_label,
           CASE t.link_kind
               WHEN 'INVOICE' THEN (SELECT i.journal_entry_id FROM invoices i WHERE i.id = t.link_id)
               WHEN 'BILL' THEN (SELECT b.journal_entry_id FROM bills b WHERE b.id = t.link_id)
           END AS link_entry_id,
           t.assigned_user_id, u.display_name AS assigned_name, t.is_done, t.done_at, t.created_at
    FROM todos t
    LEFT JOIN customers c ON c.id = t.customer_id
    LEFT JOIN vendors v ON v.id = t.vendor_id
    LEFT JOIN employees e ON e.id = t.employee_id
    LEFT JOIN users u ON u.id = t.assigned_user_id";

/// Every to-do: open ones first, by due date (undated last), then done ones, latest first.
pub async fn list_todos(pool: &SqlitePool) -> Result<Vec<TodoView>, String> {
    sqlx::query_as(&format!(
        "{TODO_SELECT} ORDER BY t.is_done, CASE WHEN t.is_done = 1 THEN t.done_at END DESC, t.due_date IS NULL, t.due_date, t.id"
    ))
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

pub async fn get_todo(pool: &SqlitePool, id: i64) -> Result<TodoView, String> {
    sqlx::query_as(&format!("{TODO_SELECT} WHERE t.id = ?1"))
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| "TODO_NOT_FOUND: that to-do no longer exists".to_string())
}

async fn exists(pool: &SqlitePool, sql: &str, id: i64) -> Result<bool, String> {
    sqlx::query_scalar(sql).bind(id).fetch_one(pool).await.map_err(db_err)
}

async fn validate_todo(pool: &SqlitePool, input: &TodoInput) -> Result<(), String> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err("TODO_TITLE_REQUIRED: give the to-do a title".into());
    }
    if title.chars().count() > 200 {
        return Err("TODO_TITLE_TOO_LONG: a title is at most 200 characters".into());
    }
    if input.note.chars().count() > 4000 {
        return Err("TODO_NOTE_TOO_LONG: a note is at most 4,000 characters".into());
    }
    let parties = [input.customer_id, input.vendor_id, input.employee_id].iter().filter(|p| p.is_some()).count();
    if parties > 1 {
        return Err("TODO_ONE_NAME: a to-do is about one customer, vendor or employee".into());
    }
    let checks = [
        (input.customer_id, "SELECT EXISTS (SELECT 1 FROM customers WHERE id = ?1)", "customer"),
        (input.vendor_id, "SELECT EXISTS (SELECT 1 FROM vendors WHERE id = ?1)", "vendor"),
        (input.employee_id, "SELECT EXISTS (SELECT 1 FROM employees WHERE id = ?1)", "employee"),
    ];
    for (id, sql, noun) in checks {
        if let Some(id) = id {
            if !exists(pool, sql, id).await? {
                return Err(format!("TODO_BAD_NAME: that {noun} no longer exists"));
            }
        }
    }
    match (input.link_kind.as_deref(), input.link_id) {
        (None, None) => {}
        (Some("INVOICE"), Some(id)) => {
            if !exists(pool, "SELECT EXISTS (SELECT 1 FROM invoices WHERE id = ?1)", id).await? {
                return Err("TODO_BAD_LINK: that invoice no longer exists".into());
            }
        }
        (Some("BILL"), Some(id)) => {
            if !exists(pool, "SELECT EXISTS (SELECT 1 FROM bills WHERE id = ?1)", id).await? {
                return Err("TODO_BAD_LINK: that bill no longer exists".into());
            }
        }
        _ => return Err("TODO_BAD_LINK: a to-do links to one invoice or bill".into()),
    }
    if let Some(user) = input.assigned_user_id {
        if !exists(pool, "SELECT EXISTS (SELECT 1 FROM users WHERE id = ?1 AND is_active = 1)", user).await? {
            return Err("TODO_BAD_ASSIGNEE: assign the to-do to an active user".into());
        }
    }
    Ok(())
}

/// Adds a to-do (`id` None) or replaces one's details; returns it as listed.
pub async fn save_todo(pool: &SqlitePool, id: Option<i64>, input: &TodoInput, viewer: Viewer) -> Result<TodoView, String> {
    validate_todo(pool, input).await?;
    let due = input.due_date.map(|d| d.to_string());
    let id = match id {
        None => sqlx::query_scalar(
            "INSERT INTO todos (title, note, due_date, customer_id, vendor_id, employee_id, link_kind, link_id, assigned_user_id, created_by_user_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) RETURNING id",
        )
        .bind(input.title.trim())
        .bind(input.note.trim())
        .bind(due)
        .bind(input.customer_id)
        .bind(input.vendor_id)
        .bind(input.employee_id)
        .bind(&input.link_kind)
        .bind(input.link_id)
        .bind(input.assigned_user_id)
        .bind(viewer.user_id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?,
        Some(id) => {
            let changed = sqlx::query(
                "UPDATE todos SET title = ?2, note = ?3, due_date = ?4, customer_id = ?5, vendor_id = ?6, employee_id = ?7,
                        link_kind = ?8, link_id = ?9, assigned_user_id = ?10, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                 WHERE id = ?1",
            )
            .bind(id)
            .bind(input.title.trim())
            .bind(input.note.trim())
            .bind(due)
            .bind(input.customer_id)
            .bind(input.vendor_id)
            .bind(input.employee_id)
            .bind(&input.link_kind)
            .bind(input.link_id)
            .bind(input.assigned_user_id)
            .execute(pool)
            .await
            .map_err(db_err)?;
            if changed.rows_affected() == 0 {
                return Err("TODO_NOT_FOUND: that to-do no longer exists".into());
            }
            id
        }
    };
    get_todo(pool, id).await
}

/// Ticks a to-do off or reopens it. A View-only user may do this only for to-dos assigned to them.
pub async fn set_done(pool: &SqlitePool, id: i64, done: bool, viewer: Viewer) -> Result<TodoView, String> {
    let todo = get_todo(pool, id).await?;
    if viewer.role == Some(Role::ViewOnly) && (todo.assigned_user_id.is_none() || todo.assigned_user_id != viewer.user_id) {
        return Err("ROLE_VIEW_ONLY: view only: you can complete only the to-dos assigned to you".into());
    }
    sqlx::query(
        "UPDATE todos SET is_done = ?2, done_at = CASE WHEN ?2 = 1 THEN COALESCE(done_at, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')) END,
                updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE id = ?1",
    )
    .bind(id)
    .bind(done)
    .execute(pool)
    .await
    .map_err(db_err)?;
    get_todo(pool, id).await
}

pub async fn delete_todo(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let deleted = sqlx::query("DELETE FROM todos WHERE id = ?1").bind(id).execute(pool).await.map_err(db_err)?;
    if deleted.rows_affected() == 0 {
        return Err("TODO_NOT_FOUND: that to-do no longer exists".into());
    }
    Ok(())
}

/// Someone a to-do can be assigned to.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Assignee {
    pub id: i64,
    pub display_name: String,
}

/// Active users, for the Assigned to picker (empty in a file without users).
pub async fn assignees(pool: &SqlitePool) -> Result<Vec<Assignee>, String> {
    sqlx::query_as("SELECT id, display_name FROM users WHERE is_active = 1 ORDER BY display_name COLLATE NOCASE")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// An invoice or bill a to-do can link to.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct LinkOption {
    pub kind: String,
    pub id: i64,
    pub label: String,
    pub date: String,
}

/// A customer's invoices or a vendor's bills, newest first, to link a to-do to.
pub async fn link_options(pool: &SqlitePool, customer_id: Option<i64>, vendor_id: Option<i64>) -> Result<Vec<LinkOption>, String> {
    match (customer_id, vendor_id) {
        (Some(customer), _) => sqlx::query_as(
            "SELECT 'INVOICE' AS kind, id, 'Invoice ' || invoice_number AS label, issue_date AS date
             FROM invoices WHERE customer_id = ?1 AND is_voided = 0 ORDER BY issue_date DESC, invoice_number DESC LIMIT 200",
        )
        .bind(customer)
        .fetch_all(pool)
        .await
        .map_err(db_err),
        (None, Some(vendor)) => sqlx::query_as(
            "SELECT 'BILL' AS kind, id, 'Bill' || COALESCE(' ' || NULLIF(bill_number, ''), '') || ' dated ' || bill_date AS label, bill_date AS date
             FROM bills WHERE vendor_id = ?1 AND is_voided = 0 ORDER BY bill_date DESC, id DESC LIMIT 200",
        )
        .bind(vendor)
        .fetch_all(pool)
        .await
        .map_err(db_err),
        (None, None) => Ok(Vec::new()),
    }
}

// ------------------------------------------------------------------------------------------------
// Reminders
// ------------------------------------------------------------------------------------------------

/// One thing that needs attention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderItem {
    /// The invoice, bill, to-do, recurring transaction or emailed document it is about.
    pub id: Option<i64>,
    pub label: String,
    pub detail: String,
    pub date: Option<String>,
    pub amount_cents: Option<i64>,
    /// The posted journal entry behind it, to open it.
    pub entry_id: Option<i64>,
}

/// One reminder category and its items.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReminderGroup {
    /// `OVERDUE_INVOICES`, `BILLS_DUE`, `TODOS`, `RECURRING`, `EMAIL_QUEUE`, `TAX_DATES` or `BACKUP`.
    pub key: &'static str,
    pub label: String,
    /// The items' amounts added up, for groups with amounts.
    pub total_cents: Option<i64>,
    pub items: Vec<ReminderItem>,
}

fn group(key: &'static str, label: String, items: Vec<ReminderItem>) -> ReminderGroup {
    let amounts: Vec<i64> = items.iter().filter_map(|i| i.amount_cents).collect();
    let total_cents = (!amounts.is_empty()).then(|| amounts.iter().sum());
    ReminderGroup { key, label, total_cents, items }
}

fn ahead(as_of: NaiveDate, days: i64) -> String {
    (as_of + Duration::days(days)).to_string()
}

fn days_word(n: i64) -> String {
    if n == 1 { "1 day".into() } else { format!("{n} days") }
}

/// Posted, unvoided invoices with an open balance whose due date is more than `days` days before `as_of`.
pub async fn overdue_invoices(pool: &SqlitePool, as_of: NaiveDate, days: i64) -> Result<Vec<ReminderItem>, String> {
    let rows: Vec<(i64, i64, String, String, i64, Option<i64>)> = sqlx::query_as(
        "SELECT i.id, i.invoice_number, COALESCE(c.name, i.customer_name), i.due_date, b.home_open_cents, i.journal_entry_id
         FROM invoices i JOIN v_invoice_balances b ON b.invoice_id = i.id LEFT JOIN customers c ON c.id = i.customer_id
         WHERE i.is_voided = 0 AND i.status <> 'DRAFT' AND b.home_open_cents > 0
           AND julianday(?1) - julianday(i.due_date) > ?2
         ORDER BY i.due_date, i.invoice_number",
    )
    .bind(as_of.to_string())
    .bind(days)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|(id, number, customer, due, open, entry)| {
            let late = NaiveDate::parse_from_str(&due, "%Y-%m-%d").map(|d| (as_of - d).num_days()).unwrap_or(0);
            ReminderItem {
                id: Some(id),
                label: format!("Invoice {number} · {customer}"),
                detail: format!("{} overdue", days_word(late)),
                date: Some(due),
                amount_cents: Some(open),
                entry_id: entry,
            }
        })
        .collect())
}

/// Posted, unvoided bills still owed that are due on or before `as_of` + `days` (overdue ones included).
pub async fn bills_due(pool: &SqlitePool, as_of: NaiveDate, days: i64) -> Result<Vec<ReminderItem>, String> {
    let rows: Vec<(i64, Option<String>, String, String, i64, Option<i64>)> = sqlx::query_as(
        "SELECT b.id, b.bill_number, v.name, b.due_date, b.total_cents - b.amount_paid_cents, b.journal_entry_id
         FROM bills b JOIN vendors v ON v.id = b.vendor_id
         WHERE b.is_voided = 0 AND b.status IN ('UNPAID', 'PARTIAL') AND b.due_date <= ?1
         ORDER BY b.due_date, v.name COLLATE NOCASE, b.id",
    )
    .bind(ahead(as_of, days))
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|(id, number, vendor, due, owed, entry)| {
            let label = match number.filter(|n| !n.trim().is_empty()) {
                Some(n) => format!("Bill {n} · {vendor}"),
                None => format!("Bill · {vendor}"),
            };
            let detail = match NaiveDate::parse_from_str(&due, "%Y-%m-%d").map(|d| (d - as_of).num_days()) {
                Ok(0) => "Due today".to_string(),
                Ok(n) if n < 0 => format!("{} overdue", days_word(-n)),
                Ok(n) => format!("Due in {}", days_word(n)),
                Err(_) => String::new(),
            };
            ReminderItem { id: Some(id), label, detail, date: Some(due), amount_cents: Some(owed), entry_id: entry }
        })
        .collect())
}

/// Open to-dos due on or before `as_of` + `days`. With users on, the viewer's own and unassigned ones.
pub async fn todos_due(pool: &SqlitePool, as_of: NaiveDate, days: i64, viewer: Viewer) -> Result<Vec<ReminderItem>, String> {
    let rows: Vec<TodoView> = sqlx::query_as(&format!(
        "{TODO_SELECT} WHERE t.is_done = 0 AND t.due_date IS NOT NULL AND t.due_date <= ?1
           AND (?2 IS NULL OR t.assigned_user_id IS NULL OR t.assigned_user_id = ?2)
         ORDER BY t.due_date, t.id"
    ))
    .bind(ahead(as_of, days))
    .bind(viewer.user_id)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|t| {
            let about = [t.customer_name.as_deref(), t.vendor_name.as_deref(), t.employee_name.as_deref(), t.link_label.as_deref()]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            let when = match t.due_date.as_deref().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).map(|d| (d - as_of).num_days()) {
                Some(0) => "Due today".to_string(),
                Some(n) if n < 0 => format!("{} overdue", days_word(-n)),
                Some(n) => format!("Due in {}", days_word(n)),
                None => String::new(),
            };
            ReminderItem {
                id: Some(t.id),
                label: t.title,
                detail: if about.is_empty() { when } else { format!("{when} · {about}") },
                date: t.due_date,
                amount_cents: None,
                entry_id: t.link_entry_id,
            }
        })
        .collect())
}

/// Active recurring transactions whose next date is on or before `as_of` + `days` (and not past their end).
pub async fn recurring_due(pool: &SqlitePool, as_of: NaiveDate, days: i64) -> Result<Vec<ReminderItem>, String> {
    let until = as_of + Duration::days(days);
    Ok(crate::workflows::list_recurring(pool)
        .await?
        .into_iter()
        .filter(|t| t.is_active && t.next_date <= until && t.end_date.is_none_or(|end| t.next_date <= end))
        .map(|t| {
            let kind = match t.kind.as_str() {
                "JOURNAL" => "Journal entry",
                "EXPENSE" => "Expense",
                "BILL" => "Bill",
                "INVOICE" => "Invoice",
                other => other,
            };
            ReminderItem {
                id: Some(t.id),
                label: t.name,
                detail: match t.payee {
                    Some(payee) => format!("{kind} · {payee}"),
                    None => kind.to_string(),
                },
                date: Some(t.next_date.to_string()),
                amount_cents: (t.amount_cents != 0).then_some(t.amount_cents),
                entry_id: None,
            }
        })
        .collect())
}

/// Forms marked Email later and not yet sent.
pub async fn email_queue(pool: &SqlitePool) -> Result<Vec<ReminderItem>, String> {
    Ok(crate::email_log::queue(pool)
        .await?
        .into_iter()
        .map(|q| {
            let kind = match q.kind {
                crate::forms::FormKind::Estimate => "Estimate",
                crate::forms::FormKind::SalesReceipt => "Sales receipt",
                crate::forms::FormKind::Statement => "Statement",
                crate::forms::FormKind::Invoice => "Invoice",
            };
            ReminderItem {
                id: Some(q.document_id),
                label: format!("{kind} {} · {}", q.number, q.customer_name),
                detail: if q.customer_email.is_some() { "Ready to send".into() } else { "No email address on file".into() },
                date: Some(q.date),
                amount_cents: q.currency_code.is_none().then_some(q.total_cents),
                entry_id: None,
            }
        })
        .collect())
}

/// Federal tax due dates for the company's tax form from `as_of` through `as_of` + `days`.
pub async fn tax_dates(pool: &SqlitePool, as_of: NaiveDate, days: i64) -> Result<Vec<ReminderItem>, String> {
    let form = crate::preferences::get(pool, "tax_form").await.unwrap_or_else(|_| "SCHEDULE_C".into());
    let start_month = crate::preferences::fiscal_start_month(pool).await?;
    let until = as_of + Duration::days(days);
    Ok(crate::preferences::tax_calendar(&form, start_month, as_of)
        .into_iter()
        .filter(|d| d.due_date >= as_of && d.due_date <= until)
        .map(|d| {
            let n = (d.due_date - as_of).num_days();
            ReminderItem {
                id: None,
                label: d.label,
                detail: if n == 0 { format!("{} · due today", d.form) } else { format!("{} · due in {}", d.form, days_word(n)) },
                date: Some(d.due_date.to_string()),
                amount_cents: None,
                entry_id: None,
            }
        })
        .collect())
}

/// A backup reminder when the newest backup is more than `days` days old, or there is none.
pub fn backup_due(newest_backup: Option<NaiveDate>, as_of: NaiveDate, days: i64) -> Vec<ReminderItem> {
    let (detail, date) = match newest_backup {
        Some(last) if (as_of - last).num_days() <= days => return Vec::new(),
        Some(last) => (format!("The last backup is {} old", days_word((as_of - last).num_days())), Some(last.to_string())),
        None => ("No backup of this company yet".to_string(), None),
    };
    vec![ReminderItem { id: None, label: "Back up the company file".into(), detail, date, amount_cents: None, entry_id: None }]
}

/// The reminders the viewer's settings ask for, as of `as_of`; categories with nothing in them are
/// left out. `newest_backup` is the day of the newest backup in the backups folder.
pub async fn reminders(pool: &SqlitePool, as_of: NaiveDate, viewer: Viewer, newest_backup: Option<NaiveDate>) -> Result<Vec<ReminderGroup>, String> {
    let s = settings(pool, viewer).await?;
    let mut groups = Vec::new();
    if s.overdue_invoices.show {
        groups.push(group("OVERDUE_INVOICES", "Overdue invoices".into(), overdue_invoices(pool, as_of, s.overdue_invoices.days).await?));
    }
    if s.bills_due.show {
        groups.push(group("BILLS_DUE", format!("Bills to pay (due within {})", days_word(s.bills_due.days)), bills_due(pool, as_of, s.bills_due.days).await?));
    }
    if s.todos.show {
        groups.push(group("TODOS", "To-dos due".into(), todos_due(pool, as_of, s.todos.days, viewer).await?));
    }
    if s.recurring.show {
        groups.push(group("RECURRING", "Recurring transactions to enter".into(), recurring_due(pool, as_of, s.recurring.days).await?));
    }
    if s.email_queue.show {
        groups.push(group("EMAIL_QUEUE", "Forms to email".into(), email_queue(pool).await?));
    }
    if s.tax_dates.show {
        groups.push(group("TAX_DATES", format!("Tax dates (next {})", days_word(s.tax_dates.days)), tax_dates(pool, as_of, s.tax_dates.days).await?));
    }
    if s.backup.show {
        groups.push(group("BACKUP", "Backup".into(), backup_due(newest_backup, as_of, s.backup.days)));
    }
    groups.retain(|g| !g.items.is_empty());
    Ok(groups)
}

#[cfg(test)]
mod tests;
