//! Printed envelopes: the company's return address in the top left corner and a customer's or
//! vendor's name and address in the middle, on a standard business envelope.
//!
//! The envelope layout (size, font, whether to print the return address, and a nudge to line the
//! address up with a printer's feed) is one per company, edited in Custom Forms, and kept as a
//! company preference. An envelope is laid out into the same drawing operations as a printed form,
//! so the screen preview and the printed envelope match.

#![allow(clippy::float_arithmetic, reason = "envelope layout in points; no amounts are printed")]

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::forms::{self, FormKind, RenderedForm};
use crate::ledger::db_err;
use crate::pdf::{self, Align, FontFamily, Op, Page, Rgb};

/// Where the layout is kept (`system_preferences`).
pub const LAYOUT_KEY: &str = "envelope_layout";

/// Envelope sizes, each printed long edge across.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EnvelopeSize {
    /// The standard US business envelope, 9½ × 4⅛ inches.
    #[default]
    Number10,
    /// 8⅞ × 3⅞ inches, the size that fits inside a #10 as a return envelope.
    Number9,
    /// 7½ × 3⅞ inches.
    Monarch,
    /// 220 × 110 mm.
    Dl,
    /// 229 × 162 mm.
    C5,
}

impl EnvelopeSize {
    /// Width and height in points (1/72 inch).
    pub fn size(self) -> (f32, f32) {
        const MM: f32 = 72.0 / 25.4;
        match self {
            EnvelopeSize::Number10 => (684.0, 297.0),
            EnvelopeSize::Number9 => (639.0, 279.0),
            EnvelopeSize::Monarch => (540.0, 279.0),
            EnvelopeSize::Dl => (220.0 * MM, 110.0 * MM),
            EnvelopeSize::C5 => (229.0 * MM, 162.0 * MM),
        }
    }

    /// The CSS page size the screen prints with, such as "9.5in 4.125in".
    pub fn css(self) -> &'static str {
        match self {
            EnvelopeSize::Number10 => "9.5in 4.125in",
            EnvelopeSize::Number9 => "8.875in 3.875in",
            EnvelopeSize::Monarch => "7.5in 3.875in",
            EnvelopeSize::Dl => "220mm 110mm",
            EnvelopeSize::C5 => "229mm 162mm",
        }
    }
}

/// How envelopes print. Missing fields take their defaults.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EnvelopeLayout {
    pub size: EnvelopeSize,
    pub font: FontFamily,
    /// The company's name and address in the top left corner (off for pre-printed envelopes).
    pub return_address: bool,
    /// Moves the recipient's address right (or left, when negative), in points.
    pub nudge_right: i32,
    /// Moves the recipient's address down (or up, when negative), in points.
    pub nudge_down: i32,
}

impl Default for EnvelopeLayout {
    fn default() -> Self {
        EnvelopeLayout { size: EnvelopeSize::Number10, font: FontFamily::Helvetica, return_address: true, nudge_right: 0, nudge_down: 0 }
    }
}

/// How far the address can be nudged: an inch either way.
const MAX_NUDGE: i32 = 72;

impl EnvelopeLayout {
    pub fn validated(self) -> Result<Self, String> {
        if self.nudge_right.abs() > MAX_NUDGE || self.nudge_down.abs() > MAX_NUDGE {
            return Err("ENVELOPE_NUDGE: move the address by at most one inch (72 points) either way".into());
        }
        Ok(self)
    }
}

pub async fn get_layout(pool: &SqlitePool) -> Result<EnvelopeLayout, String> {
    let stored: Option<String> = sqlx::query_scalar("SELECT value FROM system_preferences WHERE key = ?1").bind(LAYOUT_KEY).fetch_optional(pool).await.map_err(db_err)?;
    // A layout that no longer reads falls back to the default rather than blocking printing.
    Ok(stored.and_then(|json| serde_json::from_str::<EnvelopeLayout>(&json).ok()).and_then(|l| l.validated().ok()).unwrap_or_default())
}

