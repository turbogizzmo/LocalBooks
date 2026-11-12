//! Vouchers as PDF files: bills, checks, expenses, credits, deposits and journal entries, drawn as
//! their printed voucher is (`src/components/VoucherPrint.jsx`): the header the screen gives, and
//! the ledger lines the entry posted. The PDF is saved, or emailed to the vendor or customer named
//! on it through the same mail app and SMTP paths as customer forms (`commands/email.rs`).

#![allow(clippy::float_arithmetic, reason = "page layout in points; amounts arrive already formatted from cents")]

use serde::Deserialize;
use sqlx::SqlitePool;

use crate::forms::{self, Company};
use crate::ledger::{self, JournalLineView};
use crate::pdf::{self, Align, FontFamily, Op, Page, Rgb};

/// The voucher's header, as the screen shows it (`TransactionVoucher`'s `doc`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherDoc {
    pub entry_id: i64,
    /// "Bill", "Check", "Deposit", ...
    pub title: String,
    pub number: Option<String>,
    pub date: String,
    pub due_date: Option<String>,
    /// The vendor, customer or payee named on it.
    pub name: Option<String>,
    /// What the name is ("Vendor", "Received from"); "Name" when empty.
    pub name_label: Option<String>,
    pub memo: Option<String>,
    pub amount_cents: Option<i64>,
    pub currency_code: Option<String>,
    #[serde(default)]
    pub voided: bool,
    pub footer: Option<String>,
}

const WIDTH: f32 = 612.0;
const HEIGHT: f32 = 792.0;
const MARGIN: f32 = 54.0;
const INK: Rgb = Rgb(30, 30, 30);
const MUTED: Rgb = Rgb(110, 110, 110);
const RULE: Rgb = Rgb(190, 190, 190);

/// "Bill 1043 · Acme Supply": the PDF's title, and its file name with ".pdf".
pub fn title(doc: &VoucherDoc) -> String {
    let mut title = doc.title.trim().to_string();
    if let Some(n) = doc.number.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        title = format!("{title} {n}");
    }
    match doc.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => format!("{title} · {name}"),
        None => title,
    }
}

struct Pen {
    pages: Vec<Page>,
    y: f32,
}

impl Pen {
    fn page(&mut self) -> &mut Page {
        self.pages.last_mut().expect("a page")
    }

    fn text(&mut self, x: f32, text: &str, bold: bool, size: f32, color: Rgb, align: Align) {
        let y = self.y;
        self.page().ops.push(Op::Text { x, y, text: pdf::sanitize(text), family: FontFamily::Helvetica, bold, size, color, align });
    }

    fn rule(&mut self, width: f32) {
        let y = self.y;
        self.page().ops.push(Op::Line { x1: MARGIN, y1: y, x2: WIDTH - MARGIN, y2: y, color: RULE, width });
    }

    /// Starts a new page when fewer than `needed` points are left above the bottom margin.
    fn room(&mut self, needed: f32, continued: &str) {
        if self.y + needed > HEIGHT - MARGIN {
            self.pages.push(Page { width: WIDTH, height: HEIGHT, ops: Vec::new() });
            self.y = MARGIN + 10.0;
            self.text(MARGIN, continued, false, 9.0, MUTED, Align::Left);
            self.y += 18.0;
        }
    }
}

