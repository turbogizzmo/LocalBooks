//! Lines on a sales form that are not plain items: subtotals and discounts (and, in later steps,
//! percent charges and group headings). The design is `docs/plans/item-types.md`.
//!
//! A form sends its item lines as before, plus `formLines`, each placed by `afterLines`: how many
//! item lines come before it. [`lay_out`] merges the two into line order, numbers every line and
//! works out each form line's amount. It is pure, and every sales writer calls it before writing
//! anything, so the form, the stored lines and the posted entry agree.
//!
//! Item lines stay in the document's item table; form lines go in `<document>_form_lines`, sharing
//! the line numbering.

use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, Transaction};

use crate::ledger::{db_err, mul_div_round, JournalLine, MAX_LINE_CENTS};
use crate::sales_lists::SalesDoc;

/// What a form line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FormLineKind {
    /// The sum of the lines since the previous subtotal (or the top of the form). Posts nothing.
    Subtotal,
    /// An amount, or a percent of the line above, taken off; debited to its account.
    Discount,
}

impl FormLineKind {
    fn key(self) -> &'static str {
        match self {
            FormLineKind::Subtotal => "SUBTOTAL",
            FormLineKind::Discount => "DISCOUNT",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        match key {
            "SUBTOTAL" => Some(FormLineKind::Subtotal),
            "DISCOUNT" => Some(FormLineKind::Discount),
            _ => None,
        }
    }
}

/// A subtotal (or another non-item line) as a form sends it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FormLineInput {
    /// How many of the form's item lines come before this line.
    pub after_lines: usize,
    pub kind: FormLineKind,
    /// The text printed on the line; a subtotal defaults to "Subtotal", a discount to "Discount".
    #[serde(default)]
    pub description: String,
    /// Discount: the discount item it came from.
    #[serde(default)]
    pub product_service_id: Option<i64>,
    /// Discount: a percentage of the line above, in parts per million.
    #[serde(default)]
    pub percent_ppm: Option<i64>,
    /// Discount: the amount taken off (positive), when it is not a percentage.
    #[serde(default)]
    pub amount_cents: Option<i64>,
    /// Discount: the account debited.
    #[serde(default)]
    pub account_id: Option<i64>,
    #[serde(default)]
    pub class_id: Option<i64>,
    /// Discount: taken off the taxable amount too.
    #[serde(default)]
    pub is_taxable: bool,
}

impl FormLineInput {
    fn blank(after_lines: usize, kind: FormLineKind) -> Self {
        FormLineInput {
            after_lines,
            kind,
            description: String::new(),
            product_service_id: None,
            percent_ppm: None,
            amount_cents: None,
            account_id: None,
            class_id: None,
            is_taxable: false,
        }
    }

    pub fn subtotal(after_lines: usize) -> Self {
        Self::blank(after_lines, FormLineKind::Subtotal)
    }

    /// A discount of a percentage (`percent_ppm`) or an amount off the line above.
    pub fn discount(after_lines: usize, account_id: i64, percent_ppm: Option<i64>, amount_cents: Option<i64>, is_taxable: bool) -> Self {
        FormLineInput { account_id: Some(account_id), percent_ppm, amount_cents, is_taxable, ..Self::blank(after_lines, FormLineKind::Discount) }
    }

    fn text(&self) -> String {
        match (self.description.trim(), self.kind) {
            ("", FormLineKind::Subtotal) => "Subtotal".to_string(),
            ("", FormLineKind::Discount) => "Discount".to_string(),
            (d, _) => d.to_string(),
        }
    }
}

/// An item line's amount and whether it is taxed, as [`lay_out`] needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemAmount {
    pub amount_cents: i64,
    pub is_taxable: bool,
}

/// One line of the laid-out form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Laid {
    /// The item line at this index of the form's `lines`.
    Item(usize),
    /// The form line at this index of `formLines`, with its amount.
    Form { index: usize, amount_cents: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Every line in order, with its line number (from 1).
    pub rows: Vec<(i64, Laid)>,
    /// Item lines plus charges less line discounts: what the document discount comes off.
    pub lines_cents: i64,
    /// The taxable part of `lines_cents`.
    pub taxable_cents: i64,
}

impl Layout {
    /// The line number of each item line, by its index in `lines`.
    pub fn item_numbers(&self) -> Vec<i64> {
        let mut numbers = vec![0; self.rows.iter().filter(|(_, l)| matches!(l, Laid::Item(_))).count()];
        for (n, laid) in &self.rows {
            if let Laid::Item(i) = laid {
                numbers[*i] = *n;
            }
        }
        numbers
    }

    /// The form lines with their numbers and amounts, in line order.
    pub fn form_rows(&self) -> impl Iterator<Item = (i64, usize, i64)> + '_ {
        self.rows.iter().filter_map(|(n, l)| match l {
            Laid::Form { index, amount_cents } => Some((*n, *index, *amount_cents)),
            Laid::Item(_) => None,
        })
    }
}

