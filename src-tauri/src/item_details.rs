//! The rest of the item record and list (gap T43; migration 0108): an item's purchase description
//! and cost, making several items active or inactive at once, and adding or changing many items
//! from rows pasted from a spreadsheet (Add/Edit Multiple Items).

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::{db_err, rollback, MAX_LINE_CENTS};

fn clean(text: Option<&str>) -> Option<String> {
    text.map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// Sets what purchase forms say for an item and what it costs to buy; `None` clears either.
pub async fn set_purchase_details(pool: &SqlitePool, product_id: i64, description: Option<&str>, cost_cents: Option<i64>) -> Result<(), String> {
    let description = clean(description);
    if description.as_ref().is_some_and(|d| d.chars().count() > 4000) {
        return Err("ITEM_PURCHASE_TOO_LONG: a purchase description is at most 4,000 characters".into());
    }
    if cost_cents.is_some_and(|c| !(0..=MAX_LINE_CENTS).contains(&c)) {
        return Err("ITEM_BAD_COST: the cost must be zero or more".into());
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let done = sqlx::query("UPDATE products_services SET purchase_description = ?1, purchase_cost_cents = ?2 WHERE id = ?3")
        .bind(&description)
        .bind(cost_cents)
        .bind(product_id)
        .execute(&mut *tx)
        .await;
    match done {
        Ok(d) if d.rows_affected() == 1 => crate::ledger::commit_write(tx).await.map_err(db_err),
        Ok(_) => Err(rollback(tx, format!("ITEM_NOT_FOUND: product or service {product_id} does not exist")).await),
        Err(e) => Err(rollback(tx, db_err(e)).await),
    }
}

/// Makes the ticked items active or inactive together. Returns how many changed.
pub async fn set_items_active(pool: &SqlitePool, ids: &[i64], active: bool) -> Result<usize, String> {
    if ids.is_empty() {
        return Err("ITEM_NONE_TICKED: tick the items first".into());
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let mut changed = 0;
    for id in ids {
        match sqlx::query("UPDATE products_services SET is_active = ?1 WHERE id = ?2 AND is_active <> ?1").bind(active).bind(id).execute(&mut *tx).await {
            Ok(d) => changed += d.rows_affected() as usize,
            Err(e) => return Err(rollback(tx, db_err(e)).await),
        }
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(changed)
}

/// One row pasted into Add/Edit Multiple Items: an item to add, or (by its name) to change.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PastedItem {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// The sales price as typed ("12.50"); empty leaves an item's price, or 0 for a new one.
    #[serde(default)]
    pub rate: Option<String>,
    /// The income account's name or number; empty uses the first income account for a new item.
    #[serde(default)]
    pub income_account: Option<String>,
    #[serde(default)]
    pub purchase_description: Option<String>,
    #[serde(default)]
    pub cost: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PasteProblem {
    /// 1 for the first pasted row.
    pub row: usize,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasteResult {
    pub added: usize,
    pub updated: usize,
    /// Every row that can't be saved; while there are any, nothing is.
    pub problems: Vec<PasteProblem>,
}

struct Ready {
    existing: Option<i64>,
    name: String,
    description: Option<String>,
    rate: Option<i64>,
    account: Option<i64>,
    purchase_description: Option<String>,
    cost: Option<i64>,
}

fn money(raw: Option<&str>, label: &str, row: usize, problems: &mut Vec<PasteProblem>) -> Option<i64> {
    let raw = raw.map(str::trim).filter(|s| !s.is_empty())?;
    match crate::bank::parse_amount_cents(raw) {
        Ok(c) if (0..=MAX_LINE_CENTS).contains(&c) => Some(c),
        _ => {
            problems.push(PasteProblem { row, message: format!("The {label} \"{raw}\" isn't an amount of zero or more") });
            None
        }
    }
}

/// Adds the pasted rows that name new items and changes the ones that name existing items (only
/// the cells that are filled in). Every row is checked first; nothing is saved while any fails.
pub async fn paste_items(pool: &SqlitePool, rows: &[PastedItem]) -> Result<PasteResult, String> {
    let mut problems = Vec::new();
    let mut ready = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let fallback: Option<i64> = sqlx::query_scalar(
        "SELECT a.id FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type
         WHERE a.is_active = 1 AND t.classification = 'INCOME' ORDER BY a.account_type <> 'INCOME', a.account_number, a.id LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    for (i, r) in rows.iter().enumerate() {
        let row = i + 1;
        let Some(name) = clean(Some(&r.name)) else {
            problems.push(PasteProblem { row, message: "The name is empty".into() });
            continue;
        };
        if seen.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
            problems.push(PasteProblem { row, message: format!("{name} is pasted twice") });
            continue;
        }
        seen.push(name.clone());
        let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM products_services WHERE name = ?1 COLLATE NOCASE").bind(&name).fetch_optional(pool).await.map_err(db_err)?;
        let rate = money(r.rate.as_deref(), "price", row, &mut problems);
        let cost = money(r.cost.as_deref(), "cost", row, &mut problems);
        let account = match clean(r.income_account.as_deref()) {
            Some(a) => {
                let found: Option<i64> = sqlx::query_scalar(
                    "SELECT c.id FROM chart_of_accounts c JOIN account_types t ON t.code = c.account_type
                     WHERE (c.name = ?1 COLLATE NOCASE OR c.account_number = ?1) AND t.classification = 'INCOME' AND c.is_active = 1",
                )
                .bind(&a)
                .fetch_optional(pool)
                .await
                .map_err(db_err)?;
                if found.is_none() {
                    problems.push(PasteProblem { row, message: format!("\"{a}\" isn't an active income account") });
                }
                found
            }
            None if existing.is_none() && fallback.is_none() => {
                problems.push(PasteProblem { row, message: "Add an income account before adding items".into() });
                None
            }
            None => None,
        };
        ready.push(Ready {
            existing,
            name,
            description: clean(r.description.as_deref()),
            rate,
            account,
            purchase_description: clean(r.purchase_description.as_deref()),
            cost,
        });
    }
    if !problems.is_empty() {
        return Ok(PasteResult { problems, ..Default::default() });
    }
    let mut out = PasteResult::default();
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    for r in &ready {
        let result = match r.existing {
            Some(id) => {
                out.updated += 1;
                sqlx::query(
                    "UPDATE products_services SET description = COALESCE(?2, description), rate_cents = COALESCE(?3, rate_cents),
                            income_account_id = COALESCE(?4, income_account_id), purchase_description = COALESCE(?5, purchase_description),
                            purchase_cost_cents = COALESCE(?6, purchase_cost_cents)
                     WHERE id = ?1",
                )
                .bind(id)
                .bind(&r.description)
                .bind(r.rate)
                .bind(r.account)
                .bind(&r.purchase_description)
                .bind(r.cost)
                .execute(&mut *tx)
                .await
            }
            None => {
                out.added += 1;
                sqlx::query(
                    "INSERT INTO products_services (name, description, rate_cents, income_account_id, purchase_description, purchase_cost_cents)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                )
                .bind(&r.name)
                .bind(&r.description)
                .bind(r.rate.unwrap_or(0))
                .bind(r.account.or(fallback))
                .bind(&r.purchase_description)
                .bind(r.cost)
                .execute(&mut *tx)
                .await
            }
        };
        if let Err(e) = result {
            return Err(rollback(tx, format!("{} ({})", db_err(e), r.name)).await);
        }
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(out)
}

#[cfg(test)]
mod tests;
