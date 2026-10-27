//! A small PDF writer for printed forms.
//!
//! Forms are laid out once (see `forms`) into pages of drawing operations; this module measures
//! text for that layout and serializes the same pages to PDF 1.4. It uses the standard Type 1 fonts
//! every PDF reader has (Helvetica, Times, Courier) with WinAnsi encoding, so no font files are
//! embedded, and embeds the company logo as an image. Text is sanitized to what WinAnsi can show
//! before it is laid out, so the on-screen preview and the PDF print the same characters.

#![allow(clippy::float_arithmetic, reason = "page layout in points; no amounts are computed here")]

use std::fmt::Write as _;
use std::io::Write as _;

use serde::{Deserialize, Serialize};

// ------------------------------------------------------------------------------------------------
// Fonts and text
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FontFamily {
    #[default]
    Helvetica,
    Times,
    Courier,
}

impl FontFamily {
    fn base_font(self, bold: bool) -> &'static str {
        match (self, bold) {
            (FontFamily::Helvetica, false) => "Helvetica",
            (FontFamily::Helvetica, true) => "Helvetica-Bold",
            (FontFamily::Times, false) => "Times-Roman",
            (FontFamily::Times, true) => "Times-Bold",
            (FontFamily::Courier, false) => "Courier",
            (FontFamily::Courier, true) => "Courier-Bold",
        }
    }
}

/// Advance widths (1/1000 em) of ASCII 32..=126, from the Adobe Core 14 AFM files.
#[rustfmt::skip]
const HELVETICA: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556,
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556,
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556,
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];
#[rustfmt::skip]
const HELVETICA_BOLD: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611,
    975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556,
    333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611,
    611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];
#[rustfmt::skip]
const TIMES: [u16; 95] = [
    250, 333, 408, 500, 500, 833, 778, 180, 333, 333, 500, 564, 250, 333, 250, 278,
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 278, 278, 564, 564, 564, 444,
    921, 722, 667, 667, 722, 611, 556, 722, 722, 333, 389, 722, 611, 889, 722, 722,
    556, 722, 667, 556, 611, 722, 722, 944, 722, 722, 611, 333, 278, 333, 469, 500,
    333, 444, 500, 444, 500, 444, 333, 500, 500, 278, 278, 500, 278, 778, 500, 500,
    500, 500, 333, 389, 278, 500, 500, 722, 500, 500, 444, 480, 200, 480, 541,
];
#[rustfmt::skip]
const TIMES_BOLD: [u16; 95] = [
    250, 333, 555, 500, 500, 1000, 833, 278, 333, 333, 500, 570, 250, 333, 250, 278,
    500, 500, 500, 500, 500, 500, 500, 500, 500, 500, 333, 333, 570, 570, 570, 500,
    930, 722, 667, 722, 722, 667, 611, 778, 778, 389, 500, 778, 667, 944, 722, 778,
    611, 778, 722, 556, 667, 722, 722, 1000, 722, 722, 667, 333, 278, 333, 581, 500,
    333, 500, 556, 444, 556, 444, 333, 500, 556, 278, 333, 556, 278, 833, 556, 500,
    556, 556, 444, 389, 333, 556, 500, 722, 500, 500, 444, 394, 220, 394, 520,
];

/// Unicode characters WinAnsi places in 0x80..=0x9F.
const WINANSI_HIGH: [(char, u8); 27] = [
    ('€', 0x80), ('‚', 0x82), ('ƒ', 0x83), ('„', 0x84), ('…', 0x85), ('†', 0x86), ('‡', 0x87),
    ('ˆ', 0x88), ('‰', 0x89), ('Š', 0x8A), ('‹', 0x8B), ('Œ', 0x8C), ('Ž', 0x8E), ('\u{2018}', 0x91),
    ('\u{2019}', 0x92), ('\u{201C}', 0x93), ('\u{201D}', 0x94), ('•', 0x95), ('–', 0x96), ('—', 0x97),
    ('˜', 0x98), ('™', 0x99), ('š', 0x9A), ('›', 0x9B), ('œ', 0x9C), ('ž', 0x9E), ('Ÿ', 0x9F),
];

