//! Statements are generated here with the app's own PDF writer: made-up banks, people and amounts.
//! Never commit a real statement.

use super::*;
use crate::pdf::{write_pdf, Align, FontFamily, Op, Page, PdfImage, Rgb};

const W: f32 = 612.0;
const H: f32 = 792.0;

fn text(x: f32, y: f32, s: &str) -> Op {
    Op::Text { x, y, text: s.into(), family: FontFamily::Helvetica, bold: false, size: 9.0, color: Rgb(0, 0, 0), align: Align::Left }
}

fn bold(x: f32, y: f32, s: &str) -> Op {
    Op::Text { x, y, text: s.into(), family: FontFamily::Helvetica, bold: true, size: 9.0, color: Rgb(0, 0, 0), align: Align::Left }
}

/// Right-aligned at `x`, as statements print amounts.
fn right(x: f32, y: f32, s: &str) -> Op {
    Op::Text { x, y, text: s.into(), family: FontFamily::Helvetica, bold: false, size: 9.0, color: Rgb(0, 0, 0), align: Align::Right }
}

fn page(ops: Vec<Op>) -> Page {
    Page { width: W, height: H, ops }
}

/// A checking statement over two pages: yearless dates, separate withdrawal and deposit columns, a
/// running balance, wrapped descriptions, a date printed once for two transactions, a repeated
/// header and page footers.
pub(crate) fn checking_statement_pdf() -> Vec<u8> {
    let header = |y: f32| vec![bold(40.0, y, "Date"), bold(90.0, y, "Description"), right(400.0, y, "Withdrawals"), right(480.0, y, "Deposits"), right(560.0, y, "Balance")];
    let mut first = vec![
        bold(40.0, 50.0, "Maple Street Credit Union"),
        text(40.0, 64.0, "Pat Example, 12 Sample Road, Springfield"),
        text(40.0, 78.0, "Statement period: January 1, 2026 to January 31, 2026"),
        text(40.0, 92.0, "Account 0000-1234 Everyday Checking"),
        text(40.0, 120.0, "Beginning balance on January 1, 2026"),
        right(560.0, 120.0, "$1,000.00"),
        text(40.0, 134.0, "Ending balance on January 31, 2026"),
        right(560.0, 134.0, "$2,019.40"),
    ];
    first.extend(header(170.0));
    let mut y = 186.0;
    let row = |ops: &mut Vec<Op>, y: &mut f32, date: &str, desc: &str, out: &str, inn: &str, bal: &str| {
        if !date.is_empty() {
            ops.push(text(40.0, *y, date));
        }
        ops.push(text(90.0, *y, desc));
        if !out.is_empty() {
            ops.push(right(400.0, *y, out));
        }
        if !inn.is_empty() {
            ops.push(right(480.0, *y, inn));
        }
        if !bal.is_empty() {
            ops.push(right(560.0, *y, bal));
        }
        *y += 13.0;
    };
    row(&mut first, &mut y, "01/02", "Opening balance", "", "", "1,000.00");
    row(&mut first, &mut y, "01/03", "POS PURCHASE GREENLEAF GROCERY", "54.20", "", "945.80");
    first.push(text(90.0, y, "SPRINGFIELD CARD 4321"));
    y += 13.0;
    row(&mut first, &mut y, "01/05", "PAYROLL ACME WIDGETS", "", "1,850.00", "2,795.80");
    row(&mut first, &mut y, "", "ATM WITHDRAWAL 0042", "100.00", "", "2,695.80");
    row(&mut first, &mut y, "01/09", "CHECK 1043", "1,250.00", "", "1,445.80");
    first.push(text(40.0, 760.0, "Page 1 of 2"));

    let mut second = vec![text(40.0, 50.0, "Maple Street Credit Union  Statement period: January 1, 2026 to January 31, 2026")];
    second.extend(header(80.0));
    let mut y = 96.0;
    row(&mut second, &mut y, "01/15", "ONLINE TRANSFER FROM SAVINGS", "", "400.00", "1,845.80");
    row(&mut second, &mut y, "01/20", "ELECTRIC CO AUTOPAY", "126.40", "", "1,719.40");
    second.push(text(90.0, y, "ACCOUNT 99-123"));
    y += 13.0;
    row(&mut second, &mut y, "01/28", "MOBILE DEPOSIT", "", "300.00", "2,019.40");
    row(&mut second, &mut y, "", "Total", "1,530.60", "2,550.00", "");
    y += 40.0;
    second.push(bold(40.0, y, "Daily balance summary"));
    y += 14.0;
    second.extend([bold(40.0, y, "Date"), right(200.0, y, "Balance"), bold(260.0, y, "Date"), right(420.0, y, "Balance")]);
    y += 13.0;
    second.extend([text(40.0, y, "01/03"), right(200.0, y, "945.80"), text(260.0, y, "01/05"), right(420.0, y, "2,695.80")]);
    second.push(text(40.0, 760.0, "Page 2 of 2"));
    write_pdf(&[page(first), page(second)], None, "Statement")
}

