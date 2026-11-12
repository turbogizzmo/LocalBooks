use super::*;
use crate::db::test_pool;
use crate::expenses::{post_vendor_bill, BillInput, BillLineInput};
use crate::invoices::{create_and_post_invoice, InvoiceInput, InvoiceLineInput};
use crate::money::Cents;
use crate::sales::{post_sales_receipt, SalesLineInput, SalesReceiptInput};
use crate::sales_lists::{document_details, SalesDetails, SalesDoc};

fn d(month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, month, day).unwrap()
}

fn terms(name: &str, net_days: i64, discount_ppm: i64, discount_days: Option<i64>) -> TermsInput {
    TermsInput { name: name.into(), net_days, discount_ppm, discount_days }
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn account_of_type(pool: &SqlitePool, account_type: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_type = ?1 AND is_active = 1 ORDER BY id LIMIT 1")
        .bind(account_type)
        .fetch_one(pool)
        .await
        .unwrap()
}

fn invoice(customer: &str, income: i64, cents: i64, details: SalesDetails) -> InvoiceInput {
    InvoiceInput {
        invoice_number: None,
        customer_name: customer.into(),
        issue_date: d(10, 1),
        due_date: d(10, 31),
        lines: vec![InvoiceLineInput {
            description: "Work".into(),
            quantity_e4: 10_000,
            rate_cents: Cents(cents),
            income_account_id: income,
            product_service_id: None,
            class_id: None,
            is_taxable: false,
            service_date: None,
        }],
        currency: None,
        tax_rate_id: None,
        discount_cents: Cents::ZERO,
        details,
    }
}

fn party_input(name: &str) -> crate::parties::PartyDetailsInput {
    serde_json::from_value(serde_json::json!({ "name": name, "keepTaxId": true })).unwrap()
}

#[test]
fn a_term_sets_the_due_date_and_discount_date() {
    assert_eq!(due_date(d(10, 1), 30).unwrap(), d(10, 31));
    assert_eq!(due_date(d(1, 31), 30).unwrap(), d(3, 2), "counts days, not months");
    assert_eq!(due_date(d(10, 1), 0).unwrap(), d(10, 1), "due on receipt");
    assert!(due_date(d(10, 1), -1).is_err());
    let view = TermsView { id: 1, name: "2% 10 Net 30".into(), net_days: 30, discount_ppm: 20_000, discount_days: Some(10), is_active: true, is_used: false };
    assert_eq!(view.dates(d(10, 1)).unwrap(), (d(10, 31), Some(d(10, 11))));
    assert_eq!(early_discount_cents(12_345, 20_000), Some(247), "2% of $123.45, rounded half away from zero");
    assert_eq!(early_discount_cents(10_0000, 0), Some(0));
}

#[tokio::test]
async fn the_migration_seeds_the_four_terms_and_six_payment_methods() {
    let pool = test_pool().await;
    let names: Vec<(String, i64)> = list_terms(&pool).await.unwrap().into_iter().map(|t| (t.name, t.net_days)).collect();
    assert_eq!(names, [("Due on receipt".into(), 0), ("Net 15".into(), 15), ("Net 30".into(), 30), ("Net 60".into(), 60)]);
    let methods = list_payment_methods(&pool).await.unwrap();
    assert_eq!(methods.len(), 6);
    for kind in PAYMENT_KINDS {
        assert_eq!(methods.iter().filter(|m| m.kind == *kind).count(), 1, "one method of kind {kind}");
    }
    assert!(list_profile_entries(&pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn the_migration_links_existing_records_and_payments() {
    use std::borrow::Cow;
    let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    let mut before = sqlx::migrate::Migrator::new(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations"))).await.unwrap();
    before.migrations = Cow::Owned(before.migrations.iter().filter(|m| m.version < 64).cloned().collect());
    crate::db::migrate_with(&pool, &before).await.unwrap();
    sqlx::query("INSERT INTO customers (name, terms_days) VALUES ('Acme', 30), ('Birch', 45), ('Cobalt', 0), ('Dune', NULL)").execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO vendors (name, terms_days) VALUES ('Staples', 45)").execute(&pool).await.unwrap();

    crate::db::migrate(&pool).await.unwrap();
    let linked: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT c.name, t.name FROM customers c LEFT JOIN terms t ON t.id = c.terms_id ORDER BY c.name").fetch_all(&pool).await.unwrap();
    assert_eq!(
        linked,
        [
            ("Acme".into(), Some("Net 30".into())),
            ("Birch".into(), Some("Net 45".into())),
            ("Cobalt".into(), Some("Due on receipt".into())),
            ("Dune".into(), None)
        ]
    );
    let vendor: Option<String> = sqlx::query_scalar("SELECT t.name FROM vendors v JOIN terms t ON t.id = v.terms_id").fetch_optional(&pool).await.unwrap();
    assert_eq!(vendor.as_deref(), Some("Net 45"), "the vendor shares the Net 45 term added for Birch");
    assert_eq!(list_terms(&pool).await.unwrap().len(), 5);
}

#[tokio::test]
async fn records_written_with_days_only_are_linked_to_a_term() {
    let pool = test_pool().await;
    // Imports write days, never a term.
    sqlx::query("INSERT INTO customers (name, terms_days) VALUES ('Acme', 30), ('Birch', 21)").execute(&pool).await.unwrap();
    let net21 = list_terms(&pool).await.unwrap().into_iter().find(|t| t.name == "Net 21").expect("added for the days");
    let birch: Option<i64> = sqlx::query_scalar("SELECT terms_id FROM customers WHERE name = 'Birch'").fetch_one(&pool).await.unwrap();
    assert_eq!(birch, Some(net21.id));
    sqlx::query("UPDATE customers SET terms_days = 15 WHERE name = 'Acme'").execute(&pool).await.unwrap();
    let acme: String =
        sqlx::query_scalar("SELECT t.name FROM customers c JOIN terms t ON t.id = c.terms_id WHERE c.name = 'Acme'").fetch_one(&pool).await.unwrap();
    assert_eq!(acme, "Net 15");
}

#[tokio::test]
async fn terms_are_validated_unique_and_kept_once_used() {
    let pool = test_pool().await;
    let two_ten = create_terms(&pool, terms(" 2% 10 Net 30 ", 30, 20_000, Some(10))).await.unwrap();
    assert_eq!((two_ten.name.as_str(), two_ten.discount_ppm, two_ten.discount_days, two_ten.is_used), ("2% 10 Net 30", 20_000, Some(10), false));
    assert!(create_terms(&pool, terms("net 30", 30, 0, None)).await.unwrap_err().starts_with("TERMS_DUPLICATE"));
    assert!(create_terms(&pool, terms("", 30, 0, None)).await.unwrap_err().starts_with("TERMS_NAME_REQUIRED"));
    assert!(create_terms(&pool, terms("Net 400", 400, 0, None)).await.unwrap_err().starts_with("TERMS_BAD_DAYS"));
    assert!(create_terms(&pool, terms("Bad", 30, 20_000, None)).await.unwrap_err().starts_with("TERMS_DISCOUNT_DAYS_REQUIRED"));
    assert!(create_terms(&pool, terms("Bad", 30, 20_000, Some(31))).await.unwrap_err().starts_with("TERMS_BAD_DISCOUNT_DAYS"));
    assert!(create_terms(&pool, terms("Bad", 30, PPM, Some(10))).await.unwrap_err().starts_with("TERMS_BAD_DISCOUNT"));
    let no_discount = create_terms(&pool, terms("Net 45", 45, 0, Some(10))).await.unwrap();
    assert_eq!(no_discount.discount_days, None, "discount days without a discount are dropped");

    // A customer with the term follows a change to its days.
    let acme = crate::sales::create_customer(&pool, "Acme", None).await.unwrap();
    let mut input = party_input("Acme");
    input.terms_id = Some(two_ten.id);
    let saved = crate::parties::update_party(&pool, "customer", acme.id, input, crate::tax_ids::Access::OPEN).await.unwrap();
    assert_eq!((saved.terms_id, saved.terms_days), (Some(two_ten.id), Some(30)));
    update_terms(&pool, two_ten.id, terms("2% 10 Net 45", 45, 20_000, Some(10))).await.unwrap();
    let days: Option<i64> = sqlx::query_scalar("SELECT terms_days FROM customers WHERE id = ?1").bind(acme.id).fetch_one(&pool).await.unwrap();
    assert_eq!(days, Some(45));
    let kept: Option<i64> = sqlx::query_scalar("SELECT terms_id FROM customers WHERE id = ?1").bind(acme.id).fetch_one(&pool).await.unwrap();
    assert_eq!(kept, Some(two_ten.id), "keeps its term, not the plain Net 45 with the same days");

    assert!(delete_terms(&pool, two_ten.id).await.unwrap_err().starts_with("TERMS_IN_USE"));
    delete_terms(&pool, no_discount.id).await.unwrap();
    assert!(!set_terms_active(&pool, two_ten.id, false).await.unwrap().is_active);
    // The customer keeps the inactive term on save; another customer cannot pick it.
    let mut again = party_input("Acme");
    again.terms_id = Some(two_ten.id);
    crate::parties::update_party(&pool, "customer", acme.id, again, crate::tax_ids::Access::OPEN).await.unwrap();
    let birch = crate::sales::create_customer(&pool, "Birch", None).await.unwrap();
    let mut other = party_input("Birch");
    other.terms_id = Some(two_ten.id);
    let err = crate::parties::update_party(&pool, "customer", birch.id, other, crate::tax_ids::Access::OPEN).await.unwrap_err();
    assert!(err.starts_with("PARTY_INACTIVE_TERMS"), "{err}");
}

#[tokio::test]
async fn an_invoice_and_a_bill_keep_their_term_and_early_payment_discount() {
    let pool = test_pool().await;
    let services = account(&pool, "4100").await;
    let two_ten = create_terms(&pool, terms("2% 10 Net 30", 30, 20_000, Some(10))).await.unwrap();

    let details = SalesDetails { terms_id: Some(two_ten.id), ..Default::default() };
    let inv = create_and_post_invoice(&pool, invoice("Acme", services, 123_456, details)).await.unwrap();
    assert_eq!(inv.terms.terms_name.as_deref(), Some("2% 10 Net 30"));
    assert_eq!(inv.terms.discount_until.as_deref(), Some("2026-10-11"));
    assert_eq!(inv.terms.early_discount_cents, 2_469, "2% of $1,234.56");
    assert_eq!(document_details(&pool, SalesDoc::Invoice, inv.id).await.unwrap().terms_name.as_deref(), Some("2% 10 Net 30"));
    assert!(list_terms(&pool).await.unwrap().iter().find(|t| t.id == two_ten.id).unwrap().is_used);

    // Only invoices carry a term.
    let receipt = SalesReceiptInput {
        receipt_number: None,
        customer_name: None,
        receipt_date: d(10, 1),
        deposit_account_id: account_of_type(&pool, "BANK").await,
        payment_method: "CASH".into(),
        payment_method_id: None,
        reference: None,
        memo: None,
        lines: vec![SalesLineInput {
            product_service_id: None,
            description: "Sale".into(),
            quantity_e4: 10_000,
            rate_cents: Cents(500),
            income_account_id: services,
            class_id: None,
            is_taxable: false,
            service_date: None,
        }],
        tax_rate_id: None,
        discount_cents: Cents::ZERO,
        details: SalesDetails { terms_id: Some(two_ten.id), ..Default::default() },
    };
    assert!(post_sales_receipt(&pool, receipt).await.unwrap_err().starts_with("SR_NO_TERMS"));

    let expense = account_of_type(&pool, "EXPENSE").await;
    let bill = |terms_id| BillInput {
        form_lines: Vec::new(),
        vendor_name: "Staples".into(),
        bill_number: None,
        bill_date: d(10, 1),
        due_date: d(10, 31),
        memo: None,
        total_cents: Cents(50_000),
        lines: vec![BillLineInput {
            account_id: expense,
            amount_cents: Cents(50_000),
            memo: None,
            quantity_e4: None,
            rate_cents: None,
            product_service_id: None,
            class_id: None,
            customer_id: None,
            billable: None,
        }],
        terms_id,
    };
    let posted = post_vendor_bill(&pool, bill(Some(two_ten.id))).await.unwrap();
    assert_eq!((posted.terms.discount_until.as_deref(), posted.terms.early_discount_cents), (Some("2026-10-11"), 1_000));

    set_terms_active(&pool, two_ten.id, false).await.unwrap();
    assert!(post_vendor_bill(&pool, bill(Some(two_ten.id))).await.unwrap_err().starts_with("BILL_INACTIVE_TERMS"));
    // Editing the bill keeps its term although it is inactive now.
    crate::edits::replace_bill(&pool, posted.id, bill(Some(two_ten.id))).await.unwrap();
}

#[tokio::test]
async fn payment_methods_name_a_kind_of_payment() {
    let pool = test_pool().await;
    let visa = create_payment_method(&pool, PaymentMethodInput { name: " Visa ".into(), kind: "CREDIT_CARD".into() }).await.unwrap();
    assert_eq!((visa.name.as_str(), visa.kind.as_str()), ("Visa", "CREDIT_CARD"));
    assert!(create_payment_method(&pool, PaymentMethodInput { name: "visa".into(), kind: "CASH".into() })
        .await
        .unwrap_err()
        .starts_with("PAYMENT_METHOD_DUPLICATE"));
    assert!(create_payment_method(&pool, PaymentMethodInput { name: "Barter".into(), kind: "GOATS".into() })
        .await
        .unwrap_err()
        .starts_with("PAYMENT_METHOD_BAD_KIND"));

    let services = account(&pool, "4100").await;
    let bank = account_of_type(&pool, "BANK").await;
    let receipt = |method: Option<i64>, kind: &str| SalesReceiptInput {
        receipt_number: None,
        customer_name: None,
        receipt_date: d(10, 1),
        deposit_account_id: bank,
        payment_method: kind.into(),
        payment_method_id: method,
        reference: None,
        memo: None,
        lines: vec![SalesLineInput {
            product_service_id: None,
            description: "Sale".into(),
            quantity_e4: 10_000,
            rate_cents: Cents(500),
            income_account_id: services,
            class_id: None,
            is_taxable: false,
            service_date: None,
        }],
        tax_rate_id: None,
        discount_cents: Cents::ZERO,
        details: SalesDetails::default(),
    };
    // The chosen method decides the kind recorded.
    let sold = post_sales_receipt(&pool, receipt(Some(visa.id), "CASH")).await.unwrap();
    let stored: (String, Option<i64>) =
        sqlx::query_as("SELECT payment_method, payment_method_id FROM sales_receipts WHERE id = ?1").bind(sold.id).fetch_one(&pool).await.unwrap();
    assert_eq!(stored, ("CREDIT_CARD".into(), Some(visa.id)));
    // A kind alone is recorded with the first method of that kind.
    let cash = post_sales_receipt(&pool, receipt(None, "CASH")).await.unwrap();
    let name: String = sqlx::query_scalar("SELECT m.name FROM sales_receipts r JOIN payment_methods m ON m.id = r.payment_method_id WHERE r.id = ?1")
        .bind(cash.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "Cash");

    assert!(update_payment_method(&pool, visa.id, PaymentMethodInput { name: "Visa".into(), kind: "CHECK".into() })
        .await
        .unwrap_err()
        .starts_with("PAYMENT_METHOD_IN_USE"));
    assert_eq!(
        update_payment_method(&pool, visa.id, PaymentMethodInput { name: "Visa card".into(), kind: "CREDIT_CARD".into() }).await.unwrap().name,
        "Visa card"
    );
    assert!(delete_payment_method(&pool, visa.id).await.unwrap_err().starts_with("PAYMENT_METHOD_IN_USE"));
    set_payment_method_active(&pool, visa.id, false).await.unwrap();
    assert!(post_sales_receipt(&pool, receipt(Some(visa.id), "CASH")).await.unwrap_err().starts_with("SR_INACTIVE_METHOD"));
    // With every check method inactive, a kind-only payment records the kind and no method.
    let check = list_payment_methods(&pool).await.unwrap().into_iter().find(|m| m.kind == "CHECK").unwrap();
    set_payment_method_active(&pool, check.id, false).await.unwrap();
    let by_check = post_sales_receipt(&pool, receipt(None, "CHECK")).await.unwrap();
    let stored: (String, Option<i64>) =
        sqlx::query_as("SELECT payment_method, payment_method_id FROM sales_receipts WHERE id = ?1").bind(by_check.id).fetch_one(&pool).await.unwrap();
    assert_eq!(stored, ("CHECK".into(), None));
    // A sales receipt's details (no terms column) save with a customer message.
    let thanks = create_profile_entry(&pool, ProfileList::CustomerMessage, "Thanks!").await.unwrap();
    let with_message = SalesReceiptInput {
        receipt_number: None,
        details: SalesDetails { customer_message_id: Some(thanks.id), ..Default::default() },
        ..receipt(None, "CASH")
    };
    let sold = post_sales_receipt(&pool, with_message).await.unwrap();
    assert_eq!(document_details(&pool, SalesDoc::SalesReceipt, sold.id).await.unwrap().customer_message.as_deref(), Some("Thanks!"));
    let unused = create_payment_method(&pool, PaymentMethodInput { name: "Venmo".into(), kind: "OTHER".into() }).await.unwrap();
    delete_payment_method(&pool, unused.id).await.unwrap();
}

#[tokio::test]
async fn types_go_on_records_and_messages_print_on_sales_forms() {
    let pool = test_pool().await;
    let retail = create_profile_entry(&pool, ProfileList::CustomerType, " Retail ").await.unwrap();
    assert_eq!((retail.list.as_str(), retail.name.as_str()), ("CUSTOMER_TYPE", "Retail"));
    assert!(create_profile_entry(&pool, ProfileList::CustomerType, "retail").await.unwrap_err().starts_with("PROFILE_DUPLICATE"));
    let supplier = create_profile_entry(&pool, ProfileList::VendorType, "Retail").await.unwrap();
    let remodel = create_profile_entry(&pool, ProfileList::JobType, "Remodel").await.unwrap();
    assert!(create_profile_entry(&pool, ProfileList::JobType, &"x".repeat(32)).await.unwrap_err().starts_with("PROFILE_NAME_TOO_LONG"));
    let thanks = create_profile_entry(&pool, ProfileList::CustomerMessage, &format!("Thank you for your business. {}", "x".repeat(60))).await.unwrap();
    assert!(create_profile_entry(&pool, ProfileList::CustomerMessage, &"x".repeat(102)).await.unwrap_err().starts_with("PROFILE_NAME_TOO_LONG"));

    let acme = crate::sales::create_customer(&pool, "Acme", None).await.unwrap();
    let mut input = party_input("Acme");
    input.customer_type_id = Some(supplier.id);
    let err = crate::parties::update_party(&pool, "customer", acme.id, input, crate::tax_ids::Access::OPEN).await.unwrap_err();
    assert!(err.starts_with("PARTY_UNKNOWN_CUSTOMER_TYPE"), "a vendor type is not a customer type: {err}");
    let mut input = party_input("Acme");
    input.customer_type_id = Some(retail.id);
    input.job_type_id = Some(remodel.id);
    let saved = crate::parties::update_party(&pool, "customer", acme.id, input, crate::tax_ids::Access::OPEN).await.unwrap();
    assert_eq!((saved.customer_type_id, saved.job_type_id, saved.is_job), (Some(retail.id), Some(remodel.id), false));
    let staples = crate::expenses::create_vendor(&pool, "Staples", None).await.unwrap();
    let mut input = party_input("Staples");
    input.vendor_type_id = Some(supplier.id);
    assert_eq!(crate::parties::update_party(&pool, "vendor", staples.id, input, crate::tax_ids::Access::OPEN).await.unwrap().vendor_type_id, Some(supplier.id));
    assert!(rename_profile_entry(&pool, retail.id, "Wholesale").await.unwrap().is_used);
    assert!(delete_profile_entry(&pool, retail.id).await.unwrap_err().starts_with("PROFILE_IN_USE"));

    let services = account(&pool, "4100").await;
    let inv = create_and_post_invoice(&pool, invoice("Acme", services, 10_000, SalesDetails { customer_message_id: Some(thanks.id), ..Default::default() }))
        .await
        .unwrap();
    let details = document_details(&pool, SalesDoc::Invoice, inv.id).await.unwrap();
    assert_eq!(details.customer_message.as_deref(), Some(thanks.name.as_str()));
    let err = create_and_post_invoice(&pool, invoice("Acme", services, 10_000, SalesDetails { customer_message_id: Some(remodel.id), ..Default::default() }))
        .await
        .unwrap_err();
    assert!(err.starts_with("INV_UNKNOWN_CUSTOMER_MESSAGE"), "{err}");
    set_profile_entry_active(&pool, thanks.id, false).await.unwrap();
    let err = create_and_post_invoice(&pool, invoice("Acme", services, 10_000, SalesDetails { customer_message_id: Some(thanks.id), ..Default::default() }))
        .await
        .unwrap_err();
    assert!(err.starts_with("INV_INACTIVE_CUSTOMER_MESSAGE"), "{err}");
}