/// The WinAnsi code for a character, if the standard fonts can show it.
fn winansi(c: char) -> Option<u8> {
    match c {
        ' '..='~' => Some(c as u8),
        '\u{A0}'..='\u{FF}' => Some(c as u32 as u8),
        _ => WINANSI_HIGH.iter().find(|(u, _)| *u == c).map(|(_, b)| *b),
    }
}

/// Replaces what the standard fonts cannot show: whitespace becomes a space, a few currency signs
/// are spelled out, anything else becomes "?".
pub fn sanitize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\t' | '\r' | '\n' => out.push(' '),
            '₹' => out.push_str("Rs"),
            '₩' => out.push('W'),
            '₽' => out.push_str("RUB"),
            '\u{2212}' => out.push('-'),
            '\u{2009}' | '\u{202F}' => out.push(' '),
            c if winansi(c).is_some() => out.push(c),
            c if c.is_control() => {}
            _ => out.push('?'),
        }
    }
    out
}

/// An ASCII character with about the same width, for WinAnsi codes above 126.
fn ascii_stand_in(code: u8) -> u8 {
    match code {
        0xC0..=0xC5 => b'A',
        0xC7 => b'C',
        0xC8..=0xCB => b'E',
        0xCC..=0xCF => b'I',
        0xD0 => b'D',
        0xD1 => b'N',
        0xD2..=0xD6 | 0xD8 => b'O',
        0xD9..=0xDC => b'U',
        0xDD => b'Y',
        0xDE => b'P',
        0xDF => b'B',
        0xE0..=0xE5 => b'a',
        0xE7 => b'c',
        0xE8..=0xEB => b'e',
        0xEC..=0xEF => b'i',
        0xF1 => b'n',
        0xF2..=0xF6 | 0xF8 | 0xF0 => b'o',
        0xF9..=0xFC => b'u',
        0xFD | 0xFF => b'y',
        0xFE => b'p',
        0xC6 | 0x8C => b'W',
        0xE6 | 0x9C => b'm',
        0x8A => b'S',
        0x9A => b's',
        0x8E => b'Z',
        0x9E => b'z',
        0x9F => b'Y',
        0x91 | 0x92 | 0x82 | 0xB4 | 0x60 => b'\'',
        0x93 | 0x94 | 0x84 => b'"',
        0x95 | 0xB7 => b'.',
        0x96 => b'-',
        0xA0 => b' ',
        0xAB | 0xBB | 0x8B | 0x9B => b'<',
        _ => b'0',
    }
}

fn char_width(family: FontFamily, bold: bool, code: u8) -> f32 {
    let table = match (family, bold) {
        (FontFamily::Courier, _) => return 600.0,
        (FontFamily::Helvetica, false) => &HELVETICA,
        (FontFamily::Helvetica, true) => &HELVETICA_BOLD,
        (FontFamily::Times, false) => &TIMES,
        (FontFamily::Times, true) => &TIMES_BOLD,
    };
    match code {
        0x97 => 1000.0,
        0x85 | 0x89 => 1000.0,
        32..=126 => f32::from(table[usize::from(code - 32)]),
        _ => f32::from(table[usize::from(ascii_stand_in(code) - 32)]),
    }
}

/// Width in points of already-sanitized text.
pub fn text_width(family: FontFamily, bold: bool, size: f32, text: &str) -> f32 {
    text.chars().filter_map(winansi).map(|code| char_width(family, bold, code)).sum::<f32>() * size / 1000.0
}

/// Breaks sanitized text into lines no wider than `width`; words longer than a line are split.
/// Explicit line breaks in `text` are kept.
pub fn wrap(family: FontFamily, bold: bool, size: f32, text: &str, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        for word in sanitize(paragraph).split(' ').filter(|w| !w.is_empty()) {
            let candidate = if line.is_empty() { word.to_string() } else { format!("{line} {word}") };
            if text_width(family, bold, size, &candidate) <= width {
                line = candidate;
                continue;
            }
            if !line.is_empty() {
                lines.push(std::mem::take(&mut line));
            }
            // A word too long for a line on its own is split at the last character that fits.
            let mut rest = word.to_string();
            while text_width(family, bold, size, &rest) > width {
                let mut cut = 0;
                for (i, c) in rest.char_indices() {
                    if text_width(family, bold, size, &rest[..i + c.len_utf8()]) > width {
                        break;
                    }
                    cut = i + c.len_utf8();
                }
                if cut == 0 {
                    cut = rest.chars().next().map_or(rest.len(), char::len_utf8);
                }
                lines.push(rest[..cut].to_string());
                rest = rest[cut..].to_string();
            }
            line = rest;
        }
        lines.push(line);
    }
    lines
}

