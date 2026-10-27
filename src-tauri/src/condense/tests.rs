use super::*;
use crate::closing::ClosingInput;
use crate::db::{test_config, Db};
use crate::permissions::Need;
use serde_json::json;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

/// Late enough that every seeded date is in the past.
fn today() -> NaiveDate {
    d(2026, 10, 26)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("localbooks-condense-{tag}-{}-{}", std::process::id(), Local::now().format("%H%M%S%f")));
        std::fs::create_dir_all(&dir).unwrap();
        // This test's own record of audit-trail heads, so the chain is checked against one.
        crate::audit_chain::install_test_anchor_store(dir.join("anchors.json"));
        Self(dir)
    }

    fn company(&self) -> PathBuf {
        self.0.join("Company.localbooks")
    }

    async fn open(&self) -> Db {
        let db = Db::open_at(self.company(), test_config(&self.0)).await.unwrap();
        db.refresh_gate().await;
        db
    }

    fn files(&self, kind: &str) -> Vec<BackupFile> {
        backup::list_backups(&self.0.join("backups")).unwrap().into_iter().filter(|b| b.kind == kind).collect()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn close_books(pool: &SqlitePool, date: NaiveDate, passcode: Option<&str>) {
    crate::closing::set_closing_date(pool, &ClosingInput { lock_date: Some(date), current_passcode: passcode.map(Into::into), new_passcode: None }).await.unwrap();
}

/// Clears every line on `account_id` dated on or before `through` in one completed reconciliation.
async fn reconcile(pool: &SqlitePool, account_id: i64, through: NaiveDate) {
    let lines: Vec<_> = crate::reconcile::unreconciled_lines(pool, account_id)
        .await
        .unwrap()
        .into_iter()
        .filter(|l| l.entry_date.as_str() <= through.to_string().as_str())
        .collect();
    if lines.is_empty() {
        return;
    }
    let cleared = crate::reconcile::account_state(pool, account_id).await.unwrap().reconciled_balance_cents;
    let ending = cleared + lines.iter().map(|l| l.amount_cents).sum::<i64>();
    let rec = crate::reconcile::start(pool, account_id, through, ending).await.unwrap();
    let ids: Vec<i64> = lines.iter().map(|l| l.line_id).collect();
    assert_eq!(crate::reconcile::set_lines_cleared(pool, rec.id, &ids, true).await.unwrap().difference_cents, 0);
    crate::reconcile::lock(pool, rec.id).await.unwrap();
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

/// What a person compares before and after: balances, aging, inventory, reconciliation.
async fn books_picture(pool: &SqlitePool, cutoff: NaiveDate) -> serde_json::Value {
    let mut picture = serde_json::Map::new();
    picture.insert("trialBalanceAtCutoff".into(), json!(balances_through(pool, cutoff).await.unwrap().into_iter().collect::<Vec<_>>()));
    picture.insert("trialBalanceToday".into(), json!(balances_through(pool, today()).await.unwrap().into_iter().collect::<Vec<_>>()));
    picture.insert("monthly".into(), json!(monthly_balances(pool).await.unwrap().into_iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>()));
    for date in [cutoff, today()] {
        picture.insert(format!("ar-{date}"), serde_json::to_value(crate::report_center::aging(pool, false, date).await.unwrap()).unwrap());
        picture.insert(format!("ap-{date}"), serde_json::to_value(crate::report_center::aging(pool, true, date).await.unwrap()).unwrap());
        picture.insert(format!("stock-{date}"), serde_json::to_value(crate::report_center::inventory_valuation(pool, date).await.unwrap()).unwrap());
    }
    picture.insert("reconcile".into(), json!(reconcile_state(pool).await.unwrap().into_iter().collect::<Vec<_>>()));
    picture.insert("trialBalanceReport".into(), serde_json::to_value(crate::reports::trial_balance(pool).await.unwrap()).unwrap());
    serde_json::Value::Object(picture)
}

async fn assert_audit_intact(pool: &SqlitePool, what: &str) {
    let audit = ledger::verify_ledger_audit(pool).await.unwrap();
    assert!(audit.intact, "{what}: {:?}", audit.findings);
    let chain = crate::audit_chain::verify(pool).await.unwrap();
    assert!(chain.problems.is_empty(), "{what}: {:?}", chain.problems);
}

/// Harbor Lane Design Co. (the golden fixture) in a company file, with its first quarter closed.
async fn harbor_lane(tmp: &TempDir) -> Db {
    let pool = crate::db::open(&tmp.company()).await.unwrap();
    let pool = crate::harness::fixture::harbor_lane_in(pool).await;
    close_books(&pool, d(2025, 3, 31), None).await;
    pool.close().await;
    tmp.open().await
}

#[tokio::test]
async fn every_table_is_either_a_list_or_a_transaction() {
    let pool = crate::db::test_pool().await;
    let mut conn = pool.acquire().await.unwrap();
    check_tables_known(&mut conn).await.unwrap();
    for (table, dates) in TRANSACTION_TABLES {
        assert!(!KEPT_TABLES.contains(table), "{table} is in both lists");
        for column in *dates {
            let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)").bind(table).bind(column).fetch_one(&mut *conn).await.unwrap();
            assert!(exists, "{table}.{column} dates the table's rows");
        }
    }
    for list in [TRANSACTION_TABLES.iter().map(|t| t.0).collect::<Vec<_>>(), KEPT_TABLES.to_vec()] {
        let mut sorted = list.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted, list, "listed once each, alphabetically");
    }
    // A new table makes condensing refuse until it is classified.
    sqlx::query("CREATE TABLE brand_new (id INTEGER PRIMARY KEY)").execute(&mut *conn).await.unwrap();
    assert!(check_tables_known(&mut conn).await.unwrap_err().starts_with("CONDENSE_UNKNOWN_TABLE"));
}

