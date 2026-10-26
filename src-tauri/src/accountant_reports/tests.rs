use std::collections::HashSet;

use super::*;
use crate::db::test_pool;
use crate::ledger::{JournalEntry, JournalLine};

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

fn entry(date: NaiveDate, memo: &str, lines: Vec<JournalLine>) -> JournalEntry {
    JournalEntry { id: None, date, description: memo.into(), lines, currency: None }
}

#[test]
fn tax_lines_are_unique_and_cover_every_form() {
    let lines = tax_lines();
    let codes: HashSet<&str> = lines.iter().map(|t| t.code.as_str()).collect();
    assert_eq!(codes.len(), lines.len(), "codes are unique");
    for (form, _) in crate::preferences::TAX_FORMS {
        assert!(lines.iter().any(|t| t.form == *form && t.kind == TaxLineKind::Income), "{form} has income lines");
        assert!(lines.iter().any(|t| t.form == *form && t.kind == TaxLineKind::Deduction), "{form} has deductions");
    }
    assert!(!lines.iter().any(|t| t.form == "SCHEDULE_C" && t.kind == TaxLineKind::BalanceSheet), "Schedule C has no balance sheet");
    assert_eq!(lines.iter().find(|t| t.code == "1065:L-owner-loans").unwrap().label, "Loans from partners");
    assert_eq!(lines.iter().find(|t| t.code == "1120S:L-owner-loans").unwrap().label, "Loans from shareholders");
}

#[test]
fn imported_tax_lines_match_by_form_and_label() {
    assert_eq!(imported_tax_line("Sch C: Advertising").as_deref(), Some("SCHEDULE_C:8"));
    assert_eq!(imported_tax_line("Schedule C: Rent or lease: other business property").as_deref(), Some("SCHEDULE_C:20b"));
    assert_eq!(imported_tax_line("Form 1120S: Rents").as_deref(), Some("1120S:11"));
    assert_eq!(imported_tax_line("Form 1120: Rents").as_deref(), Some("1120:16"));
    assert_eq!(imported_tax_line("Form 1065: Guaranteed payments to partners").as_deref(), Some("1065:10"));
    assert_eq!(imported_tax_line("Form 990: Advertising and promotion").as_deref(), Some("990:IX 12"));
    // Unknown labels are kept as the file names them; unassigned ones are dropped.
    assert_eq!(imported_tax_line("Sch C: Something new").as_deref(), Some("Sch C: Something new"));
    assert_eq!(imported_tax_line("Advertising").as_deref(), Some("Advertising"));
    assert_eq!(imported_tax_line("<Unassigned>"), None);
    assert_eq!(imported_tax_line("Not tax-related"), None);
    assert_eq!(imported_tax_line("  "), None);
    assert_eq!(quickbooks_tax_line(1462), "QuickBooks tax line 1462");
}

#[test]
fn iif_accounts_bring_their_tax_line() {
    let file = "!ACCNT\tNAME\tACCNTTYPE\tSCD\tTAXLINE\r\n\
                ACCNT\tAdvertising\tEXP\t\tSch C: Advertising\r\n\
                ACCNT\tRent\tEXP\t1462\t\r\n\
                ACCNT\tChecking\tBANK\t0\t\r\n";
    let ledger = crate::iif_import::read(file.as_bytes()).unwrap();
    let line = |name: &str| ledger.accounts.iter().find(|a| a.full_name == name).unwrap().tax_line.clone();
    assert_eq!(line("Advertising").as_deref(), Some("SCHEDULE_C:8"));
    assert_eq!(line("Rent").as_deref(), Some("QuickBooks tax line 1462"));
    assert_eq!(line("Checking"), None);
}

