use super::*;
use crate::db::test_pool;

fn field(label: &str, on_sales_forms: bool) -> CustomFieldInput {
    CustomFieldInput { label: label.into(), on_sales_forms }
}

#[tokio::test]
async fn each_list_holds_fifteen_uniquely_labeled_fields() {
    let pool = test_pool().await;
    for n in 1..=MAX_FIELDS {
        let f = create_custom_field(&pool, FieldList::Customer, field(&format!("Field {n}"), false)).await.unwrap();
        assert_eq!(f.position, n);
    }
    let err = create_custom_field(&pool, FieldList::Customer, field("One more", false)).await.unwrap_err();
    assert!(err.starts_with("CF_LIST_FULL"), "{err}");
    // Other lists have their own fifteen, and a label may repeat across lists but not within one.
    create_custom_field(&pool, FieldList::Vendor, field("Field 1", false)).await.unwrap();
    assert!(create_custom_field(&pool, FieldList::Vendor, field("field 1", false)).await.unwrap_err().starts_with("CF_DUPLICATE"));
    assert!(create_custom_field(&pool, FieldList::Vendor, field("Account rep", true)).await.unwrap_err().starts_with("CF_NOT_ON_FORMS"));
    assert!(create_custom_field(&pool, FieldList::Item, field(&"x".repeat(32), false)).await.unwrap_err().starts_with("CF_LABEL_TOO_LONG"));
    assert!(create_custom_field(&pool, FieldList::Item, field("  ", false)).await.unwrap_err().starts_with("CF_LABEL_REQUIRED"));
    assert!(parse_list("employee").unwrap_err().starts_with("CF_BAD_LIST"));
}

#[tokio::test]
async fn records_keep_their_own_values() {
    let pool = test_pool().await;
    let acme = crate::sales::create_customer(&pool, "Acme", None).await.unwrap();
    let globex = crate::sales::create_customer(&pool, "Globex", None).await.unwrap();
    let region = create_custom_field(&pool, FieldList::Customer, field("Region", false)).await.unwrap();
    let po = create_custom_field(&pool, FieldList::Customer, field("PO number", true)).await.unwrap();

    let saved = save_record_values(&pool, FieldList::Customer, acme.id, &[FieldValue { field_id: region.id, value: " West ".into() }]).await.unwrap();
    let shown: Vec<(&str, &str, bool)> = saved.iter().map(|f| (f.label.as_str(), f.value.as_str(), f.on_sales_forms)).collect();
    assert_eq!(shown, [("Region", "West", false), ("PO number", "", true)]);
    assert!(record_fields(&pool, FieldList::Customer, globex.id).await.unwrap().iter().all(|f| f.value.is_empty()));

    // A blank value clears it; fields left out keep theirs.
    save_record_values(&pool, FieldList::Customer, acme.id, &[FieldValue { field_id: po.id, value: "A-1".into() }]).await.unwrap();
    let after = save_record_values(&pool, FieldList::Customer, acme.id, &[FieldValue { field_id: region.id, value: "".into() }]).await.unwrap();
    assert_eq!(after.iter().map(|f| f.value.as_str()).collect::<Vec<_>>(), ["", "A-1"]);

    // A customer field is not a vendor's, and values have a limit.
    let err = save_record_values(&pool, FieldList::Vendor, acme.id, &[FieldValue { field_id: po.id, value: "x".into() }]).await.unwrap_err();
    assert!(err.starts_with("CF_NO_RECORD") || err.starts_with("CF_NOT_FOUND"), "{err}");
    let err = save_record_values(&pool, FieldList::Customer, acme.id, &[FieldValue { field_id: po.id, value: "x".repeat(101) }]).await.unwrap_err();
    assert!(err.starts_with("CF_VALUE_TOO_LONG"), "{err}");

    // Deleting a field drops its values and closes the gap in the order.
    assert_eq!(list_custom_fields(&pool).await.unwrap().iter().find(|f| f.id == po.id).unwrap().value_count, 1);
    delete_custom_field(&pool, region.id).await.unwrap();
    let fields = list_custom_fields(&pool).await.unwrap();
    assert_eq!(fields.iter().map(|f| (f.label.as_str(), f.position)).collect::<Vec<_>>(), [("PO number", 1)]);

    // Renaming keeps the values; vendor fields still cannot go on forms.
    let renamed = update_custom_field(&pool, po.id, field("Customer PO", true)).await.unwrap();
    assert_eq!((renamed.label.as_str(), renamed.value_count), ("Customer PO", 1));
}

#[tokio::test]
async fn a_field_printed_on_a_form_stays() {
    let pool = test_pool().await;
    let po = create_custom_field(&pool, FieldList::Customer, field("PO number", true)).await.unwrap();
    let mut tx = crate::ledger::begin_write(&pool).await.unwrap();
    write_document_fields(&mut tx, SalesDoc::Invoice, 1, &[FieldValue { field_id: po.id, value: "77".into() }, FieldValue { field_id: po.id, value: " ".into() }])
        .await
        .unwrap();
    crate::ledger::commit_write(tx).await.unwrap();
    assert_eq!(document_fields(&pool, SalesDoc::Invoice, 1).await.unwrap()[0].value, "77");
    assert!(delete_custom_field(&pool, po.id).await.unwrap_err().starts_with("CF_PRINTED"));
    // Taking it off forms is allowed; what was printed stays with the document.
    update_custom_field(&pool, po.id, field("PO number", false)).await.unwrap();
    assert_eq!(document_fields(&pool, SalesDoc::Invoice, 1).await.unwrap().len(), 1);
}

#[tokio::test]
async fn imported_values_add_fields_while_there_is_room() {
    let pool = test_pool().await;
    let income: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(&pool).await.unwrap();
    let item_id: i64 =
        sqlx::query_scalar("INSERT INTO products_services (name, rate_cents, income_account_id) VALUES ('Widget', 100, ?1) RETURNING id").bind(income).fetch_one(&pool).await.unwrap();
    for n in 1..=14 {
        create_custom_field(&pool, FieldList::Item, field(&format!("F{n}"), false)).await.unwrap();
    }
    let mut tx = crate::ledger::begin_write(&pool).await.unwrap();
    let values = [("Color".to_string(), "Red".to_string()), ("f1".into(), "one".into()), ("Size".into(), "L".into()), ("Empty".into(), " ".into())];
    let dropped = set_values_by_label(&mut tx, FieldList::Item, item_id, &values).await.unwrap();
    crate::ledger::commit_write(tx).await.unwrap();
    assert_eq!(dropped, 1, "Size had no room after Color took the 15th place");
    let set: Vec<(String, String)> =
        record_fields(&pool, FieldList::Item, item_id).await.unwrap().into_iter().filter(|f| !f.value.is_empty()).map(|f| (f.label, f.value)).collect();
    assert_eq!(set, [("F1".to_string(), "one".to_string()), ("Color".into(), "Red".into())]);
}
