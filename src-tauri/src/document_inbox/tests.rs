use super::*;
use crate::db::test_pool;
use crate::pdf::{Align, FontFamily, Op, Page, Rgb};

fn lines(text: &[&str]) -> Vec<String> {
    text.iter().map(|s| s.to_string()).collect()
}

/// A one-page PDF with these lines of text, written by LocalBooks' own PDF writer.
#[allow(clippy::float_arithmetic, reason = "text positions on a test page; no amounts are computed")]
fn pdf(text: &[&str]) -> Vec<u8> {
    let ops = text
        .iter()
        .enumerate()
        .map(|(i, t)| Op::Text { x: 72.0, y: 72.0 + 18.0 * i as f32, text: t.to_string(), family: FontFamily::Helvetica, bold: false, size: 11.0, color: Rgb(0, 0, 0), align: Align::Left })
        .collect();
    crate::pdf::write_pdf(&[Page { width: 612.0, height: 792.0, ops }], None, "Bill")
}

#[test]
fn hints_read_the_total_the_date_and_the_name() {
    let h = hints_from_lines(&lines(&["Paper Supply Co", "Invoice 1043   Date 10/23/2026", "Toner   2 x 45.00   90.00", "Subtotal   90.00", "Tax   7.20", "Total   $97.20"]));
    assert_eq!(h, Hints { amount_cents: Some(9_720), date: "2026-10-23".parse().ok(), name: Some("Paper Supply Co".into()) });
    // "Amount due" beats "Total"; without either, the largest amount.
    assert_eq!(hints_from_lines(&lines(&["Total 120.00", "Amount due 20.00"])).amount_cents, Some(2_000));
    assert_eq!(hints_from_lines(&lines(&["Coffee 4.50", "Muffin 3.25", "Version 2.0.1"])).amount_cents, Some(450));
    assert_eq!(hints_from_lines(&[]), Hints::default());
}

#[tokio::test]
async fn a_pdf_in_the_inbox_is_read_and_attaches_to_the_expense_it_becomes() {
    let pool = test_pool().await;
    let file = pdf(&["Harbor Hardware", "Receipt 2026-03-04", "Hinges 12.00", "Total 13.08"]);
    let doc = add(&pool, "/Users/someone/Downloads/receipt.pdf", &file).await.unwrap();
    assert_eq!((doc.file_name.as_str(), doc.media_type.as_str()), ("receipt.pdf", "application/pdf"));
    assert_eq!((doc.amount_cents, doc.document_date.as_deref(), doc.name_hint.as_deref()), (Some(1_308), Some("2026-03-04"), Some("Harbor Hardware")));
    let photo = add(&pool, "lunch.jpg", b"\xff\xd8\xff not really a jpeg").await.unwrap();
    assert_eq!((photo.amount_cents, photo.name_hint), (None, None), "no text, no OCR: no hints");
    assert_eq!(list(&pool).await.unwrap().len(), 2);
    assert_eq!(content(&pool, doc.id).await.unwrap().2, file);
    assert_eq!(set_note(&pool, doc.id, Some("  ask Pat  ")).await.unwrap().note.as_deref(), Some("ask Pat"));

    // Entered as an expense: posting attaches the file and takes it out of the inbox.
    let (checking, supplies): (i64, i64) = (
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '1010'").fetch_one(&pool).await.unwrap(),
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '6300'").fetch_one(&pool).await.unwrap(),
    );
    let input: crate::expenses::ExpenseInput = serde_json::from_value(serde_json::json!({ "kind": "EXPENSE", "payeeName": "Harbor Hardware", "paymentAccountId": checking,
        "expenseDate": "2026-03-04", "lines": [{ "accountId": supplies, "amountCents": 1_308 }] }))
    .unwrap();
    let entry = crate::expenses::post_expense(&pool, input).await.unwrap().journal_entry_id.unwrap();
    let attached = attach(&pool, doc.id, entry).await.unwrap();
    assert_eq!((attached.file_name.as_str(), attached.journal_entry_id), ("receipt.pdf", entry));
    assert_eq!(crate::workflows::attachment_content(&pool, attached.id).await.unwrap().1, file);
    assert_eq!(list(&pool).await.unwrap().iter().map(|d| d.id).collect::<Vec<_>>(), [photo.id]);
    assert!(attach(&pool, doc.id, entry).await.unwrap_err().starts_with("INBOX_NOT_FOUND"), "once");
    assert!(attach(&pool, photo.id, 999_999).await.unwrap_err().starts_with("ATTACH_NOT_POSTED"));
    assert_eq!(list(&pool).await.unwrap().len(), 1, "a failed attach leaves it waiting");

    remove(&pool, photo.id).await.unwrap();
    assert!(list(&pool).await.unwrap().is_empty());
    assert!(add(&pool, "empty.pdf", b"").await.unwrap_err().starts_with("INBOX_SIZE"));
}