#[tokio::test]
async fn setting_a_tax_line_checks_the_line_and_the_account() {
    let pool = test_pool().await;
    let advertising = account(&pool, "6000").await;
    let checking = account(&pool, "1010").await;
    set_account_tax_line(&pool, advertising, Some("SCHEDULE_C:8")).await.unwrap();
    set_account_tax_line(&pool, checking, Some("1120S:L-cash")).await.unwrap();
    let saved = account_tax_lines(&pool).await.unwrap();
    let of = |id: i64| saved.iter().find(|a| a.account_id == id).unwrap().tax_line.clone();
    assert_eq!(of(advertising).as_deref(), Some("SCHEDULE_C:8"));
    assert_eq!(of(checking).as_deref(), Some("1120S:L-cash"));

    let err = set_account_tax_line(&pool, advertising, Some("SCHEDULE_C:99")).await.unwrap_err();
    assert!(err.starts_with("TAX_LINE_UNKNOWN"), "{err}");
    let err = set_account_tax_line(&pool, advertising, Some("1120S:L-cash")).await.unwrap_err();
    assert!(err.starts_with("TAX_LINE_KIND"), "{err}");
    let err = set_account_tax_line(&pool, checking, Some("SCHEDULE_C:8")).await.unwrap_err();
    assert!(err.starts_with("TAX_LINE_KIND"), "{err}");
    let err = set_account_tax_line(&pool, 999_999, None).await.unwrap_err();
    assert!(err.starts_with("COA_NOT_FOUND"), "{err}");

    set_account_tax_line(&pool, advertising, Some(" ")).await.unwrap();
    let saved = account_tax_lines(&pool).await.unwrap();
    assert_eq!(saved.iter().find(|a| a.account_id == advertising).unwrap().tax_line, None, "blank clears it");
}

#[tokio::test]
async fn income_tax_sums_each_line_and_lists_what_is_unassigned() {
    let pool = test_pool().await;
    let (checking, sales, advertising, rent) = (account(&pool, "1010").await, account(&pool, "4000").await, account(&pool, "6000").await, account(&pool, "6300").await);
    let post = |date, memo: &str, lines| crate::ledger::post_journal_entry(&pool, entry(date, memo, lines));
    post(d(2025, 3, 1), "Sale", vec![JournalLine::debit(checking, 100_000), JournalLine::credit(sales, 100_000)]).await.unwrap();
    post(d(2025, 3, 2), "Refund", vec![JournalLine::debit(sales, 5_000), JournalLine::credit(checking, 5_000)]).await.unwrap();
    post(d(2025, 4, 1), "Ads", vec![JournalLine::debit(advertising, 20_000), JournalLine::credit(checking, 20_000)]).await.unwrap();
    post(d(2025, 4, 2), "Rent", vec![JournalLine::debit(rent, 30_000), JournalLine::credit(checking, 30_000)]).await.unwrap();
    post(d(2024, 12, 31), "Last year", vec![JournalLine::debit(advertising, 99_000), JournalLine::credit(checking, 99_000)]).await.unwrap();
    set_account_tax_line(&pool, sales, Some("SCHEDULE_C:1")).await.unwrap();
    set_account_tax_line(&pool, advertising, Some("SCHEDULE_C:8")).await.unwrap();

    let summary = income_tax(&pool, "SCHEDULE_C", d(2025, 1, 1), d(2025, 12, 31), false, None).await.unwrap();
    let amount = |label: &str| summary.report.rows.iter().find(|r| r.cells[0] == json!(label)).map(|r| r.cells[1].clone());
    assert_eq!(amount("1 · Gross receipts or sales"), Some(json!(95_000)), "income is credits less debits");
    assert_eq!(amount("8 · Advertising"), Some(json!(20_000)), "only the year's activity");
    assert_eq!(amount("Total Income"), Some(json!(95_000)));
    assert_eq!(amount("Total Deductions"), Some(json!(20_000)));
    assert!(amount("Unassigned (income and expense accounts with no tax line)").is_some());
    assert!(summary.report.rows.iter().any(|r| r.depth == 1 && r.cells[1] == json!(30_000)), "rent is listed as unassigned");
    assert!(summary.notes.iter().any(|n| n.contains("1 income or expense account")), "{:?}", summary.notes);

    let detail = income_tax(&pool, "SCHEDULE_C", d(2025, 1, 1), d(2025, 12, 31), true, Some("SCHEDULE_C:1")).await.unwrap();
    assert_eq!(detail.report.line_count, 2, "the sale and the refund");
    let amounts: Vec<Value> = detail.report.rows.iter().filter(|r| r.kind == RowKind::Detail).map(|r| r.cells[6].clone()).collect();
    assert_eq!(amounts, vec![json!(100_000), json!(-5_000)]);

    // Another form's report shows these lines under Other tax lines.
    let other = income_tax(&pool, "1120S", d(2025, 1, 1), d(2025, 12, 31), false, None).await.unwrap();
    assert!(other.report.rows.iter().any(|r| r.cells[0] == json!("1 · Gross receipts or sales (Schedule C (Form 1040))")));
    assert!(income_tax(&pool, "1040", d(2025, 1, 1), d(2025, 12, 31), false, None).await.unwrap_err().starts_with("PREF_BAD_TAX_FORM"));
}

