//! Lines on a sales form that are not plain items: subtotals (and, in later steps, discounts,
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

use crate::ledger::{db_err, MAX_LINE_CENTS};
use crate::sales_lists::SalesDoc;

/// What a form line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FormLineKind {
    /// The sum of the lines since the previous subtotal (or the top of the form). Posts nothing.
    Subtotal,
}

impl FormLineKind {
    fn key(self) -> &'static str {
        match self {
            FormLineKind::Subtotal => "SUBTOTAL",
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
    /// The text printed on the line; a subtotal defaults to "Subtotal".
    #[serde(default)]
    pub description: String,
}

impl FormLineInput {
    pub fn subtotal(after_lines: usize) -> Self {
        FormLineInput { after_lines, kind: FormLineKind::Subtotal, description: String::new() }
    }

    fn text(&self) -> String {
        match self.description.trim() {
            "" => "Subtotal".to_string(),
            d => d.to_string(),
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
pub fn lay_out(items: &[ItemAmount], form: &[FormLineInput], code: &str) -> Result<Layout, String> {
    if let Some(f) = form.iter().find(|f| f.after_lines > items.len()) {
        return Err(format!("{code}_BAD_FORM_LINE: a line is placed after line {} of {} item lines", f.after_lines, items.len()));
    }
    let mut rows = Vec::with_capacity(items.len() + form.len());
    let mut number = 0i64;
    let mut since_subtotal = 0i64;
    let mut lines_cents = 0i64;
    let mut taxable_cents = 0i64;
    for position in 0..=items.len() {
        for (index, f) in form.iter().enumerate().filter(|(_, f)| f.after_lines == position) {
            number += 1;
            let amount_cents = match f.kind {
                FormLineKind::Subtotal => {
                    let sum = since_subtotal;
                    since_subtotal = 0;
                    sum
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
            rows.push((number, Laid::Item(position)));
        }
    }
    Ok(Layout { rows, lines_cents, taxable_cents })
}

fn table(doc: SalesDoc) -> (&'static str, &'static str) {
    match doc {
        SalesDoc::Invoice => ("invoice_form_lines", "invoice_id"),
        SalesDoc::SalesReceipt => ("sales_receipt_form_lines", "sales_receipt_id"),
        SalesDoc::Estimate => ("estimate_form_lines", "estimate_id"),
    }
}

/// Writes a document's form lines, after its row is inserted and before it posts.
pub(crate) async fn write_form_lines(tx: &mut Transaction<'_, Sqlite>, doc: SalesDoc, id: i64, form: &[FormLineInput], layout: &Layout) -> Result<(), String> {
    let (table, key) = table(doc);
    for (number, index, amount_cents) in layout.form_rows() {
        let f = &form[index];
        sqlx::query(&format!("INSERT INTO {table} ({key}, line_number, kind, description, amount_cents) VALUES (?1, ?2, ?3, ?4, ?5)"))
            .bind(id)
            .bind(number)
            .bind(f.kind.key())
            .bind(f.text())
            .bind(amount_cents)
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
}

/// A document's form lines in line order.
pub async fn form_lines_of<'e, E: sqlx::Executor<'e, Database = Sqlite>>(executor: E, doc: SalesDoc, id: i64) -> Result<Vec<FormLineView>, String> {
    let (table, key) = table(doc);
    sqlx::query_as::<_, FormLineView>(&format!("SELECT line_number, kind, description, amount_cents FROM {table} WHERE {key} = ?1 ORDER BY line_number"))
        .bind(id)
        .fetch_all(executor)
        .await
        .map_err(db_err)
}

/// Stored form lines back as inputs, placed among item lines numbered `item_numbers`.
pub fn inputs_of(views: &[FormLineView], item_numbers: &[i64]) -> Vec<FormLineInput> {
    views
        .iter()
        .map(|v| FormLineInput {
            after_lines: item_numbers.iter().filter(|n| **n < v.line_number).count(),
            kind: match v.kind.as_str() {
                _ => FormLineKind::Subtotal,
            },
            description: v.description.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests;
