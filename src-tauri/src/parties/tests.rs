use super::*;
use crate::db::test_pool;

#[tokio::test]
async fn protected_tax_ids_need_the_key_to_see_or_change() {
    let pool = test_pool().await;
    let (admin, _) = crate::users::turn_on(&pool, "Owner", None, "owner password").await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let with_key = Access::of(&mut conn, admin.key.as_ref()).await.unwrap();
    let without = Access::of(&mut conn, None).await.unwrap();
    drop(conn);
    let staples = crate::expenses::create_vendor(&pool, "Staples", None).await.unwrap();
    let input = |tax_id: Option<&str>, keep: bool| PartyDetailsInput {
        name: "Staples".into(),
        company_name: None,
        email: None,
        phone: None,
        address: None,
        terms_days: None,
        notes: None,
        tax_id: tax_id.map(str::to_string),
        keep_tax_id: keep,
        is_1099: true,
        tax_rate_id: None,
        is_tax_exempt: false,
        tax_exemption: None,
        form_style_id: None,
        sales_rep_id: None,
        price_level_id: None,
        custom_fields: Vec::new(),
    };

    let saved = update_party(&pool, "vendor", staples.id, input(Some("123-45-6789"), false), with_key).await.unwrap();
    assert_eq!((saved.tax_id.as_deref(), saved.tax_id_masked.as_deref()), (Some("123-45-6789"), Some("***-**-6789")));
    let stored: String = sqlx::query_scalar("SELECT tax_id FROM vendors WHERE id = ?1").bind(staples.id).fetch_one(&pool).await.unwrap();
    assert!(crate::crypto::is_encrypted(&stored) && !stored.contains("6789"));

    let seen = get_party(&pool, "vendor", staples.id, without).await.unwrap();
    assert_eq!((seen.tax_id, seen.tax_id_masked.as_deref(), seen.tax_id_locked), (None, Some("***-**-6789"), true));
    assert!(update_party(&pool, "vendor", staples.id, input(None, false), without).await.unwrap_err().starts_with("TAX_ID_LOCKED"), "cannot clear it either");
    let kept = update_party(&pool, "vendor", staples.id, input(None, true), without).await.unwrap();
    assert_eq!(kept.tax_id_masked.as_deref(), Some("***-**-6789"), "other changes keep the ID");
    assert_eq!(get_party(&pool, "vendor", staples.id, with_key).await.unwrap().tax_id.as_deref(), Some("123-45-6789"));

    let cleared = update_party(&pool, "vendor", staples.id, input(Some(" "), false), with_key).await.unwrap();
    assert_eq!((cleared.tax_id, cleared.tax_id_masked), (None, None));
}

#[tokio::test]
async fn customers_vendors_and_products_can_be_edited_and_inactivated() {
    let pool = test_pool().await;
    let acme = crate::sales::create_customer(&pool, "Acme", None).await.unwrap();
    crate::sales::create_customer(&pool, "Globex", None).await.unwrap();
    let input = |name: &str| PartyDetailsInput {
        name: name.into(),
        company_name: Some(" Acme Corp ".into()),
        email: Some("ap@acme.test".into()),
        phone: None,
        address: Some("1 Main St\nSpringfield".into()),
        terms_days: Some(15),
        notes: None,
        tax_id: None,
        keep_tax_id: false,
        is_1099: false,
        tax_rate_id: None,
        is_tax_exempt: false,
        tax_exemption: None,
        form_style_id: None,
        sales_rep_id: None,
        price_level_id: None,
        custom_fields: Vec::new(),
    };
    let saved = update_party(&pool, "customer", acme.id, input("Acme Inc"), Access::OPEN).await.unwrap();
    assert_eq!((saved.name.as_str(), saved.company_name.as_deref(), saved.terms_days), ("Acme Inc", Some("Acme Corp"), Some(15)));
    assert!(update_party(&pool, "customer", acme.id, PartyDetailsInput { form_style_id: Some(999), ..input("Acme Inc") }, Access::OPEN).await.unwrap_err().starts_with("PARTY_BAD_FORM_STYLE"));
    let styled = update_party(&pool, "customer", acme.id, PartyDetailsInput { form_style_id: Some(1), ..input("Acme Inc") }, Access::OPEN).await.unwrap();
    assert_eq!(styled.form_style_id, Some(1));
    assert!(update_party(&pool, "customer", acme.id, input("globex"), Access::OPEN).await.unwrap_err().starts_with("PARTY_DUPLICATE"));
    assert!(!set_party_active(&pool, "customer", acme.id, false, Access::OPEN).await.unwrap().is_active);

    // Jobs: typed as Customer:Job, renamed with their customer.
    let job = crate::sales::create_customer(&pool, "Acme Inc : Kitchen", None).await.unwrap();
    assert_eq!(job.name, "Acme Inc:Kitchen");
    let parent: Option<i64> = sqlx::query_scalar("SELECT parent_id FROM customers WHERE id = ?1").bind(job.id).fetch_one(&pool).await.unwrap();
    assert_eq!(parent, Some(acme.id));
    update_party(&pool, "customer", acme.id, input("Acme Holdings"), Access::OPEN).await.unwrap();
    assert_eq!(get_party(&pool, "customer", job.id, Access::OPEN).await.unwrap().name, "Acme Holdings:Kitchen");

    let staples = crate::expenses::create_vendor(&pool, "Staples", None).await.unwrap();
    let vendor = update_party(&pool, "vendor", staples.id, PartyDetailsInput { tax_id: Some("12-3456789".into()), is_1099: true, ..input("Staples") }, Access::OPEN).await.unwrap();
    assert!(vendor.is_1099 && vendor.tax_id.as_deref() == Some("12-3456789"));
    assert_eq!(vendor.tax_id_masked.as_deref(), Some("**-***6789"));

    let sales: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4000'").fetch_one(&pool).await.unwrap();
    let rent: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '6300'").fetch_one(&pool).await.unwrap();
    let widget = crate::sales::create_product(&pool, ProductInput { name: "Widget".into(), description: None, rate_cents: 100, income_account_id: sales, is_taxable: true }).await.unwrap();
    let edited = update_product(&pool, widget.id, ProductInput { name: "Widget Pro".into(), description: Some("Better".into()), rate_cents: 250, income_account_id: sales, is_taxable: true }).await.unwrap();
    assert_eq!((edited.name.as_str(), edited.rate_cents), ("Widget Pro", 250));
    let err = update_product(&pool, widget.id, ProductInput { name: "Widget Pro".into(), description: None, rate_cents: 250, income_account_id: rent, is_taxable: true }).await.unwrap_err();
    assert!(err.starts_with("PRODUCT_NOT_INCOME_ACCOUNT"), "{err}");
    assert!(!set_product_active(&pool, widget.id, false).await.unwrap().is_active);
}