/// Posts deposits and checks, and reconciles all but the last of them through `end`.
async fn reconcile_all_but_last(pool: &SqlitePool, account_id: i64, end: NaiveDate) -> i64 {
    let lines: Vec<_> = crate::reconcile::unreconciled_lines(pool, account_id).await.unwrap().into_iter().filter(|l| l.entry_date.as_str() <= end.to_string().as_str()).collect();
    let (clear, _) = lines.split_at(lines.len() - 1);
    let state = crate::reconcile::account_state(pool, account_id).await.unwrap();
    let ending = state.reconciled_balance_cents + clear.iter().map(|l| l.amount_cents).sum::<i64>();
    let rec = crate::reconcile::start(pool, account_id, end, ending).await.unwrap();
    crate::reconcile::set_lines_cleared(pool, rec.id, &clear.iter().map(|l| l.line_id).collect::<Vec<_>>(), true).await.unwrap();
    crate::reconcile::lock(pool, rec.id).await.unwrap();
    rec.id
}

#[tokio::test]
async fn reconciliations_keep_a_snapshot_and_report_changes_made_afterwards() {
    let pool = test_pool().await;
    let (checking, sales, office) = (account(&pool, "1010").await, account(&pool, "4000").await, account(&pool, "6250").await);
    for (day, amount) in [(2, 50_000), (5, -12_000), (9, -3_000), (20, 8_000)] {
        let lines = if amount > 0 {
            vec![JournalLine::debit(checking, amount), JournalLine::credit(sales, amount)]
        } else {
            vec![JournalLine::debit(office, -amount), JournalLine::credit(checking, -amount)]
        };
        crate::ledger::post_journal_entry(&pool, entry(d(2025, 1, day), "Activity", lines)).await.unwrap();
    }
    let january = reconcile_all_but_last(&pool, checking, d(2025, 1, 31)).await;
    let (count, total): (i64, i64) = sqlx::query_as("SELECT cleared_count, cleared_total_cents FROM reconciliation_snapshots WHERE reconciliation_id = ?1")
        .bind(january)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!((count, total), (3, 35_000), "three lines cleared, $350 net");
    let err = sqlx::query("DELETE FROM reconciliation_snapshot_lines").execute(&pool).await.unwrap_err().to_string();
    assert!(err.contains("REC_SNAPSHOT_IMMUTABLE"), "{err}");

    // The January statement left the $80 deposit outstanding; February clears it.
    crate::ledger::post_journal_entry(&pool, entry(d(2025, 2, 3), "Activity", vec![JournalLine::debit(office, 1_000), JournalLine::credit(checking, 1_000)])).await.unwrap();
    crate::ledger::post_journal_entry(&pool, entry(d(2025, 2, 4), "Activity", vec![JournalLine::debit(office, 2_000), JournalLine::credit(checking, 2_000)])).await.unwrap();
    reconcile_all_but_last(&pool, checking, d(2025, 2, 28)).await;

    let detail = previous_reconciliation(&pool, january, true).await.unwrap();
    let status: Vec<String> = detail.report.rows.iter().filter(|r| r.kind == RowKind::Detail && r.journal_entry_id.is_some()).map(|r| r.cells[5].as_str().unwrap().to_string()).collect();
    assert_eq!(status, vec!["Cleared", "Cleared", "Cleared", "Cleared 2025-02-28"]);
    let last = detail.report.rows.last().unwrap();
    assert_eq!((last.cells[0].clone(), last.cells[7].clone()), (json!("Register Balance as of 2025-01-31"), json!(43_000)));

    let summary = previous_reconciliation(&pool, january, false).await.unwrap();
    let find = |label: &str| summary.report.rows.iter().find(|r| r.cells[0] == json!(label)).unwrap().cells.clone();
    assert_eq!(find("Checks and Payments"), vec![json!("Checks and Payments"), json!(2), json!(-15_000)]);
    assert_eq!(find("Deposits and Credits"), vec![json!("Deposits and Credits"), json!(1), json!(50_000)]);
    assert_eq!(find("Cleared Balance")[2], json!(35_000));
    assert!(previous_reconciliation(&pool, 999, false).await.unwrap_err().starts_with("REC_NOT_COMPLETED"));

    let clean = discrepancy(&pool, Some(checking), d(2025, 3, 1)).await.unwrap();
    assert_eq!(clean.report.line_count, 0, "nothing changed yet");
    assert!(clean.notes[0].starts_with("2 completed reconciliations checked; 0 changes"), "{:?}", clean.notes);

    // Something outside LocalBooks edits the file: the schema's guards are gone for this.
    let triggers: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name IN ('journal_lines', 'journal_entries')")
        .fetch_all(&pool)
        .await
        .unwrap();
    for t in triggers {
        sqlx::query(&format!("DROP TRIGGER {t}")).execute(&pool).await.unwrap();
    }
    let cleared: Vec<(i64, i64)> = sqlx::query_as("SELECT id, journal_entry_id FROM journal_lines WHERE reconciliation_id = ?1 ORDER BY id").bind(january).fetch_all(&pool).await.unwrap();
    sqlx::query("UPDATE journal_lines SET credit_cents = 13_000 WHERE id = ?1").bind(cleared[1].0).execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM journal_lines WHERE id = ?1").bind(cleared[2].0).execute(&pool).await.unwrap();
    sqlx::query("UPDATE journal_entries SET entry_date = '2025-01-03' WHERE id = ?1").bind(cleared[0].1).execute(&pool).await.unwrap();
    let report = discrepancy(&pool, None, d(2025, 3, 1)).await.unwrap();
    let changes: Vec<(String, Value)> = report.report.rows.iter().filter(|r| r.kind == RowKind::Detail).map(|r| (r.cells[4].as_str().unwrap().to_string(), r.cells[7].clone())).collect();
    assert_eq!(
        changes,
        vec![
            ("Date changed to 2025-01-03".to_string(), json!(0)),
            ("Amount changed".to_string(), json!(-1_000)),
            ("Deleted".to_string(), json!(3_000)),
        ],
        "{:#?}",
        report.report.rows
    );
}