#[tokio::test]
async fn condensing_harbor_lane_keeps_every_balance_and_open_item() {
    let tmp = TempDir::new("harbor");
    let db = harbor_lane(&tmp).await;
    let cutoff = d(2025, 3, 31);
    let pool = db.pool().await;
    let before = books_picture(&pool, cutoff).await;
    let entries_before = count(&pool, "SELECT COUNT(*) FROM journal_entries").await;
    let invoices_before = count(&pool, "SELECT COUNT(*) FROM invoices").await;
    let open_invoices: Vec<i64> = sqlx::query_scalar("SELECT id FROM invoices WHERE total_cents > amount_paid_cents AND is_voided = 0 ORDER BY id").fetch_all(&pool).await.unwrap();

    let preview = preview(&pool, cutoff).await.unwrap();
    assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
    assert!(preview.entries_summarized > 0 && preview.summary_entries > 0, "{preview:?}");
    assert_eq!(preview.entries_before_cutoff, preview.entries_summarized + preview.kept.iter().map(|k| k.entries).sum::<i64>());

    let result = condense_live_on(&db, cutoff, today()).await.unwrap();
    assert!(result.verification.passed, "{:?}", result.verification);
    assert_eq!(result.archive.kind, "ARCHIVE");
    assert_eq!(tmp.files("ARCHIVE").len(), 1);
    assert_eq!(tmp.files("PRE_CONDENSE").len(), 1);

    // The live file is the condensed one: fewer entries, same picture, valid audit trail.
    let pool = db.pool().await;
    assert_eq!(db.path().await.unwrap(), tmp.company(), "same company file, condensed in place");
    let entries_after = count(&pool, "SELECT COUNT(*) FROM journal_entries").await;
    assert_eq!(entries_after, entries_before - preview.entries_summarized + preview.summary_entries);
    assert!(count(&pool, "SELECT COUNT(*) FROM invoices").await < invoices_before);
    assert_eq!(books_picture(&pool, cutoff).await, before);
    let still_open: Vec<i64> = sqlx::query_scalar("SELECT id FROM invoices WHERE total_cents > amount_paid_cents AND is_voided = 0 ORDER BY id").fetch_all(&pool).await.unwrap();
    assert_eq!(still_open, open_invoices, "every open invoice is carried as it was");
    assert_audit_intact(&pool, "condensed file").await;
    let summaries = count(&pool, "SELECT COUNT(*) FROM journal_entries WHERE memo LIKE 'Condensed: %' AND entry_date <= '2025-03-31'").await;
    assert_eq!(summaries, preview.summary_entries);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM ledger_audit_log WHERE action_type = 'CONDENSE'").await, 1);
    let report = file_dentist::checkup(&pool, 0).await.unwrap();
    assert!(report.checks.iter().all(|c| c.status != CheckStatus::Fail), "{:?}", report.checks);
    // New entries keep working after the condense, in the open period.
    let (checking, services) = (account(&pool, "1010").await, account(&pool, "4100").await);
    ledger::post_journal_entry(
        &pool,
        ledger::JournalEntry { id: None, date: d(2025, 4, 2), description: "After".into(), lines: vec![ledger::JournalLine::debit(checking, 100), ledger::JournalLine::credit(services, 100)], currency: None },
    )
    .await
    .unwrap();
    assert_audit_intact(&pool, "condensed file after a new entry").await;
    let status = status(&pool).await.unwrap();
    assert_eq!(status.condensed_through.as_deref(), Some("2025-03-31"));
    assert!(status.archive_exists && !status.is_archive);

    // The archive keeps the full history, verifies, and opens read-only.
    let archive = PathBuf::from(status.archive_path.unwrap());
    db.mount(&archive).await.unwrap();
    let archive_pool = db.pool().await;
    assert_eq!(count(&archive_pool, "SELECT COUNT(*) FROM journal_entries").await, entries_before);
    assert_eq!(books_picture(&archive_pool, cutoff).await, before);
    assert_audit_intact(&archive_pool, "archive").await;
    assert!(super::status(&archive_pool).await.unwrap().is_archive);
    assert_eq!(db.check(Need::Write, true).unwrap_err(), ARCHIVE_READ_ONLY);
    assert_eq!(db.check(Need::Admin, true).unwrap_err(), ARCHIVE_READ_ONLY);
    db.check(Need::Read, false).unwrap();
    assert_eq!(preview_pool(&archive_pool, cutoff, today()).await.unwrap_err(), ARCHIVE_READ_ONLY);
    db.mount(&tmp.company()).await.unwrap();
    db.check(Need::Write, true).unwrap();
}

