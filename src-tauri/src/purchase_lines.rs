//! Discount and charge lines on the purchase forms (gap T21): bills, checks and expenses, vendor
//! credits and purchase orders. The sales forms' version is `item_types`; the purchase forms are
//! built from account split lines instead of items, so these lines sit beside the split lines.
//!
//! A form sends its split lines as before, plus `formLines`, each placed by `afterLine`: the split
//! line (numbered from 1) it follows and applies to. A line is a fixed amount or a percent of that
//! split line ([`lay_out`], pure, rounding half away from zero). A **discount** comes off the
//! document total and posts the other way from the split lines (on a bill, a credit to its
//! account); a **charge** is added to the total and posts like a split line. Every writer lays the
//! lines out before writing anything, so the form, the stored lines and the posted entry agree.
//!
//! Only one discount or charge follows a split line: a second one would have no split line above
//! it to apply to (`<code>_FORM_LINE_NEEDS_BASE`).

use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, Transaction};

use crate::ledger::{db_err, mul_div_round, JournalLine, MAX_LINE_CENTS};

/// A discount (taken off) or a charge (added on).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PurchaseLineKind {
    Discount,
    Charge,
}

impl PurchaseLineKind {
    fn key(self) -> &'static str {
        match self {
            PurchaseLineKind::Discount => "DISCOUNT",
            PurchaseLineKind::Charge => "CHARGE",
        }
    }

    fn label(self) -> &'static str {
        match self {
            PurchaseLineKind::Discount => "Discount",
            PurchaseLineKind::Charge => "Charge",
        }
    }

    /// The item type a line of this kind may name.
    fn item_type(self) -> &'static str {
        match self {
            PurchaseLineKind::Discount => "DISCOUNT",
            PurchaseLineKind::Charge => "OTHER_CHARGE",
        }
    }
}

/// The document a line belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PurchaseDoc {
    Bill,
    /// A check, an expense or a card credit.
    Expense,
    VendorCredit,
    PurchaseOrder,
}

impl PurchaseDoc {
    fn column(self) -> &'static str {
        match self {
            PurchaseDoc::Bill => "bill_id",
            PurchaseDoc::Expense => "expense_id",
            PurchaseDoc::VendorCredit => "vendor_credit_id",
            PurchaseDoc::PurchaseOrder => "purchase_order_id",
        }
    }
}

/// A discount or charge line as a form sends it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseFormLineInput {
    /// The split line (numbered from 1) this line follows and applies to.
    pub after_line: usize,
    pub kind: PurchaseLineKind,
    /// The text on the line; blank means "Discount" or "Charge".
    #[serde(default)]
    pub description: String,
    /// The Discount or Other Charge item it came from.
    #[serde(default)]
    pub product_service_id: Option<i64>,
    /// A percent of the split line above, in parts per million.
    #[serde(default)]
    pub percent_ppm: Option<i64>,
    /// A fixed amount (positive), when it is not a percent.
    #[serde(default)]
    pub amount_cents: Option<i64>,
    /// The account it posts to.
    pub account_id: i64,
    #[serde(default)]
    pub class_id: Option<i64>,
}

/// A stored line, as the forms load it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PurchaseFormLineView {
    pub after_line: i64,
    pub kind: String,
    pub description: String,
    pub product_service_id: Option<i64>,
    pub percent_ppm: Option<i64>,
    /// What the line came to (always positive).
    pub amount_cents: i64,
    pub account_id: i64,
    pub class_id: Option<i64>,
}

