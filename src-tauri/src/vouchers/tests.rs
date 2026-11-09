use super::*;
use crate::db::test_pool;
use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

fn doc(entry_id: i64, name: Option<&str>, label: &str) -> VoucherDoc {
    VoucherDoc {
        entry_id,
        title: "Bill".into(),
        number: Some("INV-42".into()),
        date: "2026-10-01".into(),
        due_date: Some("2026-10-31".into()),
        name: name.map(Into::into),
        name_label: Some(label.into()),
        memo: Some("Toner and paper".into()),
        amount_cents: Some(12_345),
        currency_code: None,
        voided: false,
        footer: None,
    }
}

async fn entry(pool: &SqlitePool) -> i64 {
    let (rent, checking) = (account(pool, "6300").await, account(pool, "1010").await);
    let lines = vec![JournalLine::debit(rent, 12_345).with_memo(Some("October")), JournalLine::credit(checking, 12_345)];
    post_journal_entry(pool, JournalEntry { id: None, date: "2026-10-01".parse().unwrap(), description: "Rent".into(), lines, currency: None }).await.unwrap()
}

fn texts(pages: &[Page]) -> Vec<String> {
    pages.iter().flat_map(|p| p.ops.iter()).filter_map(|op| if let Op::Text { text, .. } = op { Some(text.clone()) } else { None }).collect()
}

#[tokio::test]
async fn a_voucher_pdf_has_its_header_lines_and_totals() {
    let pool = test_pool().await;
    let id = entry(&pool).await;
    let company = forms::load_company(&pool).await.unwrap();
    let lines = ledger::journal_entry_lines(&pool, id).await.unwrap();
    let pages = layout(&doc(id, Some("Paper Supply Co"), "Vendor"), &company, &lines).unwrap();
    let all = texts(&pages);
    for expected in ["BILL", "Vendor", "Paper Supply Co", "INV-42", "10/01/2026", "Rent & Lease", "October", "Total", "$123.45"] {
        assert!(all.iter().any(|t| t.contains(expected)), "{expected} is on the voucher: {all:?}");
    }
    let (file, bytes) = pdf_file(&pool, &doc(id, Some("Paper Supply Co"), "Vendor")).await.unwrap();
    assert_eq!(file, "Bill INV-42 · Paper Supply Co.pdf");
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(pdf_file(&pool, &doc(999_999, None, "Vendor")).await.unwrap_err().starts_with("VOUCHER_NOT_FOUND"));
}

#[tokio::test]
async fn a_long_voucher_runs_onto_more_pages() {
    let pool = test_pool().await;
    let company = forms::load_company(&pool).await.unwrap();
    let line = |n: i64| JournalLineView { line_number: n, account_id: 1, account_name: "Office Supplies".into(), debit_cents: 100, credit_cents: 0, memo: Some(format!("Line {n}")), class_id: None, class_name: None };
    let lines: Vec<_> = (1..=120).map(line).collect();
    let pages = layout(&doc(1, None, "Name"), &company, &lines).unwrap();
    assert!(pages.len() > 1);
    assert!(texts(&pages[1..]).iter().any(|t| t.contains("(continued)")));
}

#[tokio::test]
async fn email_goes_to_the_vendor_or_customer_named_on_it() {
    let pool = test_pool().await;
    let id = entry(&pool).await;
    crate::expenses::create_vendor(&pool, "Paper Supply Co", Some("billing@paper.example")).await.unwrap();
    crate::sales::create_customer(&pool, "Harbor Cafe", Some("owner@harbor.example")).await.unwrap();
    let draft = email_draft(&pool, &doc(id, Some("paper supply co"), "Vendor")).await.unwrap();
    assert_eq!(draft.to, "billing@paper.example");
    assert_eq!(draft.file_name, "Bill INV-42 · paper supply co.pdf");
    assert!(draft.body.starts_with("Dear paper supply co,"));
    assert_eq!(recipient(&pool, &doc(id, Some("Harbor Cafe"), "Received from")).await.unwrap(), "owner@harbor.example");
    assert_eq!(recipient(&pool, &doc(id, Some("Nobody"), "Vendor")).await.unwrap(), "");
    assert_eq!(recipient(&pool, &doc(id, None, "Vendor")).await.unwrap(), "");
}
