use super::*;
use crate::db::test_pool;
use crate::invoices::{create_and_post_invoice, invoice_detail, InvoiceInput, InvoiceLineInput};
use crate::money::Cents;
use crate::parties::{get_party, update_party, PartyDetailsInput};
use crate::sales_lists::SalesDetails;
use crate::tax_ids::Access;
use chrono::NaiveDate;

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
}

async fn services(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4000'").fetch_one(pool).await.unwrap()
}

fn invoice(customer: &str, account: i64, cents: i64, header: SalesHeader) -> InvoiceInput {
    InvoiceInput {
        invoice_number: None,
        customer_name: customer.into(),
        issue_date: d(1),
        due_date: d(31),
        lines: vec![InvoiceLineInput { description: "Work".into(), quantity_e4: 10_000, rate_cents: Cents(cents), income_account_id: account, product_service_id: None, class_id: None, is_taxable: false, service_date: None }],
        currency: None,
        tax_rate_id: None,
        discount_cents: Cents::ZERO,
        details: SalesDetails { header, ..Default::default() },
    }
}

async fn customer(pool: &SqlitePool, name: &str, input: PartyDetailsInput) -> i64 {
    let id = crate::sales::create_customer(pool, name, None).await.unwrap().id;
    update_party(pool, "customer", id, PartyDetailsInput { name: name.into(), ..input }, Access::OPEN).await.unwrap();
    id
}