/// Works out each form line's amount from the split lines' amounts (`splits`, in line order), and
/// the document total: the split lines, plus charges, less discounts. `code` prefixes the errors
/// (`BILL`, `EXP`, `VC`, `PO`).
pub fn lay_out(splits: &[i64], form: &[PurchaseFormLineInput], code: &str) -> Result<(Vec<i64>, i64), String> {
    let mut total: i64 = 0;
    for amount in splits {
        total = total.checked_add(*amount).filter(|t| *t <= MAX_LINE_CENTS).ok_or_else(|| format!("{code}_AMOUNT_TOO_LARGE: the total is too large"))?;
    }
    let mut amounts = Vec::with_capacity(form.len());
    let mut placed: Vec<usize> = Vec::with_capacity(form.len());
    for line in form {
        let what = line.kind.label().to_lowercase();
        let after = line.after_line;
        if after == 0 || after > splits.len() {
            return Err(format!("{code}_FORM_LINE_PLACE: a {what} line must follow one of the split lines"));
        }
        if placed.contains(&after) {
            return Err(format!("{code}_FORM_LINE_NEEDS_BASE: split line {after} already has a discount or charge after it; a {what} applies to the split line just above it"));
        }
        placed.push(after);
        let base = splits[after - 1];
        let amount = match (line.percent_ppm, line.amount_cents) {
            (Some(ppm), None) => {
                if !(1..=1_000_000).contains(&ppm) {
                    return Err(format!("{code}_FORM_LINE_PERCENT: a {what} is from 0.0001% to 100%"));
                }
                let amount = mul_div_round(base, ppm, 1_000_000).ok_or_else(|| format!("{code}_AMOUNT_TOO_LARGE: the {what} is too large"))?;
                if amount < 1 {
                    return Err(format!("{code}_FORM_LINE_ZERO: the {what} after split line {after} comes to less than a cent"));
                }
                amount
            }
            (None, Some(cents)) => {
                if !(1..=MAX_LINE_CENTS).contains(&cents) {
                    return Err(format!("{code}_FORM_LINE_AMOUNT: a {what} needs an amount above zero"));
                }
                cents
            }
            _ => return Err(format!("{code}_FORM_LINE_AMOUNT: give a {what} an amount or a percent, not both")),
        };
        if line.kind == PurchaseLineKind::Discount && amount > base {
            return Err(format!("{code}_DISCOUNT_TOO_LARGE: the discount after split line {after} is more than that line"));
        }
        total = match line.kind {
            PurchaseLineKind::Charge => total.checked_add(amount).filter(|t| *t <= MAX_LINE_CENTS),
            PurchaseLineKind::Discount => total.checked_sub(amount),
        }
        .ok_or_else(|| format!("{code}_AMOUNT_TOO_LARGE: the total is too large"))?;
        amounts.push(amount);
    }
    if total < 1 {
        return Err(format!("{code}_TOTAL_NOT_POSITIVE: the discounts take the whole amount off; the total must be above zero"));
    }
    Ok((amounts, total))
}

