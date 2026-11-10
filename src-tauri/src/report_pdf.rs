//! Reports, lists and registers as PDF: the same sheet the Excel export writes (`xlsx_export`),
//! laid out as pages of a table, for emailing a report or a scheduled report run. Amounts arrive as
//! integer cents and are formatted the way the screens show them.

#![allow(clippy::float_arithmetic, reason = "page layout in points; amounts arrive as integer cents and are only formatted")]

use serde_json::Value;

use crate::forms::format_money;
use crate::pdf::{self, Align, FontFamily, Op, Page, Rgb};
use crate::xlsx_export::{ColumnKind, RowKind, Sheet};

const MARGIN: f32 = 36.0;
const BODY: f32 = 8.5;
const LEADING: f32 = 12.0;
const INK: Rgb = Rgb(0x1f, 0x29, 0x37);
const MUTED: Rgb = Rgb(0x6b, 0x72, 0x80);
const RULE: Rgb = Rgb(0xd4, 0xd7, 0xdc);
const FONT: FontFamily = FontFamily::Helvetica;

/// "2026-03-01" → "03/01/2026"; anything else as it is.
fn date_text(iso: &str) -> String {
    match iso.get(..10).map(|d| d.split('-').collect::<Vec<_>>()).as_deref() {
        Some([y, m, d]) if y.len() == 4 => format!("{m}/{d}/{y}{}", iso.get(10..).map(|t| t.replace('T', " ")).unwrap_or_default()),
        _ => iso.to_string(),
    }
}

/// One cell as printed.
pub fn cell_text(value: &Value, kind: ColumnKind, symbol: &str) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => match kind {
            ColumnKind::Date | ColumnKind::Datetime => date_text(s),
            _ => s.clone(),
        },
        Value::Number(n) => match (kind, n.as_i64()) {
            (ColumnKind::Money, Some(cents)) => format_money(cents, None, symbol),
            (ColumnKind::Percent, Some(bp)) => {
                let sign = if bp < 0 { "-" } else { "" };
                let abs = bp.unsigned_abs();
                format!("{sign}{}.{:02}%", abs / 100, abs % 100)
            }
            _ => n.to_string(),
        },
        Value::Bool(b) => if *b { "Yes" } else { "No" }.to_string(),
        other => other.to_string(),
    }
}

fn right_aligned(kind: ColumnKind) -> bool {
    matches!(kind, ColumnKind::Money | ColumnKind::Number | ColumnKind::Percent)
}

