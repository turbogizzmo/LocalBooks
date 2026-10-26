use std::io::{Cursor, Read};

use calamine::{open_workbook_from_rs, Data, DataType, Range, Reader, Xlsx};
use serde_json::json;

use super::*;
use crate::db::test_pool;
use crate::ledger::{post_journal_entry, JournalEntry, JournalLine};

/// The first worksheet of a workbook, read back with calamine.
fn read_back(bytes: &[u8]) -> (String, Range<Data>) {
    let mut book: Xlsx<_> = open_workbook_from_rs(Cursor::new(bytes.to_vec())).expect("a valid .xlsx");
    let name = book.sheet_names()[0].clone();
    let range = book.worksheet_range(&name).unwrap();
    (name, range)
}

/// One part of the .xlsx package (`xl/styles.xml`, `xl/worksheets/sheet1.xml`) as text.
fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
    let mut out = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut out).unwrap();
    out
}

fn cell(range: &Range<Data>, row: u32, col: u32) -> &Data {
    range.get_value((row, col)).unwrap_or(&Data::Empty)
}

fn text(s: &str) -> Data {
    Data::String(s.into())
}

fn sample() -> Sheet {
    serde_json::from_value(json!({
        "company": "Acme & Co",
        "title": "A/R Aging Detail: Open [Invoices]",
        "subtitle": "As of March 31, 2026",
        "notes": ["Accrual basis"],
        "columns": [
            { "label": "Customer" },
            { "label": "Date", "kind": "date" },
            { "label": "Qty", "kind": "number" },
            { "label": "Open balance", "kind": "money" },
            { "label": "% of total", "kind": "percent" },
            { "label": "Recorded", "kind": "datetime" }
        ],
        "rows": [
            { "kind": "section", "cells": ["Customers"] },
            { "depth": 1, "cells": ["Northwind", "2026-03-02", 3, 123456, 1250, "2026-03-02 14:05:09"] },
            { "depth": 2, "cells": ["Northwind:Seattle", "2026-03-05", 1.5, -9905, null] },
            { "depth": 1, "cells": ["Not a date", "soon", "", "", ""] },
            { "kind": "total", "cells": ["TOTAL", null, 4.5, 113551, 10000] }
        ],
        "footer": "Amounts in US dollars"
    }))
    .unwrap()
}