/// Lays out the voucher on US Letter pages.
pub fn layout(doc: &VoucherDoc, company: &Company, lines: &[JournalLineView]) -> Result<Vec<Page>, String> {
    let money = |cents: i64| forms::format_money(cents, None, &company.currency_symbol);
    let mut pen = Pen { pages: vec![Page { width: WIDTH, height: HEIGHT, ops: Vec::new() }], y: MARGIN + 12.0 };

    // Company on the left, the voucher's title on the right.
    pen.text(MARGIN, &company.name, true, 14.0, INK, Align::Left);
    pen.text(WIDTH - MARGIN, &doc.title.to_uppercase(), true, 16.0, INK, Align::Right);
    for detail in
        [company.address.as_str(), company.phone.as_str(), company.email.as_str()].into_iter().flat_map(|d| d.lines()).filter(|d| !d.trim().is_empty())
    {
        pen.y += 12.0;
        pen.text(MARGIN, detail.trim(), false, 9.0, MUTED, Align::Left);
    }
    if doc.voided {
        pen.y += 14.0;
        pen.text(WIDTH - MARGIN, "VOID", true, 14.0, Rgb(180, 30, 30), Align::Right);
    }
    pen.y += 28.0;

    // Who it names, its memo, and the number, date, due date and amount.
    let top = pen.y;
    if let Some(name) = doc.name.as_deref().filter(|n| !n.trim().is_empty()) {
        pen.text(MARGIN, doc.name_label.as_deref().filter(|l| !l.trim().is_empty()).unwrap_or("Name"), true, 8.0, MUTED, Align::Left);
        pen.y += 13.0;
        pen.text(MARGIN, name, true, 11.0, INK, Align::Left);
        pen.y += 14.0;
    }
    if let Some(memo) = doc.memo.as_deref().filter(|m| !m.trim().is_empty()) {
        for line in pdf::wrap(FontFamily::Helvetica, false, 9.0, memo, 300.0) {
            pen.text(MARGIN, &line, false, 9.0, INK, Align::Left);
            pen.y += 12.0;
        }
    }
    let left_bottom = pen.y;
    pen.y = top;
    let meta = [
        doc.number.as_deref().filter(|n| !n.trim().is_empty()).map(|n| ("No.", n.to_string())),
        Some(("Date", forms::format_date(&doc.date))),
        doc.due_date.as_deref().map(|d| ("Due", forms::format_date(d))),
        doc.amount_cents.map(|c| ("Amount", forms::format_money(c, doc.currency_code.as_deref(), &company.currency_symbol))),
    ];
    for (label, value) in meta.into_iter().flatten() {
        pen.text(WIDTH - MARGIN - 130.0, label, true, 9.0, MUTED, Align::Left);
        pen.text(WIDTH - MARGIN, &value, false, 9.0, INK, Align::Right);
        pen.y += 13.0;
    }
    pen.y = pen.y.max(left_bottom) + 16.0;

    // The lines it posted.
    let (account_x, memo_x, debit_x, credit_x) = (MARGIN, MARGIN + 190.0, WIDTH - MARGIN - 85.0, WIDTH - MARGIN);
    let head = |pen: &mut Pen| {
        pen.text(account_x, "Account", true, 9.0, INK, Align::Left);
        pen.text(memo_x, "Memo", true, 9.0, INK, Align::Left);
        pen.text(debit_x, "Debit", true, 9.0, INK, Align::Right);
        pen.text(credit_x, "Credit", true, 9.0, INK, Align::Right);
        pen.y += 5.0;
        pen.rule(0.8);
        pen.y += 13.0;
    };
    head(&mut pen);
    let continued = format!("{} (continued)", title(doc));
    for l in lines {
        let account = match &l.class_name {
            Some(class) => format!("{} · {class}", l.account_name),
            None => l.account_name.clone(),
        };
        let account_lines = pdf::wrap(FontFamily::Helvetica, false, 9.0, &account, 180.0);
        let memo_lines = pdf::wrap(FontFamily::Helvetica, false, 9.0, l.memo.as_deref().unwrap_or(""), debit_x - memo_x - 70.0);
        let rows = account_lines.len().max(memo_lines.len()).max(1);
        pen.room(rows as f32 * 12.0 + 6.0, &continued);
        let start = pen.y;
        for (i, text) in account_lines.iter().enumerate() {
            pen.y = start + i as f32 * 12.0;
            pen.text(account_x, text, false, 9.0, INK, Align::Left);
        }
        for (i, text) in memo_lines.iter().enumerate() {
            pen.y = start + i as f32 * 12.0;
            pen.text(memo_x, text, false, 9.0, INK, Align::Left);
        }
        pen.y = start;
        if l.debit_cents != 0 {
            pen.text(debit_x, &money(l.debit_cents), false, 9.0, INK, Align::Right);
        }
        if l.credit_cents != 0 {
            pen.text(credit_x, &money(l.credit_cents), false, 9.0, INK, Align::Right);
        }
        pen.y = start + rows as f32 * 12.0 + 4.0;
    }
    pen.room(30.0, &continued);
    pen.y -= 8.0;
    pen.rule(0.8);
    pen.y += 13.0;
    let total = |side: fn(&JournalLineView) -> i64| {
        lines.iter().try_fold(0i64, |sum, l| sum.checked_add(side(l))).ok_or("VOUCHER_TOO_LARGE: the voucher's lines add up to more than can be shown")
    };
    let (debits, credits) = (total(|l| l.debit_cents)?, total(|l| l.credit_cents)?);
    pen.text(account_x, "Total", true, 9.0, INK, Align::Left);
    pen.text(debit_x, &money(debits), true, 9.0, INK, Align::Right);
    pen.text(credit_x, &money(credits), true, 9.0, INK, Align::Right);
    if let Some(footer) = doc.footer.as_deref().filter(|f| !f.trim().is_empty()) {
        pen.y += 24.0;
        for line in pdf::wrap(FontFamily::Helvetica, false, 9.0, footer, WIDTH - 2.0 * MARGIN) {
            pen.room(12.0, &continued);
            pen.text(MARGIN, &line, false, 9.0, MUTED, Align::Left);
            pen.y += 12.0;
        }
    }
    Ok(pen.pages)
}