// ------------------------------------------------------------------------------------------------
// Pages
// ------------------------------------------------------------------------------------------------

/// An sRGB color, written as "#rrggbb".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const WHITE: Rgb = Rgb(255, 255, 255);

    pub fn parse(hex: &str) -> Option<Rgb> {
        let h = hex.strip_prefix('#')?;
        if h.len() != 6 || !h.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
        Some(Rgb(byte(0)?, byte(2)?, byte(4)?))
    }

    fn pdf(self) -> String {
        let c = |v: u8| num(f32::from(v) / 255.0);
        format!("{} {} {}", c(self.0), c(self.1), c(self.2))
    }
}

impl Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Align {
    Left,
    Right,
    Center,
}

/// One drawing operation. Coordinates are points from the page's top-left corner; text `y` is the
/// baseline.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "op", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Op {
    Text { x: f32, y: f32, text: String, family: FontFamily, bold: bool, size: f32, color: Rgb, align: Align },
    Rect { x: f32, y: f32, w: f32, h: f32, fill: Option<Rgb>, stroke: Option<Rgb>, line_width: f32 },
    Line { x1: f32, y1: f32, x2: f32, y2: f32, color: Rgb, width: f32 },
    /// The document's image (the company logo), scaled into the box.
    Image { x: f32, y: f32, w: f32, h: f32 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub width: f32,
    pub height: f32,
    pub ops: Vec<Op>,
}

/// An image ready to embed: 8-bit RGB samples (or a JPEG stream) and optional 8-bit alpha.
#[derive(Debug, Clone)]
pub struct PdfImage {
    pub width: u32,
    pub height: u32,
    pub data: ImageData,
    pub alpha: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub enum ImageData {
    /// A baseline JPEG embedded as is; `gray` for one-channel files.
    Jpeg { bytes: Vec<u8>, gray: bool },
    Rgb(Vec<u8>),
}

/// Longest side, in pixels, a logo is embedded at (more than enough at print sizes).
const MAX_IMAGE_SIDE: u32 = 800;

impl PdfImage {
    /// Decodes a PNG, JPEG, GIF or WebP file.
    pub fn decode(bytes: &[u8]) -> Result<PdfImage, String> {
        use image::{GenericImageView, ImageDecoder};
        let format = image::guess_format(bytes).map_err(|e| format!("PDF_IMAGE: {e}"))?;
        // Small RGB or grayscale JPEGs go in unchanged (read from the header, without decoding);
        // everything else is re-encoded losslessly.
        if format == image::ImageFormat::Jpeg {
            if let Ok(jpeg) = image::codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(bytes)) {
                let ((w, h), color) = (jpeg.dimensions(), jpeg.color_type());
                if w > 0 && h > 0 && w.max(h) <= MAX_IMAGE_SIDE && matches!(color, image::ColorType::Rgb8 | image::ColorType::L8) {
                    return Ok(PdfImage { width: w, height: h, data: ImageData::Jpeg { bytes: bytes.to_vec(), gray: color == image::ColorType::L8 }, alpha: None });
                }
            }
        }
        let img = image::load_from_memory_with_format(bytes, format).map_err(|e| format!("PDF_IMAGE: the logo cannot be read: {e}"))?;
        let (w, h) = img.dimensions();
        if w == 0 || h == 0 {
            return Err("PDF_IMAGE: the logo is empty".into());
        }
        let img = if w.max(h) > MAX_IMAGE_SIDE { img.resize(MAX_IMAGE_SIDE, MAX_IMAGE_SIDE, image::imageops::FilterType::Lanczos3) } else { img };
        let rgba = img.to_rgba8();
        let (width, height) = rgba.dimensions();
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        let mut alpha = Vec::with_capacity((width * height) as usize);
        for px in rgba.pixels() {
            rgb.extend_from_slice(&px.0[..3]);
            alpha.push(px.0[3]);
        }
        let opaque = alpha.iter().all(|&a| a == 255);
        Ok(PdfImage { width, height, data: ImageData::Rgb(rgb), alpha: (!opaque).then_some(alpha) })
    }
}

