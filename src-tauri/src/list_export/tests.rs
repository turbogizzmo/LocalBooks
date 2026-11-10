use super::*;

async fn names(pool: &SqlitePool, sql: &str) -> Vec<String> {
    let mut v: Vec<String> = sqlx::query_scalar(sql).fetch_all(pool).await.unwrap();
    v.sort_by_key(|n| n.to_lowercase());
    v
}

#[tokio::test]
async fn the_lists_read_back_through_the_iif_import() {
    let pool = crate::harness::fixture::harbor_lane().await;
    // A name with quotes, a company with a tab, and notes over two lines survive as one field each
    // (the tab and line break become spaces).
    sqlx::query("UPDATE customers SET name = 'Joe''s \"Best\" Diner', company_name = 'Joe' || char(9) || 'Diner LLC', notes = 'Line one\nLine two' WHERE id = (SELECT MIN(id) FROM customers)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE vendors SET is_1099 = 1, tax_id = '12-3456789' WHERE id = (SELECT MIN(id) FROM vendors)").execute(&pool).await.unwrap();

    let text = lists_iif(&pool).await.unwrap();
    assert!(text.lines().all(|l| !l.contains('\n')), "one record a line");
    let ledger = crate::iif_import::read(text.as_bytes()).unwrap();

    // Every account, by full name and type.
    let mut accounts: Vec<(String, &str)> = ledger.accounts.iter().map(|a| (a.full_name.clone(), a.account_type)).collect();
    accounts.sort_by_key(|a| a.0.to_lowercase());
    let expected: Vec<(String, String)> = sqlx::query_as(
        "SELECT v.full_name, v.account_type FROM v_chart_of_accounts v JOIN chart_of_accounts c ON c.id = v.id WHERE c.merged_into_id IS NULL ORDER BY lower(v.full_name)",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(accounts.iter().map(|(n, t)| (n.clone(), t.to_string())).collect::<Vec<_>>(), expected);

    // Customers (jobs as Customer:Job), vendors and items by name.
    let mut customers: Vec<String> = ledger.customers.iter().map(|p| p.name.clone()).collect();
    customers.sort_by_key(|n| n.to_lowercase());
    assert_eq!(customers, names(&pool, "WITH RECURSIVE p (id, n) AS (SELECT id, name FROM customers WHERE parent_id IS NULL UNION ALL SELECT c.id, p.n || ':' || c.name FROM customers c JOIN p ON c.parent_id = p.id) SELECT n FROM p").await);
    assert!(customers.contains(&"Joe's \"Best\" Diner".to_string()));
    let joe = ledger.customers.iter().find(|p| p.name.starts_with("Joe's")).unwrap();
    assert_eq!(joe.notes.as_deref(), Some("Line one Line two"));
    assert_eq!(joe.company_name.as_deref(), Some("Joe Diner LLC"), "a tab can't split the row");
    let mut vendors: Vec<String> = ledger.vendors.iter().map(|p| p.name.clone()).collect();
    vendors.sort_by_key(|n| n.to_lowercase());
    assert_eq!(vendors, names(&pool, "SELECT name FROM vendors").await);
    assert!(ledger.vendors.iter().any(|v| v.is_1099 && v.tax_id.as_deref() == Some("12-3456789")));

    // Items the import keeps (services, parts, other charges) with their prices.
    let products: Vec<(String, i64)> = sqlx::query_as("SELECT name, rate_cents FROM products_services WHERE item_type <> 'DISCOUNT' AND percent_ppm IS NULL").fetch_all(&pool).await.unwrap();
    for (name, rate) in products {
        let item = ledger.items.iter().find(|i| i.name == name).unwrap_or_else(|| panic!("{name} exported"));
        assert_eq!(item.rate_cents, rate.abs(), "{name}");
    }
}

#[tokio::test]
async fn addresses_are_one_row_a_name() {
    let pool = crate::harness::fixture::harbor_lane().await;
    sqlx::query("UPDATE customers SET billing_address = '1 Main St\nSuite 2\nPortland, OR 97201' WHERE id = (SELECT MIN(id) FROM customers)").execute(&pool).await.unwrap();
    let text = addresses_text(&pool).await.unwrap();
    let rows: Vec<Vec<&str>> = text.lines().map(|l| l.split('\t').collect()).collect();
    assert_eq!(rows[0][..3], ["Type", "Name", "Company"]);
    assert!(rows.iter().all(|r| r.len() == 10), "every row has every column");
    let active: i64 = sqlx::query_scalar("SELECT (SELECT COUNT(*) FROM customers WHERE is_active = 1 AND merged_into_id IS NULL) + (SELECT COUNT(*) FROM vendors WHERE is_active = 1 AND merged_into_id IS NULL)").fetch_one(&pool).await.unwrap();
    assert_eq!(rows.len() as i64 - 1, active);
    assert!(rows.iter().any(|r| r[3..6] == ["1 Main St", "Suite 2", "Portland, OR 97201"]));
}