/// The voucher's PDF: (file name, bytes).
pub async fn pdf_file(pool: &SqlitePool, doc: &VoucherDoc) -> Result<(String, Vec<u8>), String> {
    let company = forms::load_company(pool).await?;
    let lines = ledger::journal_entry_lines(pool, doc.entry_id).await?;
    if lines.is_empty() {
        return Err("VOUCHER_NOT_FOUND: that transaction has no posted lines".into());
    }
    let title = title(doc);
    let pages = layout(doc, &company, &lines)?;
    Ok((forms::file_name(&title), pdf::write_pdf(&pages, None, &title)))
}

/// The email address of the vendor or customer named on the voucher, or empty. A vendor's is
/// looked for first on purchase vouchers (named "Vendor", "Payee" or "Pay to"), a customer's first
/// otherwise.
pub async fn recipient(pool: &SqlitePool, doc: &VoucherDoc) -> Result<String, String> {
    let Some(name) = doc.name.as_deref().map(str::trim).filter(|n| !n.is_empty()) else { return Ok(String::new()) };
    let vendor_first = matches!(doc.name_label.as_deref().map(str::trim), Some("Vendor" | "Payee" | "Pay to" | "Pay to the order of"));
    let order = if vendor_first { ["vendors", "customers"] } else { ["customers", "vendors"] };
    for table in order {
        let email: Option<Option<String>> = sqlx::query_scalar(&format!("SELECT email FROM {table} WHERE name = ?1 COLLATE NOCASE ORDER BY id LIMIT 1"))
            .bind(name)
            .fetch_optional(pool)
            .await
            .map_err(ledger::db_err)?;
        if let Some(email) = email.flatten().map(|e| e.trim().to_string()).filter(|e| !e.is_empty()) {
            return Ok(email);
        }
    }
    Ok(String::new())
}

/// The email a voucher goes out with, before the user edits it.
pub async fn email_draft(pool: &SqlitePool, doc: &VoucherDoc) -> Result<forms::EmailDraft, String> {
    let company = forms::load_company(pool).await?;
    let title = title(doc);
    let greeting = doc.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map_or_else(|| "Hello,".to_string(), |n| format!("Dear {n},"));
    Ok(forms::EmailDraft {
        to: recipient(pool, doc).await?,
        subject: format!("{title} from {}", company.name),
        body: format!("{greeting}\n\nPlease find attached {} for your records.\n\n{}", title, company.name),
        file_name: forms::file_name(&title),
    })
}

#[cfg(test)]
mod tests;