#[tokio::test]
async fn a_to_do_keeps_the_invoice_it_is_about() {
    let tmp = TempDir::new("todo");
    let db = harbor_lane(&tmp).await;
    let cutoff = d(2025, 3, 31);
    let pool = db.pool().await;
    let removed = |p: &CondensePreview| p.rows_removed.get("invoices").copied().unwrap_or(0);
    let without = removed(&preview(&pool, cutoff).await.unwrap());
    // A paid invoice from the condensed months that a to-do points at (link_kind has no foreign key).
    let paid: Vec<i64> = sqlx::query_scalar("SELECT id FROM invoices WHERE issue_date <= '2025-03-31' AND amount_paid_cents = total_cents ORDER BY id").fetch_all(&pool).await.unwrap();
    let mut linked = None;
    for id in paid {
        let todo: i64 = sqlx::query_scalar("INSERT INTO todos (title, link_kind, link_id) VALUES ('Send a thank-you', 'INVOICE', ?1) RETURNING id").bind(id).fetch_one(&pool).await.unwrap();
        if removed(&preview(&pool, cutoff).await.unwrap()) == without - 1 {
            linked = Some((id, todo));
            break;
        }
        sqlx::query("DELETE FROM todos WHERE id = ?1").bind(todo).execute(&pool).await.unwrap();
    }
    let (invoice, todo) = linked.expect("a paid invoice that condensing would otherwise remove");

    let result = condense_live_on(&db, cutoff, today()).await.unwrap();
    assert!(result.verification.passed, "{:?}", result.verification);
    let pool = db.pool().await;
    assert_eq!(count(&pool, &format!("SELECT COUNT(*) FROM invoices WHERE id = {invoice}")).await, 1);
    assert_eq!(count(&pool, &format!("SELECT COUNT(*) FROM todos WHERE id = {todo} AND link_id = {invoice}")).await, 1);
}

