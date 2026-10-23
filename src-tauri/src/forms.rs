//! Printed forms: invoices, estimates, sales receipts and customer statements as the form
//! designer lays them out.
//!
//! A form style is a named set of templates, one per form type (layout preset, title, paper,
//! font, accent color, logo placement, which company details and columns show, and the customer
//! message). One style is the default; a customer can have their own. A document is gathered into
//! a `FormDoc`, laid out once into pages of drawing operations, and those pages are what the
//! designer previews, what prints, and what is written to a PDF file, so all three match.
//! Styles are stored in the company file; a style without a template for a form type uses the
//! defaults.

use base64::Engine;
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::db_err;
use crate::pdf::{self, Align, FontFamily, Op, Page, PdfImage, Rgb};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FormKind {
    Invoice,
    Estimate,
    SalesReceipt,
    /// A customer statement; its document id is the customer's id, with a statement period.
    Statement,
}

impl FormKind {
    #[cfg(test)]
    pub const ALL: [FormKind; 4] = [FormKind::Invoice, FormKind::Estimate, FormKind::SalesReceipt, FormKind::Statement];

    pub(crate) fn key(self) -> &'static str {
        match self {
            FormKind::Invoice => "INVOICE",
            FormKind::Estimate => "ESTIMATE",
            FormKind::SalesReceipt => "SALES_RECEIPT",
            FormKind::Statement => "STATEMENT",
        }
    }

    fn default_title(self) -> &'static str {
        match self {
            FormKind::Invoice => "Invoice",
            FormKind::Estimate => "Estimate",
            FormKind::SalesReceipt => "Sales Receipt",
            FormKind::Statement => "Statement",
        }
    }
}

/// A customer statement's period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Period {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
}

impl Period {
    /// Last month, the Customer Statements screen's default.
    pub fn last_month(today: NaiveDate) -> Period {
        let first_this_month = today.with_day(1).unwrap_or(today);
        let end_date = first_this_month.pred_opt().unwrap_or(first_this_month);
        Period { start_date: end_date.with_day(1).unwrap_or(end_date), end_date }
    }
}

// ------------------------------------------------------------------------------------------------
// Templates
// ------------------------------------------------------------------------------------------------

/// The overall look: where the title sits and how dense the type is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FormLayout {
    /// Company and title side by side over an accent rule.
    #[default]
    Classic,
    /// The title in a solid accent band, a shaded table header and banded rows.
    Modern,
    /// Smaller type and tighter rows, for long documents.
    Compact,
}

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
    pub layout: FormLayout,
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
    /// Statements: the aging summary under the amount due.
    pub show_aging: bool,
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
            layout: FormLayout::Classic,
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
            show_aging: true,
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
            FormKind::SalesReceipt => "Thank you for your business.",
            FormKind::Statement => "Please contact us with any questions about your account.",
        };
        let (subject, body) = match kind {
            FormKind::Invoice => (
                "Invoice {number} from {company}",
                "Dear {customer},\n\nPlease find attached invoice {number} for {total}, due {due_date}.\n\nThank you for your business.\n{company}",
            ),
            FormKind::Estimate => ("Estimate {number} from {company}", "Dear {customer},\n\nPlease find attached estimate {number} for {total}.\n\nWe look forward to working with you.\n{company}"),
            FormKind::SalesReceipt => ("Sales receipt {number} from {company}", "Dear {customer},\n\nPlease find attached your receipt {number} for {total}.\n\nThank you for your business.\n{company}"),
            FormKind::Statement => ("Statement from {company}", "Dear {customer},\n\nPlease find attached your statement as of {date}. The amount due is {balance}.\n\n{company}"),
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

/// A named set of form templates.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct FormStyle {
    pub id: i64,
    pub name: String,
    pub is_default: bool,
    /// Customers whose forms use this style instead of the default.
    pub customer_count: i64,
}

