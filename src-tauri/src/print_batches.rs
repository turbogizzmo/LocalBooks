//! File › Print Forms batches beyond invoices and checks: credit memos, sales receipts, purchase
//! orders and bill payment stubs from a date range, to tick and print together, and sheets of
//! mailing labels for customers or vendors.
//!
//! The documents themselves print with their own forms (`forms.rs`, or the purchase order screen);
//! this module only lists what a batch can hold and lays out label sheets.

#![allow(clippy::float_arithmetic, reason = "label positions in points; no amounts are printed")]

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::envelopes::address_block;
use crate::forms::RenderedForm;
use crate::ledger::db_err;
use crate::pdf::{self, Align, FontFamily, Op, Page, Rgb};

/// What a Print Forms batch holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BatchKind {
    CreditMemo,
    SalesReceipt,
    PurchaseOrder,
    /// One row per check (or payment), however many bills it paid: its stub lists them all.
    BillPayment,
}

/// A document that can go in a batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BatchRow {
    /// The document to print: for a bill payment, the first payment on its check.
    pub id: i64,
    pub number: Option<String>,
    pub date: String,
    /// The customer or vendor.
    pub name: String,
    pub total_cents: i64,
    pub is_voided: bool,
}

/// The documents of one kind dated `from` to `to`, oldest first.
pub async fn list(pool: &SqlitePool, kind: BatchKind, from: NaiveDate, to: NaiveDate) -> Result<Vec<BatchRow>, String> {
    if to < from {
        return Err("PRINT_BATCH_DATES: the end date is before the start date".into());
    }
    let sql = match kind {
        BatchKind::CreditMemo => {
            "SELECT m.id, CAST(m.credit_number AS TEXT) AS number, m.credit_date AS date, c.name, m.total_cents, m.is_voided
             FROM credit_memos m JOIN customers c ON c.id = m.customer_id
             -- Unapplied payments kept as credit aren't credit memos and have no form.
             WHERE m.credit_date BETWEEN ?1 AND ?2 AND m.payment_account_id IS NULL ORDER BY m.credit_date, m.credit_number"
        }
        BatchKind::SalesReceipt => {
            "SELECT r.id, CAST(r.receipt_number AS TEXT) AS number, r.receipt_date AS date, COALESCE(c.name, '') AS name, r.total_cents, r.is_voided
             FROM sales_receipts r LEFT JOIN customers c ON c.id = r.customer_id
             WHERE r.receipt_date BETWEEN ?1 AND ?2 ORDER BY r.receipt_date, r.receipt_number"
        }
        BatchKind::PurchaseOrder => {
            "SELECT o.id, CAST(o.po_number AS TEXT) AS number, o.po_date AS date, v.name, o.total_cents, 0 AS is_voided
             FROM purchase_orders o JOIN vendors v ON v.id = o.vendor_id
             WHERE o.po_date BETWEEN ?1 AND ?2 ORDER BY o.po_date, o.po_number"
        }
        // Grouped as the stub groups them (`forms::bill_payment_doc`): the same vendor, account,
        // date and voided state, and the same check, or the same reference when there's no check.
        BatchKind::BillPayment => {
            "SELECT MIN(p.id) AS id, NULLIF(trim(MAX(p.reference)), '') AS number, p.payment_date AS date, v.name, SUM(p.amount_cents) AS total_cents,
                    p.is_voided
             FROM bill_payments p JOIN bills b ON b.id = p.bill_id JOIN vendors v ON v.id = b.vendor_id
             WHERE p.payment_date BETWEEN ?1 AND ?2
             GROUP BY b.vendor_id, p.payment_account_id, p.payment_date, p.is_voided,
                      CASE WHEN p.check_group_id IS NOT NULL THEN 'g' || p.check_group_id
                           WHEN p.reference IS NOT NULL AND trim(p.reference) <> '' THEN 'r' || p.reference
                           ELSE 'p' || p.id END
             ORDER BY p.payment_date, MIN(p.id)"
        }
    };
    sqlx::query_as(sql).bind(from.to_string()).bind(to.to_string()).fetch_all(pool).await.map_err(db_err)
}

