//! Printed forms: invoices and estimates as the form designer lays them out.
//!
//! Each form type has one template (title, paper, font, accent color, logo placement, which
//! company details and columns show, and the customer message). A document is gathered into a
//! `FormDoc`, laid out once into pages of drawing operations, and those pages are what the
//! designer previews, what prints, and what is written to a PDF file, so all three match.
//! Templates are stored in the company file; without one the defaults apply.

use base64::Engine;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::db_err;
use crate::pdf::{self, Align, FontFamily, Op, Page, PdfImage, Rgb};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FormKind {
    Invoice,
    Estimate,
}

impl FormKind {
    fn key(self) -> &'static str {
        match self {
            FormKind::Invoice => "INVOICE",
            FormKind::Estimate => "ESTIMATE",
        }
    }

    fn default_title(self) -> &'static str {
        match self {
            FormKind::Invoice => "Invoice",
            FormKind::Estimate => "Estimate",
        }
    }
}

// ------------------------------------------------------------------------------------------------
// Templates
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Paper {
    #[default]
    Letter,
    A4,
}

impl Paper {
    fn size(self) -> (f32, f32) {
        match self {
            Paper::Letter => (612.0, 792.0),
            Paper::A4 => (595.28, 841.89),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LogoPlacement {
    Left,
    #[default]
    Right,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LogoSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl LogoSize {
    fn max_height(self) -> f32 {
        match self {
            LogoSize::Small => 36.0,
            LogoSize::Medium => 56.0,
            LogoSize::Large => 80.0,
        }
    }
}

/// How one form type looks. Missing fields take their defaults, so templates saved by older
/// versions keep working.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FormTemplate {
    /// Printed at the top right, e.g. "Invoice" or "Tax Invoice".
    pub title: String,
    pub paper: Paper,
    pub font: FontFamily,
    /// "#rrggbb": the title, rules and table header.
    pub accent_color: String,
    pub logo: LogoPlacement,
    pub logo_size: LogoSize,
    pub show_company_address: bool,
    pub show_company_phone: bool,
    pub show_company_email: bool,
    pub show_ein: bool,
    pub show_customer_email: bool,
    /// Fill the table header with the accent color (otherwise accent text over a rule).
    pub shade_table_header: bool,
    pub show_line_numbers: bool,
    pub show_quantity: bool,
    pub show_rate: bool,
    pub description_label: String,
    pub quantity_label: String,
    pub rate_label: String,
    pub amount_label: String,
    /// Invoices: list payments received above the balance due.
    pub show_payments: bool,
    /// Below the totals. Placeholders: {number}, {customer}, {date}, {due_date}, {total}, {balance}, {company}.
    pub message: String,
    /// Payment instructions or terms and conditions, below the message.
    pub terms: String,
    /// At the bottom of every page.
    pub footer: String,
    /// The email a form is sent with; the same placeholders as the message.
    pub email_subject: String,
    pub email_body: String,
}

impl Default for FormTemplate {
    fn default() -> Self {
        FormTemplate {
            title: String::new(),
            paper: Paper::Letter,
            font: FontFamily::Helvetica,
            accent_color: "#2ca01c".into(),
            logo: LogoPlacement::Right,
            logo_size: LogoSize::Medium,
            show_company_address: true,
            show_company_phone: true,
            show_company_email: true,
            show_ein: true,
            show_customer_email: false,
            shade_table_header: true,
            show_line_numbers: true,
            show_quantity: true,
            show_rate: true,
            description_label: "Description".into(),
            quantity_label: "Qty".into(),
            rate_label: "Rate".into(),
            amount_label: "Amount".into(),
            show_payments: true,
            message: String::new(),
            terms: String::new(),
            footer: String::new(),
            email_subject: String::new(),
            email_body: String::new(),
        }
    }
}

impl FormTemplate {
    pub fn default_for(kind: FormKind) -> Self {
        let message = match kind {
            FormKind::Invoice => "Please reference invoice {number} with your payment. Thank you for your business.",
            FormKind::Estimate => "Thank you for the opportunity. This estimate is valid until the expiration date shown.",
        };
        let (subject, body) = match kind {
            FormKind::Invoice => (
                "Invoice {number} from {company}",
                "Dear {customer},\n\nPlease find attached invoice {number} for {total}, due {due_date}.\n\nThank you for your business.\n{company}",
            ),
            FormKind::Estimate => ("Estimate {number} from {company}", "Dear {customer},\n\nPlease find attached estimate {number} for {total}.\n\nWe look forward to working with you.\n{company}"),
        };
        FormTemplate {
            title: kind.default_title().into(),
            message: message.into(),
            email_subject: subject.into(),
            email_body: body.into(),
            ..FormTemplate::default()
        }
    }

    /// Trims and checks every field; blank labels fall back to their defaults.
    pub fn validated(mut self, kind: FormKind) -> Result<Self, String> {
        let defaults = FormTemplate::default_for(kind);
        let field = |value: &mut String, fallback: &str, max: usize, label: &str| -> Result<(), String> {
            let trimmed = value.trim();
            if trimmed.chars().count() > max {
                return Err(format!("FORM_TOO_LONG: {label} must be at most {max} characters"));
            }
            *value = if trimmed.is_empty() { fallback.to_string() } else { trimmed.to_string() };
            Ok(())
        };
        field(&mut self.title, &defaults.title, 40, "the title")?;
        field(&mut self.description_label, &defaults.description_label, 24, "a column label")?;
        field(&mut self.quantity_label, &defaults.quantity_label, 24, "a column label")?;
        field(&mut self.rate_label, &defaults.rate_label, 24, "a column label")?;
        field(&mut self.amount_label, &defaults.amount_label, 24, "a column label")?;
        field(&mut self.message, "", 600, "the message")?;
        field(&mut self.terms, "", 1000, "the terms")?;
        field(&mut self.footer, "", 150, "the footer")?;
        field(&mut self.email_subject, &defaults.email_subject, 200, "the email subject")?;
        field(&mut self.email_body, &defaults.email_body, 4000, "the email message")?;
        let color = self.accent_color.trim().to_ascii_lowercase();
        if Rgb::parse(&color).is_none() {
            return Err("FORM_BAD_COLOR: the accent color must be a hex color such as #2ca01c".into());
        }
        self.accent_color = color;
        Ok(self)
    }
}

pub async fn get_template(pool: &SqlitePool, kind: FormKind) -> Result<FormTemplate, String> {
    let stored: Option<String> = sqlx::query_scalar("SELECT template_json FROM form_templates WHERE form_type = ?1")
        .bind(kind.key())
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    let Some(json) = stored else { return Ok(FormTemplate::default_for(kind)) };
    // A template that no longer reads (hand-edited, or from a newer version) falls back to defaults
    // rather than blocking every print.
    Ok(serde_json::from_str::<FormTemplate>(&json).ok().and_then(|t| t.validated(kind).ok()).unwrap_or_else(|| FormTemplate::default_for(kind)))
}

pub async fn save_template(pool: &SqlitePool, kind: FormKind, template: FormTemplate) -> Result<FormTemplate, String> {
    let template = template.validated(kind)?;
    let json = serde_json::to_string(&template).map_err(|e| format!("FORM_SAVE: {e}"))?;
    sqlx::query(
        "INSERT INTO form_templates (form_type, template_json) VALUES (?1, ?2)
         ON CONFLICT (form_type) DO UPDATE SET template_json = excluded.template_json, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(kind.key())
    .bind(&json)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(template)
}

pub async fn reset_template(pool: &SqlitePool, kind: FormKind) -> Result<FormTemplate, String> {
    sqlx::query("DELETE FROM form_templates WHERE form_type = ?1").bind(kind.key()).execute(pool).await.map_err(db_err)?;
    Ok(FormTemplate::default_for(kind))
}

// ------------------------------------------------------------------------------------------------
// Document data
// ------------------------------------------------------------------------------------------------

/// The company details printed in the header.
#[derive(Debug, Clone, Default)]
pub struct Company {
    pub name: String,
    pub legal_name: String,
    pub address: String,
    pub phone: String,
    pub email: String,
    pub ein: String,
    pub currency_symbol: String,
    /// The stored logo as a data URL.
    pub logo: Option<String>,
}

pub async fn load_company(pool: &SqlitePool) -> Result<Company, String> {
    let prefs = crate::preferences::get_all(pool).await?;
    let pref = |key: &str| prefs.get(key).cloned().unwrap_or_default();
    Ok(Company {
        name: pref("company_name"),
        legal_name: pref("company_legal_name"),
        address: pref("company_address"),
        phone: pref("company_phone"),
        email: pref("company_email"),
        ein: pref("company_ein"),
        currency_symbol: Some(pref("currency_symbol")).filter(|s| !s.is_empty()).unwrap_or_else(|| "$".into()),
        logo: crate::preferences::get_logo(pool).await?,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct FormLine {
    pub description: String,
    pub quantity: String,
    pub rate: String,
    pub amount: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TotalRow {
    pub label: String,
    pub value: String,
    /// The last, emphasized row (Balance due / Total).
    pub grand: bool,
    /// A payment received, hidden when the template hides payments.
    pub payment: bool,
}

/// Everything a form prints about one document, already formatted.
#[derive(Debug, Clone)]
pub struct FormDoc {
    pub kind: FormKind,
    pub number: String,
    pub voided: bool,
    pub customer_name: String,
    pub customer_email: Option<String>,
    /// Company name and billing address lines (after the customer's name).
    pub customer_details: Vec<String>,
    /// Label/value pairs beside the customer block; `true` emphasizes the value.
    pub meta: Vec<(String, String, bool)>,
    pub lines: Vec<FormLine>,
    pub totals: Vec<TotalRow>,
    pub memo: Option<String>,
    pub date: String,
    pub due_date: String,
    pub total: String,
    pub balance: String,
}

/// 123456 → "$1,234.56" (or "EUR 1,234.56" for a foreign-currency document), as the app shows it.
pub fn format_money(cents: i64, currency_code: Option<&str>, symbol: &str) -> String {
    match currency_code {
        Some(code) => crate::ledger::format_money(cents, &format!("{code} ")),
        None => crate::ledger::format_money(cents, symbol),
    }
}

/// 15_000 → "1.5"; 1_234_0000 → "1,234".
pub fn format_quantity(e4: i64) -> String {
    let abs = e4.unsigned_abs();
    let digits = (abs / 10_000).to_string();
    let mut whole = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            whole.push(',');
        }
        whole.push(c);
    }
    let frac = format!("{:04}", abs % 10_000);
    let frac = frac.trim_end_matches('0');
    format!("{}{whole}{}{frac}", if e4 < 0 { "-" } else { "" }, if frac.is_empty() { "" } else { "." })
}

/// "2026-10-23" → "10/23/2026".
pub fn format_date(iso: &str) -> String {
    match iso.split('-').collect::<Vec<_>>()[..] {
        [y, m, d] => format!("{m}/{d}/{y}"),
        _ => iso.to_string(),
    }
}

struct Customer {
    company_name: Option<String>,
    billing_address: Option<String>,
    email: Option<String>,
}

async fn customer(pool: &SqlitePool, id: Option<i64>, name: &str) -> Result<Customer, String> {
    let row: Option<(Option<String>, Option<String>, Option<String>)> = match id {
        Some(id) => sqlx::query_as("SELECT company_name, billing_address, email FROM customers WHERE id = ?1").bind(id).fetch_optional(pool).await,
        None => sqlx::query_as("SELECT company_name, billing_address, email FROM customers WHERE name = ?1 COLLATE NOCASE").bind(name).fetch_optional(pool).await,
    }
    .map_err(db_err)?;
    let (company_name, billing_address, email) = row.unwrap_or_default();
    Ok(Customer { company_name, billing_address, email })
}

fn customer_details(name: &str, c: &Customer) -> Vec<String> {
    let mut details = Vec::new();
    if let Some(company) = c.company_name.as_deref().map(str::trim).filter(|s| !s.is_empty() && *s != name) {
        details.push(company.to_string());
    }
    if let Some(address) = &c.billing_address {
        details.extend(address.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from));
    }
    details
}

/// Subtotal, discount and tax rows, shown only when the document has a discount or tax.
fn breakdown(rows: &mut Vec<TotalRow>, total: i64, tax: i64, discount: i64, tax_name: Option<&str>, money: &dyn Fn(i64) -> String) {
    if tax == 0 && discount == 0 {
        return;
    }
    let row = |label: String, value: String| TotalRow { label, value, grand: false, payment: false };
    rows.push(row("Subtotal".into(), money(total - tax + discount)));
    if discount > 0 {
        rows.push(row("Discount".into(), money(-discount)));
    }
    if tax > 0 {
        rows.push(row(tax_name.map_or_else(|| "Sales tax".to_string(), |n| format!("Sales tax ({n})")), money(tax)));
    }
}

pub async fn invoice_doc(pool: &SqlitePool, invoice_id: i64, symbol: &str) -> Result<FormDoc, String> {
    let detail = crate::invoices::invoice_detail(pool, invoice_id).await?;
    let inv = &detail.invoice;
    let code = inv.currency_code.as_deref();
    let money = |cents: i64| format_money(cents, code, symbol);
    let cust = customer(pool, inv.customer_id, &inv.customer_name).await?;
    let terms_days = match (chrono::NaiveDate::parse_from_str(&inv.issue_date, "%Y-%m-%d"), chrono::NaiveDate::parse_from_str(&inv.due_date, "%Y-%m-%d")) {
        (Ok(a), Ok(b)) => (b - a).num_days(),
        _ => 0,
    };
    let balance = if inv.is_voided { 0 } else { inv.balance_cents };
    let mut meta = vec![
        ("Invoice date".to_string(), format_date(&inv.issue_date), false),
        ("Terms".to_string(), if terms_days == 0 { "Due on receipt".into() } else { format!("Net {terms_days}") }, false),
        ("Due date".to_string(), format_date(&inv.due_date), false),
        ("Balance due".to_string(), money(balance), true),
    ];
    if let Some(code) = code {
        meta.push(("Currency".into(), code.into(), false));
    }
    let lines = detail
        .items
        .iter()
        .map(|i| FormLine { description: i.description.clone(), quantity: format_quantity(i.quantity_e4), rate: money(i.rate_cents), amount: money(i.amount_cents) })
        .collect();
    let mut totals = Vec::new();
    breakdown(&mut totals, inv.total_cents, inv.tax_cents, inv.discount_cents, inv.tax_rate_name.as_deref(), &money);
    totals.push(TotalRow { label: "Total".into(), value: money(inv.total_cents), grand: false, payment: false });
    for p in &detail.payments {
        totals.push(TotalRow { label: format!("Payment received {}", format_date(&p.payment_date.to_string())), value: money(-p.amount_cents), grand: false, payment: true });
    }
    if inv.credits_applied_cents > 0 {
        totals.push(TotalRow { label: "Credits applied".into(), value: money(-inv.credits_applied_cents), grand: false, payment: true });
    }
    totals.push(TotalRow { label: "Balance due".into(), value: money(balance), grand: true, payment: false });
    Ok(FormDoc {
        kind: FormKind::Invoice,
        number: inv.invoice_number.to_string(),
        voided: inv.is_voided,
        customer_name: inv.customer_name.clone(),
        customer_details: customer_details(&inv.customer_name, &cust),
        customer_email: cust.email,
        meta,
        lines,
        totals,
        memo: inv.is_voided.then(|| "This invoice has been voided and no payment is due.".to_string()),
        date: format_date(&inv.issue_date),
        due_date: format_date(&inv.due_date),
        total: money(inv.total_cents),
        balance: money(balance),
    })
}

pub async fn estimate_doc(pool: &SqlitePool, estimate_id: i64, symbol: &str) -> Result<FormDoc, String> {
    type Header = (i64, i64, String, String, Option<String>, Option<String>, i64, i64, i64, Option<String>);
    let header: Option<Header> = sqlx::query_as(
        "SELECT e.estimate_number, e.customer_id, c.name, e.estimate_date, e.expiration_date, e.memo, e.total_cents, e.tax_cents, e.discount_cents, t.name
         FROM estimates e JOIN customers c ON c.id = e.customer_id LEFT JOIN tax_rates t ON t.id = e.tax_rate_id
         WHERE e.id = ?1",
    )
    .bind(estimate_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let (number, customer_id, name, date, expires, memo, total, tax, discount, tax_name) = header.ok_or("FORM_NOT_FOUND: that estimate no longer exists")?;
    let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as("SELECT description, quantity_e4, rate_cents, amount_cents FROM estimate_lines WHERE estimate_id = ?1 ORDER BY line_number")
        .bind(estimate_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    let money = |cents: i64| format_money(cents, None, symbol);
    let cust = customer(pool, Some(customer_id), &name).await?;
    let mut meta = vec![("Estimate date".to_string(), format_date(&date), false)];
    if let Some(expires) = &expires {
        meta.push(("Expires".into(), format_date(expires), false));
    }
    meta.push(("Total".into(), money(total), true));
    let mut totals = Vec::new();
    breakdown(&mut totals, total, tax, discount, tax_name.as_deref(), &money);
    totals.push(TotalRow { label: "Total".into(), value: money(total), grand: true, payment: false });
    Ok(FormDoc {
        kind: FormKind::Estimate,
        number: number.to_string(),
        voided: false,
        customer_name: name.clone(),
        customer_details: customer_details(&name, &cust),
        customer_email: cust.email,
        meta,
        lines: rows.into_iter().map(|(description, qty, rate, amount)| FormLine { description, quantity: format_quantity(qty), rate: money(rate), amount: money(amount) }).collect(),
        totals,
        memo: memo.filter(|m| !m.trim().is_empty()),
        date: format_date(&date),
        due_date: expires.as_deref().map(format_date).unwrap_or_default(),
        total: money(total),
        balance: money(total),
    })
}

/// A made-up document for the designer's preview when no real one is chosen.
pub fn sample_doc(kind: FormKind, symbol: &str) -> FormDoc {
    let money = |cents: i64| format_money(cents, None, symbol);
    let line = |d: &str, q: i64, r: i64| FormLine { description: d.into(), quantity: format_quantity(q), rate: money(r), amount: money(q * r / 10_000) };
    let lines = vec![
        line("Website redesign: discovery, wireframes and two rounds of revisions", 1_0000, 240_000),
        line("Consulting (hours)", 12_5000, 12_500),
        line("Hosting, 12 months", 1_0000, 18_000),
    ];
    let total = 240_000 + 156_250 + 18_000;
    let tax = 0;
    let customer_details = vec!["Sample Company LLC".into(), "100 Main Street".into(), "Springfield, IL 62701".into()];
    let (meta, totals, balance) = match kind {
        FormKind::Invoice => (
            vec![
                ("Invoice date".to_string(), "10/01/2026".to_string(), false),
                ("Terms".to_string(), "Net 30".to_string(), false),
                ("Due date".to_string(), "10/31/2026".to_string(), false),
                ("Balance due".to_string(), money(total - 100_000), true),
            ],
            vec![
                TotalRow { label: "Total".into(), value: money(total + tax), grand: false, payment: false },
                TotalRow { label: "Payment received 10/15/2026".into(), value: money(-100_000), grand: false, payment: true },
                TotalRow { label: "Balance due".into(), value: money(total - 100_000), grand: true, payment: false },
            ],
            money(total - 100_000),
        ),
        FormKind::Estimate => (
            vec![
                ("Estimate date".to_string(), "10/01/2026".to_string(), false),
                ("Expires".to_string(), "10/31/2026".to_string(), false),
                ("Total".to_string(), money(total), true),
            ],
            vec![TotalRow { label: "Total".into(), value: money(total), grand: true, payment: false }],
            money(total),
        ),
    };
    FormDoc {
        kind,
        number: "1001".into(),
        voided: false,
        customer_name: "Sample Customer".into(),
        customer_email: Some("billing@example.com".into()),
        customer_details,
        meta,
        lines,
        totals,
        memo: None,
        date: "10/01/2026".into(),
        due_date: "10/31/2026".into(),
        total: money(total),
        balance,
    }
}

fn fill_placeholders(text: &str, doc: &FormDoc, company: &Company) -> String {
    [
        ("{number}", doc.number.as_str()),
        ("{customer}", doc.customer_name.as_str()),
        ("{date}", doc.date.as_str()),
        ("{due_date}", doc.due_date.as_str()),
        ("{total}", doc.total.as_str()),
        ("{balance}", doc.balance.as_str()),
        ("{company}", company.name.as_str()),
    ]
    .iter()
    .fold(text.to_string(), |acc, (key, value)| acc.replace(key, value))
}

// ------------------------------------------------------------------------------------------------
// Layout
// ------------------------------------------------------------------------------------------------

const MARGIN: f32 = 48.0;
/// Space kept at the bottom of every page for the footer.
const FOOTER_SPACE: f32 = 30.0;
const INK: Rgb = Rgb(0x22, 0x22, 0x22);
const MUTED: Rgb = Rgb(0x6b, 0x6b, 0x6b);
const RULE: Rgb = Rgb(0xd9, 0xd9, 0xd9);
const VOID_MARK: Rgb = Rgb(0xf3, 0xc4, 0xc4);

struct Layout<'a> {
    tpl: &'a FormTemplate,
    font: FontFamily,
    accent: Rgb,
    width: f32,
    height: f32,
    pages: Vec<Page>,
    y: f32,
}

impl Layout<'_> {
    fn right(&self) -> f32 {
        self.width - MARGIN
    }

    fn bottom(&self) -> f32 {
        self.height - MARGIN - FOOTER_SPACE
    }

    fn push(&mut self, op: Op) {
        self.pages.last_mut().expect("layout always has a page").ops.push(op);
    }

    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, x: f32, y: f32, text: &str, bold: bool, size: f32, color: Rgb, align: Align) {
        let text = pdf::sanitize(text);
        if !text.trim().is_empty() {
            self.push(Op::Text { x, y, text, family: self.font, bold, size, color, align });
        }
    }

    fn line(&mut self, x1: f32, x2: f32, y: f32, color: Rgb, width: f32) {
        self.push(Op::Line { x1, y1: y, x2, y2: y, color, width });
    }

    fn wrap(&self, text: &str, bold: bool, size: f32, width: f32) -> Vec<String> {
        pdf::wrap(self.font, bold, size, text, width)
    }

    /// `label`, shortened with "…" so it and `value` fit side by side in `width`.
    fn fit(&self, label: &str, bold: bool, size: f32, value: &str, width: f32) -> String {
        let room = width - 3.0 * CELL_PAD - pdf::text_width(self.font, bold, size, &pdf::sanitize(value));
        let mut text = pdf::sanitize(label);
        if pdf::text_width(self.font, bold, size, &text) <= room {
            return text;
        }
        while !text.is_empty() && pdf::text_width(self.font, bold, size, &format!("{text}…")) > room {
            text.pop();
        }
        format!("{}…", text.trim_end())
    }

    fn new_page(&mut self) {
        self.pages.push(Page { width: self.width, height: self.height, ops: Vec::new() });
        self.y = MARGIN;
    }

    /// Starts a new page unless `needed` points still fit on this one.
    fn ensure(&mut self, needed: f32, doc: &FormDoc) -> bool {
        if self.y + needed <= self.bottom() {
            return false;
        }
        self.new_page();
        let heading = format!("{} {} (continued)", self.tpl.title, doc.number);
        self.text(MARGIN, self.y + 9.0, &heading, true, 9.0, MUTED, Align::Left);
        self.y += 24.0;
        true
    }
}

/// Column x positions (left edges) and widths for the line table.
struct Columns {
    number: Option<(f32, f32)>,
    description: (f32, f32),
    quantity: Option<(f32, f32)>,
    rate: Option<(f32, f32)>,
    amount: (f32, f32),
}

fn columns(tpl: &FormTemplate, left: f32, right: f32) -> Columns {
    let mut x = right;
    let mut take = |w: f32| {
        x -= w;
        (x, w)
    };
    let amount = take(92.0);
    let rate = tpl.show_rate.then(|| take(84.0));
    let quantity = tpl.show_quantity.then(|| take(56.0));
    let number = tpl.show_line_numbers.then_some((left, 26.0));
    let desc_left = number.map_or(left, |(l, w)| l + w);
    Columns { number, description: (desc_left, x - desc_left), quantity, rate, amount }
}

const CELL_PAD: f32 = 5.0;
const BODY: f32 = 9.5;
const LEADING: f32 = 12.5;

fn table_header(l: &mut Layout, cols: &Columns) {
    let (left, right, y) = (MARGIN, l.right(), l.y);
    let shaded = l.tpl.shade_table_header;
    let color = if shaded { Rgb::WHITE } else { l.accent };
    if shaded {
        l.push(Op::Rect { x: left, y, w: right - left, h: 20.0, fill: Some(l.accent), stroke: None, line_width: 0.0 });
    } else {
        l.line(left, right, y + 20.0, l.accent, 1.0);
    }
    let base = y + 13.5;
    let tpl = l.tpl;
    if let Some((x, w)) = cols.number {
        l.text(x + w - CELL_PAD, base, "#", true, 8.5, color, Align::Right);
    }
    l.text(cols.description.0 + CELL_PAD, base, &tpl.description_label, true, 8.5, color, Align::Left);
    if let Some((x, w)) = cols.quantity {
        l.text(x + w - CELL_PAD, base, &tpl.quantity_label, true, 8.5, color, Align::Right);
    }
    if let Some((x, w)) = cols.rate {
        l.text(x + w - CELL_PAD, base, &tpl.rate_label, true, 8.5, color, Align::Right);
    }
    l.text(cols.amount.0 + cols.amount.1 - CELL_PAD, base, &tpl.amount_label, true, 8.5, color, Align::Right);
    l.y += 20.0;
}

/// Image size in points, scaled to fit the template's logo box.
fn logo_box(tpl: &FormTemplate, logo: Option<(u32, u32)>) -> Option<(f32, f32)> {
    let (pw, ph) = logo.filter(|_| tpl.logo != LogoPlacement::Hidden)?;
    let (max_w, max_h) = (200.0_f32, tpl.logo_size.max_height());
    let scale = (max_w / pw as f32).min(max_h / ph as f32);
    Some((pw as f32 * scale, ph as f32 * scale))
}

/// Lays a document out on pages. `logo` is the logo's pixel size, if the company has one.
pub fn layout(doc: &FormDoc, company: &Company, tpl: &FormTemplate, logo: Option<(u32, u32)>) -> Vec<Page> {
    let (width, height) = tpl.paper.size();
    let accent = Rgb::parse(&tpl.accent_color).unwrap_or(Rgb(0x2c, 0xa0, 0x1c));
    let mut l = Layout { tpl, font: tpl.font, accent, width, height, pages: Vec::new(), y: MARGIN };
    l.new_page();
    if doc.voided {
        l.text(width / 2.0, height * 0.42, "VOID", true, 110.0, VOID_MARK, Align::Center);
    }
    let right = l.right();
    let top = MARGIN;

    // Header: company on the left, title (and logo, by default) on the right.
    let logo = logo_box(tpl, logo);
    let mut left_x = MARGIN;
    let mut left_bottom = top;
    let mut right_y = top;
    if let Some((lw, lh)) = logo {
        match tpl.logo {
            LogoPlacement::Left => {
                l.push(Op::Image { x: MARGIN, y: top, w: lw, h: lh });
                left_x = MARGIN + lw + 14.0;
                left_bottom = top + lh;
            }
            _ => {
                l.push(Op::Image { x: right - lw, y: top, w: lw, h: lh });
                right_y = top + lh + 8.0;
            }
        }
    }
    // The right column holds the title and, when it is placed there, the logo above it.
    let right_logo_w = logo.filter(|_| tpl.logo == LogoPlacement::Right).map_or(0.0, |(lw, _)| lw);
    let title_w = pdf::text_width(l.font, true, 24.0, &pdf::sanitize(&tpl.title)).max(120.0).max(right_logo_w);
    let company_w = (right - title_w - 24.0 - left_x).max(140.0);
    let name_lines = l.wrap(&company.name, true, 14.0, company_w);
    for (i, line) in name_lines.iter().enumerate() {
        l.text(left_x, top + 14.0 + i as f32 * 17.0, line, true, 14.0, INK, Align::Left);
    }
    let mut y = top + 14.0 + name_lines.len().saturating_sub(1) as f32 * 17.0 + 6.0;
    let mut details = Vec::new();
    if !company.legal_name.is_empty() && company.legal_name != company.name {
        details.push(company.legal_name.clone());
    }
    if tpl.show_company_address && !company.address.is_empty() {
        details.extend(company.address.lines().map(String::from));
    }
    let contact: Vec<&str> =
        [(tpl.show_company_phone, company.phone.as_str()), (tpl.show_company_email, company.email.as_str())].into_iter().filter(|(on, v)| *on && !v.is_empty()).map(|(_, v)| v).collect();
    if !contact.is_empty() {
        details.push(contact.join(" · "));
    }
    if tpl.show_ein && !company.ein.is_empty() {
        details.push(format!("EIN {}", company.ein));
    }
    for detail in &details {
        for line in l.wrap(detail, false, 9.0, company_w) {
            y += 12.0;
            l.text(left_x, y, &line, false, 9.0, MUTED, Align::Left);
        }
    }
    left_bottom = left_bottom.max(y + 4.0);
    l.text(right, right_y + 22.0, &tpl.title, true, 24.0, accent, Align::Right);
    l.text(right, right_y + 40.0, &format!("No. {}", doc.number), false, 10.0, MUTED, Align::Right);
    let header_bottom = left_bottom.max(right_y + 44.0) + 12.0;
    l.line(MARGIN, right, header_bottom, accent, 1.5);
    l.y = header_bottom + 22.0;

    // Customer on the left; dates and amounts on the right.
    let meta_x = right - 210.0;
    let party_label = match doc.kind {
        FormKind::Invoice => "BILL TO",
        FormKind::Estimate => "PREPARED FOR",
    };
    let start = l.y;
    l.text(MARGIN, start, party_label, true, 7.5, MUTED, Align::Left);
    let mut py = start;
    let party_w = meta_x - MARGIN - 20.0;
    let mut party: Vec<(String, bool)> = vec![(doc.customer_name.clone(), true)];
    party.extend(doc.customer_details.iter().map(|d| (d.clone(), false)));
    if tpl.show_customer_email {
        party.extend(doc.customer_email.iter().map(|e| (e.clone(), false)));
    }
    for (text, bold) in &party {
        for line in l.wrap(text, *bold, 10.0, party_w) {
            py += 13.5;
            l.text(MARGIN, py, &line, *bold, 10.0, INK, Align::Left);
        }
    }
    let mut my = start - 4.0;
    for (label, value, strong) in &doc.meta {
        my += 15.0;
        l.text(meta_x, my, label, false, 9.0, MUTED, Align::Left);
        l.text(right, my, value, *strong, if *strong { 10.0 } else { 9.0 }, INK, Align::Right);
    }
    l.y = py.max(my) + 24.0;

    // Line items.
    let cols = columns(tpl, MARGIN, right);
    table_header(&mut l, &cols);
    for (i, item) in doc.lines.iter().enumerate() {
        let desc = if item.description.trim().is_empty() { "—".to_string() } else { item.description.clone() };
        let lines = l.wrap(&desc, false, BODY, cols.description.1 - 2.0 * CELL_PAD);
        let row_h = lines.len().max(1) as f32 * LEADING + 8.0;
        if l.ensure(row_h, doc) {
            table_header(&mut l, &cols);
        }
        let base = l.y + 4.0 + BODY;
        if let Some((x, w)) = cols.number {
            l.text(x + w - CELL_PAD, base, &(i + 1).to_string(), false, BODY, MUTED, Align::Right);
        }
        for (n, line) in lines.iter().enumerate() {
            l.text(cols.description.0 + CELL_PAD, base + n as f32 * LEADING, line, false, BODY, INK, Align::Left);
        }
        if let Some((x, w)) = cols.quantity {
            l.text(x + w - CELL_PAD, base, &item.quantity, false, BODY, INK, Align::Right);
        }
        if let Some((x, w)) = cols.rate {
            l.text(x + w - CELL_PAD, base, &item.rate, false, BODY, INK, Align::Right);
        }
        l.text(cols.amount.0 + cols.amount.1 - CELL_PAD, base, &item.amount, false, BODY, INK, Align::Right);
        l.y += row_h;
        l.line(MARGIN, right, l.y, RULE, 0.5);
    }

    // Totals, kept together.
    let rows: Vec<&TotalRow> = doc.totals.iter().filter(|r| tpl.show_payments || !r.payment).collect();
    l.y += 8.0;
    l.ensure(rows.len() as f32 * 17.0 + 12.0, doc);
    let label_x = right - 250.0;
    for row in rows {
        if row.grand {
            l.y += 4.0;
            l.line(label_x, right, l.y, accent, 1.0);
            l.y += 17.0;
            let label = l.fit(&row.label, true, 11.0, &row.value, right - label_x);
            l.text(label_x + CELL_PAD, l.y - 3.0, &label, true, 11.0, INK, Align::Left);
            l.text(right - CELL_PAD, l.y - 3.0, &row.value, true, 11.0, INK, Align::Right);
        } else {
            l.y += 15.0;
            let label = l.fit(&row.label, false, BODY, &row.value, right - label_x);
            l.text(label_x + CELL_PAD, l.y - 3.0, &label, false, BODY, INK, Align::Left);
            l.text(right - CELL_PAD, l.y - 3.0, &row.value, false, BODY, INK, Align::Right);
        }
    }
    l.y += 18.0;

    // Memo, message and terms.
    let paragraph = |l: &mut Layout, label: Option<&str>, text: &str, color: Rgb| {
        let lines = l.wrap(text, false, BODY, right - MARGIN);
        if lines.iter().all(|s| s.is_empty()) {
            return;
        }
        if let Some(label) = label {
            l.ensure(LEADING * 2.0, doc);
            l.y += 10.0;
            l.text(MARGIN, l.y, label, true, 7.5, MUTED, Align::Left);
            l.y += 3.0;
        }
        for line in lines {
            l.ensure(LEADING, doc);
            l.y += LEADING;
            l.text(MARGIN, l.y, &line, false, BODY, color, Align::Left);
        }
        l.y += 8.0;
    };
    if let Some(memo) = &doc.memo {
        paragraph(&mut l, if doc.voided { None } else { Some("NOTES") }, memo, INK);
    }
    if !doc.voided {
        paragraph(&mut l, None, &fill_placeholders(&tpl.message, doc, company), INK);
    }
    if !tpl.terms.is_empty() {
        l.ensure(LEADING * 2.0, doc);
        l.y += 4.0;
        l.line(MARGIN, right, l.y, RULE, 0.5);
        paragraph(&mut l, Some("TERMS"), &fill_placeholders(&tpl.terms, doc, company), MUTED);
    }

    // Footer and page numbers on every page.
    let footer = fill_placeholders(&tpl.footer, doc, company);
    let count = l.pages.len();
    for n in 0..count {
        let base = height - MARGIN + 6.0;
        let page = &mut l.pages[n];
        let mut add = |x: f32, text: String, align: Align| {
            let text = pdf::sanitize(&text);
            if !text.trim().is_empty() {
                page.ops.push(Op::Text { x, y: base, text, family: tpl.font, bold: false, size: 8.0, color: MUTED, align });
            }
        };
        add(width / 2.0, footer.clone(), Align::Center);
        if count > 1 {
            add(right, format!("Page {} of {count}", n + 1), Align::Right);
        }
    }
    l.pages
}

// ------------------------------------------------------------------------------------------------
// Rendering
// ------------------------------------------------------------------------------------------------

/// A laid-out form for the screen (preview and printing).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedForm {
    /// "Invoice 1001": the suggested file name and the PDF's title.
    pub title: String,
    pub pages: Vec<Page>,
    /// The logo the pages' image operations draw, as a data URL.
    pub logo: Option<String>,
    /// The customer's email address, for sending the form.
    pub customer_email: Option<String>,
}

pub struct Rendered {
    pub form: RenderedForm,
    pub image: Option<PdfImage>,
}

fn logo_bytes(data_url: Option<&str>) -> Option<Vec<u8>> {
    let (_, encoded) = data_url?.split_once("base64,")?;
    base64::engine::general_purpose::STANDARD.decode(encoded).ok()
}

fn decode_logo(data_url: Option<&str>) -> Option<PdfImage> {
    let bytes = logo_bytes(data_url)?;
    match PdfImage::decode(&bytes) {
        Ok(image) => Some(image),
        Err(err) => {
            // A logo the decoder cannot read prints without the logo rather than failing.
            eprintln!("form logo skipped: {err}");
            None
        }
    }
}

async fn document(pool: &SqlitePool, kind: FormKind, document_id: Option<i64>, symbol: &str) -> Result<FormDoc, String> {
    match (kind, document_id) {
        (_, None) => Ok(sample_doc(kind, symbol)),
        (FormKind::Invoice, Some(id)) => invoice_doc(pool, id, symbol).await,
        (FormKind::Estimate, Some(id)) => estimate_doc(pool, id, symbol).await,
    }
}

/// Lays out a document (or the sample, without one) with `template`, or the saved template.
/// `with_image` also decodes the logo for a PDF; the screen needs only its size.
pub async fn render(pool: &SqlitePool, kind: FormKind, document_id: Option<i64>, template: Option<FormTemplate>, with_image: bool) -> Result<Rendered, String> {
    let tpl = match template {
        Some(t) => t.validated(kind)?,
        None => get_template(pool, kind).await?,
    };
    let company = load_company(pool).await?;
    let doc = document(pool, kind, document_id, &company.currency_symbol).await?;
    let logo = company.logo.as_deref().filter(|_| tpl.logo != LogoPlacement::Hidden);
    let (image, size) = if with_image {
        let image = decode_logo(logo);
        let size = image.as_ref().map(|i| (i.width, i.height));
        (image, size)
    } else {
        (None, logo_bytes(logo).as_deref().and_then(pdf::image_size))
    };
    let pages = layout(&doc, &company, &tpl, size);
    let title = format!("{} {}", tpl.title, doc.number);
    Ok(Rendered { form: RenderedForm { title, pages, logo: size.and(company.logo), customer_email: doc.customer_email }, image })
}

/// The PDF file for a document with its saved template: (suggested file name, bytes).
pub async fn pdf_file(pool: &SqlitePool, kind: FormKind, document_id: i64) -> Result<(String, Vec<u8>), String> {
    let rendered = render(pool, kind, Some(document_id), None, true).await?;
    let bytes = pdf::write_pdf(&rendered.form.pages, rendered.image.as_ref(), &rendered.form.title);
    Ok((file_name(&rendered.form.title), bytes))
}

/// The email a document is sent with, before the user edits it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailDraft {
    /// The customer's email address, or empty.
    pub to: String,
    pub subject: String,
    pub body: String,
    /// The attachment's file name.
    pub file_name: String,
}

pub async fn email_draft(pool: &SqlitePool, kind: FormKind, document_id: i64) -> Result<EmailDraft, String> {
    let tpl = get_template(pool, kind).await?;
    let company = load_company(pool).await?;
    let doc = document(pool, kind, Some(document_id), &company.currency_symbol).await?;
    Ok(EmailDraft {
        to: doc.customer_email.clone().unwrap_or_default(),
        subject: fill_placeholders(&tpl.email_subject, &doc, &company),
        body: fill_placeholders(&tpl.email_body, &doc, &company),
        file_name: file_name(&format!("{} {}", tpl.title, doc.number)),
    })
}

/// "Invoice 1001" → "Invoice 1001.pdf", without characters file systems refuse.
pub fn file_name(title: &str) -> String {
    let clean: String = title.chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '-' } else { c }).collect();
    format!("{}.pdf", clean.trim().trim_matches('.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_pool;
    use crate::invoices::{create_and_post_invoice, InvoiceInput, InvoiceLineInput};
    use chrono::NaiveDate;

    fn texts(pages: &[Page]) -> Vec<String> {
        pages
            .iter()
            .flat_map(|p| p.ops.iter())
            .filter_map(|op| match op {
                Op::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn formats_like_the_app() {
        assert_eq!(format_money(123_456, None, "$"), "$1,234.56");
        assert_eq!(format_money(-99, None, "€"), "-€0.99");
        assert_eq!(format_money(100_000_000, Some("EUR"), "$"), "EUR 1,000,000.00");
        assert_eq!(format_quantity(15_000), "1.5");
        assert_eq!(format_quantity(12_340_000), "1,234");
        assert_eq!(format_quantity(1), "0.0001");
        assert_eq!(format_date("2026-10-23"), "10/23/2026");
    }

    #[test]
    fn templates_validate_and_fill_defaults() {
        let t = FormTemplate { title: "  ".into(), accent_color: "#ABCDEF".into(), amount_label: "".into(), ..FormTemplate::default() };
        let t = t.validated(FormKind::Estimate).unwrap();
        assert_eq!((t.title.as_str(), t.accent_color.as_str(), t.amount_label.as_str()), ("Estimate", "#abcdef", "Amount"));
        assert!(FormTemplate { accent_color: "green".into(), ..FormTemplate::default() }.validated(FormKind::Invoice).is_err());
        assert!(FormTemplate { title: "x".repeat(41), ..FormTemplate::default() }.validated(FormKind::Invoice).is_err());
        // Older saved templates without newer fields still read.
        let partial: FormTemplate = serde_json::from_str(r##"{"title":"Bill","accentColor":"#000000"}"##).unwrap();
        assert_eq!((partial.title.as_str(), partial.show_rate), ("Bill", true));
    }

    #[test]
    fn layout_follows_the_template() {
        let company = Company { name: "Acme Studio".into(), address: "1 Loop Rd".into(), email: "hi@acme.test".into(), currency_symbol: "$".into(), ..Company::default() };
        let doc = sample_doc(FormKind::Invoice, "$");
        let tpl = FormTemplate::default_for(FormKind::Invoice);
        let all = texts(&layout(&doc, &company, &tpl, Some((400, 100))));
        for expected in ["Acme Studio", "Invoice", "No. 1001", "BILL TO", "Sample Customer", "Qty", "Balance due", "Payment received 10/15/2026", "-$1,000.00"] {
            assert!(all.iter().any(|t| t == expected), "missing {expected:?} in {all:?}");
        }
        assert!(all.iter().any(|t| t.contains("Please reference invoice 1001")));

        let tpl = FormTemplate {
            title: "Tax Invoice".into(),
            show_quantity: false,
            show_rate: false,
            show_payments: false,
            show_company_email: false,
            footer: "Thanks from {company}".into(),
            terms: "Pay by bank transfer.".into(),
            ..FormTemplate::default_for(FormKind::Invoice)
        };
        let pages = layout(&doc, &company, &tpl, None);
        let all = texts(&pages);
        assert!(all.iter().any(|t| t == "Tax Invoice"));
        assert!(all.iter().any(|t| t == "Thanks from Acme Studio"));
        assert!(all.iter().any(|t| t == "Pay by bank transfer."));
        for absent in ["Qty", "Rate", "hi@acme.test", "Payment received 10/15/2026"] {
            assert!(!all.iter().any(|t| t == absent), "{absent:?} should be hidden");
        }
        assert!(!pages.iter().any(|p| p.ops.iter().any(|op| matches!(op, Op::Image { .. }))));
    }

    #[test]
    fn header_and_totals_text_stays_clear_of_the_logo_and_amounts() {
        let company = Company { name: "Northwest Regional Landscaping and Snow Removal Cooperative".into(), address: "1234 Southeast Industrial Parkway, Building 7, Unit 12, Portland, OR 97214".into(), currency_symbol: "$".into(), ..Company::default() };
        let mut doc = sample_doc(FormKind::Invoice, "$");
        doc.totals.insert(0, TotalRow { label: "Sales tax (Multnomah County Combined Special District Rate)".into(), value: "$1,234.56".into(), grand: false, payment: false });
        let tpl = FormTemplate { logo_size: LogoSize::Large, ..FormTemplate::default_for(FormKind::Invoice) };
        let pages = layout(&doc, &company, &tpl, Some((1000, 200)));
        let (logo_x, logo_bottom) = pages[0].ops.iter().find_map(|op| if let Op::Image { x, y, h, .. } = op { Some((*x, y + h)) } else { None }).unwrap();
        for op in &pages[0].ops {
            if let Op::Text { x, y, text, family, bold, size, align: Align::Left, .. } = op {
                if *y < logo_bottom {
                    assert!(x + pdf::text_width(*family, *bold, *size, text) <= logo_x, "{text:?} runs under the logo");
                }
            }
        }
        let tax = texts(&pages).into_iter().find(|t| t.starts_with("Sales tax")).unwrap();
        assert!(tax.ends_with('…'), "{tax:?} should be shortened");
    }

    #[test]
    fn long_documents_paginate_and_stay_on_the_page() {
        let company = Company { name: "Acme".into(), currency_symbol: "$".into(), ..Company::default() };
        let mut doc = sample_doc(FormKind::Estimate, "$");
        doc.lines = (0..80).map(|i| FormLine { description: format!("Line {i} with a fairly long description that wraps onto a second line in the table"), quantity: "1".into(), rate: "$1.00".into(), amount: "$1.00".into() }).collect();
        let tpl = FormTemplate { paper: Paper::A4, ..FormTemplate::default_for(FormKind::Estimate) };
        let pages = layout(&doc, &company, &tpl, None);
        assert!(pages.len() > 2);
        let all = texts(&pages);
        assert!(all.iter().any(|t| t == &format!("Page {} of {}", pages.len(), pages.len())));
        assert!(all.iter().any(|t| t == "Estimate 1001 (continued)"));
        assert!(all.iter().any(|t| t == "Line 79 with a fairly long description that wraps onto a"));
        for page in &pages {
            for op in &page.ops {
                if let Op::Text { y, text, .. } = op {
                    assert!(*y > 0.0 && *y < page.height - MARGIN + 10.0, "{text:?} at {y} is off the page");
                }
            }
        }
    }

    #[tokio::test]
    async fn real_invoices_render_saved_templates_to_pdf() {
        let pool = test_pool().await;
        let services: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(&pool).await.unwrap();
        let d = |day| NaiveDate::from_ymd_opt(2026, 10, day).unwrap();
        let invoice = create_and_post_invoice(
            &pool,
            InvoiceInput {
                customer_name: "Initech".into(),
                issue_date: d(1),
                due_date: d(31),
                lines: vec![InvoiceLineInput { description: "Design (hours)".into(), quantity_e4: 15_000, rate_cents: 10_000, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
                currency: None,
                tax_rate_id: None,
                discount_cents: 0,
            },
        )
        .await
        .unwrap();
        sqlx::query("UPDATE customers SET billing_address = '9 Elm St\nAustin, TX', email = 'ap@initech.test' WHERE name = 'Initech'").execute(&pool).await.unwrap();

        assert_eq!(get_template(&pool, FormKind::Invoice).await.unwrap(), FormTemplate::default_for(FormKind::Invoice));
        let custom = FormTemplate { title: "Statement of Work".into(), show_customer_email: true, ..FormTemplate::default_for(FormKind::Invoice) };
        save_template(&pool, FormKind::Invoice, custom.clone()).await.unwrap();
        assert_eq!(get_template(&pool, FormKind::Invoice).await.unwrap().title, "Statement of Work");
        // Templates are per form type.
        assert_eq!(get_template(&pool, FormKind::Estimate).await.unwrap().title, "Estimate");

        let rendered = render(&pool, FormKind::Invoice, Some(invoice.id), None, false).await.unwrap();
        assert_eq!(rendered.form.title, format!("Statement of Work {}", invoice.invoice_number));
        assert_eq!(rendered.form.customer_email.as_deref(), Some("ap@initech.test"));
        let all = texts(&rendered.form.pages);
        for expected in ["Initech", "9 Elm St", "Austin, TX", "ap@initech.test", "1.5", "$100.00", "$150.00", "Net 30"] {
            assert!(all.iter().any(|t| t == expected), "missing {expected:?} in {all:?}");
        }

        let (name, bytes) = pdf_file(&pool, FormKind::Invoice, invoice.id).await.unwrap();
        assert_eq!(name, format!("Statement of Work {}.pdf", invoice.invoice_number));
        assert!(bytes.starts_with(b"%PDF-1.4"));

        let draft = email_draft(&pool, FormKind::Invoice, invoice.id).await.unwrap();
        assert_eq!(draft.to, "ap@initech.test");
        assert_eq!(draft.subject, format!("Invoice {} from {}", invoice.invoice_number, load_company(&pool).await.unwrap().name));
        assert!(draft.body.starts_with("Dear Initech,\n\nPlease find attached invoice"), "{}", draft.body);
        assert!(draft.body.contains("$150.00, due 10/31/2026"));
        assert_eq!(draft.file_name, format!("Statement of Work {}.pdf", invoice.invoice_number));

        assert_eq!(reset_template(&pool, FormKind::Invoice).await.unwrap().title, "Invoice");
        assert_eq!(get_template(&pool, FormKind::Invoice).await.unwrap().title, "Invoice");
        assert!(render(&pool, FormKind::Estimate, Some(999), None, false).await.is_err());
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_name("Invoice 1001"), "Invoice 1001.pdf");
        assert_eq!(file_name("A/B: \"x\""), "A-B- -x-.pdf");
    }
}
