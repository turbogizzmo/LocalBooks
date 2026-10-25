use super::*;
use crate::db::test_pool;

fn input(name: &str, number: Option<&str>, account_type: &str, parent_id: Option<i64>) -> AccountInput {
    AccountInput { name: name.into(), account_number: number.map(Into::into), account_type: account_type.into(), parent_id, description: None }
}

#[tokio::test]
async fn accounts_are_created_edited_and_inactivated_within_the_rules() {
    let pool = test_pool().await;
    let utilities = create_account(&pool, input("Utilities 2", Some("6510"), "EXPENSE", None)).await.unwrap();
    let water = create_account(&pool, input("Water", None, "EXPENSE", Some(utilities.id))).await.unwrap();
    assert_eq!(water.full_name, "Utilities 2:Water");

    let dup = create_account(&pool, input("Other", Some("6510"), "EXPENSE", None)).await.unwrap_err();
    assert!(dup.starts_with("COA_DUPLICATE_NUMBER"), "{dup}");
    let dup = create_account(&pool, input("water", None, "EXPENSE", Some(utilities.id))).await.unwrap_err();
    assert!(dup.starts_with("COA_DUPLICATE_NAME"), "{dup}");
    let mismatch = create_account(&pool, input("Grant", None, "INCOME", Some(utilities.id))).await.unwrap_err();
    assert!(mismatch.starts_with("COA_PARENT_TYPE_MISMATCH"), "{mismatch}");

    let renamed = update_account(&pool, water.id, AccountInput { description: Some(" City water ".into()), ..input("Water & Sewer", Some("6511"), "EXPENSE", Some(utilities.id)) }).await.unwrap();
    assert_eq!((renamed.full_name.as_str(), renamed.account_number.as_deref(), renamed.description.as_deref()), ("Utilities 2:Water & Sewer", Some("6511"), Some("City water")));

    assert!(set_account_active(&pool, utilities.id, false).await.unwrap_err().starts_with("COA_HAS_ACTIVE_SUBACCOUNTS"));
    set_account_active(&pool, water.id, false).await.unwrap();
    assert!(!set_account_active(&pool, utilities.id, false).await.unwrap().is_active);
    assert!(set_account_active(&pool, water.id, true).await.unwrap_err().starts_with("COA_PARENT_INACTIVE"));

    let ar: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = 'ACCOUNTS_RECEIVABLE' AND is_system = 1").fetch_one(&pool).await.unwrap();
    let err = set_account_active(&pool, ar, false).await.unwrap_err();
    assert!(err.starts_with("COA_SYSTEM_ACCOUNT"), "{err}");
}