/// A card statement from December into January: month-name dates without a year, two date columns,
/// one amount column with charges positive and payments marked CR.
pub(crate) fn card_statement_pdf() -> Vec<u8> {
    let mut ops = vec![
        bold(40.0, 50.0, "Example Rewards Visa"),
        text(40.0, 64.0, "Billing period Dec 15, 2025 - Jan 14, 2026"),
        text(40.0, 90.0, "Previous Balance"),
        right(300.0, 90.0, "$500.00"),
        text(40.0, 104.0, "New Balance"),
        right(300.0, 104.0, "$632.15"),
        text(40.0, 118.0, "Payment Due Date 02/08/2026"),
        bold(40.0, 150.0, "Trans Date"),
        bold(110.0, 150.0, "Post Date"),
        bold(180.0, 150.0, "Description"),
        right(560.0, 150.0, "Amount"),
    ];
    let rows = [
        ("Dec 18", "Dec 19", "CORNER CAFE", "12.50"),
        ("Dec 22", "Dec 23", "PAYMENT THANK YOU", "500.00 CR"),
        ("Dec 30", "Dec 31", "OFFICE SUPPLY DEPOT", "219.65"),
        ("Jan 03", "Jan 04", "CLOUD HOSTING CO", "400.00"),
    ];
    let mut y = 166.0;
    for (t, p, d, a) in rows {
        ops.extend([text(40.0, y, t), text(110.0, y, p), text(180.0, y, d), right(560.0, y, a)]);
        y += 13.0;
    }
    write_pdf(&[page(ops)], None, "Card statement")
}

/// What a scanner produces: a page that is one picture and no text.
pub(crate) fn scanned_statement_pdf() -> Vec<u8> {
    let png = {
        let mut bytes = Vec::new();
        let img = image::RgbImage::from_pixel(40, 40, image::Rgb([200, 200, 200]));
        img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
        bytes
    };
    let img = PdfImage::decode(&png).unwrap();
    write_pdf(&[page(vec![Op::Image { x: 0.0, y: 0.0, w: W, h: H }])], Some(&img), "Scan")
}

fn cells<'a>(t: &'a PdfTable, col: &str) -> Vec<&'a str> {
    let i = t.headers.iter().position(|h| h == col).unwrap_or_else(|| panic!("no column {col} in {:?}", t.headers));
    t.records.iter().map(|(_, r)| r[i].as_str()).collect()
}

#[test]
fn reads_a_checking_statement_table() {
    let t = read_table(&checking_statement_pdf()).unwrap();
    assert_eq!(t.headers, ["Date", "Description", "Withdrawals", "Deposits", "Balance"]);
    assert_eq!((t.opening_cents, t.closing_cents), (Some(100_000), Some(201_940)));
    assert_eq!(t.balance_column.as_deref(), Some("Balance"));
    assert_eq!(
        cells(&t, "Date"),
        ["01/03/2026", "01/05/2026", "01/05/2026", "01/09/2026", "01/15/2026", "01/20/2026", "01/28/2026"],
        "yearless dates get the statement's year; a date printed once covers the line below it"
    );
    assert_eq!(
        cells(&t, "Description"),
        [
            "POS PURCHASE GREENLEAF GROCERY SPRINGFIELD CARD 4321",
            "PAYROLL ACME WIDGETS",
            "ATM WITHDRAWAL 0042",
            "CHECK 1043",
            "ONLINE TRANSFER FROM SAVINGS",
            "ELECTRIC CO AUTOPAY ACCOUNT 99-123",
            "MOBILE DEPOSIT",
        ],
        "wrapped descriptions are joined; the opening-balance row, totals, footers and the daily summary are not transactions"
    );
    assert_eq!(cells(&t, "Withdrawals"), ["54.20", "", "100.00", "1,250.00", "", "126.40", ""]);
    assert_eq!(cells(&t, "Deposits"), ["", "1,850.00", "", "", "400.00", "", "300.00"]);
    assert_eq!(cells(&t, "Balance").last(), Some(&"2,019.40"));
    assert_eq!(t.records.iter().map(|(n, _)| *n).collect::<Vec<_>>(), (1..=7).collect::<Vec<u64>>());
}