/// An image file's pixel size, read from its header.
pub fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok().filter(|(w, h)| *w > 0 && *h > 0)
}

// ------------------------------------------------------------------------------------------------
// Serialization
// ------------------------------------------------------------------------------------------------

/// A number as PDF writes it: at most two decimals, no trailing zeros.
fn num(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.to_string()
    }
}

/// A PDF literal string of WinAnsi bytes.
fn literal(text: &str) -> String {
    let mut out = String::from("(");
    for code in text.chars().filter_map(winansi) {
        match code {
            b'(' | b')' | b'\\' => {
                out.push('\\');
                out.push(code as char);
            }
            32..=126 => out.push(code as char),
            _ => {
                let _ = write!(out, "\\{code:03o}");
            }
        }
    }
    out.push(')');
    out
}

/// A PDF text string for the document information dictionary (UTF-16BE with a byte-order mark).
fn info_string(text: &str) -> String {
    let mut out = String::from("<FEFF");
    for unit in text.encode_utf16() {
        let _ = write!(out, "{unit:04X}");
    }
    out.push('>');
    out
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).expect("writing to memory cannot fail");
    encoder.finish().expect("writing to memory cannot fail")
}

struct Writer {
    out: Vec<u8>,
    offsets: Vec<usize>,
}

impl Writer {
    /// Reserves an object number; objects may be written in any order.
    fn reserve(&mut self) -> usize {
        self.offsets.push(0);
        self.offsets.len()
    }

    fn object(&mut self, id: usize, body: &str) {
        self.offsets[id - 1] = self.out.len();
        self.out.extend_from_slice(format!("{id} 0 obj\n{body}\nendobj\n").as_bytes());
    }

    fn stream(&mut self, id: usize, dict: &str, data: &[u8]) {
        self.offsets[id - 1] = self.out.len();
        self.out.extend_from_slice(format!("{id} 0 obj\n<< {dict} /Length {} >>\nstream\n", data.len()).as_bytes());
        self.out.extend_from_slice(data);
        self.out.extend_from_slice(b"\nendstream\nendobj\n");
    }
}