pub async fn list_styles(pool: &SqlitePool) -> Result<Vec<FormStyle>, String> {
    sqlx::query_as(
        "SELECT s.id, s.name, s.is_default, (SELECT COUNT(*) FROM customers WHERE form_style_id = s.id) AS customer_count
         FROM form_styles s ORDER BY s.is_default DESC, s.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

async fn find_style(pool: &SqlitePool, id: i64) -> Result<FormStyle, String> {
    list_styles(pool).await?.into_iter().find(|s| s.id == id).ok_or_else(|| "FORM_STYLE_NOT_FOUND: that form style no longer exists".to_string())
}

fn style_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 60 {
        return Err("FORM_STYLE_NAME: give the style a name of up to 60 characters".into());
    }
    Ok(name.to_string())
}

fn duplicate_name(err: sqlx::Error, name: &str) -> String {
    let msg = db_err(err);
    if msg.contains("UNIQUE") {
        format!("FORM_STYLE_DUPLICATE: there is already a style named '{name}'")
    } else {
        msg
    }
}

/// A new style; with `copy_from`, it starts as a copy of that style's templates.
pub async fn create_style(pool: &SqlitePool, name: &str, copy_from: Option<i64>) -> Result<FormStyle, String> {
    let name = style_name(name)?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    let id: i64 = sqlx::query_scalar("INSERT INTO form_styles (name) VALUES (?1) RETURNING id")
        .bind(&name)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| duplicate_name(e, &name))?;
    if let Some(source) = copy_from {
        sqlx::query("INSERT INTO form_templates (style_id, form_type, template_json) SELECT ?1, form_type, template_json FROM form_templates WHERE style_id = ?2")
            .bind(id)
            .bind(source)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    tx.commit().await.map_err(db_err)?;
    find_style(pool, id).await
}

pub async fn rename_style(pool: &SqlitePool, id: i64, name: &str) -> Result<FormStyle, String> {
    let name = style_name(name)?;
    let done = sqlx::query("UPDATE form_styles SET name = ?1 WHERE id = ?2").bind(&name).bind(id).execute(pool).await.map_err(|e| duplicate_name(e, &name))?;
    if done.rows_affected() == 0 {
        return Err("FORM_STYLE_NOT_FOUND: that form style no longer exists".into());
    }
    find_style(pool, id).await
}

pub async fn set_default_style(pool: &SqlitePool, id: i64) -> Result<(), String> {
    find_style(pool, id).await?;
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("UPDATE form_styles SET is_default = 0 WHERE is_default = 1").execute(&mut *tx).await.map_err(db_err)?;
    sqlx::query("UPDATE form_styles SET is_default = 1 WHERE id = ?1").bind(id).execute(&mut *tx).await.map_err(db_err)?;
    tx.commit().await.map_err(db_err)
}

/// Deletes a style and its templates; its customers go back to the default style.
pub async fn delete_style(pool: &SqlitePool, id: i64) -> Result<(), String> {
    if find_style(pool, id).await?.is_default {
        return Err("FORM_STYLE_DEFAULT: make another style the default before deleting this one".into());
    }
    let mut tx = pool.begin().await.map_err(db_err)?;
    sqlx::query("UPDATE customers SET form_style_id = NULL WHERE form_style_id = ?1").bind(id).execute(&mut *tx).await.map_err(db_err)?;
    sqlx::query("DELETE FROM form_templates WHERE style_id = ?1").bind(id).execute(&mut *tx).await.map_err(db_err)?;
    sqlx::query("DELETE FROM form_styles WHERE id = ?1").bind(id).execute(&mut *tx).await.map_err(db_err)?;
    tx.commit().await.map_err(db_err)
}

/// `style` itself, or the default style when `None`.
async fn style_id(pool: &SqlitePool, style: Option<i64>) -> Result<i64, String> {
    match style {
        Some(id) => Ok(find_style(pool, id).await?.id),
        None => sqlx::query_scalar("SELECT id FROM form_styles WHERE is_default = 1").fetch_optional(pool).await.map_err(db_err)?.ok_or_else(|| "FORM_STYLE_NONE: there is no default form style".to_string()),
    }
}

/// The style a customer's forms use: theirs, or the default.
pub async fn customer_style(pool: &SqlitePool, customer_id: Option<i64>) -> Result<i64, String> {
    let own: Option<i64> = sqlx::query_scalar("SELECT s.id FROM customers c JOIN form_styles s ON s.id = c.form_style_id WHERE c.id = ?1")
        .bind(customer_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    match own {
        Some(id) => Ok(id),
        None => style_id(pool, None).await,
    }
}

/// A style's template for a form type (`style` None = the default style).
pub async fn get_template(pool: &SqlitePool, style: Option<i64>, kind: FormKind) -> Result<FormTemplate, String> {
    let style = style_id(pool, style).await?;
    let stored: Option<String> = sqlx::query_scalar("SELECT template_json FROM form_templates WHERE style_id = ?1 AND form_type = ?2")
        .bind(style)
        .bind(kind.key())
        .fetch_optional(pool)
        .await
        .map_err(db_err)?;
    let Some(json) = stored else { return Ok(FormTemplate::default_for(kind)) };
    // A template that no longer reads (hand-edited, or from a newer version) falls back to defaults
    // rather than blocking every print.
    Ok(serde_json::from_str::<FormTemplate>(&json).ok().and_then(|t| t.validated(kind).ok()).unwrap_or_else(|| FormTemplate::default_for(kind)))
}

pub async fn save_template(pool: &SqlitePool, style: Option<i64>, kind: FormKind, template: FormTemplate) -> Result<FormTemplate, String> {
    let style = style_id(pool, style).await?;
    let template = template.validated(kind)?;
    let json = serde_json::to_string(&template).map_err(|e| format!("FORM_SAVE: {e}"))?;
    sqlx::query(
        "INSERT INTO form_templates (style_id, form_type, template_json) VALUES (?1, ?2, ?3)
         ON CONFLICT (style_id, form_type) DO UPDATE SET template_json = excluded.template_json, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
    )
    .bind(style)
    .bind(kind.key())
    .bind(&json)
    .execute(pool)
    .await
    .map_err(db_err)?;
    Ok(template)
}

pub async fn reset_template(pool: &SqlitePool, style: Option<i64>, kind: FormKind) -> Result<FormTemplate, String> {
    let style = style_id(pool, style).await?;
    sqlx::query("DELETE FROM form_templates WHERE style_id = ?1 AND form_type = ?2").bind(style).bind(kind.key()).execute(pool).await.map_err(db_err)?;
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

/// A statement's transactions and aging, already formatted.
#[derive(Debug, Clone, PartialEq)]
pub struct StatementTable {
    /// Date, transaction, details, amount, running balance.
    pub rows: Vec<[String; 5]>,
    /// Current, 1–30, 31–60, 61–90 and over 90 days past due.
    pub aging: [String; 5],
}

/// Everything a form prints about one document, already formatted.
#[derive(Debug, Clone)]
pub struct FormDoc {
    pub kind: FormKind,
    pub number: String,
    /// Under the title; "No. {number}" when not set.
    pub subtitle: Option<String>,
    pub voided: bool,
    /// Picks the customer's form style.
    pub customer_id: Option<i64>,
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
    /// Statements only: printed instead of the line items.
    pub statement: Option<StatementTable>,
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
        subtitle: None,
        voided: inv.is_voided,
        customer_id: inv.customer_id,
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
        statement: None,
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
        subtitle: None,
        voided: false,
        customer_id: Some(customer_id),
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
        statement: None,
    })
}

const PAYMENT_METHODS: &[(&str, &str)] =
    &[("CASH", "Cash"), ("CHECK", "Check"), ("CREDIT_CARD", "Credit card"), ("ACH", "ACH / bank transfer"), ("WIRE", "Wire"), ("OTHER", "Other")];

pub async fn sales_receipt_doc(pool: &SqlitePool, receipt_id: i64, symbol: &str) -> Result<FormDoc, String> {
    type Header = (i64, Option<i64>, Option<String>, String, String, Option<String>, Option<String>, i64, i64, i64, Option<String>, bool);
    let header: Option<Header> = sqlx::query_as(
        "SELECT r.receipt_number, r.customer_id, c.name, r.receipt_date, r.payment_method, r.reference, r.memo, r.total_cents, r.tax_cents,
                r.discount_cents, t.name, r.is_voided
         FROM sales_receipts r LEFT JOIN customers c ON c.id = r.customer_id LEFT JOIN tax_rates t ON t.id = r.tax_rate_id
         WHERE r.id = ?1",
    )
    .bind(receipt_id)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let (number, customer_id, name, date, method, reference, memo, total, tax, discount, tax_name, voided) =
        header.ok_or("FORM_NOT_FOUND: that sales receipt no longer exists")?;
    let rows: Vec<(String, i64, i64, i64)> =
        sqlx::query_as("SELECT description, quantity_e4, rate_cents, amount_cents FROM sales_receipt_lines WHERE sales_receipt_id = ?1 ORDER BY line_number")
            .bind(receipt_id)
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
    let money = |cents: i64| format_money(cents, None, symbol);
    // A walk-in sale has no customer.
    let name = name.unwrap_or_else(|| "Cash customer".into());
    let cust = match customer_id {
        Some(id) => customer(pool, Some(id), &name).await?,
        None => Customer { company_name: None, billing_address: None, email: None },
    };
    let method = PAYMENT_METHODS.iter().find(|(k, _)| *k == method).map_or("Other", |(_, l)| l);
    let paid_by = match reference.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        Some(r) => format!("{method} #{r}"),
        None => method.to_string(),
    };
    let meta = vec![("Receipt date".to_string(), format_date(&date), false), ("Payment".to_string(), paid_by, false), ("Total".to_string(), money(total), true)];
    let mut totals = Vec::new();
    breakdown(&mut totals, total, tax, discount, tax_name.as_deref(), &money);
    totals.push(TotalRow { label: "Total".into(), value: money(total), grand: true, payment: false });
    Ok(FormDoc {
        kind: FormKind::SalesReceipt,
        number: number.to_string(),
        subtitle: None,
        voided,
        customer_id,
        customer_details: customer_details(&name, &cust),
        customer_name: name,
        customer_email: cust.email,
        meta,
        lines: rows.into_iter().map(|(description, qty, rate, amount)| FormLine { description, quantity: format_quantity(qty), rate: money(rate), amount: money(amount) }).collect(),
        totals,
        memo: if voided { Some("This sales receipt has been voided.".into()) } else { memo.filter(|m| !m.trim().is_empty()) },
        date: format_date(&date),
        due_date: String::new(),
        total: money(total),
        balance: money(0),
        statement: None,
    })
}

