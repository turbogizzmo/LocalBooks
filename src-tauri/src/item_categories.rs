//! Item categories (gap T40; migration 0105): a Category List with sub-categories, and a category
//! on each item. They group items for the item list, the item box on forms and the item reports,
//! and post nothing. An item with no category is "Uncategorized".

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::{db_err, rollback};

/// Turns categories on or off (Preferences › Items & Inventory); off hides them, keeping them.
pub const PREF_KEY: &str = "item_categories";

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: i64,
    pub name: String,
    /// "Parent:Child", for lists and filters.
    pub full_name: String,
    pub parent_id: Option<i64>,
    pub depth: i64,
    pub is_active: bool,
    /// Items directly in this category (not its sub-categories).
    pub item_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryInput {
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<i64>,
}

/// Every category, parents before their sub-categories, each level by name.
pub async fn list_categories(pool: &SqlitePool) -> Result<Vec<Category>, String> {
    sqlx::query_as::<_, Category>(
        "WITH RECURSIVE tree (id, name, full_name, parent_id, depth, is_active, sort_key) AS (
             SELECT id, name, name, parent_id, 0, is_active, lower(name) FROM item_categories WHERE parent_id IS NULL
             UNION ALL
             SELECT c.id, c.name, t.full_name || ':' || c.name, c.parent_id, t.depth + 1, c.is_active, t.sort_key || char(1) || lower(c.name)
             FROM item_categories c JOIN tree t ON c.parent_id = t.id WHERE t.depth < 64
         )
         SELECT t.id, t.name, t.full_name, t.parent_id, t.depth, t.is_active,
                (SELECT COUNT(*) FROM products_services p WHERE p.category_id = t.id) AS item_count
         FROM tree t ORDER BY t.sort_key",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

async fn category(pool: &SqlitePool, id: i64) -> Result<Category, String> {
    list_categories(pool).await?.into_iter().find(|c| c.id == id).ok_or_else(|| "CATEGORY_NOT_FOUND: that category does not exist".to_string())
}

fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("CATEGORY_NO_NAME: enter a category name".into());
    }
    if name.contains(':') {
        return Err("CATEGORY_BAD_NAME: a category name can't contain a colon; choose a parent category for a sub-category".into());
    }
    if name.chars().count() > 60 {
        return Err("CATEGORY_TOO_LONG: a category name is at most 60 characters".into());
    }
    Ok(name.to_string())
}

fn unique(err: sqlx::Error, name: &str) -> String {
    match err {
        sqlx::Error::Database(db) if db.message().contains("UNIQUE") => format!("CATEGORY_DUPLICATE: there is already a category named '{name}' there"),
        sqlx::Error::Database(db) if db.message().contains("CATEGORY_") => db.message().to_string(),
        other => db_err(other),
    }
}

async fn check_parent(pool: &SqlitePool, parent_id: Option<i64>) -> Result<(), String> {
    if let Some(parent) = parent_id {
        let active: Option<bool> = sqlx::query_scalar("SELECT is_active FROM item_categories WHERE id = ?1").bind(parent).fetch_optional(pool).await.map_err(db_err)?;
        match active {
            None => return Err("CATEGORY_NOT_FOUND: that parent category does not exist".into()),
            Some(false) => return Err("CATEGORY_INACTIVE: that parent category is inactive".into()),
            Some(true) => {}
        }
    }
    Ok(())
}

pub async fn create_category(pool: &SqlitePool, input: CategoryInput) -> Result<Category, String> {
    let name = clean_name(&input.name)?;
    check_parent(pool, input.parent_id).await?;
    let id: i64 = sqlx::query_scalar("INSERT INTO item_categories (name, parent_id) VALUES (?1, ?2) RETURNING id")
        .bind(&name)
        .bind(input.parent_id)
        .fetch_one(pool)
        .await
        .map_err(|e| unique(e, &name))?;
    category(pool, id).await
}

/// Renames a category or moves it under another (or to the top, with no parent).
pub async fn update_category(pool: &SqlitePool, id: i64, input: CategoryInput) -> Result<Category, String> {
    let name = clean_name(&input.name)?;
    let current: Option<Option<i64>> = sqlx::query_scalar("SELECT parent_id FROM item_categories WHERE id = ?1").bind(id).fetch_optional(pool).await.map_err(db_err)?;
    let current = current.ok_or("CATEGORY_NOT_FOUND: that category does not exist")?;
    if input.parent_id != current {
        check_parent(pool, input.parent_id).await?;
    }
    sqlx::query("UPDATE item_categories SET name = ?1, parent_id = ?2 WHERE id = ?3")
        .bind(&name)
        .bind(input.parent_id)
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| unique(e, &name))?;
    category(pool, id).await
}

/// Makes a category inactive (its items keep it) or active again. Inactive, it is left out of the
/// lists to pick from; a sub-category of an inactive one is made inactive too.
pub async fn set_category_active(pool: &SqlitePool, id: i64, active: bool) -> Result<Category, String> {
    let rows = sqlx::query(
        "WITH RECURSIVE down (id) AS (SELECT ?1 UNION ALL SELECT c.id FROM item_categories c JOIN down ON c.parent_id = down.id)
         UPDATE item_categories SET is_active = ?2 WHERE id IN (SELECT id FROM down) AND (?2 = 0 OR id = ?1)",
    )
    .bind(id)
    .bind(active)
    .execute(pool)
    .await
    .map_err(db_err)?;
    if rows.rows_affected() == 0 {
        return Err("CATEGORY_NOT_FOUND: that category does not exist".into());
    }
    category(pool, id).await
}

/// Deletes a category that no item and no sub-category uses.
pub async fn delete_category(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let (items, children): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM products_services WHERE category_id = ?1), (SELECT COUNT(*) FROM item_categories WHERE parent_id = ?1)",
    )
    .bind(id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    if items > 0 {
        return Err(format!("CATEGORY_IN_USE: {items} item{} use this category; move them to another first", if items == 1 { "" } else { "s" }));
    }
    if children > 0 {
        return Err("CATEGORY_IN_USE: this category has sub-categories; move or delete them first".into());
    }
    let done = sqlx::query("DELETE FROM item_categories WHERE id = ?1").bind(id).execute(pool).await.map_err(db_err)?;
    if done.rows_affected() == 0 {
        return Err("CATEGORY_NOT_FOUND: that category does not exist".into());
    }
    Ok(())
}

/// Moves items to a category (`None`: Uncategorized). All move, or none do.
pub async fn move_items(pool: &SqlitePool, item_ids: &[i64], category_id: Option<i64>) -> Result<usize, String> {
    if item_ids.is_empty() {
        return Err("CATEGORY_NO_ITEMS: choose the items to move".into());
    }
    check_parent(pool, category_id).await.map_err(|e| e.replace("parent category", "category"))?;
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    for id in item_ids {
        let moved = sqlx::query("UPDATE products_services SET category_id = ?1 WHERE id = ?2").bind(category_id).bind(id).execute(&mut *tx).await;
        match moved {
            Ok(r) if r.rows_affected() == 1 => {}
            Ok(_) => return Err(rollback(tx, format!("PRODUCT_NOT_FOUND: item {id} does not exist")).await),
            Err(err) => return Err(rollback(tx, db_err(err)).await),
        }
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    Ok(item_ids.len())
}

#[cfg(test)]
mod tests;