fn add(a: i64, b: i64, code: &str) -> Result<i64, String> {
    a.checked_add(b)
        .filter(|t| t.abs() <= MAX_LINE_CENTS)
        .ok_or_else(|| format!("{code}_AMOUNT_TOO_LARGE: the form's lines add up to more than the largest amount"))
}

/// Merges item lines and form lines into line order and works out each form line's amount.
///
/// A subtotal sums the lines since the previous subtotal. A discount comes off "the line above":
/// an item line or a subtotal (not another discount), and never more than that line's amount.
pub fn lay_out(items: &[ItemAmount], form: &[FormLineInput], code: &str) -> Result<Layout, String> {
    if let Some(f) = form.iter().find(|f| f.after_lines > items.len()) {
        return Err(format!("{code}_BAD_FORM_LINE: a line is placed after line {} of {} item lines", f.after_lines, items.len()));
    }
    let mut rows = Vec::with_capacity(items.len() + form.len());
    let mut number = 0i64;
    let mut since_subtotal = 0i64;
    let mut lines_cents = 0i64;
    let mut taxable_cents = 0i64;
    // The amount of the line above, when a discount may apply to it.
    let mut base: Option<i64> = None;
    for position in 0..=items.len() {
        for (index, f) in form.iter().enumerate().filter(|(_, f)| f.after_lines == position) {
            number += 1;
            let amount_cents = match f.kind {
                FormLineKind::Subtotal => {
                    let sum = since_subtotal;
                    since_subtotal = 0;
                    base = Some(sum);
                    sum
                }
                FormLineKind::Discount => {
                    let n = number;
                    let above = base.take().filter(|b| *b > 0).ok_or_else(|| {
                        format!("{code}_DISCOUNT_NEEDS_BASE: the discount on line {n} must come right after an item or a subtotal")
                    })?;
                    let off = match (f.percent_ppm, f.amount_cents) {
                        (Some(ppm), None) if (1..=1_000_000).contains(&ppm) => mul_div_round(above, ppm, 1_000_000).unwrap_or(0),
                        (None, Some(cents)) if cents > 0 => cents,
                        _ => return Err(format!("{code}_BAD_DISCOUNT_AMOUNT: the discount on line {n} needs an amount above zero or a percentage up to 100%")),
                    };
                    if off < 1 {
                        return Err(format!("{code}_DISCOUNT_ROUNDS_TO_ZERO: the discount on line {n} rounds to 0.00"));
                    }
                    if off > above {
                        return Err(format!("{code}_DISCOUNT_TOO_LARGE: the discount on line {n} is more than the line above it"));
                    }
                    since_subtotal -= off;
                    lines_cents -= off;
                    if f.is_taxable {
                        taxable_cents -= off;
                    }
                    -off
                }
            };
            rows.push((number, Laid::Form { index, amount_cents }));
        }
        if let Some(item) = items.get(position) {
            number += 1;
            since_subtotal = add(since_subtotal, item.amount_cents, code)?;
            lines_cents = add(lines_cents, item.amount_cents, code)?;
            if item.is_taxable {
                taxable_cents = add(taxable_cents, item.amount_cents, code)?;
            }
            base = Some(item.amount_cents);
            rows.push((number, Laid::Item(position)));
        }
    }
    if taxable_cents < 0 {
        return Err(format!("{code}_TAXABLE_NEGATIVE: the taxable discounts are more than the taxable lines; mark a discount not taxable"));
    }
    Ok(Layout { rows, lines_cents, taxable_cents })
}

/// "10%" or "2.5%" from parts per million, as a form prints a percent discount or charge.
pub fn percent_text(ppm: i64) -> String {
    let whole = ppm / 10_000;
    let frac = ppm % 10_000;
    if frac == 0 {
        format!("{whole}%")
    } else {
        format!("{whole}.{}%", format!("{frac:04}").trim_end_matches('0'))
    }
}

fn table(doc: SalesDoc) -> (&'static str, &'static str) {
    match doc {
        SalesDoc::Invoice => ("invoice_form_lines", "invoice_id"),
        SalesDoc::SalesReceipt => ("sales_receipt_form_lines", "sales_receipt_id"),
        SalesDoc::Estimate => ("estimate_form_lines", "estimate_id"),
    }
}

