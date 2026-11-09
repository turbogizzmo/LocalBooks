use super::*;

fn sheet(csv: &str) -> Sheet {
    read("people.csv", csv.as_bytes()).unwrap()
}

#[test]
fn headings_suggest_the_mapping_and_the_template_maps_itself() {
    let s = sheet("Customer Name,E-mail,Phone Number,Payment Terms,Notes\nRivera,jo@r.example,555,Net 30,\n");
    let m = suggest(SheetKind::Customers, &s.headers);
    assert_eq!((m["name"], m["email"], m["phone"], m["terms"], m["notes"]), (0, 1, 2, 3, 4));
    for kind in [SheetKind::Customers, SheetKind::Vendors, SheetKind::Invoices] {
        let t = sheet(&template(kind));
        let m = suggest(kind, &t.headers);
        assert_eq!(m.len(), fields(kind).len(), "{kind:?} template maps every field");
        assert!(check(kind, &t, &m, None).problems.is_empty(), "{kind:?} template's example row is clean");
    }
    let inv = sheet("Inv #,Txn Date,Name,Qty,Price,Account\n");
    let m = suggest(SheetKind::Invoices, &inv.headers);
    assert_eq!((m["number"], m["date"], m["customer"], m["quantity"], m["rate"], m["account"]), (0, 1, 2, 3, 4, 5));
}

#[test]
fn the_heading_row_is_found_and_blank_rows_are_skipped_with_file_row_numbers() {
    let s = sheet("My customers\n\nName,Email\nAcme,a@x.test\n,\nBirch,b@x.test\n");
    assert_eq!(s.headers, ["Name", "Email"]);
    assert_eq!(s.rows.iter().map(|r| r.0).collect::<Vec<_>>(), [4, 6]);
}

#[test]
fn customer_rows_that_would_fail_are_listed_and_nothing_is_built() {
    let s = sheet("Name,Email,Terms,Active\nAcme,acme@x.test,Net 30,yes\n,b@x.test,,\nacme,not-an-email,Soon,maybe\n");
    let m = suggest(SheetKind::Customers, &s.headers);
    let c = check(SheetKind::Customers, &s, &m, None);
    let msgs: Vec<(usize, &str)> = c.problems.iter().map(|p| (p.row, p.message.as_str())).collect();
    assert_eq!(msgs[0], (3, "The name is empty"));
    assert!(msgs.iter().any(|(r, m)| *r == 4 && m.contains("also on row 2")));
    assert!(msgs.iter().any(|(r, m)| *r == 4 && m.contains("terms")));
    assert!(msgs.iter().any(|(r, m)| *r == 4 && m.contains("yes or no")));
    assert!(msgs.iter().any(|(r, m)| *r == 4 && m.contains("email")));
    assert!(to_ledger(SheetKind::Customers, &s, &m, None).is_err());
    // A required field left unmapped is a problem too.
    let unmapped = check(SheetKind::Customers, &s, &Mapping::new(), None);
    assert_eq!(unmapped.problems[0].message, "Choose the column that holds Name");
}

#[test]
fn clean_customers_and_vendors_become_a_ledger() {
    let s = sheet("Name,Company,Email,Billing address,Terms,Active\nRivera,Rivera LLC,jo@r.example,\"412 Orchard Lane\nSpringfield, OR 97477\",Net 15,yes\nOld Client,,,,,no\n");
    let m = suggest(SheetKind::Customers, &s.headers);
    let ledger = to_ledger(SheetKind::Customers, &s, &m, None).unwrap();
    assert_eq!(ledger.customers.len(), 2);
    let r = ledger.customers.iter().find(|c| c.name == "Rivera").unwrap();
    assert_eq!((r.company_name.as_deref(), r.terms_days, r.active, r.address.as_deref()), (Some("Rivera LLC"), Some(15), true, Some("412 Orchard Lane\nSpringfield, OR 97477")));
    assert!(!ledger.customers.iter().find(|c| c.name == "Old Client").unwrap().active);

    let v = sheet("Name,Tax ID,1099\nPaper Co,12-3456789,yes\n");
    let ledger = to_ledger(SheetKind::Vendors, &v, &suggest(SheetKind::Vendors, &v.headers), None).unwrap();
    assert_eq!((ledger.vendors[0].tax_id.as_deref(), ledger.vendors[0].is_1099), (Some("12-3456789"), true));
}