/// Whose addresses go on mailing labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LabelNames {
    Customers,
    Vendors,
}

/// A name that can get a label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelRow {
    pub id: i64,
    pub name: String,
    /// The label's lines, the name first.
    pub lines: Vec<String>,
    /// False when there is no address beyond the name.
    pub has_address: bool,
}

/// The active customers or vendors, by name, with the lines their labels print.
pub async fn label_rows(pool: &SqlitePool, names: LabelNames) -> Result<Vec<LabelRow>, String> {
    let sql = match names {
        LabelNames::Customers => "SELECT id, name, company_name, billing_address FROM customers WHERE is_active = 1 ORDER BY name COLLATE NOCASE",
        LabelNames::Vendors => "SELECT id, name, company_name, address FROM vendors WHERE is_active = 1 ORDER BY name COLLATE NOCASE",
    };
    let rows: Vec<(i64, String, Option<String>, Option<String>)> = sqlx::query_as(sql).fetch_all(pool).await.map_err(db_err)?;
    Ok(rows
        .into_iter()
        .map(|(id, name, company, address)| {
            let has_address = address.as_deref().is_some_and(|a| !a.trim().is_empty());
            LabelRow { id, lines: address_block(&name, company.as_deref(), address.as_deref()), name, has_address }
        })
        .collect())
}

/// A US Letter sheet of 30 address labels, 2⅝ × 1 inch, three across and ten down (the common
/// 1-inch address label sheet sold by several makers).
pub const LETTER_30: LabelSheet = LabelSheet {
    width: 612.0,
    height: 792.0,
    columns: 3,
    rows: 10,
    left: 13.5,
    top: 36.0,
    label_width: 189.0,
    label_height: 72.0,
    pitch_x: 198.0,
    pitch_y: 72.0,
};

/// The geometry of a label sheet, in points.
#[derive(Debug, Clone, Copy)]
pub struct LabelSheet {
    pub width: f32,
    pub height: f32,
    pub columns: usize,
    pub rows: usize,
    pub left: f32,
    pub top: f32,
    pub label_width: f32,
    pub label_height: f32,
    pub pitch_x: f32,
    pub pitch_y: f32,
}

const INK: Rgb = Rgb(0x1f, 0x29, 0x37);
const SIZE: f32 = 9.0;
const LEADING: f32 = 11.0;
/// The space kept clear inside each label's edge.
const INSET: f32 = 9.0;

/// `text` shortened with an ellipsis to fit `width` in the weight it prints in.
fn fit(text: &str, width: f32, bold: bool) -> String {
    if pdf::text_width(FontFamily::Helvetica, bold, SIZE, text) <= width {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let shorter = format!("{}…", chars.iter().collect::<String>().trim_end());
        if pdf::text_width(FontFamily::Helvetica, bold, SIZE, &shorter) <= width {
            return shorter;
        }
    }
    "…".into()
}