#[tokio::test]
async fn condensing_twice_keeps_both_records_verifiable() {
    let tmp = TempDir::new("twice");
    let db = harbor_lane(&tmp).await;
    condense_live_on(&db, d(2025, 3, 31), today()).await.unwrap();

    let pool = db.pool().await;
    let (checking, card) = (account(&pool, "1010").await, account(&pool, "2100").await);
    reconcile(&pool, checking, d(2025, 6, 30)).await;
    reconcile(&pool, card, d(2025, 6, 30)).await;
    close_books(&pool, d(2025, 6, 30), None).await;
    let before = books_picture(&pool, d(2025, 6, 30)).await;
    let result = condense_live_on(&db, d(2025, 6, 30), today()).await.unwrap();
    assert!(result.verification.passed, "{:?}", result.verification);

    let pool = db.pool().await;
    assert_eq!(books_picture(&pool, d(2025, 6, 30)).await, before);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM ledger_audit_log WHERE action_type = 'CONDENSE'").await, 2);
    assert_audit_intact(&pool, "twice-condensed file").await;
    assert_eq!(tmp.files("ARCHIVE").len(), 2);
}

#[tokio::test]
async fn condensing_edge_cases_keeps_users_keys_and_open_documents() {
    let tmp = TempDir::new("edge");
    crate::test_companies::edge_cases(&tmp.company()).await;
    let pool = crate::db::open(&tmp.company()).await.unwrap();
    for number in ["1010", "1020", "2100"] {
        reconcile(&pool, account(&pool, number).await, d(2024, 12, 31)).await;
    }
    pool.close().await;
    let db = tmp.open().await;
    let pool = db.pool().await;
    let cutoff = d(2024, 12, 31);
    let before = books_picture(&pool, cutoff).await;
    let keys = count(&pool, "SELECT COUNT(*) FROM key_slots").await;

    let result = condense_live_on(&db, cutoff, today()).await.unwrap();
    assert!(result.verification.passed, "{:?}", result.verification);
    assert!(result.preview.entries_summarized > 0);
    // The partly paid job invoice and the year-end invoice stay open documents.
    assert!(result.preview.kept.iter().any(|k| k.reason == KeepReason::OpenBalance), "{:?}", result.preview.kept);

    let pool = db.pool().await;
    assert_eq!(books_picture(&pool, cutoff).await, before);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM key_slots").await, keys);
    assert_audit_intact(&pool, "condensed Edge Cases").await;
    // Users still sign in to the condensed file, with their encryption keys.
    let session = crate::users::sign_in(&pool, "owner", crate::test_companies::ADMIN_PASSWORD).await.unwrap();
    assert!(session.key.is_some());
}