/// Checks what the form lines refer to: a discount's account is an active income account, its
/// item (if any) a discount item, and its class usable.
pub(crate) async fn check_form_lines(tx: &mut Transaction<'_, Sqlite>, form: &[FormLineInput], layout: &Layout, code: &str) -> Result<(), String> {
    for (n, index, _) in layout.form_rows() {
        let f = &form[index];
        if f.kind != FormLineKind::Discount {
            continue;
        }
        let account = f.account_id.ok_or_else(|| format!("{code}_DISCOUNT_NO_ACCOUNT: choose the account the discount on line {n} posts to"))?;
        let found: Option<(String, bool)> =
            sqlx::query_as("SELECT t.classification, a.is_active FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1")
                .bind(account)
                .fetch_optional(&mut **tx)
                .await
                .map_err(db_err)?;
        match found {
            Some((classification, true)) if classification == "INCOME" => {}
            Some((_, false)) => return Err(format!("{code}_INACTIVE_ACCOUNT: line {n} uses an inactive account")),
            _ => return Err(format!("{code}_NOT_INCOME_ACCOUNT: the discount on line {n} must use an Income account")),
        }
        if let Some(product) = f.product_service_id {
            let item_type: Option<String> =
                sqlx::query_scalar("SELECT item_type FROM products_services WHERE id = ?1").bind(product).fetch_optional(&mut **tx).await.map_err(db_err)?;
            if item_type.as_deref() != Some("DISCOUNT") {
                return Err(format!("{code}_NOT_A_DISCOUNT: the item on line {n} is not a discount item"));
            }
        }
        if let Some(class) = f.class_id {
            crate::classes::check_usable(tx, class, &format!("{code}: line {n}")).await?;
        }
    }
    Ok(())
}

/// The debits the discount lines post: each to its account, with its class.
pub(crate) fn discount_debits(form: &[FormLineInput], layout: &Layout) -> Vec<JournalLine> {
    layout
        .form_rows()
        .filter(|(_, index, _)| form[*index].kind == FormLineKind::Discount)
        .filter_map(|(_, index, amount)| {
            let f = &form[index];
            Some(JournalLine::debit(f.account_id?, -amount).with_class(f.class_id).with_memo(Some(&f.text())))
        })
        .collect()
}

/// Writes a document's form lines, after its row is inserted and before it posts. `taxed`: the
/// document has a tax rate (as for item lines, a line is stored taxable only then).
pub(crate) async fn write_form_lines(tx: &mut Transaction<'_, Sqlite>, doc: SalesDoc, id: i64, form: &[FormLineInput], layout: &Layout, taxed: bool) -> Result<(), String> {
    let (table, key) = table(doc);
    for (number, index, amount_cents) in layout.form_rows() {
        let f = &form[index];
        let discount = f.kind == FormLineKind::Discount;
        sqlx::query(&format!(
            "INSERT INTO {table} ({key}, line_number, kind, description, amount_cents, product_service_id, percent_ppm, account_id, class_id, is_taxable)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
        ))
        .bind(id)
        .bind(number)
        .bind(f.kind.key())
        .bind(f.text())
        .bind(amount_cents)
        .bind(f.product_service_id.filter(|_| discount))
        .bind(f.percent_ppm.filter(|_| discount))
        .bind(f.account_id.filter(|_| discount))
        .bind(f.class_id.filter(|_| discount))
        .bind(discount && f.is_taxable && taxed)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    Ok(())
}

/// A stored form line, for showing, printing and loading a document back into its form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct FormLineView {
    pub line_number: i64,
    pub kind: String,
    pub description: String,
    pub amount_cents: i64,
    pub product_service_id: Option<i64>,
    pub percent_ppm: Option<i64>,
    pub account_id: Option<i64>,
    pub class_id: Option<i64>,
    pub is_taxable: bool,
}

/// A document's form lines in line order.
pub async fn form_lines_of<'e, E: sqlx::Executor<'e, Database = Sqlite>>(executor: E, doc: SalesDoc, id: i64) -> Result<Vec<FormLineView>, String> {
    let (table, key) = table(doc);
    sqlx::query_as::<_, FormLineView>(&format!("SELECT line_number, kind, description, amount_cents, product_service_id, percent_ppm, account_id, class_id, is_taxable FROM {table} WHERE {key} = ?1 ORDER BY line_number"))
        .bind(id)
        .fetch_all(executor)
        .await
        .map_err(db_err)
}

/// Stored form lines back as inputs, placed among item lines numbered `item_numbers`.
pub fn inputs_of(views: &[FormLineView], item_numbers: &[i64]) -> Vec<FormLineInput> {
    views
        .iter()
        .filter_map(|v| {
            Some(FormLineInput {
                after_lines: item_numbers.iter().filter(|n| **n < v.line_number).count(),
                kind: FormLineKind::from_key(&v.kind)?,
                description: v.description.clone(),
                product_service_id: v.product_service_id,
                percent_ppm: v.percent_ppm,
                amount_cents: if v.kind == "DISCOUNT" && v.percent_ppm.is_none() { Some(-v.amount_cents) } else { None },
                account_id: v.account_id,
                class_id: v.class_id,
                is_taxable: v.is_taxable,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