/// Lays out labels on sheets, starting at label `skip` (0-based) so a part-used sheet can go
/// through again. Each label is its lines, the name in bold, cut to the label.
pub fn layout(sheet: LabelSheet, labels: &[Vec<String>], skip: usize) -> Vec<Page> {
    let per_sheet = sheet.columns * sheet.rows;
    let skip = skip.min(per_sheet.saturating_sub(1));
    let fits = (((sheet.label_height - 2.0 * INSET + LEADING - SIZE) / LEADING).floor() as usize).max(1);
    let mut pages: Vec<Page> = Vec::new();
    for (n, lines) in labels.iter().enumerate() {
        let slot = n + skip;
        if slot.is_multiple_of(per_sheet) || pages.is_empty() {
            pages.push(Page { width: sheet.width, height: sheet.height, ops: Vec::new() });
        }
        let place = slot % per_sheet;
        let (row, column) = (place / sheet.columns, place % sheet.columns);
        let x = sheet.left + column as f32 * sheet.pitch_x + INSET;
        let used = lines.len().min(fits);
        // Centred down the label.
        let block = SIZE + LEADING * used.saturating_sub(1) as f32;
        let mut y = sheet.top + row as f32 * sheet.pitch_y + (sheet.label_height - block) / 2.0 + SIZE;
        let page = pages.last_mut().expect("a page was pushed");
        for (i, line) in lines.iter().take(fits).enumerate() {
            page.ops.push(Op::Text {
                x,
                y,
                text: fit(line, sheet.label_width - 2.0 * INSET, i == 0),
                family: FontFamily::Helvetica,
                bold: i == 0,
                size: SIZE,
                color: INK,
                align: Align::Left,
            });
            y += LEADING;
        }
    }
    pages
}