#[tokio::test]
async fn inventory_is_carried_at_quantity_and_cost() {
    let tmp = TempDir::new("stock");
    let pool = crate::db::open(&tmp.company()).await.unwrap();
    let (sales, checking) = (account(&pool, "4000").await, account(&pool, "1010").await);
    let product = crate::sales::create_product(&pool, crate::sales::ProductInput { name: "Widget".into(), description: None, rate_cents: 2_500, income_account_id: sales, is_taxable: false })
        .await
        .unwrap()
        .id;
    let tracked = crate::inventory::track_product(&pool, &crate::inventory::TrackInput { product_id: product, sku: None, asset_account_id: None, cogs_account_id: None }).await.unwrap();
    let buy = |date: &str, qty: i64, rate: i64| {
        let pool = pool.clone();
        let date = date.to_string();
        async move {
            let input: crate::expenses::BillInput = serde_json::from_value(json!({
                "vendorName": "Supplier", "billNumber": null, "billDate": date, "dueDate": date, "memo": null, "totalCents": qty * rate,
                "lines": [{ "accountId": tracked.asset_account_id, "amountCents": qty * rate, "memo": null, "quantityE4": qty * 10_000, "rateCents": rate, "productServiceId": product }]
            }))
            .unwrap();
            let bill = crate::expenses::post_vendor_bill(&pool, input).await.unwrap();
            let paid = crate::expenses::BillPaymentInput { bill_id: bill.id, payment_account_id: checking, payment_date: NaiveDate::parse_from_str(&date, "%Y-%m-%d").unwrap(), amount_cents: qty * rate, reference: None };
            crate::expenses::pay_vendor_bill(&pool, paid).await.unwrap();
        }
    };
    let sell = |pool: &SqlitePool, date: &str, qty: i64| {
        let pool = pool.clone();
        let date = date.to_string();
        async move {
            let input: crate::sales::SalesReceiptInput = serde_json::from_value(json!({
                "customerName": null, "receiptDate": date, "depositAccountId": checking, "paymentMethod": "CASH", "reference": null, "memo": null,
                "lines": [{ "productServiceId": product, "description": "Widget", "quantityE4": qty * 10_000, "rateCents": 2_500, "incomeAccountId": sales }]
            }))
            .unwrap();
            crate::sales::post_sales_receipt(&pool, input).await.unwrap();
        }
    };
    buy("2024-02-01", 10, 1_000).await;
    sell(&pool, "2024-03-01", 4).await;
    buy("2024-06-01", 5, 1_300).await;
    sell(&pool, "2024-09-01", 3).await;
    buy("2025-02-01", 2, 1_500).await;
    sell(&pool, "2025-03-01", 6).await;
    reconcile(&pool, checking, d(2024, 12, 31)).await;
    close_books(&pool, d(2024, 12, 31), None).await;
    pool.close().await;

    let db = tmp.open().await;
    let pool = db.pool().await;
    let cutoff = d(2024, 12, 31);
    let before = books_picture(&pool, cutoff).await;
    let item_before: (i64, i64) = sqlx::query_as("SELECT qty_on_hand_e4, value_cents FROM inventory_items WHERE id = ?1").bind(tracked.id).fetch_one(&pool).await.unwrap();
    let result = condense_live_on(&db, cutoff, today()).await.unwrap();
    assert!(result.verification.passed, "{:?}", result.verification);

    let pool = db.pool().await;
    assert_eq!(books_picture(&pool, cutoff).await, before);
    let item_after: (i64, i64) = sqlx::query_as("SELECT qty_on_hand_e4, value_cents FROM inventory_items WHERE id = ?1").bind(tracked.id).fetch_one(&pool).await.unwrap();
    assert_eq!(item_after, item_before);
    // Four 2024 movements became one, carrying 8 on hand at their cost; 2025's are kept.
    let movements: Vec<(String, i64, i64)> =
        sqlx::query_as("SELECT movement_date, qty_after_e4, value_after_cents FROM stock_ledger_entries WHERE item_id = ?1 ORDER BY id").bind(tracked.id).fetch_all(&pool).await.unwrap();
    assert_eq!(movements.len(), 3, "{movements:?}");
    assert_eq!(movements[0].0, "2024-09-01");
    assert_eq!(movements[0].1, 80_000);
    assert_audit_intact(&pool, "condensed shop").await;
    // Selling again continues from the carried quantity and cost.
    sell(&pool, "2025-04-01", 1).await;
    assert_audit_intact(&pool, "condensed shop after a sale").await;
}

#[tokio::test]
async fn five_years_condenses_its_first_three_years() {
    let tmp = TempDir::new("five");
    crate::test_companies::five_years(&tmp.company()).await;
    let pool = crate::db::open(&tmp.company()).await.unwrap();
    let cutoff = d(2023, 12, 31);
    for number in ["1010", "2100"] {
        reconcile(&pool, account(&pool, number).await, cutoff).await;
    }
    close_books(&pool, cutoff, None).await;
    pool.close().await;
    let db = tmp.open().await;
    let pool = db.pool().await;
    let before = books_picture(&pool, cutoff).await;
    let invoices_before = count(&pool, "SELECT COUNT(*) FROM invoices").await;

    let result = condense_live_on(&db, cutoff, today()).await.unwrap();
    assert!(result.verification.passed, "{:?}", result.verification);
    let pool = db.pool().await;
    assert_eq!(books_picture(&pool, cutoff).await, before);
    let invoices_after = count(&pool, "SELECT COUNT(*) FROM invoices").await;
    assert!(invoices_after < invoices_before * 2 / 3, "{invoices_after} of {invoices_before} invoices remain");
    assert!(result.size_after < result.size_before, "{} → {}", result.size_before, result.size_after);
    // Unpaid invoices from the condensed years are still there to collect.
    assert!(count(&pool, "SELECT COUNT(*) FROM invoices WHERE issue_date <= '2023-12-31' AND amount_paid_cents < total_cents").await > 0);
    assert_audit_intact(&pool, "condensed Five Years").await;
}