#[test]
fn reads_a_card_statement_across_the_new_year() {
    let t = read_table(&card_statement_pdf()).unwrap();
    assert_eq!(t.headers, ["Trans Date", "Post Date", "Description", "Amount"]);
    assert_eq!((t.opening_cents, t.closing_cents, t.balance_column.as_deref()), (Some(50_000), Some(63_215), None));
    assert_eq!(cells(&t, "Trans Date"), ["Dec 18, 2025", "Dec 22, 2025", "Dec 30, 2025", "Jan 03, 2026"]);
    // Only payments are marked, so CR is the exception: money that lowers what is owed.
    assert_eq!(cells(&t, "Amount"), ["12.50", "-500.00", "219.65", "400.00"]);
}

#[test]
fn scanned_pdf_is_refused_with_a_clear_message() {
    let err = read_table(&scanned_statement_pdf()).unwrap_err();
    assert!(err.starts_with("BANK_PDF_NO_TEXT"), "{err}");
    assert!(err.contains("OCR"), "{err}");
}

#[test]
fn text_without_a_table_is_refused() {
    let pdf = write_pdf(&[page(vec![text(40.0, 60.0, "Dear customer, thank you for banking with us. Your new card is on its way.")])], None, "Letter");
    let err = read_table(&pdf).unwrap_err();
    assert!(err.starts_with("BANK_PDF_NO_TABLE"), "{err}");
}

#[test]
fn damaged_pdf_is_refused() {
    let err = read_table(b"%PDF-1.4\nnot really a pdf").unwrap_err();
    assert!(err.starts_with("BANK_PDF_READ"), "{err}");
}

#[test]
fn recognizes_amounts_and_yearless_dates() {
    for yes in ["1,234.56", "$1,234.56", "-12.00", "(12.00)", "12.00-", "0.99", "-$5.00"] {
        assert!(is_amount(yes), "{yes}");
    }
    for no in ["12", "1043", "01/15", "12.5", "ABC", "1,23.45x", ".50", "99-123"] {
        assert!(!is_amount(no), "{no}");
    }
    assert_eq!(split_marker("45.00CR"), ("45.00".to_string(), Some(Marker::Cr)));
    assert_eq!(split_marker("CREDIT"), ("CREDIT".to_string(), None));
    let period = Some((NaiveDate::from_ymd_opt(2025, 12, 15).unwrap(), NaiveDate::from_ymd_opt(2026, 1, 14).unwrap()));
    assert_eq!(complete_date("12/20", period).as_deref(), Some("12/20/2025"));
    assert_eq!(complete_date("1/02", period).as_deref(), Some("1/02/2026"));
    assert_eq!(complete_date("20 Dec", period).as_deref(), Some("20 Dec 2025"));
    assert_eq!(complete_date("03/01/2026", None).as_deref(), Some("03/01/2026"));
    assert_eq!(complete_date("CHECK", period), None);
}

/// Writes the sample statements for a hands-on check:
/// `LOCALBOOKS_SAMPLE_DIR=/some/dir cargo test -- --ignored write_sample_statements`.
#[test]
#[ignore]
fn write_sample_statements() {
    let dir = std::path::PathBuf::from(std::env::var("LOCALBOOKS_SAMPLE_DIR").expect("set LOCALBOOKS_SAMPLE_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("sample-checking-statement.pdf"), checking_statement_pdf()).unwrap();
    std::fs::write(dir.join("sample-card-statement.pdf"), card_statement_pdf()).unwrap();
    std::fs::write(dir.join("sample-scanned-statement.pdf"), scanned_statement_pdf()).unwrap();
}
