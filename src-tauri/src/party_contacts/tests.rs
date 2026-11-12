use super::*;
use crate::db::test_pool;
use crate::parties::{duplicate_party, get_party, update_party, PartyDetailsInput};
use crate::tax_ids::Access;
use chrono::NaiveDate;

fn contact(name: &str) -> ContactInput {
    ContactInput { name: name.into(), ..Default::default() }
}

async fn audit_rows(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM ledger_audit_log WHERE action_type = 'LIST_CHANGE'").fetch_one(pool).await.unwrap()
}

async fn recipient(pool: &SqlitePool, id: i64, use_: &str) -> Option<String> {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT {} FROM customers c WHERE c.id = ?1", recipient_sql(use_)))).bind(id).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn a_customer_keeps_any_number_of_contacts_and_uses_the_defaults() {
    let pool = test_pool().await;
    let id = crate::sales::create_customer(&pool, "Acme", Some("office@acme.example")).await.unwrap().id;
    let input = PartyDetailsInput { name: "Acme".into(), email: Some("office@acme.example".into()), address: Some("1 Main St".into()), ..Default::default() };
    update_party(&pool, "customer", id, input.clone(), Access::OPEN).await.unwrap();

    let saved = save_contacts(
        &pool,
        "customer",
        id,
        &[
            ContactInput { role: Some("Owner".into()), for_invoices: true, ..contact("Ann Lee") },
            ContactInput { role: Some("Accounts payable".into()), email: Some("ap@acme.example".into()), for_statements: true, ..contact("Bo Park") },
            ContactInput { role: Some("Site".into()), mobile: Some("555-0199".into()), ..contact("Cy Diaz") },
        ],
    )
    .await
    .unwrap();
    let names: Vec<&str> = saved.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Ann Lee", "Bo Park", "Cy Diaz"]);
    let acme = get_party(&pool, "customer", id, Access::OPEN).await.unwrap();
    assert_eq!((acme.contact_name.as_deref(), acme.alt_contact_name.as_deref()), (Some("Ann Lee"), Some("Bo Park")));

    // New invoices name the invoice contact in Bill To; invoices go to the customer's email, as
    // Ann has none, and statements to Bo's.
    let defaults = crate::sales_header::customer_sales_defaults(&pool, id).await.unwrap();
    assert_eq!(defaults.bill_to, "Ann Lee\n1 Main St");
    assert_eq!(recipient(&pool, id, "invoices").await.as_deref(), Some("office@acme.example"));
    assert_eq!(recipient(&pool, id, "statements").await.as_deref(), Some("ap@acme.example"));
    let june = crate::forms::Period { start_date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(), end_date: NaiveDate::from_ymd_opt(2026, 6, 30).unwrap() };
    let statement = crate::forms::statement_doc(&pool, id, june, "$").await.unwrap();
    assert_eq!(statement.customer_details, ["Bo Park", "1 Main St"]);
    assert_eq!(statement.customer_email.as_deref(), Some("ap@acme.example"));

    // Cy becomes the invoice contact and Ann is removed; the record keeps working with them.
    let ids: Vec<i64> = saved.iter().map(|c| c.id).collect();
    let before = audit_rows(&pool).await;
    let saved = save_contacts(
        &pool,
        "customer",
        id,
        &[
            ContactInput { id: Some(ids[2]), email: Some("cy@acme.example".into()), for_invoices: true, ..contact("Cy Diaz") },
            ContactInput {
                id: Some(ids[1]),
                role: Some("Accounts payable".into()),
                email: Some("ap@acme.example".into()),
                for_statements: true,
                ..contact("Bo Park")
            },
        ],
    )
    .await
    .unwrap();
    assert_eq!(
        saved.iter().map(|c| (c.name.as_str(), c.for_invoices, c.for_statements)).collect::<Vec<_>>(),
        [("Cy Diaz", true, false), ("Bo Park", false, true)]
    );
    assert_eq!(saved[0].mobile, None, "a field left out is cleared");
    assert_eq!(recipient(&pool, id, "invoices").await.as_deref(), Some("cy@acme.example"));
    let acme = get_party(&pool, "customer", id, Access::OPEN).await.unwrap();
    assert_eq!((acme.contact_name.as_deref(), acme.alt_contact_name.as_deref()), (Some("Cy Diaz"), Some("Bo Park")));
    assert!(audit_rows(&pool).await > before, "contact changes are in the audit trail");

    // Saving the same list again changes nothing and records nothing.
    let unchanged: Vec<ContactInput> = saved
        .iter()
        .map(|c| ContactInput {
            id: Some(c.id),
            name: c.name.clone(),
            role: c.role.clone(),
            email: c.email.clone(),
            phone: c.phone.clone(),
            mobile: c.mobile.clone(),
            for_invoices: c.for_invoices,
            for_statements: c.for_statements,
        })
        .collect();
    let before = audit_rows(&pool).await;
    save_contacts(&pool, "customer", id, &unchanged).await.unwrap();
    assert_eq!(audit_rows(&pool).await, before);

    // Saving the record itself leaves the contacts alone.
    update_party(&pool, "customer", id, input, Access::OPEN).await.unwrap();
    assert_eq!(get_party(&pool, "customer", id, Access::OPEN).await.unwrap().contact_name.as_deref(), Some("Cy Diaz"));

    // A copy of the customer has the same contacts.
    let copy = duplicate_party(&pool, "customer", id, "Acme West", Access::OPEN).await.unwrap();
    let copied = list_contacts(&pool, "customer", copy.id).await.unwrap();
    assert_eq!(copied.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Cy Diaz", "Bo Park"]);
    assert!(crate::ledger::verify_ledger_audit(&pool).await.unwrap().intact);
}

#[tokio::test]
async fn bad_contact_lists_are_refused_whole() {
    let pool = test_pool().await;
    let acme = crate::sales::create_customer(&pool, "Acme", None).await.unwrap().id;
    let other = crate::sales::create_customer(&pool, "Other", None).await.unwrap().id;
    let theirs = save_contacts(&pool, "customer", other, &[contact("Dee")]).await.unwrap()[0].id;
    let err = |r: Result<Vec<Contact>, String>| r.unwrap_err();

    let two = [ContactInput { for_invoices: true, ..contact("A") }, ContactInput { for_invoices: true, ..contact("B") }];
    assert!(err(save_contacts(&pool, "customer", acme, &two).await).starts_with("CONTACT_TWO_DEFAULTS"));
    assert!(err(save_contacts(&pool, "customer", acme, &[contact("  ")]).await).starts_with("CONTACT_NO_NAME"));
    let bad_email = ContactInput { email: Some("not an address".into()), ..contact("A") };
    assert!(err(save_contacts(&pool, "customer", acme, &[contact("Kept"), bad_email]).await).starts_with("CONTACT_BAD_EMAIL"));
    let stolen = ContactInput { id: Some(theirs), ..contact("Dee") };
    assert!(err(save_contacts(&pool, "customer", acme, &[stolen]).await).starts_with("CONTACT_NOT_FOUND"));
    assert!(list_contacts(&pool, "customer", acme).await.unwrap().is_empty(), "nothing of a refused list is kept");

    // Vendors have contacts and a main contact, but no statements.
    let vendor: i64 = sqlx::query_scalar("INSERT INTO vendors (name) VALUES ('Supply Co') RETURNING id").fetch_one(&pool).await.unwrap();
    let main = ContactInput { role: Some("Sales".into()), for_invoices: true, ..contact("Eve") };
    assert_eq!(save_contacts(&pool, "vendor", vendor, &[main, contact("Fay")]).await.unwrap().len(), 2);
    let statements = ContactInput { for_statements: true, ..contact("Eve") };
    assert!(err(save_contacts(&pool, "vendor", vendor, &[statements]).await).starts_with("CONTACT_VENDOR_STATEMENTS"));
    assert!(err(save_contacts(&pool, "vendor", 9_999, &[contact("X")]).await).starts_with("PARTY_NOT_FOUND"));
}