#[tokio::test]
async fn refuses_when_the_books_cannot_be_archived_as_verified() {
    let tmp = TempDir::new("refuse");
    let db = harbor_lane(&tmp).await;
    let pool = db.pool().await;
    let digest = content_digest(&pool).await.unwrap();

    // Without a closing date, or past it, there is nothing closed to condense.
    let err = condense_live_on(&db, d(2025, 6, 30), today()).await.unwrap_err();
    assert!(err.starts_with("CONDENSE_NOT_READY") && err.contains("closing date"), "{err}");

    // An archive that cannot be written: no change, no files left behind.
    let backups = tmp.0.join("backups");
    std::fs::create_dir_all(&backups).unwrap();
    std::fs::remove_dir_all(&backups).unwrap();
    std::fs::write(&backups, b"not a folder").unwrap();
    let err = condense_live_on(&db, d(2025, 3, 31), today()).await.unwrap_err();
    assert!(err.starts_with("BACKUP_"), "{err}");
    std::fs::remove_file(&backups).unwrap();
    assert_eq!(content_digest(&db.pool().await).await.unwrap(), digest);

    // Books whose audit trail does not verify (a posted amount changed outside LocalBooks).
    let pool = db.pool().await;
    let mut conn = pool.acquire().await.unwrap();
    let trigger: String = sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE name = 'trg_jl_update_draft_only'").fetch_one(&mut *conn).await.unwrap();
    sqlx::query("DROP TRIGGER trg_jl_update_draft_only").execute(&mut *conn).await.unwrap();
    sqlx::query("UPDATE journal_lines SET memo = 'edited' WHERE id = (SELECT MIN(id) FROM journal_lines)").execute(&mut *conn).await.unwrap();
    sqlx::query(&trigger).execute(&mut *conn).await.unwrap();
    drop(conn);
    let tampered = content_digest(&pool).await.unwrap();
    let err = condense_live_on(&db, d(2025, 3, 31), today()).await.unwrap_err();
    assert!(err.starts_with("CONDENSE_AUDIT"), "{err}");
    assert_eq!(content_digest(&db.pool().await).await.unwrap(), tampered, "the books are untouched");
    assert!(tmp.files("ARCHIVE").is_empty() && tmp.files("PRE_CONDENSE").is_empty());
}

#[tokio::test]
async fn refuses_to_switch_when_the_books_changed_meanwhile() {
    let tmp = TempDir::new("changed");
    let db = harbor_lane(&tmp).await;
    let pool = db.pool().await;
    let staged = tmp.0.join("staged.db");
    backup::snapshot(&pool, &tmp.company(), &staged).await.unwrap();
    let digest = content_digest(&pool).await.unwrap();
    crate::preferences::set(&pool, "company_name", "Renamed meanwhile").await.unwrap();
    let err = db.swap_in_unchanged(&staged, &staged, &digest).await.unwrap_err();
    assert!(err.starts_with("CONDENSE_BOOKS_CHANGED"), "{err}");
    assert_eq!(crate::preferences::get(&db.pool().await, "company_name").await.unwrap(), "Renamed meanwhile");
}