pub async fn save_layout(pool: &SqlitePool, layout: EnvelopeLayout) -> Result<EnvelopeLayout, String> {
    let layout = layout.validated()?;
    let json = serde_json::to_string(&layout).map_err(|e| format!("ENVELOPE_SAVE: {e}"))?;
    sqlx::query(
        "INSERT INTO system_preferences (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(LAYOUT_KEY)
    .bind(&json)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(layout)
}

/// Back to the standard envelope (preferences are never deleted, so the default is saved).
pub async fn reset_layout(pool: &SqlitePool) -> Result<EnvelopeLayout, String> {
    save_layout(pool, EnvelopeLayout::default()).await
}

/// Who an envelope is addressed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EnvelopeTarget {
    /// A customer's name, company and billing address (the Customer Center).
    Customer { id: i64 },
    /// A vendor's name, company and address (the Vendor Center, bills and checks).
    Vendor { id: i64 },
    /// The Bill To on a sales form, as the form prints it.
    Form { form: FormKind, id: i64 },
}

/// Lines of an address block: the name, then the rest, blank lines and repeats left out.
pub(crate) fn address_block(name: &str, company: Option<&str>, address: Option<&str>) -> Vec<String> {
    let mut lines = vec![name.trim().to_string()];
    if let Some(company) = company.map(str::trim).filter(|c| !c.is_empty() && !c.eq_ignore_ascii_case(name.trim())) {
        lines.push(company.to_string());
    }
    lines.extend(address.unwrap_or_default().lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from));
    lines.retain(|l| !l.is_empty());
    lines
}

/// The recipient's address lines: the name first.
async fn recipient(pool: &SqlitePool, target: EnvelopeTarget) -> Result<Vec<String>, String> {
    match target {
        EnvelopeTarget::Customer { id } => {
            let row: Option<(String, Option<String>, Option<String>)> =
                sqlx::query_as("SELECT name, company_name, billing_address FROM customers WHERE id = ?1").bind(id).fetch_optional(pool).await.map_err(db_err)?;
            let (name, company, address) = row.ok_or_else(|| "ENVELOPE_NOT_FOUND: that customer no longer exists".to_string())?;
            Ok(address_block(&name, company.as_deref(), address.as_deref()))
        }
        EnvelopeTarget::Vendor { id } => {
            let row: Option<(String, Option<String>, Option<String>)> =
                sqlx::query_as("SELECT name, company_name, address FROM vendors WHERE id = ?1").bind(id).fetch_optional(pool).await.map_err(db_err)?;
            let (name, company, address) = row.ok_or_else(|| "ENVELOPE_NOT_FOUND: that vendor no longer exists".to_string())?;
            Ok(address_block(&name, company.as_deref(), address.as_deref()))
        }
        EnvelopeTarget::Form { form, id } => {
            let symbol = crate::preferences::currency_symbol(pool).await?;
            let doc = forms::document(pool, form, Some(id), None, &symbol).await?;
            let mut lines = vec![doc.customer_name.trim().to_string()];
            lines.extend(doc.customer_details.iter().map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && *l != doc.customer_name.trim()));
            Ok(lines)
        }
    }
}

/// The sample the Custom Forms preview shows.
fn sample_recipient() -> Vec<String> {
    vec!["Jordan Rivera".into(), "Rivera Landscaping".into(), "412 Orchard Lane, Suite 3".into(), "Springfield, OR 97477".into()]
}

const INK: Rgb = Rgb(0x1f, 0x29, 0x37);
const MUTED: Rgb = Rgb(0x4b, 0x55, 0x63);

/// Lays out one envelope. Positions follow the usual guidance for machine-read mail: the return
/// address a quarter inch in from the top left corner, the delivery address a little left of
/// center and just below the middle.
pub fn layout(layout: &EnvelopeLayout, company: &forms::Company, to: &[String]) -> Page {
    let (width, height) = layout.size.size();
    let mut ops = Vec::new();
    let text = |x: f32, y: f32, s: &str, bold: bool, size: f32, color: Rgb| Op::Text { x, y, text: s.to_string(), family: layout.font, bold, size, color, align: Align::Left };
    if layout.return_address {
        let name = if company.name.trim().is_empty() { company.legal_name.trim() } else { company.name.trim() };
        let mut y = 18.0 + 10.0;
        if !name.is_empty() {
            ops.push(text(18.0, y, name, true, 10.0, INK));
            y += 12.0;
        }
        for line in company.address.lines().map(str::trim).filter(|l| !l.is_empty()) {
            ops.push(text(18.0, y, line, false, 9.0, MUTED));
            y += 11.0;
        }
    }
    // The address block: 12-point type, 15 points apart, its widest line kept inside the envelope.
    let size = 12.0;
    let leading = 15.0;
    let widest = to.iter().map(|l| pdf::text_width(layout.font, false, size, l)).fold(0.0_f32, f32::max);
    // The nudge moves the block, but never off the envelope: its widest line and its last line
    // stay on the paper.
    let x = ((width * 0.42).min(width - 18.0 - widest).max(18.0) + layout.nudge_right as f32).clamp(0.0, (width - widest).max(0.0));
    let block = leading * to.len().saturating_sub(1) as f32;
    let mut y = (height * 0.5 + 12.0 + layout.nudge_down as f32).clamp(size, (height - 6.0 - block).max(size));
    for line in to {
        ops.push(text(x, y, line, false, size, INK));
        y += leading;
    }
    Page { width, height, ops }
}