/// Checks each line's account, item and class: an active income or expense account that isn't a
/// system account; a Discount item for a discount and an Other Charge item for a charge.
pub async fn check(tx: &mut Transaction<'_, Sqlite>, form: &[PurchaseFormLineInput], code: &str) -> Result<(), String> {
    for line in form {
        let what = line.kind.label().to_lowercase();
        let n = line.after_line;
        let account: Option<(String, bool, bool)> = sqlx::query_as(
            "SELECT t.classification, a.is_active, a.is_system FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1",
        )
        .bind(line.account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        match account {
            None => return Err(format!("{code}_UNKNOWN_ACCOUNT: the {what} after split line {n} references an account that does not exist")),
            Some((_, false, _)) => return Err(format!("{code}_INACTIVE_ACCOUNT: the {what} after split line {n} uses an inactive account")),
            Some((class, _, is_system)) if is_system || !matches!(class.as_str(), "INCOME" | "EXPENSE") => {
                return Err(format!("{code}_FORM_LINE_ACCOUNT: the {what} after split line {n} must post to an income or expense account"))
            }
            Some(_) => {}
        }
        if let Some(item) = line.product_service_id {
            let row: Option<(String, bool)> =
                sqlx::query_as("SELECT item_type, is_active FROM products_services WHERE id = ?1").bind(item).fetch_optional(&mut **tx).await.map_err(db_err)?;
            match row {
                Some((kind, true)) if kind == line.kind.item_type() => {}
                Some((_, false)) => return Err(format!("{code}_INACTIVE_ITEM: the {what} after split line {n} uses an inactive item")),
                _ => return Err(format!("{code}_FORM_LINE_ITEM: the {what} after split line {n} must use a {} item", if line.kind == PurchaseLineKind::Discount { "Discount" } else { "Other Charge" })),
            }
        }
        if let Some(class) = line.class_id {
            crate::classes::check_usable(tx, class, &format!("{code}: {what} after split line {n}")).await?;
        }
        if line.description.chars().count() > 4000 {
            return Err(format!("{code}_FORM_LINE_TEXT: the {what} after split line {n} has too long a description"));
        }
    }
    Ok(())
}

fn description(line: &PurchaseFormLineInput) -> &str {
    match line.description.trim() {
        "" => line.kind.label(),
        text => text,
    }
}

/// Stores the lines (`amounts` from [`lay_out`]) on document `id`.
pub async fn write(tx: &mut Transaction<'_, Sqlite>, doc: PurchaseDoc, id: i64, form: &[PurchaseFormLineInput], amounts: &[i64]) -> Result<(), String> {
    let sql = format!(
        "INSERT INTO purchase_form_lines ({}, after_line, kind, description, product_service_id, percent_ppm, amount_cents, account_id, class_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        doc.column()
    );
    for (line, amount) in form.iter().zip(amounts) {
        sqlx::query(&sql)
            .bind(id)
            .bind(i64::try_from(line.after_line).map_err(|_| "FORM_LINE_PLACE: line number out of range".to_string())?)
            .bind(line.kind.key())
            .bind(description(line))
            .bind(line.product_service_id)
            .bind(line.percent_ppm)
            .bind(*amount)
            .bind(line.account_id)
            .bind(line.class_id)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
    }
    Ok(())
}

/// The journal lines for the form lines. `split_debits` says which side the document's split
/// lines post to: true for bills, checks and expenses (debits), false for vendor credits and card
/// credits. A charge posts on the split lines' side, a discount on the other.
pub fn journal_lines(form: &[PurchaseFormLineInput], amounts: &[i64], split_debits: bool) -> Vec<JournalLine> {
    form.iter()
        .zip(amounts)
        .map(|(line, amount)| {
            let debit = (line.kind == PurchaseLineKind::Charge) == split_debits;
            let entry = if debit { JournalLine::debit(line.account_id, *amount) } else { JournalLine::credit(line.account_id, *amount) };
            entry.with_memo(Some(description(line))).with_class(line.class_id)
        })
        .collect()
}

/// A document's stored discount and charge lines, in place order (from the pool, or inside a
/// transaction with `&mut **tx`).
pub async fn lines_of<'e, E: sqlx::Executor<'e, Database = Sqlite>>(executor: E, doc: PurchaseDoc, id: i64) -> Result<Vec<PurchaseFormLineView>, String> {
    sqlx::query_as::<_, PurchaseFormLineView>(&format!(
        "SELECT after_line, kind, description, product_service_id, percent_ppm, amount_cents, account_id, class_id
         FROM purchase_form_lines WHERE {} = ?1 ORDER BY after_line",
        doc.column()
    ))
    .bind(id)
    .fetch_all(executor)
    .await
    .map_err(db_err)
}

/// A purchase order's discount or charge placed on a receipt's bill, with `source`: which of the
/// order's lines it is. A fixed one is placed after every receipt line from its order line, and
/// [`fit_carried`] keeps one of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CarriedLine {
    #[serde(flatten)]
    pub line: PurchaseFormLineInput,
    pub source: usize,
}

/// The carried lines a receipt's bill takes. A percent goes after every line it is placed after. A
/// fixed amount goes once, after the first line that can take it: a fixed discount more than its
/// split line (a part shipment) is not placed there, and waits for a bill whose line can take it.
pub fn fit_carried(form: Vec<CarriedLine>, splits: &[i64]) -> Vec<PurchaseFormLineInput> {
    let mut placed: Vec<usize> = Vec::new();
    form.into_iter()
        .filter_map(|c| {
            let Some(amount) = c.line.amount_cents.filter(|_| c.line.percent_ppm.is_none()) else { return Some(c.line) };
            let base = c.line.after_line.checked_sub(1).and_then(|i| splits.get(i));
            let fits = c.line.kind == PurchaseLineKind::Charge || base.is_some_and(|b| amount <= *b);
            if !fits || placed.contains(&c.source) {
                return None;
            }
            placed.push(c.source);
            Some(c.line)
        })
        .collect()
}

/// Stored lines as inputs again (a purchase order's lines carried to its bill).
pub fn inputs_of(views: &[PurchaseFormLineView]) -> Vec<PurchaseFormLineInput> {
    views
        .iter()
        .map(|v| PurchaseFormLineInput {
            after_line: usize::try_from(v.after_line).unwrap_or(0),
            kind: if v.kind == "CHARGE" { PurchaseLineKind::Charge } else { PurchaseLineKind::Discount },
            description: v.description.clone(),
            product_service_id: v.product_service_id,
            percent_ppm: v.percent_ppm,
            amount_cents: if v.percent_ppm.is_some() { None } else { Some(v.amount_cents) },
            account_id: v.account_id,
            class_id: v.class_id,
        })
        .collect()
}

#[cfg(test)]
mod tests;
