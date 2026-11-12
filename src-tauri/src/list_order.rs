//! Lists in a custom order: the chart of accounts, items and customers can be dragged into an
//! order that is kept in the company file (`list_order`, migration 0080). The screens arrange the
//! rows (`src/lib/listOrder.js`): siblings in this order, rows without a place after them in the
//! list's usual order. Nothing here touches the books.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::db_err;

/// The lists that keep a custom order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderedList {
    Accounts,
    Items,
    Customers,
}

impl OrderedList {
    fn table(self) -> &'static str {
        match self {
            OrderedList::Accounts => "chart_of_accounts",
            OrderedList::Items => "products_services",
            OrderedList::Customers => "customers",
        }
    }
}

/// The most rows an order can name (far beyond any real list).
const MAX_ROWS: usize = 100_000;

/// The ids with a place, first to last.
pub async fn get(pool: &SqlitePool, list: OrderedList) -> Result<Vec<i64>, String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT id FROM {} WHERE list_order IS NOT NULL ORDER BY list_order, id", list.table())))
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

/// Keeps `ids` as the list's order (0 first); every other row goes back to the usual order. Ids
/// that no longer exist are skipped; an id named twice is refused.
pub async fn set(pool: &SqlitePool, list: OrderedList, ids: &[i64]) -> Result<Vec<i64>, String> {
    if ids.len() > MAX_ROWS {
        return Err("LIST_ORDER_TOO_LONG: that list is too long to keep in order".into());
    }
    let mut seen = HashSet::new();
    if let Some(id) = ids.iter().find(|id| !seen.insert(**id)) {
        return Err(format!("LIST_ORDER_DUPLICATE: row {id} is named twice"));
    }
    let table = list.table();
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE {table} SET list_order = NULL WHERE list_order IS NOT NULL"))).execute(&mut *tx).await.map_err(db_err)?;
    let update = format!("UPDATE {table} SET list_order = ?1 WHERE id = ?2");
    for (place, id) in (0_i64..).zip(ids) {
        sqlx::query(sqlx::AssertSqlSafe(update.as_str())).bind(place).bind(id).execute(&mut *tx).await.map_err(db_err)?;
    }
    crate::ledger::commit_write(tx).await.map_err(db_err)?;
    get(pool, list).await
}

/// Puts the list back in its usual order.
pub async fn reset(pool: &SqlitePool, list: OrderedList) -> Result<(), String> {
    set(pool, list, &[]).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_order_is_kept_replaced_and_reset() {
        let pool = crate::db::test_pool().await;
        for name in ["Acme", "Birch", "Cedar"] {
            sqlx::query("INSERT INTO customers (name) VALUES (?1)").bind(name).execute(&pool).await.unwrap();
        }
        let id = |name: &'static str| {
            let pool = pool.clone();
            async move { sqlx::query_scalar::<_, i64>("SELECT id FROM customers WHERE name = ?1").bind(name).fetch_one(&pool).await.unwrap() }
        };
        let (a, b, c) = (id("Acme").await, id("Birch").await, id("Cedar").await);

        assert!(get(&pool, OrderedList::Customers).await.unwrap().is_empty(), "no custom order to start");
        assert_eq!(set(&pool, OrderedList::Customers, &[c, a, b]).await.unwrap(), vec![c, a, b]);
        // A shorter order: the rest go back to the usual order; a missing id is skipped.
        assert_eq!(set(&pool, OrderedList::Customers, &[b, 99_999]).await.unwrap(), vec![b]);
        assert!(set(&pool, OrderedList::Customers, &[a, a]).await.unwrap_err().starts_with("LIST_ORDER_DUPLICATE"));
        assert_eq!(get(&pool, OrderedList::Customers).await.unwrap(), vec![b], "a refused order changes nothing");
        reset(&pool, OrderedList::Customers).await.unwrap();
        assert!(get(&pool, OrderedList::Customers).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn accounts_and_items_keep_their_own_orders() {
        let pool = crate::db::test_pool().await;
        let accounts: Vec<i64> = sqlx::query_scalar("SELECT id FROM chart_of_accounts ORDER BY id LIMIT 3").fetch_all(&pool).await.unwrap();
        let reversed: Vec<i64> = accounts.iter().rev().copied().collect();
        assert_eq!(set(&pool, OrderedList::Accounts, &reversed).await.unwrap(), reversed);
        assert!(get(&pool, OrderedList::Items).await.unwrap().is_empty());
        // Reordering accounts writes no audit row (it changes no name, number or type).
        let audit: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ledger_audit_log WHERE action_type = 'ACCOUNT_CHANGE'").fetch_one(&pool).await.unwrap();
        set(&pool, OrderedList::Accounts, &accounts).await.unwrap();
        let after: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ledger_audit_log WHERE action_type = 'ACCOUNT_CHANGE'").fetch_one(&pool).await.unwrap();
        assert_eq!(audit, after);
    }
}