fn statement_label(source_type: &str) -> &'static str {
    match source_type {
        "INVOICE" => "Invoice",
        "RECEIVE_PAYMENT" => "Payment",
        "CREDIT_MEMO" => "Credit Memo",
        "SALES_RECEIPT" => "Sales Receipt",
        "JOURNAL" => "Refund",
        _ => "Adjustment",
    }
}

/// A customer's statement for a period: balance forward, each transaction with a running
/// balance, the amount due and its aging.
pub async fn statement_doc(pool: &SqlitePool, customer_id: i64, period: Period, symbol: &str) -> Result<FormDoc, String> {
    if period.end_date < period.start_date {
        return Err("FORM_BAD_PERIOD: the statement period ends before it starts".into());
    }
    let s = crate::workflows::customer_statement(pool, customer_id, period.start_date, period.end_date).await?;
    let money = |cents: i64| format_money(cents, None, symbol);
    let cust = customer(pool, Some(customer_id), &s.customer_name).await?;
    let iso = |d: NaiveDate| d.format("%Y-%m-%d").to_string();
    let mut rows = vec![[format_date(&iso(period.start_date)), "Balance forward".to_string(), String::new(), String::new(), money(s.opening_balance_cents)]];
    for r in &s.rows {
        let transaction = match r.number.as_deref().filter(|n| !n.is_empty()) {
            Some(n) => format!("{} #{n}", statement_label(&r.source_type)),
            None => statement_label(&r.source_type).to_string(),
        };
        // A job's lines name the job; the customer's own lines show their memo.
        let details = match r.party_name.strip_prefix(&format!("{}:", s.customer_name)) {
            Some(job) => job.to_string(),
            None => r.memo.clone().unwrap_or_default(),
        };
        rows.push([format_date(&iso(r.date)), transaction, details, money(r.amount_cents), money(r.balance_cents)]);
    }
    let due = money(s.closing_balance_cents);
    let end = format_date(&iso(period.end_date));
    Ok(FormDoc {
        kind: FormKind::Statement,
        number: iso(period.end_date),
        subtitle: Some(format!("{} – {end}", format_date(&iso(period.start_date)))),
        voided: false,
        customer_id: Some(customer_id),
        customer_details: customer_details(&s.customer_name, &cust),
        customer_name: s.customer_name.clone(),
        customer_email: cust.email,
        meta: vec![("Statement date".into(), end.clone(), false), ("Amount due".into(), due.clone(), true)],
        lines: Vec::new(),
        totals: vec![TotalRow { label: "Amount due".into(), value: due.clone(), grand: true, payment: false }],
        memo: None,
        date: end,
        due_date: String::new(),
        total: due.clone(),
        balance: due,
        statement: Some(StatementTable { rows, aging: s.aging_cents.map(money) }),
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
        FormKind::SalesReceipt => (
            vec![
                ("Receipt date".to_string(), "10/01/2026".to_string(), false),
                ("Payment".to_string(), "Check #2045".to_string(), false),
                ("Total".to_string(), money(total), true),
            ],
            vec![TotalRow { label: "Total".into(), value: money(total), grand: true, payment: false }],
            money(0),
        ),
        FormKind::Statement => (
            vec![("Statement date".to_string(), "10/31/2026".to_string(), false), ("Amount due".to_string(), money(total - 100_000), true)],
            vec![TotalRow { label: "Amount due".into(), value: money(total - 100_000), grand: true, payment: false }],
            money(total - 100_000),
        ),
    };
    let statement = (kind == FormKind::Statement).then(|| StatementTable {
        rows: vec![
            ["10/01/2026".into(), "Balance forward".into(), String::new(), String::new(), money(0)],
            ["10/01/2026".into(), "Invoice #1001".into(), String::new(), money(total), money(total)],
            ["10/15/2026".into(), "Payment #2045".into(), String::new(), money(-100_000), money(total - 100_000)],
        ],
        aging: [money(total - 100_000), money(0), money(0), money(0), money(0)],
    });
    FormDoc {
        kind,
        number: if kind == FormKind::Statement { "2026-10-31".into() } else { "1001".into() },
        subtitle: (kind == FormKind::Statement).then(|| "10/01/2026 – 10/31/2026".into()),
        voided: false,
        customer_id: None,
        customer_name: "Sample Customer".into(),
        customer_email: Some("billing@example.com".into()),
        customer_details,
        meta,
        lines: if statement.is_some() { Vec::new() } else { lines },
        totals,
        memo: None,
        date: if statement.is_some() { "10/31/2026".into() } else { "10/01/2026".into() },
        due_date: if statement.is_some() { String::new() } else { "10/31/2026".into() },
        total: money(total),
        balance,
        statement,
    }
}

fn fill_placeholders(text: &str, doc: &FormDoc, company: &Company) -> String {
    [
        ("{number}", doc.number.as_str()),
        ("{invoice_number}", doc.number.as_str()),
        ("{amount}", doc.total.as_str()),
        ("{balance_due}", doc.balance.as_str()),
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
        let heading = match doc.kind {
            FormKind::Statement => format!("{} · {} (continued)", self.tpl.title, doc.customer_name),
            _ => format!("{} {} (continued)", self.tpl.title, doc.number),
        };
        self.text(MARGIN, self.y + 9.0, &heading, true, 9.0, MUTED, Align::Left);
        self.y += 24.0;
        true
    }
}

const CELL_PAD: f32 = 5.0;

/// Type sizes and spacing for a layout preset.
#[derive(Clone, Copy)]
struct Metrics {
    body: f32,
    leading: f32,
    /// Vertical padding added to each table row.
    row_pad: f32,
    header_h: f32,
    title: f32,
    name: f32,
    small: f32,
}

impl Metrics {
    fn of(layout: FormLayout) -> Metrics {
        match layout {
            FormLayout::Compact => Metrics { body: 8.5, leading: 10.5, row_pad: 5.0, header_h: 16.0, title: 18.0, name: 12.0, small: 8.0 },
            FormLayout::Classic | FormLayout::Modern => Metrics { body: 9.5, leading: 12.5, row_pad: 8.0, header_h: 20.0, title: 24.0, name: 14.0, small: 9.0 },
        }
    }
}

/// `c` mixed toward white: 0 = unchanged, 1 = white.
fn tint(c: Rgb, amount: f32) -> Rgb {
    let mix = |v: u8| (f32::from(v) + (255.0 - f32::from(v)) * amount).round() as u8;
    Rgb(mix(c.0), mix(c.1), mix(c.2))
}

/// One table column: left edge, width, header label and alignment.
struct Col {
    x: f32,
    w: f32,
    label: String,
    align: Align,
}

fn shaded_header(tpl: &FormTemplate) -> bool {
    tpl.shade_table_header || tpl.layout == FormLayout::Modern
}

fn table_header(l: &mut Layout, cols: &[Col], m: Metrics) {
    let (left, right, y) = (MARGIN, l.right(), l.y);
    let shaded = shaded_header(l.tpl);
    let color = if shaded { Rgb::WHITE } else { l.accent };
    if shaded {
        l.push(Op::Rect { x: left, y, w: right - left, h: m.header_h, fill: Some(l.accent), stroke: None, line_width: 0.0 });
    } else {
        l.line(left, right, y + m.header_h, l.accent, 1.0);
    }
    let base = y + m.header_h / 2.0 + 3.5;
    let size = m.body - 1.0;
    for col in cols {
        let x = match col.align {
            Align::Right => col.x + col.w - CELL_PAD,
            _ => col.x + CELL_PAD,
        };
        l.text(x, base, &col.label, true, size, color, col.align);
    }
    l.y += m.header_h;
}

/// One body row of wrapped cells; breaks the page (repeating the header) when it does not fit.
fn table_row(l: &mut Layout, doc: &FormDoc, cols: &[Col], cells: &[Vec<String>], muted_first: bool, index: usize, m: Metrics) {
    let lines = cells.iter().map(Vec::len).max().unwrap_or(1).clamp(1, 40);
    let row_h = lines as f32 * m.leading + m.row_pad;
    if l.ensure(row_h, doc) {
        table_header(l, cols, m);
    }
    let modern = l.tpl.layout == FormLayout::Modern;
    if modern && index % 2 == 1 {
        let (right, accent) = (l.right(), l.accent);
        l.push(Op::Rect { x: MARGIN, y: l.y, w: right - MARGIN, h: row_h, fill: Some(tint(accent, 0.93)), stroke: None, line_width: 0.0 });
    }
    let base = l.y + m.row_pad / 2.0 + m.body;
    for (c, (col, text)) in cols.iter().zip(cells).enumerate() {
        let x = match col.align {
            Align::Right => col.x + col.w - CELL_PAD,
            _ => col.x + CELL_PAD,
        };
        let color = if muted_first && c == 0 { MUTED } else { INK };
        for (n, line) in text.iter().take(40).enumerate() {
            l.text(x, base + n as f32 * m.leading, line, false, m.body, color, col.align);
        }
    }
    l.y += row_h;
    if !modern {
        let right = l.right();
        l.line(MARGIN, right, l.y, RULE, 0.5);
    }
}

/// Image size in points, scaled to fit the template's logo box.
fn logo_box(tpl: &FormTemplate, logo: Option<(u32, u32)>) -> Option<(f32, f32)> {
    let (pw, ph) = logo.filter(|_| tpl.logo != LogoPlacement::Hidden)?;
    let scale = if tpl.layout == FormLayout::Compact { 0.8 } else { 1.0 };
    let (max_w, max_h) = (200.0_f32 * scale, tpl.logo_size.max_height() * scale);
    let scale = (max_w / pw as f32).min(max_h / ph as f32);
    Some((pw as f32 * scale, ph as f32 * scale))
}

/// Lays a document out on pages. `logo` is the logo's pixel size, if the company has one.
pub fn layout(doc: &FormDoc, company: &Company, tpl: &FormTemplate, logo: Option<(u32, u32)>) -> Vec<Page> {
    let (width, height) = tpl.paper.size();
    let accent = Rgb::parse(&tpl.accent_color).unwrap_or(Rgb(0x2c, 0xa0, 0x1c));
    let m = Metrics::of(tpl.layout);
    let modern = tpl.layout == FormLayout::Modern;
    let mut l = Layout { tpl, font: tpl.font, accent, width, height, pages: Vec::new(), y: MARGIN };
    l.new_page();
    if doc.voided {
        l.text(width / 2.0, height * 0.42, "VOID", true, 110.0, VOID_MARK, Align::Center);
    }
    let right = l.right();
    let subtitle = doc.subtitle.clone().unwrap_or_else(|| format!("No. {}", doc.number));
    let mut top = MARGIN;

    // Modern: the title and number in a solid accent band across the top.
    if modern {
        let band = 46.0;
        l.push(Op::Rect { x: MARGIN, y: top, w: right - MARGIN, h: band, fill: Some(accent), stroke: None, line_width: 0.0 });
        l.text(MARGIN + 14.0, top + band / 2.0 + m.title * 0.36, &tpl.title, true, m.title, Rgb::WHITE, Align::Left);
        l.text(right - 14.0, top + band / 2.0 + 4.0, &subtitle, false, 11.0, Rgb::WHITE, Align::Right);
        top += band + 16.0;
    }

    // Header: company on the left; the title (unless in the band) and logo, by default, on the right.
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
    let title_w = if modern { right_logo_w } else { pdf::text_width(l.font, true, m.title, &pdf::sanitize(&tpl.title)).max(120.0).max(right_logo_w) };
    let company_w = (right - title_w - 24.0 - left_x).max(140.0);
    let name_lead = m.name * 1.2;
    let name_lines = l.wrap(&company.name, true, m.name, company_w);
    for (i, line) in name_lines.iter().enumerate() {
        l.text(left_x, top + m.name + i as f32 * name_lead, line, true, m.name, INK, Align::Left);
    }
    let mut y = top + m.name + name_lines.len().saturating_sub(1) as f32 * name_lead + 6.0;
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
        for line in l.wrap(detail, false, m.small, company_w) {
            y += m.small + 3.0;
            l.text(left_x, y, &line, false, m.small, MUTED, Align::Left);
        }
    }
    left_bottom = left_bottom.max(y + 4.0);
    let header_bottom = if modern {
        left_bottom.max(right_y - 8.0) + 10.0
    } else {
        l.text(right, right_y + m.title - 2.0, &tpl.title, true, m.title, accent, Align::Right);
        l.text(right, right_y + m.title + 16.0, &subtitle, false, 10.0, MUTED, Align::Right);
        left_bottom.max(right_y + m.title + 20.0) + 12.0
    };
    if modern {
        l.line(MARGIN, right, header_bottom, RULE, 0.75);
    } else {
        l.line(MARGIN, right, header_bottom, accent, if tpl.layout == FormLayout::Compact { 1.0 } else { 1.5 });
    }
    l.y = header_bottom + if tpl.layout == FormLayout::Compact { 16.0 } else { 22.0 };

    // Customer on the left; dates and amounts on the right.
    let meta_x = right - 210.0;
    let party_label = match doc.kind {
        FormKind::Invoice => "BILL TO",
        FormKind::Estimate => "PREPARED FOR",
        FormKind::SalesReceipt => "SOLD TO",
        FormKind::Statement => "STATEMENT FOR",
    };
    let label_color = if modern { accent } else { MUTED };
    let start = l.y;
    l.text(MARGIN, start, party_label, true, 7.5, label_color, Align::Left);
    let mut py = start;
    let party_w = meta_x - MARGIN - 20.0;
    let party_size = m.body + 0.5;
    let mut party: Vec<(String, bool)> = vec![(doc.customer_name.clone(), true)];
    party.extend(doc.customer_details.iter().map(|d| (d.clone(), false)));
    if tpl.show_customer_email {
        party.extend(doc.customer_email.iter().map(|e| (e.clone(), false)));
    }
    for (text, bold) in &party {
        for line in l.wrap(text, *bold, party_size, party_w) {
            py += party_size + 3.5;
            l.text(MARGIN, py, &line, *bold, party_size, INK, Align::Left);
        }
    }
    let mut my = start - 4.0;
    let meta_lead = m.small + 6.0;
    for (label, value, strong) in &doc.meta {
        my += meta_lead;
        if *strong && modern {
            l.push(Op::Rect { x: meta_x - 4.0, y: my - m.small - 3.0, w: right - meta_x + 4.0, h: meta_lead, fill: Some(tint(accent, 0.88)), stroke: None, line_width: 0.0 });
        }
        l.text(meta_x, my, label, false, m.small, MUTED, Align::Left);
        l.text(right, my, value, *strong, if *strong { m.small + 1.0 } else { m.small }, INK, Align::Right);
    }
    l.y = py.max(my) + if tpl.layout == FormLayout::Compact { 16.0 } else { 24.0 };

    if let Some(statement) = &doc.statement {
        // Statement transactions: date, transaction, details, amount and running balance.
        let mut x = MARGIN;
        let mut col = |w: f32, label: &str, align: Align| {
            let c = Col { x, w, label: label.into(), align };
            x += w;
            c
        };
        let details_w = right - MARGIN - 66.0 - 116.0 - 88.0 - 92.0;
        let cols = vec![
            col(66.0, "Date", Align::Left),
            col(116.0, "Transaction", Align::Left),
            col(details_w, "Details", Align::Left),
            col(88.0, &tpl.amount_label, Align::Right),
            col(92.0, "Balance", Align::Right),
        ];
        l.ensure(m.header_h + m.leading * 2.0 + m.row_pad, doc);
        table_header(&mut l, &cols, m);
        for (i, row) in statement.rows.iter().enumerate() {
            let cells: Vec<Vec<String>> = row
                .iter()
                .zip(&cols)
                .map(|(text, c)| if c.align == Align::Left { l.wrap(text, false, m.body, c.w - 2.0 * CELL_PAD) } else { vec![text.clone()] })
                .collect();
            table_row(&mut l, doc, &cols, &cells, false, i, m);
        }
    } else {
        // Line items.
        let mut cols = Vec::new();
        let mut x = right;
        let mut take = |w: f32| {
            x -= w;
            x
        };
        let amount = Col { x: take(92.0), w: 92.0, label: tpl.amount_label.clone(), align: Align::Right };
        let rate = tpl.show_rate.then(|| Col { x: take(84.0), w: 84.0, label: tpl.rate_label.clone(), align: Align::Right });
        let quantity = tpl.show_quantity.then(|| Col { x: take(56.0), w: 56.0, label: tpl.quantity_label.clone(), align: Align::Right });
        let number = tpl.show_line_numbers.then(|| Col { x: MARGIN, w: 26.0, label: "#".into(), align: Align::Right });
        let desc_left = number.as_ref().map_or(MARGIN, |n| n.x + n.w);
        cols.extend(number);
        cols.push(Col { x: desc_left, w: x - desc_left, label: tpl.description_label.clone(), align: Align::Left });
        cols.extend(quantity);
        cols.extend(rate);
        cols.push(amount);
        l.ensure(m.header_h + m.leading + m.row_pad, doc);
        table_header(&mut l, &cols, m);
        let desc_w = cols.iter().find(|c| c.align == Align::Left).map_or(100.0, |c| c.w) - 2.0 * CELL_PAD;
        for (i, item) in doc.lines.iter().enumerate() {
            let desc = if item.description.trim().is_empty() { "—".to_string() } else { item.description.clone() };
            let mut cells = Vec::new();
            if tpl.show_line_numbers {
                cells.push(vec![(i + 1).to_string()]);
            }
            cells.push(l.wrap(&desc, false, m.body, desc_w));
            if tpl.show_quantity {
                cells.push(vec![item.quantity.clone()]);
            }
            if tpl.show_rate {
                cells.push(vec![item.rate.clone()]);
            }
            cells.push(vec![item.amount.clone()]);
            table_row(&mut l, doc, &cols, &cells, tpl.show_line_numbers, i, m);
        }
    }

    // Totals, kept together.
    let rows: Vec<&TotalRow> = doc.totals.iter().filter(|r| tpl.show_payments || !r.payment).collect();
    l.y += 8.0;
    let total_lead = m.body + 5.5;
    l.ensure(rows.len() as f32 * (total_lead + 2.0) + 12.0, doc);
    let label_x = right - 250.0;
    for row in rows {
        if row.grand {
            let size = m.body + 1.5;
            l.y += 4.0;
            if modern {
                l.push(Op::Rect { x: label_x, y: l.y, w: right - label_x, h: size + 8.0, fill: Some(tint(accent, 0.85)), stroke: None, line_width: 0.0 });
            } else {
                l.line(label_x, right, l.y, accent, 1.0);
            }
            l.y += size + 6.0;
            let label = l.fit(&row.label, true, size, &row.value, right - label_x);
            l.text(label_x + CELL_PAD, l.y - 3.0, &label, true, size, INK, Align::Left);
            l.text(right - CELL_PAD, l.y - 3.0, &row.value, true, size, INK, Align::Right);
        } else {
            l.y += total_lead;
            let label = l.fit(&row.label, false, m.body, &row.value, right - label_x);
            l.text(label_x + CELL_PAD, l.y - 3.0, &label, false, m.body, INK, Align::Left);
            l.text(right - CELL_PAD, l.y - 3.0, &row.value, false, m.body, INK, Align::Right);
        }
    }
    l.y += 18.0;

    // Statements: the amount due by age.
    if let (Some(statement), true) = (&doc.statement, tpl.show_aging) {
        let w = (right - MARGIN) / 5.0;
        let labels = ["Current", "1–30 days past due", "31–60 days", "61–90 days", "Over 90 days"];
        let cols: Vec<Col> = labels.iter().enumerate().map(|(i, label)| Col { x: MARGIN + i as f32 * w, w, label: (*label).into(), align: Align::Right }).collect();
        l.ensure(m.header_h + m.leading + m.row_pad, doc);
        table_header(&mut l, &cols, m);
        let cells: Vec<Vec<String>> = statement.aging.iter().map(|a| vec![a.clone()]).collect();
        table_row(&mut l, doc, &cols, &cells, false, 0, m);
        l.y += 12.0;
    }

    // Memo, message and terms.
    let body = m.body;
    let leading = m.leading;
    let paragraph = |l: &mut Layout, label: Option<&str>, text: &str, color: Rgb| {
        let lines = l.wrap(text, false, body, right - MARGIN);
        if lines.iter().all(|s| s.is_empty()) {
            return;
        }
        if let Some(label) = label {
            l.ensure(leading * 2.0, doc);
            l.y += 10.0;
            l.text(MARGIN, l.y, label, true, 7.5, label_color, Align::Left);
            l.y += 3.0;
        }
        for line in lines {
            l.ensure(leading, doc);
            l.y += leading;
            l.text(MARGIN, l.y, &line, false, body, color, Align::Left);
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
        l.ensure(leading * 2.0, doc);
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

/// The document to print: `document_id` is the invoice, estimate or sales receipt, or for a
/// statement the customer (with `period`, last month by default). Without one, the sample.
async fn document(pool: &SqlitePool, kind: FormKind, document_id: Option<i64>, period: Option<Period>, symbol: &str) -> Result<FormDoc, String> {
    match (kind, document_id) {
        (_, None) => Ok(sample_doc(kind, symbol)),
        (FormKind::Invoice, Some(id)) => invoice_doc(pool, id, symbol).await,
        (FormKind::Estimate, Some(id)) => estimate_doc(pool, id, symbol).await,
        (FormKind::SalesReceipt, Some(id)) => sales_receipt_doc(pool, id, symbol).await,
        (FormKind::Statement, Some(id)) => statement_doc(pool, id, period.unwrap_or_else(|| Period::last_month(chrono::Local::now().date_naive())), symbol).await,
    }
}

/// "Invoice 1001", or for a statement "Statement 2026-10-31 · Acme".
fn form_title(tpl: &FormTemplate, doc: &FormDoc) -> String {
    match doc.kind {
        FormKind::Statement => format!("{} {} · {}", tpl.title, doc.number, doc.customer_name),
        _ => format!("{} {}", tpl.title, doc.number),
    }
}

/// Lays out a document (or the sample, without one) with `template`, or else the template of the
/// customer's form style. `with_image` also decodes the logo for a PDF; the screen needs only its
/// size.
pub async fn render(pool: &SqlitePool, kind: FormKind, document_id: Option<i64>, period: Option<Period>, template: Option<FormTemplate>, with_image: bool) -> Result<Rendered, String> {
    let company = load_company(pool).await?;
    let doc = document(pool, kind, document_id, period, &company.currency_symbol).await?;
    let tpl = match template {
        Some(t) => t.validated(kind)?,
        None => get_template(pool, Some(customer_style(pool, doc.customer_id).await?), kind).await?,
    };
    let logo = company.logo.as_deref().filter(|_| tpl.logo != LogoPlacement::Hidden);
    let (image, size) = if with_image {
        let image = decode_logo(logo);
        let size = image.as_ref().map(|i| (i.width, i.height));
        (image, size)
    } else {
        (None, logo_bytes(logo).as_deref().and_then(pdf::image_size))
    };
    let pages = layout(&doc, &company, &tpl, size);
    let title = form_title(&tpl, &doc);
    Ok(Rendered { form: RenderedForm { title, pages, logo: size.and(company.logo), customer_email: doc.customer_email }, image })
}


/// [`pdf_file`] for a statement period.
pub async fn pdf_file_for(pool: &SqlitePool, kind: FormKind, document_id: i64, period: Option<Period>) -> Result<(String, Vec<u8>), String> {
    let rendered = render(pool, kind, Some(document_id), period, None, true).await?;
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

/// The email a form is sent with; `period` is for statements (see `document`).
pub async fn email_draft(pool: &SqlitePool, kind: FormKind, document_id: i64, period: Option<Period>) -> Result<EmailDraft, String> {
    let company = load_company(pool).await?;
    let doc = document(pool, kind, Some(document_id), period, &company.currency_symbol).await?;
    let tpl = get_template(pool, Some(customer_style(pool, doc.customer_id).await?), kind).await?;
    Ok(EmailDraft {
        to: doc.customer_email.clone().unwrap_or_default(),
        subject: fill_placeholders(&tpl.email_subject, &doc, &company),
        body: fill_placeholders(&tpl.email_body, &doc, &company),
        file_name: file_name(&form_title(&tpl, &doc)),
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

        assert_eq!(get_template(&pool, None, FormKind::Invoice).await.unwrap(), FormTemplate::default_for(FormKind::Invoice));
        let custom = FormTemplate { title: "Statement of Work".into(), show_customer_email: true, ..FormTemplate::default_for(FormKind::Invoice) };
        save_template(&pool, None, FormKind::Invoice, custom.clone()).await.unwrap();
        assert_eq!(get_template(&pool, None, FormKind::Invoice).await.unwrap().title, "Statement of Work");
        // Templates are per form type.
        assert_eq!(get_template(&pool, None, FormKind::Estimate).await.unwrap().title, "Estimate");

        let rendered = render(&pool, FormKind::Invoice, Some(invoice.id), None, None, false).await.unwrap();
        assert_eq!(rendered.form.title, format!("Statement of Work {}", invoice.invoice_number));
        assert_eq!(rendered.form.customer_email.as_deref(), Some("ap@initech.test"));
        let all = texts(&rendered.form.pages);
        for expected in ["Initech", "9 Elm St", "Austin, TX", "ap@initech.test", "1.5", "$100.00", "$150.00", "Net 30"] {
            assert!(all.iter().any(|t| t == expected), "missing {expected:?} in {all:?}");
        }

        let (name, bytes) = pdf_file_for(&pool, FormKind::Invoice, invoice.id, None).await.unwrap();
        assert_eq!(name, format!("Statement of Work {}.pdf", invoice.invoice_number));
        assert!(bytes.starts_with(b"%PDF-1.4"));

        let draft = email_draft(&pool, FormKind::Invoice, invoice.id, None).await.unwrap();
        assert_eq!(draft.to, "ap@initech.test");
        assert_eq!(draft.subject, format!("Invoice {} from {}", invoice.invoice_number, load_company(&pool).await.unwrap().name));
        assert!(draft.body.starts_with("Dear Initech,\n\nPlease find attached invoice"), "{}", draft.body);
        assert!(draft.body.contains("$150.00, due 10/31/2026"));
        assert_eq!(draft.file_name, format!("Statement of Work {}.pdf", invoice.invoice_number));

        assert_eq!(reset_template(&pool, None, FormKind::Invoice).await.unwrap().title, "Invoice");
        assert_eq!(get_template(&pool, None, FormKind::Invoice).await.unwrap().title, "Invoice");
        assert!(render(&pool, FormKind::Estimate, Some(999), None, None, false).await.is_err());
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_name("Invoice 1001"), "Invoice 1001.pdf");
        assert_eq!(file_name("A/B: \"x\""), "A-B- -x-.pdf");
    }

    #[test]
    fn every_layout_preset_renders_every_form_on_one_page() {
        let company = Company { name: "Acme Studio".into(), address: "1 Loop Rd\nSpringfield".into(), currency_symbol: "$".into(), ..Company::default() };
        for layout_preset in [FormLayout::Classic, FormLayout::Modern, FormLayout::Compact] {
            for kind in FormKind::ALL {
                let tpl = FormTemplate { layout: layout_preset, footer: "Pay by ACH".into(), ..FormTemplate::default_for(kind) };
                let pages = layout(&sample_doc(kind, "$"), &company, &tpl, Some((400, 100)));
                assert_eq!(pages.len(), 1, "{layout_preset:?} {kind:?}");
                let all = texts(&pages);
                assert!(all.iter().any(|t| t == &tpl.title), "{layout_preset:?} {kind:?} has its title");
                assert!(all.iter().any(|t| t == "Pay by ACH"));
                // The Modern preset puts the title in a filled accent band.
                let band = pages[0].ops.iter().any(|op| matches!(op, Op::Rect { y, h, .. } if *y == MARGIN && *h == 46.0));
                assert_eq!(band, layout_preset == FormLayout::Modern);
            }
        }
        // Compact fits more lines on a page than Classic.
        let mut doc = sample_doc(FormKind::Invoice, "$");
        doc.lines = (0..60).map(|i| FormLine { description: format!("Line {i}"), quantity: "1".into(), rate: "$1.00".into(), amount: "$1.00".into() }).collect();
        let pages = |preset| layout(&doc, &company, &FormTemplate { layout: preset, ..FormTemplate::default_for(FormKind::Invoice) }, None).len();
        assert!(pages(FormLayout::Compact) < pages(FormLayout::Classic));
    }

    #[test]
    fn statements_print_transactions_and_aging() {
        let company = Company { name: "Acme".into(), currency_symbol: "$".into(), ..Company::default() };
        let doc = sample_doc(FormKind::Statement, "$");
        let tpl = FormTemplate::default_for(FormKind::Statement);
        let all = texts(&layout(&doc, &company, &tpl, None));
        for expected in ["Statement", "10/01/2026 – 10/31/2026", "STATEMENT FOR", "Balance forward", "Invoice #1001", "Payment #2045", "Amount due", "Over 90 days"] {
            assert!(all.iter().any(|t| t == expected), "missing {expected:?} in {all:?}");
        }
        let all = texts(&layout(&doc, &company, &FormTemplate { show_aging: false, ..tpl }, None));
        assert!(!all.iter().any(|t| t == "Over 90 days"));
    }

    #[tokio::test]
    async fn styles_hold_their_own_templates_and_follow_the_customer() {
        let pool = test_pool().await;
        let styles = list_styles(&pool).await.unwrap();
        assert_eq!((styles.len(), styles[0].name.as_str(), styles[0].is_default), (1, "Standard", true));
        let standard = styles[0].id;
        save_template(&pool, None, FormKind::Invoice, FormTemplate { title: "Tax Invoice".into(), ..FormTemplate::default_for(FormKind::Invoice) }).await.unwrap();

        let modern = create_style(&pool, "Modern", Some(standard)).await.unwrap();
        assert!(!modern.is_default);
        // A copy starts with the source's templates; then it changes on its own.
        assert_eq!(get_template(&pool, Some(modern.id), FormKind::Invoice).await.unwrap().title, "Tax Invoice");
        save_template(&pool, Some(modern.id), FormKind::Invoice, FormTemplate { layout: FormLayout::Modern, title: "Bill".into(), ..FormTemplate::default_for(FormKind::Invoice) }).await.unwrap();
        assert_eq!(get_template(&pool, None, FormKind::Invoice).await.unwrap().title, "Tax Invoice");
        assert!(create_style(&pool, "modern", None).await.unwrap_err().starts_with("FORM_STYLE_DUPLICATE"));
        assert!(create_style(&pool, " ", None).await.unwrap_err().starts_with("FORM_STYLE_NAME"));
        assert_eq!(rename_style(&pool, modern.id, "Bold").await.unwrap().name, "Bold");

        let services: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(&pool).await.unwrap();
        let input = |customer: &str| -> InvoiceInput {
            serde_json::from_value(serde_json::json!({ "customerName": customer, "issueDate": "2026-10-01", "dueDate": "2026-10-31",
                "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": 5_000, "incomeAccountId": services }] }))
            .unwrap()
        };
        let styled = create_and_post_invoice(&pool, input("Initech")).await.unwrap();
        let plain = create_and_post_invoice(&pool, input("Globex")).await.unwrap();
        sqlx::query("UPDATE customers SET form_style_id = ?1 WHERE name = 'Initech'").bind(modern.id).execute(&pool).await.unwrap();
        let pool_ref = &pool;
        let title = |id| async move { render(pool_ref, FormKind::Invoice, Some(id), None, None, false).await.unwrap().form.title };
        assert_eq!(title(styled.id).await, format!("Bill {}", styled.invoice_number));
        assert_eq!(title(plain.id).await, format!("Tax Invoice {}", plain.invoice_number));
        assert_eq!(email_draft(&pool, FormKind::Invoice, styled.id, None).await.unwrap().file_name, format!("Bill {}.pdf", styled.invoice_number));

        assert!(delete_style(&pool, standard).await.unwrap_err().starts_with("FORM_STYLE_DEFAULT"));
        set_default_style(&pool, modern.id).await.unwrap();
        assert_eq!(title(plain.id).await, format!("Bill {}", plain.invoice_number));
        set_default_style(&pool, standard).await.unwrap();
        assert_eq!(list_styles(&pool).await.unwrap().iter().find(|s| s.id == modern.id).unwrap().customer_count, 1);
        delete_style(&pool, modern.id).await.unwrap();
        // Its customers fall back to the default style.
        assert_eq!(title(styled.id).await, format!("Tax Invoice {}", styled.invoice_number));
        assert_eq!(list_styles(&pool).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn sales_receipts_and_statements_render_from_the_books() {
        let pool = test_pool().await;
        let account = |n: &'static str| {
            let pool = pool.clone();
            async move { sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(n).fetch_one(&pool).await.unwrap() }
        };
        let (services, checking) = (account("4100").await, account("1010").await);
        let receipt = crate::sales::post_sales_receipt(
            &pool,
            serde_json::from_value(serde_json::json!({ "customerName": "Initech", "receiptDate": "2026-10-05", "depositAccountId": checking,
                "paymentMethod": "CHECK", "reference": "2045", "memo": "Thanks!",
                "lines": [{ "description": "Workshop", "quantityE4": 20_000, "rateCents": 7_500, "incomeAccountId": services }] }))
            .unwrap(),
        )
        .await
        .unwrap();
        let rendered = render(&pool, FormKind::SalesReceipt, Some(receipt.id), None, None, false).await.unwrap();
        assert_eq!(rendered.form.title, format!("Sales Receipt {}", receipt.receipt_number));
        let all = texts(&rendered.form.pages);
        for expected in ["SOLD TO", "Initech", "Check #2045", "Workshop", "2", "$150.00", "Thanks!"] {
            assert!(all.iter().any(|t| t == expected), "missing {expected:?} in {all:?}");
        }

        let invoice: InvoiceInput = serde_json::from_value(serde_json::json!({ "customerName": "Initech", "issueDate": "2026-10-01", "dueDate": "2026-10-31",
            "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": 40_000, "incomeAccountId": services }] }))
        .unwrap();
        let invoice = create_and_post_invoice(&pool, invoice).await.unwrap();
        let customer = invoice.customer_id.unwrap();
        let period = Period { start_date: NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(), end_date: NaiveDate::from_ymd_opt(2026, 10, 31).unwrap() };
        let rendered = render(&pool, FormKind::Statement, Some(customer), Some(period), None, false).await.unwrap();
        assert_eq!(rendered.form.title, "Statement 2026-10-31 · Initech");
        let all = texts(&rendered.form.pages);
        for expected in ["10/01/2026 – 10/31/2026", "Balance forward", &format!("Invoice #{}", invoice.invoice_number), "$400.00", "Amount due"] {
            assert!(all.iter().any(|t| t == expected), "missing {expected:?} in {all:?}");
        }
        sqlx::query("UPDATE customers SET email = 'ap@initech.test' WHERE id = ?1").bind(customer).execute(&pool).await.unwrap();
        let receipt_mail = email_draft(&pool, FormKind::SalesReceipt, receipt.id, None).await.unwrap();
        assert_eq!((receipt_mail.to.as_str(), receipt_mail.file_name.clone()), ("ap@initech.test", format!("Sales Receipt {}.pdf", receipt.receipt_number)));
        let statement_mail = email_draft(&pool, FormKind::Statement, customer, Some(period)).await.unwrap();
        assert!(statement_mail.body.contains("The amount due is $400.00"), "{}", statement_mail.body);
        assert_eq!(statement_mail.file_name, "Statement 2026-10-31 · Initech.pdf");
        let (name, bytes) = pdf_file_for(&pool, FormKind::Statement, customer, Some(period)).await.unwrap();
        assert_eq!(name, "Statement 2026-10-31 · Initech.pdf");
        assert!(bytes.starts_with(b"%PDF-1.4"));
        let backwards = Period { start_date: period.end_date, end_date: period.start_date };
        assert!(render(&pool, FormKind::Statement, Some(customer), Some(backwards), None, false).await.is_err());
        assert_eq!(Period::last_month(NaiveDate::from_ymd_opt(2026, 3, 15).unwrap()), Period { start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(), end_date: NaiveDate::from_ymd_opt(2026, 2, 28).unwrap() });
    }
}