#[tokio::test]
async fn older_reconciliations_are_checked_by_their_totals() {
    let pool = test_pool().await;
    let (checking, sales) = (account(&pool, "1010").await, account(&pool, "4000").await);
    crate::ledger::post_journal_entry(&pool, entry(d(2025, 1, 2), "Sale", vec![JournalLine::debit(checking, 10_000), JournalLine::credit(sales, 10_000)])).await.unwrap();
    crate::ledger::post_journal_entry(&pool, entry(d(2025, 1, 3), "Sale", vec![JournalLine::debit(checking, 20_000), JournalLine::credit(sales, 20_000)])).await.unwrap();
    let rec = reconcile_all_but_last(&pool, checking, d(2025, 1, 31)).await;
    // As if completed before migration 0056: no snapshot.
    for t in ["trg_rec_snapshot_lines_no_delete", "trg_rec_snapshot_no_delete"] {
        sqlx::query(&format!("DROP TRIGGER {t}")).execute(&pool).await.unwrap();
    }
    sqlx::query("DELETE FROM reconciliation_snapshot_lines").execute(&pool).await.unwrap();
    sqlx::query("DELETE FROM reconciliation_snapshots").execute(&pool).await.unwrap();
    assert!(!list_completed(&pool).await.unwrap()[0].has_snapshot);

    let report = discrepancy(&pool, None, d(2025, 3, 1)).await.unwrap();
    assert_eq!(report.report.line_count, 0);
    assert!(report.notes[1].contains("checked by its totals only"), "{:?}", report.notes);

    let triggers: Vec<String> = sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'trigger' AND tbl_name = 'journal_lines'").fetch_all(&pool).await.unwrap();
    for t in triggers {
        sqlx::query(&format!("DROP TRIGGER {t}")).execute(&pool).await.unwrap();
    }
    sqlx::query("UPDATE journal_lines SET debit_cents = 12_000 WHERE reconciliation_id = ?1").bind(rec).execute(&pool).await.unwrap();
    let report = discrepancy(&pool, None, d(2025, 3, 1)).await.unwrap();
    assert_eq!(report.report.line_count, 1);
    let row = report.report.rows.iter().find(|r| r.kind == RowKind::Detail).unwrap();
    assert_eq!((row.cells[0].clone(), row.cells[5].clone(), row.cells[6].clone(), row.cells[7].clone()), (json!("Cleared total"), json!(10_000), json!(12_000), json!(2_000)));
}