/// Lays the sheet out as pages: the company, title, period and notes at the top of the first
/// page, the column headings on every page, and a page number at the foot. Wide sheets (more than
/// five columns) print across a landscape page.
pub fn layout(sheet: &Sheet, symbol: &str) -> Vec<Page> {
    let (width, height) = if sheet.columns.len() > 5 { (792.0, 612.0) } else { (612.0, 792.0) };
    let usable = width - 2.0 * MARGIN;
    // Every column but the first gets an even share, capped; the first takes the rest.
    let n = sheet.columns.len().max(1);
    let other_w = if n > 1 { (usable * 0.62 / (n - 1) as f32).min(110.0) } else { 0.0 };
    let first_w = usable - other_w * (n - 1) as f32;
    let col_x = |i: usize| if i == 0 { MARGIN } else { MARGIN + first_w + other_w * (i - 1) as f32 };
    let col_w = |i: usize| if i == 0 { first_w } else { other_w };

    let mut pages: Vec<Page> = Vec::new();
    let mut ops: Vec<Op> = Vec::new();
    let mut y;
    let text = |ops: &mut Vec<Op>, x: f32, y: f32, s: &str, bold: bool, size: f32, color: Rgb, align: Align| {
        ops.push(Op::Text { x, y, text: s.to_string(), family: FONT, bold, size, color, align });
    };
    let fit = |s: &str, bold: bool, w: f32| -> String {
        if pdf::text_width(FONT, bold, BODY, s) <= w {
            return s.to_string();
        }
        let mut out: String = s.to_string();
        while !out.is_empty() && pdf::text_width(FONT, bold, BODY, &format!("{out}…")) > w {
            out.pop();
        }
        format!("{out}…")
    };
    let head = |ops: &mut Vec<Op>, y: &mut f32| {
        for (i, c) in sheet.columns.iter().enumerate() {
            let right = right_aligned(c.kind);
            let x = if right { col_x(i) + col_w(i) - 2.0 } else { col_x(i) + 2.0 };
            ops.push(Op::Text { x, y: *y, text: fit(&c.label, true, col_w(i) - 4.0), family: FONT, bold: true, size: BODY, color: MUTED, align: if right { Align::Right } else { Align::Left } });
        }
        ops.push(Op::Line { x1: MARGIN, y1: *y + 4.0, x2: width - MARGIN, y2: *y + 4.0, color: RULE, width: 0.8 });
        *y += LEADING + 2.0;
    };

    // The first page's title block.
    y = MARGIN + 12.0;
    if !sheet.company.trim().is_empty() {
        text(&mut ops, width / 2.0, y, sheet.company.trim(), true, 11.0, INK, Align::Center);
        y += 15.0;
    }
    text(&mut ops, width / 2.0, y, sheet.title.trim(), true, 14.0, INK, Align::Center);
    y += 15.0;
    for line in std::iter::once(&sheet.subtitle).chain(&sheet.notes).filter(|l| !l.trim().is_empty()) {
        text(&mut ops, width / 2.0, y, line.trim(), false, 9.0, MUTED, Align::Center);
        y += 12.0;
    }
    y += 8.0;
    head(&mut ops, &mut y);

    let bottom = height - MARGIN - 18.0;
    for row in &sheet.rows {
        if y > bottom {
            pages.push(Page { width, height, ops: std::mem::take(&mut ops) });
            y = MARGIN + 12.0;
            text(&mut ops, MARGIN, y, &format!("{} (continued)", sheet.title.trim()), true, 9.0, MUTED, Align::Left);
            y += 16.0;
            head(&mut ops, &mut y);
        }
        let bold = matches!(row.kind, RowKind::Section | RowKind::Subtotal | RowKind::Total | RowKind::Net);
        if matches!(row.kind, RowKind::Total | RowKind::Net | RowKind::Subtotal) {
            ops.push(Op::Line { x1: col_x(1.min(n - 1)), y1: y - LEADING + 3.0, x2: width - MARGIN, y2: y - LEADING + 3.0, color: RULE, width: 0.6 });
        }
        for (i, c) in sheet.columns.iter().enumerate() {
            let value = row.cells.get(i).unwrap_or(&Value::Null);
            let s = cell_text(value, c.kind, symbol);
            if s.is_empty() {
                continue;
            }
            let indent = if i == sheet.indent_column { f32::from(row.depth) * 10.0 } else { 0.0 };
            let right = right_aligned(c.kind);
            let w = col_w(i) - 4.0 - indent;
            let x = if right { col_x(i) + col_w(i) - 2.0 } else { col_x(i) + 2.0 + indent };
            ops.push(Op::Text { x, y, text: fit(&s, bold, w), family: FONT, bold, size: BODY, color: INK, align: if right { Align::Right } else { Align::Left } });
        }
        y += LEADING;
    }
    if let Some(footer) = sheet.footer.as_deref().filter(|f| !f.trim().is_empty()) {
        y += 6.0;
        text(&mut ops, MARGIN, y, footer.trim(), false, 8.0, MUTED, Align::Left);
    }
    pages.push(Page { width, height, ops });
    let count = pages.len();
    for (i, page) in pages.iter_mut().enumerate() {
        page.ops.push(Op::Text { x: width / 2.0, y: height - MARGIN + 6.0, text: format!("Page {} of {count}", i + 1), family: FONT, bold: false, size: 7.5, color: MUTED, align: Align::Center });
    }
    pages
}

/// "Profit and Loss.pdf": the sheet's title as a file name.
pub fn file_name(sheet: &Sheet) -> String {
    format!("{}.pdf", crate::forms::file_name(sheet.title.trim()).trim_end_matches(".pdf"))
}

/// The sheet as PDF bytes.
pub fn to_pdf(sheet: &Sheet, symbol: &str) -> Vec<u8> {
    pdf::write_pdf(&layout(sheet, symbol), None, sheet.title.trim())
}