/// Mailing labels for the chosen customers or vendors, in the order given.
pub async fn render_labels(pool: &SqlitePool, names: LabelNames, ids: &[i64], skip: usize) -> Result<RenderedForm, String> {
    if ids.is_empty() {
        return Err("LABELS_NONE: choose at least one name".into());
    }
    let mut all: std::collections::HashMap<i64, Vec<String>> = label_rows(pool, names).await?.into_iter().map(|r| (r.id, r.lines)).collect();
    let labels: Vec<Vec<String>> = ids.iter().filter_map(|id| all.remove(id)).collect();
    if labels.is_empty() {
        return Err("LABELS_NONE: none of those names is active".into());
    }
    let who = match names {
        LabelNames::Customers => "Customer",
        LabelNames::Vendors => "Vendor",
    };
    Ok(RenderedForm { title: format!("{who} mailing labels"), pages: layout(LETTER_30, &labels, skip), logo: None, customer_email: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::money::Cents;

    fn day(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn texts(page: &Page) -> Vec<(f32, f32, String, bool)> {
        page.ops
            .iter()
            .filter_map(|op| match op {
                Op::Text { x, y, text, bold, .. } => Some((*x, *y, text.clone(), *bold)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn labels_fill_the_sheet_across_then_down_and_start_after_used_ones() {
        let labels: Vec<Vec<String>> = (0..32).map(|n| vec![format!("Name {n}"), "1 Main St".into(), "Springfield, OR 97477".into()]).collect();
        let pages = layout(LETTER_30, &labels, 0);
        assert_eq!(pages.len(), 2, "30 a sheet");
        let first = texts(&pages[0]);
        assert_eq!(first.len(), 90);
        let names: Vec<&(f32, f32, String, bool)> = first.iter().filter(|t| t.3).collect();
        assert_eq!(names[0].2, "Name 0");
        assert!(names[1].0 > names[0].0 && names[1].1 == names[0].1, "the second label is to the right");
        assert!(names[3].1 > names[0].1 && names[3].0 == names[0].0, "the fourth starts the next row");
        // Every line stays inside its label.
        for (x, y, text, _) in &first {
            assert!(*x >= 13.5 && *x < 612.0 - 13.5 && *y > 36.0 && *y < 792.0 - 36.0 + 1.0, "{text} at {x},{y}");
        }

        // Skipping the 4 used labels on a sheet: the first label prints in the second row's middle.
        let skipped = layout(LETTER_30, &labels[..2], 4);
        let names: Vec<(f32, f32, String, bool)> = texts(&skipped[0]).into_iter().filter(|t| t.3).collect();
        assert_eq!(names[0].0, 13.5 + 198.0 + INSET);
        assert!(names[0].1 > 36.0 + 72.0 && names[0].1 < 36.0 + 144.0);
    }

    #[test]
    fn long_lines_are_cut_to_the_label_and_extra_lines_dropped() {
        let long = "An extremely long company name that would never fit on one address label".to_string();
        let pages = layout(LETTER_30, &[vec![long.clone(), "a".into(), "b".into(), "c".into(), "d".into(), "e".into(), "f".into()]], 0);
        let lines = texts(&pages[0]);
        assert!(lines.len() <= 5, "a 1-inch label holds a few lines: {}", lines.len());
        assert!(lines[0].2.ends_with('…') && lines[0].2.len() < long.len());
        assert!(pdf::text_width(FontFamily::Helvetica, true, SIZE, &lines[0].2) <= 189.0 - 2.0 * INSET, "the bold name fits inside the label's margins");
    }

    #[tokio::test]
    async fn a_batch_lists_each_kind_in_the_date_range_and_one_row_per_check() {
        let pool = test_pool().await;
        let (checking, rent): (i64, i64) = (
            sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '1010'").fetch_one(&pool).await.unwrap(),
            sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '6300'").fetch_one(&pool).await.unwrap(),
        );
        crate::expenses::create_vendor(&pool, "Alder Supply", None).await.unwrap();
        let mut bills = Vec::new();
        for (number, cents) in [("A-1", 10_000), ("A-2", 5_000), ("A-3", 2_500)] {
            let input: crate::expenses::BillInput = serde_json::from_value(
                serde_json::json!({ "vendorName": "Alder Supply", "billNumber": number, "billDate": "2026-09-01", "dueDate": "2026-09-30",
                "totalCents": cents, "lines": [{ "accountId": rent, "amountCents": cents }] }),
            )
            .unwrap();
            bills.push(crate::expenses::post_vendor_bill(&pool, input).await.unwrap().id);
        }
        let pay = |bill_id: i64, cents: i64, date: &str, check: Option<&str>| crate::expenses::BillPaymentInput {
            bill_id,
            payment_account_id: checking,
            payment_date: day(date),
            amount_cents: Cents(cents),
            reference: check.map(String::from),
            to_be_printed: false,
        };
        // Two bills on check 2001, one paid by itself without a reference.
        crate::expenses::pay_vendor_bills(&pool, vec![pay(bills[0], 10_000, "2026-10-05", Some("2001")), pay(bills[1], 5_000, "2026-10-05", Some("2001"))])
            .await
            .unwrap();
        crate::expenses::pay_vendor_bills(&pool, vec![pay(bills[2], 2_500, "2026-10-06", None)]).await.unwrap();
        let rows = list(&pool, BatchKind::BillPayment, day("2026-10-01"), day("2026-10-31")).await.unwrap();
        assert_eq!(rows.len(), 2, "one row per check: {rows:?}");
        assert_eq!((rows[0].number.as_deref(), rows[0].total_cents, rows[0].name.as_str()), (Some("2001"), 15_000, "Alder Supply"));
        assert_eq!(rows[1].total_cents, 2_500);
        assert!(list(&pool, BatchKind::BillPayment, day("2026-11-01"), day("2026-11-30")).await.unwrap().is_empty());
        // The row's id prints the stub for the whole check.
        let stub = crate::forms::document(&pool, crate::forms::FormKind::BillPayment, Some(rows[0].id), None, "$").await.unwrap();
        assert_eq!(stub.lines.len(), 2);

        assert!(list(&pool, BatchKind::CreditMemo, day("2026-01-01"), day("2026-12-31")).await.unwrap().is_empty());
        assert!(list(&pool, BatchKind::SalesReceipt, day("2026-01-01"), day("2026-12-31")).await.unwrap().is_empty());
        assert!(list(&pool, BatchKind::PurchaseOrder, day("2026-01-01"), day("2026-12-31")).await.unwrap().is_empty());
        assert!(list(&pool, BatchKind::CreditMemo, day("2026-12-31"), day("2026-01-01")).await.unwrap_err().starts_with("PRINT_BATCH_DATES"));
    }
}
