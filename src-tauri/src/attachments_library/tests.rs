use super::*;

async fn first(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap_or_else(|e| panic!("{sql}: {e}"))
}

#[tokio::test]
async fn files_attach_to_every_kind_of_record_and_show_in_the_library() {
    let pool = crate::harness::fixture::harbor_lane().await;
    let customer = first(&pool, "SELECT id FROM customers ORDER BY id LIMIT 1").await;
    let vendor = first(&pool, "SELECT id FROM vendors ORDER BY id LIMIT 1").await;
    let income = first(&pool, "SELECT id FROM chart_of_accounts WHERE account_type = 'INCOME' ORDER BY id LIMIT 1").await;
    let item = crate::sales::create_product(&pool, crate::sales::ProductInput { name: "Logo design".into(), description: None, rate_cents: 50_000, income_account_id: income, is_taxable: false })
        .await
        .unwrap()
        .id;
    let account = first(&pool, "SELECT id FROM chart_of_accounts WHERE account_number = '1010'").await;
    let reconciliation = first(&pool, "SELECT id FROM reconciliations ORDER BY id LIMIT 1").await;
    for (kind, id) in [("CUSTOMER", customer), ("VENDOR", vendor), ("ITEM", item), ("ACCOUNT", account), ("RECONCILIATION", reconciliation)] {
        let file = add_record_file(&pool, kind, id, &format!("/scans/{kind}.pdf"), b"%PDF synthetic").await.unwrap();
        assert_eq!((file.file_name.clone(), file.media_type.as_str()), (format!("{kind}.pdf"), "application/pdf"));
        assert_eq!(record_files(&pool, kind, id).await.unwrap().len(), 1, "{kind}");
    }
    assert!(add_record_file(&pool, "CUSTOMER", 99_999, "x.pdf", b"x").await.unwrap_err().starts_with("ATTACH_NO_RECORD"));
    assert!(add_record_file(&pool, "INVOICE", customer, "x.pdf", b"x").await.unwrap_err().starts_with("ATTACH_KIND"));
    assert!(add_record_file(&pool, "CUSTOMER", customer, "x.pdf", b"").await.unwrap_err().starts_with("ATTACH_EMPTY"));

    // A file on a posted transaction is in the library too, labelled by what it's attached to.
    let (entry, number): (i64, i64) = sqlx::query_as("SELECT journal_entry_id, invoice_number FROM invoices WHERE journal_entry_id IS NOT NULL ORDER BY id LIMIT 1").fetch_one(&pool).await.unwrap();
    crate::workflows::add_attachment(&pool, entry, "signed.png", b"png").await.unwrap();
    let all = library(&pool, None, false).await.unwrap();
    assert_eq!(all.len(), 6);
    let invoice = all.iter().find(|f| f.source == "ENTRY").unwrap();
    assert_eq!((invoice.attached_kind.as_str(), invoice.attached_number.clone()), ("INVOICE", Some(number.to_string())));
    assert!(invoice.attached_name.is_some() && invoice.attached_date.is_some());
    let on_vendor = all.iter().find(|f| f.attached_kind == "VENDOR").unwrap();
    let vendor_name: String = sqlx::query_scalar("SELECT name FROM vendors WHERE id = ?1").bind(vendor).fetch_one(&pool).await.unwrap();
    assert_eq!(on_vendor.attached_name.as_deref(), Some(vendor_name.as_str()));

    // Search: file name, keywords and what it's attached to, case ignored.
    let r = FileRef { source: Source::Record, id: on_vendor.id };
    set_details(&pool, r, Some("  W-9 2025 "), true).await.unwrap();
    assert_eq!(library(&pool, Some("w-9"), false).await.unwrap().iter().map(|f| f.id).collect::<Vec<_>>(), vec![on_vendor.id]);
    assert_eq!(library(&pool, Some(&vendor_name.to_uppercase()), false).await.unwrap().len(), 1);
    assert_eq!(library(&pool, Some("signed"), false).await.unwrap().len(), 1);
    assert_eq!(library(&pool, Some("purchase order"), false).await.unwrap().len(), 0);
    assert_eq!(library(&pool, Some("reconciliation"), false).await.unwrap().len(), 1);
    let detailed = library(&pool, Some("w-9"), false).await.unwrap().remove(0);
    assert_eq!((detailed.keywords.as_deref(), detailed.send_with_form), (Some("W-9 2025"), true));
}

