use super::*;
use crate::db::test_pool;

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn obe(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE name = 'Opening Balance Equity'").fetch_one(pool).await.unwrap()
}

async fn balance(pool: &SqlitePool, account_id: i64) -> i64 {
    sqlx::query_scalar(
        "SELECT COALESCE(SUM(l.debit_cents - l.credit_cents), 0) FROM journal_lines l
             JOIN journal_entries e ON e.id = l.journal_entry_id WHERE l.account_id = ?1 AND e.status = 'POSTED'",
    )
    .bind(account_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

fn line(invoice: &str, date: Option<&str>, cents: i64, label: &str, offset: i64) -> RawLine {
    RawLine {
        qb_invoice_id: invoice.into(),
        qb_item_id: None,
        date: date.map(|d| d.parse().unwrap()),
        amount_cents: cents,
        label: label.into(),
        amount_type: "TYPE_02",
        page_number: 7,
        page_offset: offset,
    }
}

fn extraction(lines: Vec<RawLine>) -> Extraction {
    Extraction { file_size_bytes: 4096 * 8, page_count: 8, attributed_tables: 2, header_count: 3, skipped_line_count: 1, ignored_line_count: 4, lines }
}

const INVENTORY: &str = "abmc_invoice_inventory_lineitem";
const SERVICE: &str = "abmc_invoice_service_lineitem";

/// Extraction check for a real .qbw file (read-only): set LOCALBOOKS_QBW_CHECK to its
/// path and run `cargo test -- --ignored extracts_a_real --nocapture`.
#[test]
#[ignore]
fn extracts_a_real_qbw_file() {
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK to a .qbw file");
    let mut stages = Vec::new();
    let started = std::time::Instant::now();
    let x = extract(Path::new(&path), &mut |p| {
        if stages.last() != Some(&p.stage) {
            stages.push(p.stage);
        }
    })
    .expect("extracts");
    let mut by_label: BTreeMap<&str, (usize, i64)> = BTreeMap::new();
    let mut invoices = std::collections::HashSet::new();
    let (mut dated, mut first, mut last) = (0, None::<NaiveDate>, None::<NaiveDate>);
    for l in &x.lines {
        let e = by_label.entry(l.label.as_str()).or_default();
        e.0 += 1;
        e.1 += l.amount_cents;
        invoices.insert(l.qb_invoice_id.as_str());
        if let Some(d) = l.date {
            dated += 1;
            first = Some(first.map_or(d, |f| f.min(d)));
            last = Some(last.map_or(d, |v| v.max(d)));
        }
    }
    println!("file: {path}");
    println!("read in {:.1}s; stages: {stages:?}", started.elapsed().as_secs_f64());
    println!(
        "pages {}, attributed tables {}, headers {}, lines {}, invoices {}, undecodable {}, ignored (not invoice tables) {}",
        x.page_count, x.attributed_tables, x.header_count, x.lines.len(), invoices.len(), x.skipped_line_count, x.ignored_line_count
    );
    println!("dated lines {dated}, range {first:?} .. {last:?}");
    for (label, (n, cents)) in &by_label {
        println!("  {:<45} {:>6} lines  {:>16}", display_label(label), n, crate::ledger::format_cents(*cents));
    }
}

#[test]
fn labels_read_like_source_line_types() {
    assert_eq!(display_label(INVENTORY), "Invoice inventory line items");
    assert_eq!(display_label(UNATTRIBUTED), "Unattributed invoice lines");
    assert_eq!(unix_day_to_date(0), NaiveDate::from_ymd_opt(1970, 1, 1));
    assert_eq!(unix_day_to_date(-1), NaiveDate::from_ymd_opt(1969, 12, 31));
}

#[test]
fn files_that_are_not_company_files_are_rejected_cleanly() {
    let dir = std::env::temp_dir().join(format!("localbooks-qbw-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let qbb = dir.join("books.qbb");
    std::fs::write(&qbb, b"PK\x03\x04 compressed backup").unwrap();
    let err = extract(&qbb, &mut |_| {}).unwrap_err();
    assert!(err.starts_with("QBW_COMPRESSED_BACKUP"), "{err}");

    let fake = dir.join("fake.qbw");
    std::fs::write(&fake, vec![0x5Au8; 4096 * 4]).unwrap();
    let err = extract(&fake, &mut |_| {}).unwrap_err();
    assert!(err.starts_with("QBW_NOT_COMPANY_FILE"), "{err}");

    let err = extract(&dir.join("missing.qbw"), &mut |_| {}).unwrap_err();
    assert!(err.starts_with("QBW_READ"), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn extractions_with_no_invoice_lines_are_refused() {
    let path = Path::new("/books/Books.qbw");
    let mut x = extraction(vec![]);
    x.header_count = 0;
    let err = ensure_found(path, &x).unwrap_err();
    assert!(err.starts_with("QBW_UNSUPPORTED_VERSION"), "{err}");
    assert!(err.contains("Books.qbw"), "{err}");

    x.header_count = 12;
    let err = ensure_found(path, &x).unwrap_err();
    assert!(err.starts_with("QBW_NO_INVOICES"), "{err}");

    let x = extraction(vec![line("INV0000000000001", Some("2012-03-05"), 125_000, INVENTORY, 1)]);
    assert_eq!(ensure_found(path, &x), Ok(()));
}

#[tokio::test]
async fn staged_lines_post_as_balanced_entries_once() {
    let pool = test_pool().await;
    let lines = vec![
        line("INV0000000000001", Some("2012-03-05"), 125_000, INVENTORY, 1),
        line("INV0000000000001", Some("2012-03-04"), 40_050, SERVICE, 2),
        line("INV0000000000001", None, -5_050, SERVICE, 3),
        line("INV0000000000002", Some("2012-04-01"), 9_999, SERVICE, 4),
        line("INV0000000000003", None, 1_000, SERVICE, 5),
        line("INV0000000000004", Some("2012-05-01"), 2_500, INVENTORY, 6),
        line("INV0000000000004", Some("2012-05-01"), -2_500, INVENTORY, 7),
    ];
    let batch = stage(&pool, Path::new("/books/rock castle.qbw"), &extraction(lines)).await.unwrap();
    let summary = batch_summary(&pool, batch).await.unwrap();
    assert_eq!((summary.line_count, summary.invoice_count, summary.undated_invoice_count), (7, 4, 1));
    assert_eq!(summary.file_name, "rock castle.qbw");
    assert_eq!(summary.labels.len(), 2);
    assert!(summary.labels.iter().all(|l| l.account_id.is_none()));

    let offset = obe(&pool).await;
    let input = PostBatchInput { batch_id: batch, offset_account_id: offset, undated_fallback: None };
    assert!(post_batch(&pool, &input).await.unwrap_err().starts_with("QBW_UNMAPPED"), "unmapped labels block posting");

    let sales = account(&pool, "4000").await;
    let checking = account(&pool, "1010").await;
    assert!(set_mapping(&pool, batch, INVENTORY, Some(checking)).await.unwrap_err().starts_with("QBW_BAD_ACCOUNT"));
    set_mapping(&pool, batch, INVENTORY, Some(sales)).await.unwrap();
    assert_eq!(auto_create_accounts(&pool, batch).await.unwrap(), 1, "only the unmapped label gets an account");
    let summary = batch_summary(&pool, batch).await.unwrap();
    let service = summary.labels.iter().find(|l| l.raw_label == SERVICE).unwrap();
    assert_eq!(service.account_name.as_deref(), Some("Imported invoice service line items"));
    let services_acct = service.account_id.unwrap();
    let number: String = sqlx::query_scalar("SELECT account_number FROM chart_of_accounts WHERE id = ?1").bind(services_acct).fetch_one(&pool).await.unwrap();
    assert_eq!(number, "4500");

    let bad = PostBatchInput { offset_account_id: sales, ..input.clone() };
    assert!(post_batch(&pool, &bad).await.unwrap_err().starts_with("QBW_BAD_OFFSET"));

    let result = post_batch(&pool, &input).await.unwrap();
    assert_eq!(
        result,
        PostBatchResult { entries_posted: 2, lines_posted: 4, total_cents: 169_999, skipped_already_posted: 0, skipped_undated: 1, skipped_zero_total: 1 }
    );
    assert_eq!(balance(&pool, sales).await, -125_000);
    assert_eq!(balance(&pool, services_acct).await, -(40_050 - 5_050 + 9_999));
    assert_eq!(balance(&pool, offset).await, 169_999);
    let first_date: String = sqlx::query_scalar(
        "SELECT e.entry_date FROM journal_entries e JOIN qbw_posted_invoices p ON p.journal_entry_id = e.id WHERE p.qb_invoice_id = 'INV0000000000001'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(first_date, "2012-03-04", "an invoice dates from its earliest line");

    // The undated invoice posts once a fallback date is given; nothing posts twice.
    let with_date = PostBatchInput { undated_fallback: NaiveDate::from_ymd_opt(2012, 12, 31), ..input.clone() };
    let again = post_batch(&pool, &with_date).await.unwrap();
    assert_eq!((again.entries_posted, again.skipped_already_posted, again.skipped_zero_total), (1, 2, 1));

    // A second import of the same file cannot double-post.
    let lines = vec![line("INV0000000000002", Some("2012-04-01"), 9_999, SERVICE, 4)];
    let second = stage(&pool, Path::new("/books/rock castle.qbw"), &extraction(lines)).await.unwrap();
    assert_eq!(batch_summary(&pool, second).await.unwrap().already_posted_invoice_count, 1);
    set_mapping(&pool, second, SERVICE, Some(services_acct)).await.unwrap();
    let dup = post_batch(&pool, &PostBatchInput { batch_id: second, ..input.clone() }).await.unwrap();
    assert_eq!((dup.entries_posted, dup.skipped_already_posted), (0, 1));

    // Voiding an imported entry frees that invoice to be posted again.
    let je: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM qbw_posted_invoices WHERE qb_invoice_id = 'INV0000000000002'").fetch_one(&pool).await.unwrap();
    ledger::void_transaction(&pool, je).await.unwrap();
    let repost = post_batch(&pool, &PostBatchInput { batch_id: second, ..input.clone() }).await.unwrap();
    assert_eq!(repost.entries_posted, 1);
    assert_eq!(balance(&pool, offset).await, 169_999 + 1_000);

    // Reversing the import voids what it posted and retires the account it created.
    assert_eq!(batch_summary(&pool, batch).await.unwrap().live_entry_count, 2, "invoice 2 now lives in the second import");
    assert_eq!(batch_summary(&pool, batch).await.unwrap().ignored_line_count, 4);
    let reversed = reverse_batch(&pool, batch).await.unwrap();
    assert_eq!(reversed, ReverseBatchResult { entries_voided: 2, accounts_deactivated: 0 }, "the second batch still has a live entry on the created account");
    assert_eq!(batch_summary(&pool, batch).await.unwrap().live_entry_count, 0);
    assert!(reverse_batch(&pool, batch).await.unwrap_err().starts_with("QBW_NOTHING_TO_REVERSE"));
    assert_eq!(reverse_batch(&pool, second).await.unwrap(), ReverseBatchResult { entries_voided: 1, accounts_deactivated: 1 });
    assert_eq!(balance(&pool, offset).await, 0, "the books are back where they started");
    assert_eq!(balance(&pool, services_acct).await, 0);
    let active: i64 = sqlx::query_scalar("SELECT is_active FROM chart_of_accounts WHERE id = ?1").bind(services_acct).fetch_one(&pool).await.unwrap();
    assert_eq!(active, 0);

    discard_batch(&pool, batch).await.unwrap();
    assert!(batch_summary(&pool, batch).await.unwrap_err().starts_with("QBW_NO_BATCH"));
    assert_eq!(balance(&pool, offset).await, 0, "discarding staging never touches the ledger");
}

#[tokio::test]
async fn a_failure_mid_batch_rolls_back_every_entry() {
    let pool = test_pool().await;
    let lines = vec![
        line("INV0000000000001", Some("2012-03-05"), 1_000, SERVICE, 1),
        line("INV0000000000002", Some("2012-03-06"), 2_000, SERVICE, 2),
    ];
    let batch = stage(&pool, Path::new("/books/a.qbw"), &extraction(lines)).await.unwrap();
    let sales = account(&pool, "4000").await;
    set_mapping(&pool, batch, SERVICE, Some(sales)).await.unwrap();
    // The first invoice posts inside the transaction; the second is forced to fail.
    sqlx::query(
        "CREATE TRIGGER test_fail_second BEFORE INSERT ON qbw_posted_invoices
             WHEN NEW.qb_invoice_id = 'INV0000000000002' BEGIN SELECT RAISE(ABORT, 'INJECTED_FAILURE'); END",
    )
    .execute(&pool)
    .await
    .unwrap();
    let count = || async { sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM journal_entries").fetch_one(&pool).await.unwrap() };
    let entries_before = count().await;
    let offset = obe(&pool).await;
    let input = PostBatchInput { batch_id: batch, offset_account_id: offset, undated_fallback: None };
    assert_eq!(post_batch(&pool, &input).await.unwrap_err(), "INJECTED_FAILURE");
    assert_eq!(count().await, entries_before, "the first invoice's entry rolled back too");
    assert_eq!(balance(&pool, offset).await, 0);
    assert!(batch_summary(&pool, batch).await.unwrap().posted_at.is_none());

    // A mapped account deactivated after mapping is caught before anything is written.
    sqlx::query("DROP TRIGGER test_fail_second").execute(&pool).await.unwrap();
    sqlx::query("UPDATE chart_of_accounts SET is_active = 0 WHERE id = ?1").bind(sales).execute(&pool).await.unwrap();
    assert!(post_batch(&pool, &input).await.unwrap_err().starts_with("QBW_BAD_ACCOUNT"));
}