#[tokio::test]
async fn adjusting_entries_are_marked_and_reported() {
    let pool = test_pool().await;
    let (checking, office, equipment) = (account(&pool, "1010").await, account(&pool, "6250").await, account(&pool, "1500").await);
    let plain = crate::ledger::post_journal_entry(&pool, entry(d(2025, 6, 1), "Supplies", vec![JournalLine::debit(office, 5_000), JournalLine::credit(checking, 5_000)])).await.unwrap();
    let adjusting = crate::ledger::post_journal_entry_flagged(&pool, entry(d(2025, 12, 31), "Reclass printer", vec![JournalLine::debit(equipment, 40_000), JournalLine::credit(office, 40_000)]), true).await.unwrap();
    let flags: Vec<(i64, bool)> = crate::ledger::list_journal_entries(&pool, 10).await.unwrap().into_iter().map(|e| (e.id, e.is_adjusting)).collect();
    assert!(flags.contains(&(adjusting, true)) && flags.contains(&(plain, false)), "{flags:?}");

    let preset = crate::custom_reports::presets().into_iter().find(|p| p.id == "adjusting-journal").unwrap();
    assert_eq!(preset.group, "Accountant & Taxes");
    let run = |pool: SqlitePool| {
        let mut settings = preset.settings.clone();
        settings.start_date = Some(d(2025, 1, 1));
        settings.end_date = Some(d(2025, 12, 31));
        async move { crate::custom_reports::run(&pool, &settings, d(2025, 12, 31)).await.unwrap() }
    };
    let report = run(pool.clone()).await;
    assert_eq!(report.line_count, 2, "both lines of the adjusting entry, not the plain one");
    assert!(report.rows.iter().all(|r| r.journal_entry_id.is_none_or(|e| e == adjusting)));

    // Editing keeps the mark unless told otherwise; the voided original and its reversal stay in.
    let replaced = crate::edits::replace_journal_entry(&pool, adjusting, entry(d(2025, 12, 31), "Reclass printer", vec![JournalLine::debit(equipment, 45_000), JournalLine::credit(office, 45_000)])).await.unwrap();
    let is_adjusting = |id: i64| sqlx::query_scalar::<_, bool>("SELECT is_adjusting FROM journal_entries WHERE id = ?1").bind(id).fetch_one(&pool);
    assert!(is_adjusting(replaced).await.unwrap());
    assert_eq!(run(pool.clone()).await.line_count, 6, "original, its reversal and the replacement");
    let unmarked = crate::edits::replace_journal_entry_flagged(&pool, replaced, entry(d(2025, 12, 31), "Reclass printer", vec![JournalLine::debit(equipment, 45_000), JournalLine::credit(office, 45_000)]), Some(false)).await.unwrap();
    assert!(!is_adjusting(unmarked).await.unwrap());
}