/// Serializes laid-out pages to a PDF file. `image` is what `Op::Image` draws.
pub fn write_pdf(pages: &[Page], image: Option<&PdfImage>, title: &str) -> Vec<u8> {
    let mut w = Writer { out: b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec(), offsets: Vec::new() };
    let catalog = w.reserve();
    let pages_id = w.reserve();
    let info = w.reserve();

    // One font object per (family, weight) the pages use.
    let mut fonts: Vec<(FontFamily, bool)> = pages
        .iter()
        .flat_map(|p| p.ops.iter())
        .filter_map(|op| match op {
            Op::Text { family, bold, .. } => Some((*family, *bold)),
            _ => None,
        })
        .collect();
    fonts.sort();
    fonts.dedup();
    let font_ids: Vec<usize> = fonts.iter().map(|_| w.reserve()).collect();
    for (&(family, bold), &id) in fonts.iter().zip(&font_ids) {
        w.object(id, &format!("<< /Type /Font /Subtype /Type1 /BaseFont /{} /Encoding /WinAnsiEncoding >>", family.base_font(bold)));
    }
    let font_name = |family: FontFamily, bold: bool| fonts.iter().position(|f| *f == (family, bold)).map(|i| format!("F{}", i + 1)).unwrap_or_default();

    let uses_image = pages.iter().any(|p| p.ops.iter().any(|op| matches!(op, Op::Image { .. })));
    let image_id = match image.filter(|_| uses_image) {
        Some(img) => {
            let smask = img.alpha.as_ref().map(|alpha| {
                let id = w.reserve();
                let dict = format!("/Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode", img.width, img.height);
                w.stream(id, &dict, &deflate(alpha));
                id
            });
            let id = w.reserve();
            let mut dict = format!("/Type /XObject /Subtype /Image /Width {} /Height {} /BitsPerComponent 8", img.width, img.height);
            if let Some(s) = smask {
                let _ = write!(dict, " /SMask {s} 0 R");
            }
            match &img.data {
                ImageData::Jpeg { bytes, gray } => {
                    let _ = write!(dict, " /ColorSpace /{} /Filter /DCTDecode", if *gray { "DeviceGray" } else { "DeviceRGB" });
                    w.stream(id, &dict, bytes);
                }
                ImageData::Rgb(rgb) => {
                    dict.push_str(" /ColorSpace /DeviceRGB /Filter /FlateDecode");
                    w.stream(id, &dict, &deflate(rgb));
                }
            }
            Some(id)
        }
        None => None,
    };

    let mut resources = String::from("<< /Font <<");
    for (i, id) in font_ids.iter().enumerate() {
        let _ = write!(resources, " /F{} {id} 0 R", i + 1);
    }
    resources.push_str(" >>");
    if let Some(id) = image_id {
        let _ = write!(resources, " /XObject << /Im1 {id} 0 R >>");
    }
    resources.push_str(" >>");

    let mut kids = Vec::new();
    for page in pages {
        let h = page.height;
        let mut content = String::new();
        for op in &page.ops {
            match op {
                Op::Text { x, y, text, family, bold, size, color, align } => {
                    let width = text_width(*family, *bold, *size, text);
                    let left = match align {
                        Align::Left => *x,
                        Align::Right => x - width,
                        Align::Center => x - width / 2.0,
                    };
                    let _ = writeln!(content, "BT /{} {} Tf {} rg {} {} Td {} Tj ET", font_name(*family, *bold), num(*size), color.pdf(), num(left), num(h - y), literal(text));
                }
                Op::Rect { x, y, w: rw, h: rh, fill, stroke, line_width } => {
                    let rect = format!("{} {} {} {} re", num(*x), num(h - y - rh), num(*rw), num(*rh));
                    match (fill, stroke) {
                        (Some(f), Some(s)) => {
                            let _ = writeln!(content, "{} rg {} RG {} w {rect} B", f.pdf(), s.pdf(), num(*line_width));
                        }
                        (Some(f), None) => {
                            let _ = writeln!(content, "{} rg {rect} f", f.pdf());
                        }
                        (None, Some(s)) => {
                            let _ = writeln!(content, "{} RG {} w {rect} S", s.pdf(), num(*line_width));
                        }
                        (None, None) => {}
                    }
                }
                Op::Line { x1, y1, x2, y2, color, width } => {
                    let _ = writeln!(content, "{} RG {} w {} {} m {} {} l S", color.pdf(), num(*width), num(*x1), num(h - y1), num(*x2), num(h - y2));
                }
                Op::Image { x, y, w: iw, h: ih } => {
                    if image_id.is_some() {
                        let _ = writeln!(content, "q {} 0 0 {} {} {} cm /Im1 Do Q", num(*iw), num(*ih), num(*x), num(h - y - ih));
                    }
                }
            }
        }
        let content_id = w.reserve();
        w.stream(content_id, "/Filter /FlateDecode", &deflate(content.as_bytes()));
        let page_id = w.reserve();
        w.object(
            page_id,
            &format!(
                "<< /Type /Page /Parent {pages_id} 0 R /MediaBox [0 0 {} {}] /Resources {resources} /Contents {content_id} 0 R >>",
                num(page.width),
                num(page.height)
            ),
        );
        kids.push(format!("{page_id} 0 R"));
    }

    w.object(pages_id, &format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), kids.len()));
    w.object(catalog, &format!("<< /Type /Catalog /Pages {pages_id} 0 R >>"));
    w.object(info, &format!("<< /Title {} /Producer (LocalBooks) /Creator (LocalBooks) >>", info_string(title)));

    let xref = w.out.len();
    let mut tail = format!("xref\n0 {}\n0000000000 65535 f \n", w.offsets.len() + 1);
    for offset in &w.offsets {
        let _ = writeln!(tail, "{offset:010} 00000 n ");
    }
    let _ = write!(tail, "trailer\n<< /Size {} /Root {catalog} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n", w.offsets.len() + 1);
    w.out.extend_from_slice(tail.as_bytes());
    w.out
}

#[cfg(test)]
mod tests;
