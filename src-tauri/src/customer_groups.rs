//! Customer groups (Customer Center › Groups): named groups of customers, picked by hand or kept up
//! to date by rules, and a statement reminder per group.
//!
//! An automatic group stores its rules, never its members: each time it is read, the customers'
//! current details (open balance, type, terms, rep, the state and ZIP code in the billing address,
//! tax code, credit limit, active status) are checked against every rule, so the members change as
//! soon as a customer's details do. A manual group lists its members (`customer_group_members`).
//! Nothing here touches the books.

use std::collections::HashSet;

use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::db_err;

/// What a rule looks at, and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "field", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Rule {
    /// Open balance at least (or, with `atMost`, at most) this many cents.
    Balance { cents: i64, #[serde(default)] at_most: bool },
    /// The customer type (Customer & Vendor Profile Lists).
    CustomerType { id: i64 },
    /// The terms on the customer record.
    Terms { id: i64 },
    /// The sales rep on the customer record.
    Rep { id: i64 },
    /// The two-letter state (or province) in the billing address, any case.
    State { code: String },
    /// ZIP codes starting with these digits.
    Zip { prefix: String },
    /// The sales tax code; `None` matches tax-exempt customers.
    TaxCode { id: Option<i64> },
    /// `true`: customers over their credit limit; `false`: customers with a credit limit set.
    CreditLimit { over: bool },
    /// Active (or inactive) customers.
    Status { active: bool },
}

/// One customer's details, as the rules see them.
#[derive(Debug, Clone, Default, sqlx::FromRow)]
pub struct CustomerFacts {
    pub id: i64,
    pub balance_cents: i64,
    pub customer_type_id: Option<i64>,
    pub terms_id: Option<i64>,
    pub sales_rep_id: Option<i64>,
    pub billing_address: Option<String>,
    pub tax_rate_id: Option<i64>,
    pub is_tax_exempt: bool,
    pub credit_limit_cents: Option<i64>,
    pub is_active: bool,
}

/// The state and ZIP code from the last line of an address such as "Springfield, OR 97477-1234".
pub fn state_and_zip(address: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(last) = address.unwrap_or_default().lines().map(str::trim).rfind(|l| !l.is_empty()) else { return (None, None) };
    let tokens: Vec<&str> = last.split(|c: char| c == ',' || c.is_whitespace()).filter(|t| !t.is_empty()).collect();
    let Some(zip_at) = tokens.iter().rposition(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit())) else {
        return (None, None);
    };
    let zip = Some(tokens[zip_at].to_string());
    let state = zip_at.checked_sub(1).map(|i| tokens[i]).filter(|t| t.len() == 2 && t.chars().all(|c| c.is_ascii_alphabetic())).map(str::to_ascii_uppercase);
    (state, zip)
}

/// Whether a customer meets one rule.
pub fn meets(rule: &Rule, c: &CustomerFacts) -> bool {
    match rule {
        Rule::Balance { cents, at_most } => {
            if *at_most {
                c.balance_cents <= *cents
            } else {
                c.balance_cents >= *cents
            }
        }
        Rule::CustomerType { id } => c.customer_type_id == Some(*id),
        Rule::Terms { id } => c.terms_id == Some(*id),
        Rule::Rep { id } => c.sales_rep_id == Some(*id),
        Rule::State { code } => state_and_zip(c.billing_address.as_deref()).0.is_some_and(|s| s.eq_ignore_ascii_case(code.trim())),
        Rule::Zip { prefix } => state_and_zip(c.billing_address.as_deref()).1.is_some_and(|z| z.starts_with(prefix.trim())),
        Rule::TaxCode { id: Some(id) } => !c.is_tax_exempt && c.tax_rate_id == Some(*id),
        Rule::TaxCode { id: None } => c.is_tax_exempt,
        Rule::CreditLimit { over: true } => c.credit_limit_cents.is_some_and(|limit| c.balance_cents > limit),
        Rule::CreditLimit { over: false } => c.credit_limit_cents.is_some(),
        Rule::Status { active } => c.is_active == *active,
    }
}