#[tokio::test]
async fn collections_lists_overdue_invoices_with_contact_details() {
    let pool = test_pool().await;
    let sales = account(&pool, "4000").await;
    let line = |amount: i64| crate::invoices::InvoiceLineInput {
        description: "Work".into(),
        quantity_e4: 10_000,
        rate_cents: amount,
        income_account_id: sales,
        product_service_id: None,
        class_id: None,
        is_taxable: false,
    };
    let invoice = |customer: &str, issue: NaiveDate, due: NaiveDate, amount: i64| crate::invoices::InvoiceInput {
        customer_name: customer.into(),
        issue_date: issue,
        due_date: due,
        lines: vec![line(amount)],
        currency: None,
        tax_rate_id: None,
        discount_cents: 0,
        details: Default::default(),
    };
    crate::invoices::create_and_post_invoice(&pool, invoice("Cobalt", d(2025, 1, 1), d(2025, 1, 31), 10_000)).await.unwrap();
    crate::invoices::create_and_post_invoice(&pool, invoice("Cobalt", d(2025, 2, 1), d(2025, 3, 3), 20_000)).await.unwrap();
    crate::invoices::create_and_post_invoice(&pool, invoice("Alder", d(2025, 2, 1), d(2025, 4, 30), 30_000)).await.unwrap();
    sqlx::query("UPDATE customers SET phone = '555-0100', email = 'ap@cobalt.example', company_name = 'Dana Cobalt' WHERE name = 'Cobalt'").execute(&pool).await.unwrap();

    let report = collections(&pool, d(2025, 3, 15)).await.unwrap();
    assert_eq!(report.report.line_count, 2, "only Cobalt's two invoices are past due");
    assert_eq!(report.report.rows[0].cells[0], json!("Cobalt · Contact: Dana Cobalt · Phone: 555-0100 · Email: ap@cobalt.example"));
    let aging: Vec<Value> = report.report.rows.iter().filter(|r| r.kind == RowKind::Detail).map(|r| r.cells[4].clone()).collect();
    assert_eq!(aging, vec![json!(43), json!(12)]);
    assert_eq!(report.report.rows.last().unwrap().cells[5], json!(30_000));
}

#[tokio::test]
async fn open_purchase_orders_leave_out_closed_ones() {
    let pool = test_pool().await;
    let supplies = account(&pool, "6250").await;
    let po = |vendor: &str, date: NaiveDate, amount: i64| crate::workflows::PurchaseOrderInput {
        vendor_name: vendor.into(),
        po_date: date,
        expected_date: Some(date + chrono::Days::new(14)),
        memo: Some("Paper".into()),
        lines: vec![crate::workflows::PurchaseOrderLineInput { account_id: supplies, product_service_id: None, description: None, quantity_e4: None, rate_cents: None, amount_cents: amount, class_id: None }],
    };
    crate::workflows::create_purchase_order(&pool, po("Staples", d(2025, 5, 1), 12_000)).await.unwrap();
    let closed = crate::workflows::create_purchase_order(&pool, po("Staples", d(2025, 5, 2), 5_000)).await.unwrap();
    crate::workflows::create_purchase_order(&pool, po("Uline", d(2025, 6, 1), 7_500)).await.unwrap();
    crate::workflows::set_purchase_order_closed(&pool, closed, true).await.unwrap();

    let report = open_purchase_orders(&pool, d(2025, 5, 31)).await.unwrap();
    assert_eq!(report.report.line_count, 1, "June's order is after the date and the closed one is out");
    assert_eq!(open_purchase_orders(&pool, d(2025, 12, 31)).await.unwrap().report.rows.last().unwrap().cells[5], json!(19_500));
}
