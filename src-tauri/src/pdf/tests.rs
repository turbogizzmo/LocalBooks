use super::*;

#[test]
fn measures_with_core_font_widths() {
    // "Hello" in Helvetica: H 722 + e 556 + l 222 + l 222 + o 556 = 2278 → 22.78pt at 10pt.
    assert!((text_width(FontFamily::Helvetica, false, 10.0, "Hello") - 22.78).abs() < 0.01);
    assert!((text_width(FontFamily::Courier, true, 10.0, "abc") - 18.0).abs() < 0.01);
    // Accented letters measure like their base letter.
    assert_eq!(text_width(FontFamily::Times, false, 12.0, "é"), text_width(FontFamily::Times, false, 12.0, "e"));
}

#[test]
fn sanitizes_to_what_the_fonts_can_show() {
    assert_eq!(sanitize("Café – “Ünïcode”\tline€"), "Café – “Ünïcode” line€");
    assert_eq!(sanitize("₹100 東京"), "Rs100 ??");
}

#[test]
fn wraps_words_and_splits_long_ones() {
    let lines = wrap(FontFamily::Helvetica, false, 10.0, "one two three four", 40.0);
    assert!(lines.len() > 1);
    assert!(lines.iter().all(|l| text_width(FontFamily::Helvetica, false, 10.0, l) <= 40.0));
    assert_eq!(lines.join(" "), "one two three four");
    let long = wrap(FontFamily::Helvetica, false, 10.0, "Supercalifragilistic", 30.0);
    assert!(long.len() > 1);
    assert_eq!(long.concat(), "Supercalifragilistic");
    assert_eq!(wrap(FontFamily::Helvetica, false, 10.0, "a\n\nb", 100.0), vec!["a", "", "b"]);
}

#[test]
fn writes_a_well_formed_file() {
    let page = Page {
        width: 612.0,
        height: 792.0,
        ops: vec![
            Op::Text { x: 50.0, y: 60.0, text: "Invoice (No. 1) \\ é".into(), family: FontFamily::Helvetica, bold: true, size: 12.0, color: Rgb(0, 0, 0), align: Align::Left },
            Op::Rect { x: 50.0, y: 70.0, w: 100.0, h: 20.0, fill: Some(Rgb(10, 20, 30)), stroke: None, line_width: 0.0 },
            Op::Line { x1: 50.0, y1: 100.0, x2: 500.0, y2: 100.0, color: Rgb(0, 0, 0), width: 1.0 },
            Op::Image { x: 400.0, y: 40.0, w: 80.0, h: 40.0 },
        ],
    };
    let image = PdfImage { width: 2, height: 1, data: ImageData::Rgb(vec![255, 0, 0, 0, 0, 255]), alpha: Some(vec![255, 0]) };
    let pdf = write_pdf(&[page.clone(), page], Some(&image), "Invoice 1");
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.starts_with("%PDF-1.4"));
    assert!(text.ends_with("%%EOF\n"));
    assert!(text.contains("/BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding"));
    assert!(text.contains("/Count 2"));
    assert!(text.contains("/SMask"));

    // Every xref entry points at its object's header (offsets are in bytes).
    let start: usize = text.rsplit("startxref\n").next().unwrap().lines().next().unwrap().parse().unwrap();
    let table = String::from_utf8(pdf[start..].to_vec()).expect("the cross-reference table is ASCII");
    let count: usize = table.lines().nth(1).unwrap().split(' ').nth(1).unwrap().parse().unwrap();
    for (i, line) in table.lines().skip(3).take(count - 1).enumerate() {
        let offset: usize = line[..10].parse().unwrap();
        assert!(pdf[offset..].starts_with(format!("{} 0 obj", i + 1).as_bytes()), "object {} misplaced", i + 1);
    }
}

#[test]
fn escapes_literal_strings() {
    assert_eq!(literal("a(b)\\c é"), "(a\\(b\\)\\\\c \\351)");
    assert_eq!(num(1.0), "1");
    assert_eq!(num(0.006), "0.01");
    assert_eq!(num(-0.001), "0");
}

#[test]
fn a_protected_pdf_opens_only_with_its_password() {
    let page = Page {
        width: 612.0,
        height: 792.0,
        ops: vec![Op::Text { x: 50.0, y: 60.0, text: "Profit and Loss 2026".into(), family: FontFamily::Helvetica, bold: false, size: 12.0, color: Rgb(0, 0, 0), align: Align::Left }],
    };
    let plain = write_protected_pdf(std::slice::from_ref(&page), None, "Profit and Loss", None).unwrap();
    assert_eq!(plain, write_pdf(std::slice::from_ref(&page), None, "Profit and Loss"), "no password: the usual file");
    let plain_text = String::from_utf8_lossy(&plain);
    assert!(plain_text.starts_with("%PDF-1.4") && plain_text.contains("/Producer (LocalBooks) /Creator (LocalBooks)") && !plain_text.contains("/Encrypt"));

    let pdf = write_protected_pdf(&[page.clone(), page], None, "Profit and Loss", Some("c0rrect horse")).unwrap();
    let raw = String::from_utf8_lossy(&pdf);
    assert!(raw.starts_with("%PDF-1.7") && raw.contains("/Encrypt ") && raw.contains("/ID ["));
    assert!(!raw.contains("LocalBooks") && !raw.contains("Profit"), "the title and producer are encrypted too");

    // A reader opens it with the password and sees the text.
    let doc = lopdf::Document::load_mem_with_password(&pdf, "c0rrect horse").expect("opens with the password");
    assert_eq!(doc.get_pages().len(), 2);
    assert!(doc.extract_text(&[1]).unwrap().contains("Profit and Loss 2026"));
    // And not without it, or with the wrong one.
    assert!(lopdf::Document::load_mem_with_password(&pdf, "wrong").is_err());
    let locked = lopdf::Document::load_mem(&pdf);
    assert!(locked.is_err() || locked.unwrap().extract_text(&[1]).map_or(true, |t| !t.contains("Profit")));
    assert!(write_protected_pdf(&[], None, "x", Some("")).unwrap_err().starts_with("PDF_PASSWORD_EMPTY"));
}