#[tokio::test]
async fn the_customer_record_keeps_its_new_details() {
    let pool = test_pool().await;
    let input = PartyDetailsInput {
        company_name: Some("Acme Inc".into()),
        address: Some("1 Main St\nSpringfield".into()),
        shipping_address: Some("\n  Dock 4 \n 200 Mill Rd \n\n".into()),
        phone: Some("555-0100".into()),
        alt_phone: Some("555-0101".into()),
        fax: Some("555-0102".into()),
        website: Some("acme.example".into()),
        account_number: Some("A-77".into()),
        credit_limit_cents: Some(500_000),
        resale_number: Some("RS-123".into()),
        ..Default::default()
    };
    let id = customer(&pool, "Acme", input.clone()).await;
    // The contact and alternate contact come from the contact list (party_contacts).
    let contacts = [
        crate::party_contacts::ContactInput { name: " Ann Lee ".into(), for_invoices: true, ..Default::default() },
        crate::party_contacts::ContactInput { name: "Bo Park".into(), ..Default::default() },
    ];
    crate::party_contacts::save_contacts(&pool, "customer", id, &contacts).await.unwrap();
    let acme = get_party(&pool, "customer", id, Access::OPEN).await.unwrap();
    assert_eq!(acme.address.as_deref(), Some("1 Main St\nSpringfield"), "today's address stays the billing address");
    assert_eq!(acme.shipping_address.as_deref(), Some("Dock 4\n200 Mill Rd"));
    assert_eq!((acme.contact_name.as_deref(), acme.alt_contact_name.as_deref()), (Some("Ann Lee"), Some("Bo Park")));
    assert_eq!((acme.alt_phone.as_deref(), acme.fax.as_deref(), acme.website.as_deref()), (Some("555-0101"), Some("555-0102"), Some("acme.example")));
    assert_eq!((acme.account_number.as_deref(), acme.credit_limit_cents, acme.resale_number.as_deref()), (Some("A-77"), Some(500_000), Some("RS-123")));

    let bad = |input: PartyDetailsInput| update_party(&pool, "customer", id, PartyDetailsInput { name: "Acme".into(), ..input }, Access::OPEN);
    assert!(bad(PartyDetailsInput { credit_limit_cents: Some(-1), ..input.clone() }).await.unwrap_err().starts_with("PARTY_BAD_CREDIT_LIMIT"));
    assert!(bad(PartyDetailsInput { website: Some("not a site".into()), ..input.clone() }).await.unwrap_err().starts_with("PARTY_BAD_WEBSITE"));

    // Changes to the new fields are in the audit trail.
    bad(PartyDetailsInput { credit_limit_cents: Some(600_000), fax: None, ..input.clone() }).await.unwrap();
    let changed: String = sqlx::query_scalar(
        "SELECT json_extract(payload_snapshot, '$.changed') FROM ledger_audit_log WHERE action_type = 'LIST_CHANGE' AND target_entity_id = ?1
         AND json_extract(payload_snapshot, '$.changed') LIKE '%credit limit%' ORDER BY id DESC LIMIT 1",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(changed, r#"["fax","credit limit"]"#);
}

#[tokio::test]
async fn a_job_inherits_the_addresses_it_does_not_set() {
    let pool = test_pool().await;
    let parent = customer(
        &pool,
        "Acme",
        PartyDetailsInput { company_name: Some("Acme Inc".into()), address: Some("1 Main St".into()), shipping_address: Some("Dock 4".into()), credit_limit_cents: Some(1_000), ..Default::default() },
    )
    .await;
    let job = customer(&pool, "Acme:Kitchen", PartyDetailsInput { shipping_address: Some("9 Site Rd".into()), ..Default::default() }).await;
    let parent_defaults = customer_sales_defaults(&pool, parent).await.unwrap();
    assert_eq!((parent_defaults.bill_to.as_str(), parent_defaults.ship_to.as_str()), ("Acme Inc\n1 Main St", "Acme Inc\nDock 4"));
    let job_defaults = customer_sales_defaults(&pool, job).await.unwrap();
    assert_eq!(job_defaults.bill_to, "Acme Inc\n1 Main St", "the job bills to its customer's address");
    assert_eq!(job_defaults.ship_to, "Acme Inc\n9 Site Rd", "and ships to its own");
    assert_eq!(job_defaults.credit_limit_cents, Some(1_000));

    // A customer with no addresses gets blank blocks; one whose company name is its own name
    // does not repeat it.
    let walk_in = customer(&pool, "Walk In", PartyDetailsInput { company_name: Some("walk in".into()), ..Default::default() }).await;
    assert_eq!(customer_sales_defaults(&pool, walk_in).await.unwrap(), CustomerSalesDefaults::default());
}

#[tokio::test]
async fn a_document_keeps_its_own_copy_of_the_header() {
    let pool = test_pool().await;
    let services = services(&pool).await;
    let acme = customer(&pool, "Acme", PartyDetailsInput { address: Some("1 Main St".into()), shipping_address: Some("Dock 4".into()), ..Default::default() }).await;

    // Left out: filled from the customer.
    let filled = create_and_post_invoice(&pool, invoice("Acme", services, 10_000, SalesHeader { po_number: Some(" PO-9 ".into()), fob: Some("Origin".into()), ..Default::default() })).await.unwrap();
    // Edited on the document, or cleared on purpose.
    let edited = create_and_post_invoice(
        &pool,
        invoice("Acme", services, 10_000, SalesHeader { bill_to: Some("Attn: Payables\n1 Main St\n".into()), ship_to: Some(String::new()), ..Default::default() }),
    )
    .await
    .unwrap();

    // Changing the customer afterwards changes neither document.
    update_party(&pool, "customer", acme, PartyDetailsInput { name: "Acme".into(), address: Some("77 New Ave".into()), ..Default::default() }, Access::OPEN).await.unwrap();
    let pool = &pool;
    let header = |id| async move { invoice_detail(pool, id).await.unwrap().details.header };
    assert_eq!(
        header(filled.id).await,
        SalesHeader { bill_to: Some("1 Main St".into()), ship_to: Some("Dock 4".into()), po_number: Some("PO-9".into()), fob: Some("Origin".into()) }
    );
    assert_eq!(header(edited.id).await, SalesHeader { bill_to: Some("Attn: Payables\n1 Main St".into()), ship_to: Some(String::new()), po_number: None, fob: None });

    // The printed invoice uses the document's copy.
    let doc = crate::forms::invoice_doc(pool, filled.id, "$").await.unwrap();
    assert_eq!(doc.customer_details, ["1 Main St"]);
    assert_eq!(doc.sales.ship_to, ["Dock 4"]);
    assert_eq!((doc.sales.po_number.as_deref(), doc.sales.fob.as_deref()), (Some("PO-9"), Some("Origin")));

    // An invoice written before documents kept a copy prints the customer's current address.
    sqlx::query("UPDATE invoices SET bill_to = NULL, ship_to = NULL WHERE id = ?1").bind(filled.id).execute(pool).await.unwrap();
    assert_eq!(crate::forms::invoice_doc(pool, filled.id, "$").await.unwrap().customer_details, ["77 New Ave"]);

    // Too long a P.O. number is refused.
    let err = create_and_post_invoice(pool, invoice("Acme", services, 100, SalesHeader { po_number: Some("x".repeat(26)), ..Default::default() })).await.unwrap_err();
    assert!(err.starts_with("SALES_PO_TOO_LONG"), "{err}");
}

#[tokio::test]
async fn an_edited_invoice_and_an_estimate_turned_invoice_keep_the_copy() {
    let pool = test_pool().await;
    let services = services(&pool).await;
    customer(&pool, "Acme", PartyDetailsInput { address: Some("1 Main St".into()), ..Default::default() }).await;
    let estimate = crate::estimates::create_estimate(
        &pool,
        crate::estimates::EstimateInput {
            estimate_number: None,
            customer_name: "Acme".into(),
            estimate_date: d(1),
            expiration_date: None,
            memo: None,
            lines: vec![crate::sales::SalesLineInput {
                product_service_id: None,
                description: "Work".into(),
                quantity_e4: 10_000,
                rate_cents: Cents(5_000),
                income_account_id: services,
                class_id: None,
                is_taxable: false, service_date: None,
            }],
            tax_rate_id: None,
            discount_cents: Cents::ZERO,
            details: SalesDetails { header: SalesHeader { ship_to: Some("Site B".into()), po_number: Some("Q-1".into()), ..Default::default() }, ..Default::default() },
        },
    )
    .await
    .unwrap();
    crate::estimates::convert_to_invoice(&pool, estimate.id, d(2), d(30)).await.unwrap();
    let invoice_id: i64 = sqlx::query_scalar("SELECT converted_to_invoice_id FROM estimates WHERE id = ?1").bind(estimate.id).fetch_one(&pool).await.unwrap();
    let header = invoice_detail(&pool, invoice_id).await.unwrap().details.header;
    assert_eq!((header.bill_to.as_deref(), header.ship_to.as_deref(), header.po_number.as_deref()), (Some("1 Main St"), Some("Site B"), Some("Q-1")));
}

#[tokio::test]
async fn the_credit_limit_warns_and_never_blocks() {
    let pool = test_pool().await;
    let services = services(&pool).await;
    customer(&pool, "Acme", PartyDetailsInput { credit_limit_cents: Some(100_000), ..Default::default() }).await;
    customer(&pool, "Acme:Kitchen", PartyDetailsInput::default()).await;
    customer(&pool, "Globex", PartyDetailsInput::default()).await;

    // No limit, or a customer not created yet: nothing to warn about.
    assert_eq!(credit_check(&pool, "Globex", 10_000_000, None).await.unwrap(), None);
    assert_eq!(credit_check(&pool, "Brand New", 10_000_000, None).await.unwrap(), None);

    let first = create_and_post_invoice(&pool, invoice("Acme", services, 60_000, SalesHeader::default())).await.unwrap();
    assert_eq!(credit_check(&pool, "Acme", 40_000, None).await.unwrap(), None, "reaching the limit exactly is fine");
    // The job's invoice counts against its customer's limit.
    let warning = credit_check(&pool, "Acme:Kitchen", 40_001, None).await.unwrap().expect("over the limit");
    assert_eq!(warning, CreditLimitWarning { customer_name: "Acme".into(), credit_limit_cents: 100_000, balance_cents: 60_000, new_balance_cents: 100_001 });
    // Editing the first invoice replaces it, so its own balance is not counted twice.
    assert_eq!(credit_check(&pool, "Acme", 90_000, Some(first.id)).await.unwrap(), None);

    // A warning only: the invoice still saves.
    create_and_post_invoice(&pool, invoice("Acme:Kitchen", services, 50_000, SalesHeader::default())).await.unwrap();
    let warning = credit_check(&pool, "acme", 1, None).await.unwrap().expect("already over");
    assert_eq!(warning.balance_cents, 110_000);
}

#[tokio::test]
async fn a_paid_invoice_resaved_keeps_its_own_header() {
    let pool = test_pool().await;
    let services = services(&pool).await;
    let checking: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '1010'").fetch_one(&pool).await.unwrap();
    let acme = customer(&pool, "Acme", PartyDetailsInput { address: Some("1 Main St".into()), shipping_address: Some("Dock 4".into()), ..Default::default() }).await;
    let typed = SalesHeader { bill_to: Some("Attn: Payables\n1 Main St".into()), ship_to: Some("Site B".into()), po_number: Some("PO-9".into()), fob: Some("Origin".into()) };
    let original = create_and_post_invoice(&pool, invoice("Acme", services, 10_000, typed.clone())).await.unwrap().id;
    crate::invoices::record_payment(
        &pool,
        crate::invoices::PaymentInput {
            discount_cents: Cents::ZERO,
            payment_method_id: None,
            invoice_id: original,
            deposit_account_id: checking,
            payment_date: d(5),
            amount_cents: Cents(4_000),
            payment_method: "CHECK".into(),
            reference: None,
            exchange_rate_e6: None,
        },
    )
    .await
    .unwrap();
    // The customer's record changes after the invoice was written.
    update_party(&pool, "customer", acme, PartyDetailsInput { name: "Acme".into(), address: Some("77 New Ave".into()), ..Default::default() }, Access::OPEN).await.unwrap();

    // Re-saved from the form, which loads the invoice's own copy and sends it back.
    let loaded = invoice_detail(&pool, original).await.unwrap().details.header;
    assert_eq!(loaded, typed);
    let edited = crate::edits::replace_invoice(&pool, original, invoice("Acme", services, 12_000, loaded)).await.unwrap();
    let detail = invoice_detail(&pool, edited).await.unwrap();
    assert_eq!(detail.details.header, typed);
    assert_eq!(detail.invoice.amount_paid_cents, 4_000, "the payment stays applied");

    // Re-saved by a caller that leaves the addresses out: the edited invoice still keeps the
    // original's copy, not the customer's new address. P.O. and FOB are sent as they are.
    let again = crate::edits::replace_invoice(&pool, edited, invoice("Acme", services, 12_500, SalesHeader { po_number: Some("PO-9".into()), fob: Some("Origin".into()), ..Default::default() }))
        .await
        .unwrap();
    assert_eq!(invoice_detail(&pool, again).await.unwrap().details.header, typed);
}
