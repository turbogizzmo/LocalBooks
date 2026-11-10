use super::*;
use crate::db::test_pool;
use crate::sales::{create_product, list_products, ProductInput};

fn input(name: &str, parent_id: Option<i64>) -> CategoryInput {
    CategoryInput { name: name.into(), parent_id }
}

async fn item(pool: &SqlitePool, name: &str) -> i64 {
    let income: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(pool).await.unwrap();
    create_product(pool, ProductInput { name: name.into(), description: None, rate_cents: 1_000, income_account_id: income, is_taxable: true }).await.unwrap().id
}

async fn category_of(pool: &SqlitePool, id: i64) -> Option<i64> {
    list_products(pool).await.unwrap().into_iter().find(|p| p.id == id).unwrap().category_id
}

#[tokio::test]
async fn categories_nest_hold_items_and_move_them() {
    let pool = test_pool().await;
    let tools = create_category(&pool, input(" Tools ", None)).await.unwrap();
    let power = create_category(&pool, input("Power", Some(tools.id))).await.unwrap();
    let paint = create_category(&pool, input("Paint", None)).await.unwrap();
    assert_eq!((power.full_name.as_str(), power.depth), ("Tools:Power", 1));
    let names: Vec<String> = list_categories(&pool).await.unwrap().into_iter().map(|c| c.full_name).collect();
    assert_eq!(names, ["Paint", "Tools", "Tools:Power"], "parents first, each level by name");

    // A name is used once under the same parent, and has no colon.
    assert!(create_category(&pool, input("power", Some(tools.id))).await.unwrap_err().starts_with("CATEGORY_DUPLICATE"));
    assert!(create_category(&pool, input("Power", None)).await.is_ok(), "the same name under another parent is fine");
    assert!(create_category(&pool, input("A:B", None)).await.unwrap_err().starts_with("CATEGORY_BAD_NAME"));
    // No category goes under itself or its own sub-category.
    let err = update_category(&pool, tools.id, input("Tools", Some(power.id))).await.unwrap_err();
    assert!(err.starts_with("CATEGORY_CYCLE"), "{err}");

    // Items start uncategorized and move together.
    let (drill, saw, brush) = (item(&pool, "Drill").await, item(&pool, "Saw").await, item(&pool, "Brush").await);
    assert_eq!(category_of(&pool, drill).await, None);
    assert_eq!(move_items(&pool, &[drill, saw], Some(power.id)).await.unwrap(), 2);
    move_items(&pool, &[brush], Some(paint.id)).await.unwrap();
    assert_eq!((category_of(&pool, drill).await, category_of(&pool, brush).await), (Some(power.id), Some(paint.id)));
    assert!(move_items(&pool, &[saw, 9_999], Some(paint.id)).await.unwrap_err().starts_with("PRODUCT_NOT_FOUND"));
    assert_eq!(category_of(&pool, saw).await, Some(power.id), "a failed move moves nothing");
    let counts: Vec<(String, i64)> = list_categories(&pool).await.unwrap().into_iter().map(|c| (c.full_name, c.item_count)).collect();
    assert!(counts.contains(&("Tools:Power".into(), 2)) && counts.contains(&("Tools".into(), 0)));

    // Renaming a parent renames the path; a used category can't be deleted, an empty one can.
    let renamed = update_category(&pool, power.id, input("Power tools", Some(tools.id))).await.unwrap();
    assert_eq!(renamed.full_name, "Tools:Power tools");
    assert!(delete_category(&pool, power.id).await.unwrap_err().starts_with("CATEGORY_IN_USE"));
    assert!(delete_category(&pool, tools.id).await.unwrap_err().starts_with("CATEGORY_IN_USE"));
    move_items(&pool, &[drill, saw], None).await.unwrap();
    delete_category(&pool, power.id).await.unwrap();
    assert_eq!(category_of(&pool, drill).await, None);

    // Inactive takes its sub-categories with it, and can't take new items or sub-categories.
    let brushes = create_category(&pool, input("Brushes", Some(paint.id))).await.unwrap();
    set_category_active(&pool, paint.id, false).await.unwrap();
    assert!(!category(&pool, brushes.id).await.unwrap().is_active);
    assert!(move_items(&pool, &[saw], Some(paint.id)).await.unwrap_err().starts_with("CATEGORY_INACTIVE"));
    assert!(create_category(&pool, input("Rollers", Some(paint.id))).await.unwrap_err().starts_with("CATEGORY_INACTIVE"));
    assert_eq!(category_of(&pool, brush).await, Some(paint.id), "its items keep it");

    // Moving an item is in the audit trail.
    let logged: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ledger_audit_log WHERE action_type = 'LIST_CHANGE' AND payload_snapshot LIKE '%category%'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(logged >= 4, "{logged}");
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}
