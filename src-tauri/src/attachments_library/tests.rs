use super::*;
use crate::money::Cents;

async fn first(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap_or_else(|e| panic!("{sql}: {e}"))
}

#[tokio::test]
async fn files_attach_to_every_kind_of_record_and_show_in_the_library() {
    let pool = crate::harness::fixture::harbor_lane().await;
    let customer = first(&pool, "SELECT id FROM customers ORDER BY id LIMIT 1").await;
    let vendor = first(&pool, "SELECT id FROM vendors ORDER BY id LIMIT 1").await;
    let income = first(&pool, "SELECT id FROM chart_of_accounts WHERE account_type = 'INCOME' ORDER BY id LIMIT 1").await;
    let item = crate::sales::create_product(&pool, crate::sales::ProductInput { name: "Logo design".into(), description: None, rate_cents: Cents(50_000), income_account_id: income, is_taxable: false })
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
    assert_eq!(sent, vec![("signed.png".to_string(), b"png".to_vec(), "image/png")]);

    // A deleted file isn't sent; a customer's marked file goes with their statement.
    delete(&pool, FileRef { source: Source::Entry, id: signed.id }).await.unwrap();
    assert!(files_to_send(&pool, FormKind::Invoice, invoice).await.unwrap().is_empty());
    let terms = add_record_file(&pool, "CUSTOMER", customer, "terms.pdf", b"%PDF").await.unwrap();
    set_details(&pool, FileRef { source: Source::Record, id: terms.id }, Some("terms"), true).await.unwrap();
    assert_eq!(files_to_send(&pool, FormKind::Statement, customer).await.unwrap().len(), 1);
}

#[tokio::test]
async fn save_all_attachments_leaves_deleted_files_out() {
    let pool = crate::harness::fixture::harbor_lane().await;
    let entry: i64 = first(&pool, "SELECT journal_entry_id FROM invoices WHERE journal_entry_id IS NOT NULL ORDER BY id LIMIT 1").await;
    crate::workflows::add_attachment(&pool, entry, "keep.pdf", b"%PDF").await.unwrap();
    let gone = crate::workflows::add_attachment(&pool, entry, "gone.pdf", b"%PDF").await.unwrap();
    crate::workflows::delete_attachment(&pool, gone.id).await.unwrap();
    assert_eq!(crate::attachment_export::count(&pool, Some(entry)).await.unwrap(), 1);
    let (files, _) = crate::attachment_export::write_zip(&pool, None, std::io::Cursor::new(Vec::new())).await.unwrap();
    assert_eq!(files, 1);
}

#[tokio::test]
async fn migration_0096_moves_statement_files_into_the_library() {
    use std::borrow::Cow;
    let pool = sqlx::sqlite::SqlitePoolOptions::new().max_connections(1).connect("sqlite::memory:").await.unwrap();
    let mut before = sqlx::migrate::Migrator::new(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations"))).await.unwrap();
    before.migrations = Cow::Owned(before.migrations.iter().filter(|m| m.version < 96).cloned().collect());
    crate::db::migrate_with(&pool, &before).await.unwrap();
    let checking: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '1010'").fetch_one(&pool).await.unwrap();
    let rec: i64 = sqlx::query_scalar("INSERT INTO reconciliations (account_id, statement_end_date, statement_ending_balance_cents, beginning_balance_cents) VALUES (?1, '2026-08-31', 0, 0) RETURNING id")
        .bind(checking)
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO reconciliation_statements (reconciliation_id, file_name, media_type, size_bytes, content, created_at) VALUES (?1, 'August.pdf', 'application/pdf', 4, x'25504446', '2026-09-02T10:00:00.000Z')")
        .bind(rec)
        .execute(&pool)
        .await
        .unwrap();

    crate::db::migrate(&pool).await.unwrap();
    let files = record_files(&pool, "RECONCILIATION", rec).await.unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!((files[0].file_name.as_str(), files[0].media_type.as_str(), files[0].created_at.as_str()), ("August.pdf", "application/pdf", "2026-09-02T10:00:00.000Z"));
    assert_eq!(content(&pool, FileRef { source: Source::Record, id: files[0].id }).await.unwrap().2, b"%PDF".to_vec());
    let old: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_schema WHERE name = 'reconciliation_statements'").fetch_one(&pool).await.unwrap();
    assert_eq!(old, 0, "the old table is gone");
}

#[tokio::test]
async fn a_preview_reads_only_what_it_shows() {
    let pool = crate::harness::fixture::harbor_lane().await;
    let customer = first(&pool, "SELECT id FROM customers ORDER BY id LIMIT 1").await;
    let text = add_record_file(&pool, "CUSTOMER", customer, "notes.txt", b"0123456789").await.unwrap();
    let picture = add_record_file(&pool, "CUSTOMER", customer, "logo.png", b"png bytes").await.unwrap();
    let pdf = add_record_file(&pool, "CUSTOMER", customer, "scan.pdf", b"%PDF synthetic").await.unwrap();
    let sheet = add_record_file(&pool, "CUSTOMER", customer, "prices.xlsx", b"PK zip").await.unwrap();
    let shown = |id| preview(&pool, FileRef { source: Source::Record, id }, 4);
    assert_eq!(shown(text.id).await.unwrap().2, Some(b"0123".to_vec()), "a text file is cut in SQL");
    assert_eq!(shown(picture.id).await.unwrap().2, Some(b"png bytes".to_vec()), "a picture is whole");
    let (name, media, bytes) = shown(pdf.id).await.unwrap();
    assert_eq!((name.as_str(), media.as_str(), bytes.as_deref()), ("scan.pdf", "application/pdf", Some(&b"%PDF synthetic"[..])), "a PDF is whole: the screen draws its pages");
    assert_eq!(shown(sheet.id).await.unwrap().2, None, "other kinds aren't read");
    assert!(shown(99_999).await.unwrap_err().starts_with("ATTACH_NOT_FOUND"));
}