/// The sheet as PDF bytes that open only with `password`, when one is given.
pub fn to_protected_pdf(sheet: &Sheet, symbol: &str, password: Option<&str>) -> Result<Vec<u8>, String> {
    pdf::write_protected_pdf(&layout(sheet, symbol), None, sheet.title.trim(), password)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xlsx_export::{Column, Row};
    use serde_json::json;

    fn texts(pages: &[Page]) -> Vec<String> {
        pages.iter().flat_map(|p| p.ops.iter()).filter_map(|op| if let Op::Text { text, .. } = op { Some(text.clone()) } else { None }).collect()
    }

    fn sheet(rows: usize) -> Sheet {
        Sheet {
            company: "Maple Street Bakery".into(),
            title: "Sales by Customer".into(),
            subtitle: "March 1 - March 31, 2026".into(),
            notes: vec!["Accrual basis".into()],
            columns: vec![Column { label: "Customer".into(), kind: ColumnKind::Text }, Column { label: "Date".into(), kind: ColumnKind::Date }, Column { label: "Amount".into(), kind: ColumnKind::Money }],
            rows: (0..rows).map(|i| Row { kind: RowKind::Detail, depth: 1, cells: vec![json!(format!("Customer {i}")), json!("2026-03-05"), json!(123_456)] }).chain(std::iter::once(Row { kind: RowKind::Total, depth: 0, cells: vec![json!("TOTAL"), Value::Null, json!(-99)] })).collect(),
            indent_column: 0,
            footer: Some("Thank you".into()),
        }
    }

    #[test]
    fn cells_print_as_the_screens_show_them() {
        assert_eq!(cell_text(&json!(123_456), ColumnKind::Money, "$"), "$1,234.56");
        assert_eq!(cell_text(&json!(-99), ColumnKind::Money, "$"), "-$0.99");
        assert_eq!(cell_text(&json!(1_250), ColumnKind::Percent, "$"), "12.50%");
        assert_eq!(cell_text(&json!(-5), ColumnKind::Percent, "$"), "-0.05%");
        assert_eq!(cell_text(&json!("2026-03-05"), ColumnKind::Date, "$"), "03/05/2026");
        assert_eq!(cell_text(&json!("2026-03-05T10:20:00"), ColumnKind::Datetime, "$"), "03/05/2026 10:20:00");
        assert_eq!(cell_text(&json!(3), ColumnKind::Number, "$"), "3");
        assert_eq!(cell_text(&Value::Null, ColumnKind::Money, "$"), "");
    }

    #[test]
    fn a_short_report_is_one_page_with_its_heading_rows_and_total() {
        let pages = layout(&sheet(3), "$");
        assert_eq!(pages.len(), 1);
        let t = texts(&pages);
        for expected in ["Maple Street Bakery", "Sales by Customer", "March 1 - March 31, 2026", "Accrual basis", "Customer", "Customer 2", "03/05/2026", "$1,234.56", "TOTAL", "-$0.99", "Thank you", "Page 1 of 1"] {
            assert!(t.iter().any(|s| s == expected), "missing {expected:?} in {t:?}");
        }
        assert!(to_pdf(&sheet(3), "$").starts_with(b"%PDF"));
        assert_eq!(file_name(&sheet(0)), "Sales by Customer.pdf");
    }

    #[test]
    fn long_reports_continue_on_more_pages_with_the_headings_repeated() {
        let pages = layout(&sheet(150), "$");
        assert!(pages.len() >= 3, "{} pages", pages.len());
        let last = texts(&pages[pages.len() - 1..]);
        assert!(last.iter().any(|s| s == "Sales by Customer (continued)"));
        assert!(last.iter().any(|s| s == "Customer"), "headings repeated");
        assert!(last.iter().any(|s| s == &format!("Page {} of {}", pages.len(), pages.len())));
        // Every row is somewhere.
        let all = texts(&pages);
        assert!(all.iter().any(|s| s == "Customer 149"));
        assert!(pages.iter().all(|p| p.ops.iter().all(|op| !matches!(op, Op::Text { y, .. } if *y > p.height))));
    }

    #[test]
    fn wide_sheets_go_landscape_and_long_cells_are_cut_to_fit() {
        let mut s = sheet(1);
        s.columns = (0..8).map(|i| Column { label: format!("Col {i}"), kind: ColumnKind::Text }).collect();
        s.rows = vec![Row { kind: RowKind::Detail, depth: 0, cells: (0..8).map(|_| json!("a very long cell value that cannot fit in a narrow column")).collect() }];
        let pages = layout(&s, "$");
        assert!(pages[0].width > pages[0].height);
        assert!(texts(&pages).iter().any(|t| t.ends_with('…')));
    }
}