/// Whether a customer meets every rule of a group (a group needs at least one rule).
pub fn meets_all(rules: &[Rule], c: &CustomerFacts) -> bool {
    !rules.is_empty() && rules.iter().all(|r| meets(r, c))
}

/// A group, as the Customer Center lists it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerGroup {
    pub id: i64,
    pub name: String,
    /// Empty for a manual group.
    pub rules: Vec<Rule>,
    pub automatic: bool,
    /// The members right now, by name.
    pub member_ids: Vec<i64>,
    pub member_count: usize,
    pub statement: StatementReminder,
    pub payments: PaymentReminder,
}

/// A group's payment reminder: every `every_days` days, its invoices at least
/// `min_days_overdue` days past due come up to review and send (`payment_reminders.rs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PaymentReminder {
    /// None: no reminder.
    pub every_days: Option<i64>,
    pub min_days_overdue: i64,
    pub last_sent: Option<NaiveDate>,
}

impl Default for PaymentReminder {
    fn default() -> Self {
        PaymentReminder { every_days: None, min_days_overdue: 1, last_sent: None }
    }
}

impl PaymentReminder {
    /// The day the next round is due; today's date when never sent.
    pub fn next_due(&self, today: NaiveDate) -> Option<NaiveDate> {
        let every = self.every_days?;
        Some(self.last_sent.map_or(today, |sent| sent + Duration::days(every)))
    }
}

/// A group's statement reminder: every `every_days` days the group's statements come up in
/// Reminders, until they are marked sent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StatementReminder {
    /// None: no reminder.
    pub every_days: Option<i64>,
    pub last_sent: Option<NaiveDate>,
    /// Statements list only open transactions (otherwise every transaction in the period).
    pub open_only: bool,
    /// The form style the statements use (None: each customer's own).
    pub style_id: Option<i64>,
    /// The email message sent with them (None: the Statement template's).
    pub message: Option<String>,
}

impl StatementReminder {
    /// The day the group's statements are next due; today's date when never sent.
    pub fn next_due(&self, today: NaiveDate) -> Option<NaiveDate> {
        let every = self.every_days?;
        Some(self.last_sent.map_or(today, |sent| sent + Duration::days(every)))
    }
}

/// What a group is saved with.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerGroupInput {
    pub name: String,
    /// Rules for an automatic group; empty for a manual one.
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// A manual group's members.
    #[serde(default)]
    pub member_ids: Vec<i64>,
    #[serde(default)]
    pub statement: StatementReminder,
    #[serde(default)]
    pub payments: PaymentReminder,
}