#[test]
fn money_formats_quote_the_currency_symbol() {
    assert_eq!(money_format("$"), r##""$"#,##0.00;[Red]-"$"#,##0.00"##);
    assert_eq!(money_format("€"), r##""€"#,##0.00;[Red]-"€"#,##0.00"##);
    assert_eq!(money_format(""), "#,##0.00;[Red]-#,##0.00");
}

#[test]
fn sheet_names_are_ones_excel_accepts() {
    assert_eq!(sheet_name("A/R Aging Detail: Open [Invoices]"), "A-R Aging Detail- Open -Invoice");
    assert_eq!(sheet_name("  "), "Report");
    assert_eq!(sheet_name("Profit and Loss"), "Profit and Loss");
}

#[test]
fn writes_typed_cells_under_a_quickbooks_style_header() {
    let bytes = to_xlsx(&sample(), "$").unwrap();
    let (name, range) = read_back(&bytes);
    assert_eq!(name, "A-R Aging Detail- Open -Invoice");

    // Header block: company, title, date range, notes, blank row, column headers.
    assert_eq!(cell(&range, 0, 0), &text("Acme & Co"));
    assert_eq!(cell(&range, 1, 0), &text("A/R Aging Detail: Open [Invoices]"));
    assert_eq!(cell(&range, 2, 0), &text("As of March 31, 2026"));
    assert_eq!(cell(&range, 3, 0), &text("Accrual basis"));
    assert_eq!(cell(&range, 4, 0), &Data::Empty);
    assert_eq!(cell(&range, 5, 3), &text("Open balance"));

    // Section row, then details: labels stay text without padding spaces (the indent is a format).
    assert_eq!(cell(&range, 6, 0), &text("Customers"));
    assert_eq!(cell(&range, 7, 0), &text("Northwind"));
    assert_eq!(cell(&range, 8, 0), &text("Northwind:Seattle"));

    // Money and percent are numbers, never text.
    assert_eq!(cell(&range, 7, 3), &Data::Float(1234.56));
    assert_eq!(cell(&range, 8, 3), &Data::Float(-99.05));
    assert_eq!(cell(&range, 7, 4), &Data::Float(0.125));
    assert_eq!(cell(&range, 7, 2), &Data::Float(3.0));
    assert_eq!(cell(&range, 8, 2), &Data::Float(1.5));
    assert_eq!(cell(&range, 8, 4), &Data::Empty);

    // Dates are date cells.
    let date = cell(&range, 7, 1);
    assert!(date.is_datetime(), "{date:?}");
    assert_eq!(date.as_date(), NaiveDate::from_ymd_opt(2026, 3, 2));
    let stamp = cell(&range, 7, 5);
    assert!(stamp.is_datetime(), "{stamp:?}");
    assert_eq!(stamp.as_datetime(), NaiveDate::from_ymd_opt(2026, 3, 2).unwrap().and_hms_opt(14, 5, 9));
    // Text that is not a date stays text; empty strings are empty cells.
    assert_eq!(cell(&range, 9, 1), &text("soon"));
    assert_eq!(cell(&range, 9, 3), &Data::Empty);

    // Total row, then the footer after a blank row.
    assert_eq!(cell(&range, 10, 0), &text("TOTAL"));
    assert_eq!(cell(&range, 10, 3), &Data::Float(1135.51));
    assert_eq!(cell(&range, 10, 4), &Data::Float(1.0));
    assert_eq!(cell(&range, 12, 0), &text("Amounts in US dollars"));

    // Formatting the reader can't see: currency format with red negatives, bold header and totals,
    // indent levels, frozen header row and set column widths.
    let styles = part(&bytes, "xl/styles.xml");
    assert!(styles.contains(r##"formatCode="&quot;$&quot;#,##0.00;[Red]-&quot;$&quot;#,##0.00""##), "{styles}");
    assert!(styles.contains(r#"formatCode="0.0%;[Red]-0.0%""#), "{styles}");
    assert!(styles.contains(r#"indent="1""#) && styles.contains(r#"indent="2""#), "{styles}");
    assert!(styles.contains("<b/>"), "{styles}");
    assert!(styles.contains(r#"<top style="thin">"#), "total rows are ruled: {styles}");
    let sheet = part(&bytes, "xl/worksheets/sheet1.xml");
    assert!(sheet.contains(r#"<pane ySplit="6" topLeftCell="A7" activePane="bottomLeft" state="frozen"/>"#), "{sheet}");
    assert!(sheet.contains(r#"customWidth="1""#), "{sheet}");
}

#[test]
fn rejects_a_sheet_without_columns() {
    let mut sheet = sample();
    sheet.columns.clear();
    assert!(to_xlsx(&sheet, "$").unwrap_err().starts_with("EXPORT_XLSX"));
}

#[tokio::test]
async fn exports_statements_with_numbers_indents_and_totals() {
    let pool = test_pool().await;
    let id = |n: &'static str| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(&pool).await.unwrap()
        }
    };
    let (checking, services) = (id("1010").await, id("4100").await);
    let date = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
    post_journal_entry(&pool, JournalEntry { id: None, date, description: String::new(), lines: vec![JournalLine::debit(checking, 123_456), JournalLine::credit(services, 123_456)], currency: None })
        .await
        .unwrap();
    crate::preferences::set(&pool, "company_name", "Acme Widgets").await.unwrap();

    let (start, end) = (NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), NaiveDate::from_ymd_opt(2026, 12, 31).unwrap());
    let bytes = render_report(&pool, "profit_and_loss", Some(start), Some(end), None, None).await.unwrap();
    let (name, range) = read_back(&bytes);
    assert_eq!(name, "Profit and Loss");
    assert_eq!(cell(&range, 0, 0), &text("Acme Widgets"));
    assert_eq!(cell(&range, 2, 0), &text("January 1, 2026 - December 31, 2026"));
    assert_eq!(cell(&range, 3, 0), &text("Accrual basis"));
    let rows: Vec<(Data, Data)> = (0..range.height() as u32).map(|r| (cell(&range, r, 0).clone(), cell(&range, r, 1).clone())).collect();
    assert!(rows.contains(&(text("4100 Services"), Data::Float(1234.56))), "{rows:?}");
    assert_eq!(rows.last().unwrap(), &(text("Net Income"), Data::Float(1234.56)));
    let styles = part(&bytes, "xl/styles.xml");
    assert!(styles.contains(r#"indent="1""#), "account lines keep their indent: {styles}");
    assert!(styles.contains(r#"<bottom style="double">"#), "Net Income is double-ruled: {styles}");

    let by_class = render_report(&pool, "profit_and_loss_by_class", Some(date), Some(date), None, None).await.unwrap();
    let (_, range) = read_back(&by_class);
    let header: Vec<Data> = (0..3).map(|c| cell(&range, 5, c).clone()).collect();
    assert_eq!(header, vec![Data::Empty, text("Unclassified"), text("Total")]);
    let last = range.height() as u32 - 1;
    assert_eq!((cell(&range, last, 0), cell(&range, last, 1), cell(&range, last, 2)), (&text("Net Income"), &Data::Float(1234.56), &Data::Float(1234.56)));

    crate::preferences::set(&pool, "currency_symbol", "€").await.unwrap();
    let bs = render_report(&pool, "balance_sheet", None, None, Some(date), None).await.unwrap();
    let (_, range) = read_back(&bs);
    assert_eq!(cell(&range, 2, 0), &text("As of October 1, 2026"));
    let rows: Vec<(Data, Data)> = (0..range.height() as u32).map(|r| (cell(&range, r, 0).clone(), cell(&range, r, 1).clone())).collect();
    assert!(rows.contains(&(text("TOTAL ASSETS"), Data::Float(1234.56))), "{rows:?}");
    assert!(part(&bs, "xl/styles.xml").contains("&quot;€&quot;#,##0.00"), "the company's currency symbol");

    assert!(render_report(&pool, "cash_flow", None, None, Some(date), None).await.unwrap_err().starts_with("EXPORT_UNKNOWN_REPORT"));
    assert!(render_report(&pool, "balance_sheet", None, None, None, None).await.unwrap_err().starts_with("EXPORT_BAD_RANGE"));
}