#[tokio::test]
async fn deleting_is_reversible_and_only_a_deleted_file_is_removed_for_good() {
    let pool = crate::harness::fixture::harbor_lane().await;
    let entry: i64 = first(&pool, "SELECT journal_entry_id FROM invoices WHERE journal_entry_id IS NOT NULL ORDER BY id LIMIT 1").await;
    let on_entry = crate::workflows::add_attachment(&pool, entry, "receipt.jpg", b"jpg").await.unwrap();
    let file = FileRef { source: Source::Entry, id: on_entry.id };

    // Not deleted yet: the schema refuses removing it for good.
    assert!(purge(&pool, file).await.unwrap_err().starts_with("ATTACH_NOT_DELETED"));
    let direct = sqlx::query("DELETE FROM attachments WHERE id = ?1").bind(file.id).execute(&pool).await.unwrap_err();
    assert!(direct.to_string().contains("ATTACH_NOT_DELETED"), "{direct}");

    // Delete (as the transaction's × does): hidden from the transaction and the library, kept.
    crate::workflows::delete_attachment(&pool, file.id).await.unwrap();
    assert!(crate::workflows::list_attachments(&pool, entry).await.unwrap().is_empty());
    assert!(!crate::workflows::attachment_counts(&pool).await.unwrap().contains_key(&entry));
    assert!(library(&pool, None, false).await.unwrap().is_empty());
    let deleted = library(&pool, None, true).await.unwrap();
    assert!(deleted[0].deleted_at.is_some());

    restore(&pool, file).await.unwrap();
    assert_eq!(crate::workflows::list_attachments(&pool, entry).await.unwrap().len(), 1);

    delete(&pool, file).await.unwrap();
    purge(&pool, file).await.unwrap();
    assert!(library(&pool, None, true).await.unwrap().is_empty());
    assert!(content(&pool, file).await.unwrap_err().starts_with("ATTACH_NOT_FOUND"));
}

#[tokio::test]
async fn files_marked_to_send_go_with_their_form() {
    use crate::forms::FormKind;
    let pool = crate::harness::fixture::harbor_lane().await;
    let (invoice, entry): (i64, i64) = sqlx::query_as("SELECT id, journal_entry_id FROM invoices WHERE journal_entry_id IS NOT NULL ORDER BY id LIMIT 1").fetch_one(&pool).await.unwrap();
    let customer: i64 = sqlx::query_scalar("SELECT customer_id FROM invoices WHERE id = ?1").bind(invoice).fetch_one(&pool).await.unwrap();
    let signed = crate::workflows::add_attachment(&pool, entry, "signed.png", b"png").await.unwrap();
    crate::workflows::add_attachment(&pool, entry, "internal note.txt", b"not for the customer").await.unwrap();
    assert!(files_to_send(&pool, FormKind::Invoice, invoice).await.unwrap().is_empty(), "nothing is sent unless marked");

    set_details(&pool, FileRef { source: Source::Entry, id: signed.id }, None, true).await.unwrap();
    let sent = files_to_send(&pool, FormKind::Invoice, invoice).await.unwrap();
    assert_eq!(sent, vec![("signed.png".to_string(), "image/png".to_string(), b"png".to_vec())]);

    // A deleted file isn't sent; a customer's marked file goes with their statement.
    delete(&pool, FileRef { source: Source::Entry, id: signed.id }).await.unwrap();
    assert!(files_to_send(&pool, FormKind::Invoice, invoice).await.unwrap().is_empty());
    let terms = add_record_file(&pool, "CUSTOMER", customer, "terms.pdf", b"%PDF").await.unwrap();
    set_details(&pool, FileRef { source: Source::Record, id: terms.id }, Some("terms"), true).await.unwrap();
    assert_eq!(files_to_send(&pool, FormKind::Statement, customer).await.unwrap().len(), 1);
}