#[test]
fn invoice_rows_group_by_number_and_build_balanced_invoices() {
    let s = sheet(
        "Invoice no.,Date,Customer,Due date,Item,Description,Quantity,Rate,Amount,Income account\n\
         1001,03/01/2026,Rivera,03/31/2026,Mowing,March mowing,4,60.00,,Sales:Lawn\n\
         1001,03/01/2026,Rivera,,Edging,,1,25.50,25.50,Sales:Lawn\n\
         1002,03/02/2026,Birch Books,,,Consulting,,,\"1,200.00\",Consulting Income\n",
    );
    let m = suggest(SheetKind::Invoices, &s.headers);
    let c = check(SheetKind::Invoices, &s, &m, None);
    assert!(c.problems.is_empty(), "{:?}", c.problems);
    assert_eq!((c.records, c.total_cents), (2, 24_000 + 2_550 + 120_000));
    let ledger = to_ledger(SheetKind::Invoices, &s, &m, None).unwrap();
    assert!(ledger.unbalanced.is_empty());
    assert_eq!(ledger.transactions.len(), 2);
    let first = &ledger.transactions[0];
    assert_eq!((first.kind, first.doc_num.as_deref(), first.date), ("Invoice", Some("1001"), NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()));
    assert_eq!(first.lines.iter().map(|l| l.cents).sum::<i64>(), 0);
    assert!(first.document.is_some(), "becomes an invoice document");
    assert!(ledger.customers.iter().any(|c| c.name == "Birch Books"));
    assert!(ledger.accounts.iter().any(|a| a.full_name == "Sales:Lawn" && a.account_type == "INCOME"));
}

#[test]
fn invoice_rows_that_would_fail_are_listed() {
    let s = sheet(
        "Invoice no.,Date,Customer,Quantity,Rate,Amount,Income account\n\
         1001,03/01/2026,Rivera,2,10.00,20.00,Sales\n\
         1001,03/05/2026,Rivera,,,5.00,Sales\n\
         1001,03/01/2026,Birch,,,5.00,Sales\n\
         ,13/45/2026,,,,abc,\n\
         1003,03/01/2026,Rivera,,,0,Sales\n\
         1004,03/01/2026,Rivera,2,10.00,25.00,Sales\n",
    );
    let m = suggest(SheetKind::Invoices, &s.headers);
    let c = check(SheetKind::Invoices, &s, &m, None);
    let has = |row: usize, text: &str| c.problems.iter().any(|p| p.row == row && p.message.contains(text));
    assert!(has(3, "dated 03/01/2026 on row 2"), "{:?}", c.problems);
    assert!(has(4, "is for Rivera on row 2"));
    assert!(has(5, "invoice number is empty") && has(5, "customer is empty") && has(5, "doesn't read") && has(5, "\"abc\""));
    assert!(has(6, "more than zero"));
    assert!(has(7, "isn't the quantity times the rate"));
    assert!(!c.problems.iter().any(|p| p.row == 2), "row 2 is fine");
}

/// A clean invoice sheet goes through the same staging, review and posting as any other import,
/// and becomes LocalBooks invoices owed by the customers it names.
#[tokio::test]
async fn an_invoice_sheet_stages_and_posts_as_invoices() {
    use crate::qbw_ledger_import::{auto_map_accounts, batch_summary, import_lists, post_batch, stage_as, ImportMode, PostLedgerInput};
    let pool = crate::db::test_pool().await;
    let s = sheet(
        "Invoice no.,Date,Customer,Description,Quantity,Rate,Amount,Income account\n\
         2001,03/02/2026,Rivera Bakery,Pastry boxes,3,4.50,13.50,Sales\n\
         2001,03/02/2026,Rivera Bakery,Delivery,,,5.00,Delivery Income\n\
         2002,03/09/2026,Birch & Co,Consulting,2,100.00,200.00,Sales\n",
    );
    let m = suggest(SheetKind::Invoices, &s.headers);
    let ledger = to_ledger(SheetKind::Invoices, &s, &m, None).unwrap();
    let batch = stage_as(&pool, std::path::Path::new("/sheets/March invoices.csv"), 1, &ledger, Some("SHEET")).await.unwrap();
    let summary = batch_summary(&pool, batch).await.unwrap();
    assert_eq!((summary.source_format.as_str(), summary.transaction_count, summary.unbalanced.len()), ("SHEET", 2, 0));

    auto_map_accounts(&pool, batch).await.unwrap();
    import_lists(&pool, batch).await.unwrap();
    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap();
    assert_eq!(posted.full.unwrap().invoices, 2);
    let rows: Vec<(i64, String, i64, i64)> = sqlx::query_as(
        "SELECT i.invoice_number, c.name, i.total_cents, (SELECT COUNT(*) FROM invoice_items WHERE invoice_id = i.id)
         FROM invoices i JOIN customers c ON c.id = i.customer_id ORDER BY i.invoice_number",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows, [(2001, "Rivera Bakery".into(), 1_850, 2), (2002, "Birch & Co".into(), 20_000, 1)]);
}