#[tokio::test]
async fn a_summary_that_does_not_add_up_is_reported() {
    let pool = crate::db::test_pool().await;
    let (checking, services) = (account(&pool, "1010").await, account(&pool, "4100").await);
    let post = |cents: i64| ledger::post_journal_entry(
        &pool,
        ledger::JournalEntry { id: None, date: d(2025, 1, 5), description: "Sale".into(), lines: vec![ledger::JournalLine::debit(checking, cents), ledger::JournalLine::credit(services, cents)], currency: None },
    );
    let (sale, summary) = (post(1_000).await.unwrap(), post(999).await.unwrap());
    let record = |summarizes: Vec<i64>| json!({ "removed": { "journalEntries": [] }, "summaries": [{ "entryId": summary, "summarizes": summarizes }] }).to_string();
    let mut tx = ledger::begin_write(&pool).await.unwrap();
    sqlx::query("INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot) VALUES ('CONDENSE', 0, ?1)").bind(record(vec![sale])).execute(&mut *tx).await.unwrap();
    ledger::commit_write(tx).await.unwrap();
    let audit = ledger::verify_ledger_audit(&pool).await.unwrap();
    assert!(audit.findings.iter().any(|f| f.entry_id == summary && f.problem.starts_with("Condensed into a summary")), "{:?}", audit.findings);
}

#[tokio::test]
async fn a_condensed_entry_is_accepted_only_when_a_condense_record_lists_it() {
    let pool = crate::db::test_pool().await;
    let (checking, services) = (account(&pool, "1010").await, account(&pool, "4100").await);
    let entry = ledger::post_journal_entry(
        &pool,
        ledger::JournalEntry { id: None, date: d(2025, 1, 5), description: "Sale".into(), lines: vec![ledger::JournalLine::debit(checking, 500), ledger::JournalLine::credit(services, 500)], currency: None },
    )
    .await
    .unwrap();
    let mut conn = pool.acquire().await.unwrap();
    sqlx::query("PRAGMA foreign_keys = OFF").execute(&mut *conn).await.unwrap();
    sqlx::query("DROP TRIGGER trg_je_no_delete_posted").execute(&mut *conn).await.unwrap();
    sqlx::query("DROP TRIGGER trg_jl_delete_draft_only").execute(&mut *conn).await.unwrap();
    sqlx::query("DELETE FROM journal_lines WHERE journal_entry_id = ?1").bind(entry).execute(&mut *conn).await.unwrap();
    sqlx::query("DELETE FROM audit_log WHERE journal_entry_id = ?1").bind(entry).execute(&mut *conn).await.ok();
    sqlx::query("DELETE FROM journal_entries WHERE id = ?1").bind(entry).execute(&mut *conn).await.unwrap();
    drop(conn);
    let deleted = |audit: &ledger::LedgerVerification| audit.findings.iter().any(|f| f.entry_id == entry && f.problem.contains("deleted"));
    assert!(deleted(&ledger::verify_ledger_audit(&pool).await.unwrap()));

    let mut tx = ledger::begin_write(&pool).await.unwrap();
    let record = json!({ "removed": { "journalEntries": [entry] }, "summaries": [{ "entryId": null, "summarizes": [] }] }).to_string();
    sqlx::query("INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot) VALUES ('CONDENSE', 0, ?1)").bind(record).execute(&mut *tx).await.unwrap();
    ledger::commit_write(tx).await.unwrap();
    assert!(!deleted(&ledger::verify_ledger_audit(&pool).await.unwrap()));
}

/// For trying Condense Data by hand: `LOCALBOOKS_SEED_DIR=<folder> cargo test -- --ignored seed_condense_demo`
/// writes `Condense Demo.localbooks`, Harbor Lane with its first quarter reconciled and closed.
#[tokio::test]
#[ignore]
async fn seed_condense_demo() {
    let dir = PathBuf::from(std::env::var("LOCALBOOKS_SEED_DIR").expect("set LOCALBOOKS_SEED_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Condense Demo.localbooks");
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    let pool = crate::harness::fixture::harbor_lane_in(crate::db::open(&path).await.unwrap()).await;
    close_books(&pool, d(2025, 3, 31), None).await;
    crate::db::compact(&pool).await.unwrap();
    pool.close().await;
}