/// Every customer's details, for the rules.
pub async fn customer_facts(pool: &SqlitePool) -> Result<Vec<CustomerFacts>, String> {
    sqlx::query_as(
        "SELECT c.id,
                COALESCE((SELECT SUM(b.home_open_cents) FROM invoices i JOIN v_invoice_balances b ON b.invoice_id = i.id
                          WHERE i.customer_id = c.id AND i.status IN ('UNPAID', 'PARTIAL') AND i.is_voided = 0), 0)
                - COALESCE((SELECT SUM(remaining_cents) FROM v_credit_memo_balances WHERE customer_id = c.id), 0) AS balance_cents,
                c.customer_type_id, c.terms_id, c.sales_rep_id, c.billing_address, c.tax_rate_id, c.is_tax_exempt, c.credit_limit_cents, c.is_active
         FROM customers c ORDER BY c.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

type GroupRow = (i64, String, Option<String>, Option<i64>, Option<NaiveDate>, bool, Option<i64>, Option<String>, Option<i64>, i64, Option<NaiveDate>);

async fn build(pool: &SqlitePool, rows: Vec<GroupRow>) -> Result<Vec<CustomerGroup>, String> {
    let facts = customer_facts(pool).await?;
    let manual: Vec<(i64, i64)> = sqlx::query_as("SELECT m.group_id, m.customer_id FROM customer_group_members m JOIN customers c ON c.id = m.customer_id ORDER BY c.name COLLATE NOCASE")
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    let mut out = Vec::with_capacity(rows.len());
    for (id, name, rules_json, every_days, last_sent, open_only, style_id, message, pay_every, pay_min, pay_last) in rows {
        // Rules a newer version wrote that this one can't read leave the group empty, not broken.
        let rules: Vec<Rule> = rules_json.as_deref().map(|j| serde_json::from_str(j).unwrap_or_default()).unwrap_or_default();
        let automatic = rules_json.is_some();
        let member_ids: Vec<i64> = if automatic {
            facts.iter().filter(|c| meets_all(&rules, c)).map(|c| c.id).collect()
        } else {
            manual.iter().filter(|(g, _)| *g == id).map(|(_, c)| *c).collect()
        };
        out.push(CustomerGroup {
            id,
            name,
            rules,
            automatic,
            member_count: member_ids.len(),
            member_ids,
            statement: StatementReminder { every_days, last_sent, open_only, style_id, message },
            payments: PaymentReminder { every_days: pay_every, min_days_overdue: pay_min, last_sent: pay_last },
        });
    }
    Ok(out)
}

const SELECT: &str = "SELECT id, name, rules_json, statement_every_days, statement_last_sent, statement_open_only, statement_style_id, statement_message,
    payment_every_days, payment_min_days_overdue, payment_last_sent FROM customer_groups";

pub async fn list(pool: &SqlitePool) -> Result<Vec<CustomerGroup>, String> {
    let rows: Vec<GroupRow> = sqlx::query_as(&format!("{SELECT} ORDER BY name COLLATE NOCASE")).fetch_all(pool).await.map_err(db_err)?;
    build(pool, rows).await
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<CustomerGroup, String> {
    let rows: Vec<GroupRow> = sqlx::query_as(&format!("{SELECT} WHERE id = ?1")).bind(id).fetch_all(pool).await.map_err(db_err)?;
    build(pool, rows).await?.pop().ok_or_else(|| "GROUP_NOT_FOUND: that customer group no longer exists".into())
}

fn check(input: &CustomerGroupInput) -> Result<(), String> {
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err("GROUP_NAME: give the group a name of up to 60 characters".into());
    }
    for rule in &input.rules {
        let bad = match rule {
            // As `state_and_zip` reads them: two letters, and a ZIP code that starts with a digit.
            Rule::State { code } => code.trim().len() != 2 || !code.trim().chars().all(|c| c.is_ascii_alphabetic()),
            Rule::Zip { prefix } => !prefix.trim().starts_with(|c: char| c.is_ascii_digit()) || !prefix.trim().chars().all(|c| c.is_ascii_digit() || c == '-'),
            Rule::Balance { cents, .. } => cents.unsigned_abs() > 100_000_000_000_000,
            _ => false,
        };
        if bad {
            return Err("GROUP_BAD_RULE: a state is two letters, and a ZIP code rule needs its first digits (digits and dashes)".into());
        }
    }
    if let Some(days) = input.statement.every_days {
        if !(1..=366).contains(&days) {
            return Err("GROUP_BAD_SCHEDULE: send statements every 1 to 366 days".into());
        }
    }
    if input.payments.every_days.is_some_and(|d| !(1..=366).contains(&d)) || !(0..=365).contains(&input.payments.min_days_overdue) {
        return Err("GROUP_BAD_SCHEDULE: send payment reminders every 1 to 366 days, for invoices 0 to 365 days overdue".into());
    }
    if input.statement.message.as_deref().is_some_and(|m| m.chars().count() > 2000) {
        return Err("GROUP_MESSAGE_LONG: the statement message can be at most 2,000 characters".into());
    }
    Ok(())
}

/// Adds a group (`id` None) or replaces one. A group with rules is automatic; its member list is
/// ignored.
pub async fn save(pool: &SqlitePool, id: Option<i64>, input: &CustomerGroupInput) -> Result<CustomerGroup, String> {
    check(input)?;
    let rules_json = (!input.rules.is_empty()).then(|| serde_json::to_string(&input.rules)).transpose().map_err(|e| format!("GROUP_SAVE: {e}"))?;
    let s = &input.statement;
    let message = s.message.as_deref().map(str::trim).filter(|m| !m.is_empty());
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let duplicate = |e: sqlx::Error| {
        let msg = db_err(e);
        if msg.contains("UNIQUE") { format!("GROUP_DUPLICATE: there is already a group named '{}'", input.name.trim()) } else { msg }
    };
    let id = match id {
        Some(id) => {
            let done = sqlx::query(
                "UPDATE customer_groups SET name = ?1, rules_json = ?2, statement_every_days = ?3, statement_last_sent = ?4, statement_open_only = ?5,
                        statement_style_id = ?6, statement_message = ?7, payment_every_days = ?9, payment_min_days_overdue = ?10, payment_last_sent = ?11
                 WHERE id = ?8",
            )
            .bind(input.name.trim())
            .bind(&rules_json)
            .bind(s.every_days)
            .bind(s.last_sent)
            .bind(s.open_only)
            .bind(s.style_id)
            .bind(message)
            .bind(id)
            .bind(input.payments.every_days)
            .bind(input.payments.min_days_overdue)
            .bind(input.payments.last_sent)
            .execute(&mut *tx)
            .await
            .map_err(duplicate)?;
            if done.rows_affected() == 0 {
                return Err("GROUP_NOT_FOUND: that customer group no longer exists".into());
            }
            id
        }
        None => sqlx::query_scalar(
            "INSERT INTO customer_groups (name, rules_json, statement_every_days, statement_last_sent, statement_open_only, statement_style_id, statement_message,
                                          payment_every_days, payment_min_days_overdue, payment_last_sent)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) RETURNING id",
        )
        .bind(input.name.trim())
        .bind(&rules_json)
        .bind(s.every_days)
        .bind(s.last_sent)
        .bind(s.open_only)
        .bind(s.style_id)
        .bind(message)
        .bind(input.payments.every_days)
        .bind(input.payments.min_days_overdue)
        .bind(input.payments.last_sent)
        .fetch_one(&mut *tx)
        .await
        .map_err(duplicate)?,
    };
    sqlx::query("DELETE FROM customer_group_members WHERE group_id = ?1").bind(id).execute(&mut *tx).await.map_err(db_err)?;
    if rules_json.is_none() {
        let mut seen = HashSet::new();
        for customer in input.member_ids.iter().filter(|c| seen.insert(**c)) {
            sqlx::query("INSERT INTO customer_group_members (group_id, customer_id) SELECT ?1, id FROM customers WHERE id = ?2")
                .bind(id)
                .bind(customer)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    get(pool, id).await
}

pub async fn delete(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let done = sqlx::query("DELETE FROM customer_groups WHERE id = ?1").bind(id).execute(pool).await.map_err(db_err)?;
    if done.rows_affected() == 0 {
        return Err("GROUP_NOT_FOUND: that customer group no longer exists".into());
    }
    Ok(())
}

/// Marks the group's statements sent on `date`, which starts the wait for the next ones.
pub async fn mark_statements_sent(pool: &SqlitePool, id: i64, date: NaiveDate) -> Result<CustomerGroup, String> {
    let done = sqlx::query("UPDATE customer_groups SET statement_last_sent = ?1 WHERE id = ?2").bind(date).bind(id).execute(pool).await.map_err(db_err)?;
    if done.rows_affected() == 0 {
        return Err("GROUP_NOT_FOUND: that customer group no longer exists".into());
    }
    get(pool, id).await
}

/// Groups whose statements are due on or before `as_of` and that have members.
pub async fn statements_due(pool: &SqlitePool, as_of: NaiveDate) -> Result<Vec<(CustomerGroup, NaiveDate)>, String> {
    Ok(list(pool)
        .await?
        .into_iter()
        .filter(|g| g.member_count > 0)
        .filter_map(|g| g.statement.next_due(as_of).filter(|due| *due <= as_of).map(|due| (g, due)))
        .collect())
}

#[cfg(test)]
mod tests;