/// "Envelope · Acme": the print job's title.
fn title(to: &[String]) -> String {
    match to.first().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        Some(name) => format!("Envelope · {name}"),
        None => "Envelope".into(),
    }
}

/// An envelope, ready to show and print: `target` None is the sample; `with` an unsaved layout
/// (the Custom Forms preview), else the saved one.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedEnvelope {
    #[serde(flatten)]
    pub form: RenderedForm,
    /// The CSS page size to print on.
    pub page_size: &'static str,
}

pub async fn render(pool: &SqlitePool, target: Option<EnvelopeTarget>, with: Option<EnvelopeLayout>) -> Result<RenderedEnvelope, String> {
    let layout_used = match with {
        Some(l) => l.validated()?,
        None => get_layout(pool).await?,
    };
    let to = match target {
        Some(t) => recipient(pool, t).await?,
        None => sample_recipient(),
    };
    let company = forms::load_company(pool).await?;
    let page = layout(&layout_used, &company, &to);
    Ok(RenderedEnvelope {
        form: RenderedForm { title: title(&to), pages: vec![page], logo: None, customer_email: None },
        page_size: layout_used.size.css(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn company() -> forms::Company {
        forms::Company { name: "Maple Street Bakery".into(), address: "10 Maple Street\nPortland, OR 97201".into(), ..Default::default() }
    }

    fn texts(page: &Page) -> Vec<(f32, f32, String)> {
        page.ops
            .iter()
            .filter_map(|op| match op {
                Op::Text { x, y, text, .. } => Some((*x, *y, text.clone())),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_number_10_envelope_has_the_return_address_top_left_and_the_address_in_the_middle() {
        let to = vec!["Jordan Rivera".to_string(), "412 Orchard Lane".to_string(), "Springfield, OR 97477".to_string()];
        let page = layout(&EnvelopeLayout::default(), &company(), &to);
        assert_eq!((page.width, page.height), (684.0, 297.0));
        let t = texts(&page);
        assert_eq!(t[0].2, "Maple Street Bakery");
        assert!(t[0].0 < 36.0 && t[0].1 < 36.0, "return address in the top left corner");
        assert_eq!(t[2].2, "Portland, OR 97201");
        let first = t.iter().find(|(_, _, s)| s == "Jordan Rivera").unwrap();
        assert!(first.0 > page.width * 0.3 && first.0 < page.width * 0.6, "address a little left of center");
        assert!(first.1 > page.height * 0.45 && first.1 < page.height * 0.75, "address just below the middle");
        assert_eq!(t.last().unwrap().2, "Springfield, OR 97477");
    }

    #[test]
    fn the_return_address_can_be_left_off_and_the_address_nudged() {
        let to = vec!["Jordan Rivera".to_string()];
        let plain = layout(&EnvelopeLayout::default(), &company(), &to);
        let nudged = layout(&EnvelopeLayout { return_address: false, nudge_right: 18, nudge_down: -9, ..Default::default() }, &company(), &to);
        let t = texts(&nudged);
        assert_eq!(t.len(), 1, "only the recipient");
        let before = texts(&plain).into_iter().find(|(_, _, s)| s == "Jordan Rivera").unwrap();
        assert_eq!(t[0].0, before.0 + 18.0);
        assert_eq!(t[0].1, before.1 - 9.0);
    }

    #[test]
    fn a_nudge_never_pushes_the_address_off_the_envelope() {
        let to: Vec<String> = ["Jordan Rivera", "Rivera Landscaping and Garden Design LLC", "412 Orchard Lane", "Springfield, OR 97477"].map(String::from).to_vec();
        for (right, down) in [(72, 72), (-72, -72), (72, -72), (-72, 72)] {
            let page = layout(&EnvelopeLayout { size: EnvelopeSize::Monarch, nudge_right: right, nudge_down: down, ..Default::default() }, &company(), &to);
            for (x, y, text) in texts(&page).into_iter().filter(|(_, _, t)| to.contains(t)) {
                let w = pdf::text_width(FontFamily::Helvetica, false, 12.0, &text);
                assert!(x >= 0.0 && x + w <= page.width + 0.01, "{text} at {x} runs off a {} wide envelope", page.width);
                assert!(y > 0.0 && y <= page.height, "{text} at {y} is off the envelope");
            }
        }
    }

    #[test]
    fn sizes_and_nudges_are_checked() {
        assert_eq!(EnvelopeSize::Dl.size().0.round(), 624.0);
        assert_eq!(EnvelopeSize::Number10.css(), "9.5in 4.125in");
        assert!(EnvelopeLayout { nudge_right: 73, ..Default::default() }.validated().is_err());
        assert!(EnvelopeLayout { nudge_down: -72, ..Default::default() }.validated().is_ok());
        let json = r#"{"size":"C5","returnAddress":false}"#;
        let l: EnvelopeLayout = serde_json::from_str(json).unwrap();
        assert_eq!((l.size, l.return_address, l.font), (EnvelopeSize::C5, false, FontFamily::Helvetica));
    }

    #[test]
    fn address_blocks_skip_blanks_and_a_company_repeating_the_name() {
        assert_eq!(address_block("Acme", Some("ACME"), Some("1 Main St\n\n  Town, ST 00000 ")), vec!["Acme", "1 Main St", "Town, ST 00000"]);
        assert_eq!(address_block("Jo", Some("Jo's Plumbing"), None), vec!["Jo", "Jo's Plumbing"]);
        assert_eq!(title(&["Acme".into()]), "Envelope · Acme");
    }

    #[tokio::test]
    async fn envelopes_address_customers_vendors_and_forms_and_keep_their_layout() {
        let pool = crate::db::test_pool().await;
        sqlx::query("INSERT INTO customers (name, company_name, billing_address) VALUES ('Jordan Rivera', 'Rivera Landscaping', '412 Orchard Lane\nSpringfield, OR 97477')").execute(&pool).await.unwrap();
        let customer: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Jordan Rivera'").fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO vendors (name, address) VALUES ('Paper Supply Co', '9 Mill Road\nSalem, OR 97301')").execute(&pool).await.unwrap();
        let vendor: i64 = sqlx::query_scalar("SELECT id FROM vendors WHERE name = 'Paper Supply Co'").fetch_one(&pool).await.unwrap();

        let lines = |r: &RenderedEnvelope| texts(&r.form.pages[0]).into_iter().map(|t| t.2).collect::<Vec<_>>();
        let c = render(&pool, Some(EnvelopeTarget::Customer { id: customer }), None).await.unwrap();
        assert_eq!(c.form.title, "Envelope · Jordan Rivera");
        assert!(lines(&c).ends_with(&["Jordan Rivera".into(), "Rivera Landscaping".into(), "412 Orchard Lane".into(), "Springfield, OR 97477".into()]));
        let v = render(&pool, Some(EnvelopeTarget::Vendor { id: vendor }), None).await.unwrap();
        assert!(lines(&v).ends_with(&["Paper Supply Co".into(), "9 Mill Road".into(), "Salem, OR 97301".into()]));
        assert!(render(&pool, Some(EnvelopeTarget::Vendor { id: 9999 }), None).await.unwrap_err().starts_with("ENVELOPE_NOT_FOUND"));

        // The layout is kept, read back, and reset.
        assert_eq!(get_layout(&pool).await.unwrap(), EnvelopeLayout::default());
        let saved = save_layout(&pool, EnvelopeLayout { size: EnvelopeSize::Dl, return_address: false, nudge_right: 6, ..Default::default() }).await.unwrap();
        assert_eq!(get_layout(&pool).await.unwrap(), saved);
        let sample = render(&pool, None, None).await.unwrap();
        assert_eq!(sample.page_size, "220mm 110mm");
        assert_eq!(lines(&sample)[0], "Jordan Rivera", "no return address");
        assert!(save_layout(&pool, EnvelopeLayout { nudge_down: 500, ..Default::default() }).await.is_err());
        assert_eq!(reset_layout(&pool).await.unwrap(), EnvelopeLayout::default());
        assert_eq!(get_layout(&pool).await.unwrap(), EnvelopeLayout::default());
    }
}
