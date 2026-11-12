//! Lines on a sales form that are not plain items: subtotals, discounts, percent charges and group
//! headings; and the groups themselves (a named set of items a form expands into its lines). The design is `docs/plans/item-types.md`.
//!
//! A form sends its item lines as before, plus `formLines`, each placed by `afterLines`: how many
//! item lines come before it. [`lay_out`] merges the two into line order, numbers every line and
//! works out each form line's amount. It is pure, and every sales writer calls it before writing
//! anything, so the form, the stored lines and the posted entry agree.
//!
//! Item lines stay in the document's item table; form lines go in `<document>_form_lines`, sharing
//! the line numbering. A percent charge is a positive line, so it is written to the item table
//! (quantity 1 at its amount, with its percent) and credited like any item.

use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{db_err, mul_div_round, JournalLine, MAX_LINE_CENTS};
use crate::money::Cents;
use crate::sales_lists::SalesDoc;

/// What a form line is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FormLineKind {
    /// The sum of the lines since the previous subtotal (or the top of the form). Posts nothing.
    Subtotal,
    /// An amount, or a percent of the line above, taken off; debited to its account.
    Discount,
    /// A percent of the line above, added on (shipping, handling); credited to its account. A fixed
    /// charge is an ordinary item line.
    Charge,
    /// A group's heading: the item lines after it (`group_line_count` of them) came from the group.
    Group,
}

impl FormLineKind {
    fn key(self) -> &'static str {
        match self {
            FormLineKind::Subtotal => "SUBTOTAL",
            FormLineKind::Discount => "DISCOUNT",
            FormLineKind::Charge => "CHARGE",
            FormLineKind::Group => "GROUP",
        }
    }

    fn from_key(key: &str) -> Option<Self> {
        match key {
            "SUBTOTAL" => Some(FormLineKind::Subtotal),
            "DISCOUNT" => Some(FormLineKind::Discount),
            "GROUP" => Some(FormLineKind::Group),
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
    pub amount_cents: Option<Cents>,
    /// Discount: the account debited.
    #[serde(default)]
    pub account_id: Option<i64>,
    #[serde(default)]
    pub class_id: Option<i64>,
    /// Discount: taken off the taxable amount too.
    #[serde(default)]
    pub is_taxable: bool,
    /// Group: the group it came from.
    #[serde(default)]
    pub item_group_id: Option<i64>,
    /// Group: how many of the item lines after it belong to it.
    #[serde(default)]
    pub group_line_count: usize,
    /// Group: how many of the group (scaled by 10,000), as printed when it prints as one line.
    #[serde(default)]
    pub group_quantity_e4: Option<i64>,
    /// Group: false prints the group as one line instead of its items. Left out, true.
    #[serde(default)]
    pub print_items: Option<bool>,
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
            item_group_id: None,
            group_line_count: 0,
            group_quantity_e4: None,
            print_items: None,
        }
    }

    /// A group heading over the next `count` item lines.
    #[cfg(test)]
    pub fn group(after_lines: usize, count: usize, print_items: bool) -> Self {
        FormLineInput {
            group_line_count: count,
            group_quantity_e4: Some(10_000),
            print_items: Some(print_items),
            ..Self::blank(after_lines, FormLineKind::Group)
        }
    }

    pub fn subtotal(after_lines: usize) -> Self {
        Self::blank(after_lines, FormLineKind::Subtotal)
    }

    /// A charge of `percent_ppm` of the line above, credited to `account_id`.
    pub fn charge(after_lines: usize, account_id: i64, percent_ppm: i64, is_taxable: bool) -> Self {
        FormLineInput { account_id: Some(account_id), percent_ppm: Some(percent_ppm), is_taxable, ..Self::blank(after_lines, FormLineKind::Charge) }
    }

    /// A discount of a percentage (`percent_ppm`) or an amount off the line above.
    pub fn discount(after_lines: usize, account_id: i64, percent_ppm: Option<i64>, amount_cents: Option<i64>, is_taxable: bool) -> Self {
        FormLineInput {
            account_id: Some(account_id),
            percent_ppm,
            amount_cents: amount_cents.map(Cents),
            is_taxable,
            ..Self::blank(after_lines, FormLineKind::Discount)
        }
    }

    fn text(&self) -> String {
        match (self.description.trim(), self.kind) {
            ("", FormLineKind::Subtotal) => "Subtotal".to_string(),
            ("", FormLineKind::Discount) => "Discount".to_string(),
            ("", FormLineKind::Charge) => "Charge".to_string(),
            ("", FormLineKind::Group) => "Group".to_string(),
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
    // The open group: its heading's row, the position after its last item, and its total so far.
    let mut group: Option<(usize, usize, i64)> = None;
    for position in 0..=items.len() {
        for (index, f) in form.iter().enumerate().filter(|(_, f)| f.after_lines == position) {
            number += 1;
            if group.is_some() {
                return Err(format!("{code}_INSIDE_GROUP: line {number} is inside a group; a group holds only its items"));
            }
            let amount_cents = match f.kind {
                FormLineKind::Subtotal => {
                    let sum = since_subtotal;
                    since_subtotal = 0;
                    base = Some(sum);
                    sum
                }
                FormLineKind::Discount => {
                    let n = number;
                    let above = base
                        .take()
                        .filter(|b| *b > 0)
                        .ok_or_else(|| format!("{code}_DISCOUNT_NEEDS_BASE: the discount on line {n} must come right after an item or a subtotal"))?;
                    let off = match (f.percent_ppm, f.amount_cents) {
                        (Some(ppm), None) if (1..=1_000_000).contains(&ppm) => mul_div_round(above, ppm, 1_000_000).unwrap_or(0),
                        (None, Some(cents)) if cents.is_positive() => cents.get(),
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
                FormLineKind::Charge => {
                    let n = number;
                    let above = base
                        .take()
                        .filter(|b| *b > 0)
                        .ok_or_else(|| format!("{code}_CHARGE_NEEDS_BASE: the percent charge on line {n} must come right after an item or a subtotal"))?;
                    let ppm = f.percent_ppm.filter(|p| (1..=1_000_000).contains(p)).ok_or_else(|| {
                        format!("{code}_BAD_CHARGE_PERCENT: the charge on line {n} needs a percentage up to 100% (a fixed charge is an item line)")
                    })?;
                    let on = mul_div_round(above, ppm, 1_000_000).unwrap_or(0);
                    if on < 1 {
                        return Err(format!("{code}_CHARGE_ROUNDS_TO_ZERO: the charge on line {n} rounds to 0.00"));
                    }
                    since_subtotal = add(since_subtotal, on, code)?;
                    lines_cents = add(lines_cents, on, code)?;
                    if f.is_taxable {
                        taxable_cents = add(taxable_cents, on, code)?;
                    }
                    on
                }
                FormLineKind::Group => {
                    let count = f.group_line_count;
                    if count == 0 || position + count > items.len() {
                        return Err(format!("{code}_BAD_GROUP: the group on line {number} must be followed by its items"));
                    }
                    if f.group_quantity_e4.is_some_and(|q| q <= 0) {
                        return Err(format!("{code}_BAD_GROUP: the group on line {number} needs a quantity above zero"));
                    }
                    group = Some((rows.len(), position + count, 0));
                    base = None;
                    0
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
            if let Some((row, end, total)) = group.as_mut() {
                *total = add(*total, item.amount_cents, code)?;
                if position + 1 == *end {
                    // The group is one block: a discount under it comes off its total.
                    if let (_, Laid::Form { amount_cents, .. }) = &mut rows[*row] {
                        *amount_cents = *total;
                    }
                    base = Some(*total);
                    group = None;
                }
            }
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

/// Where a document's lines are kept: its item table, its form-line table and the column naming
/// the document in both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LineTables {
    pub items: &'static str,
    pub form: &'static str,
    pub key: &'static str,
}

/// Credit memos and refund receipts (`credits.rs`).
pub(crate) const CREDIT_MEMO_LINES: LineTables = LineTables { items: "credit_memo_lines", form: "credit_memo_form_lines", key: "credit_memo_id" };

impl From<SalesDoc> for LineTables {
    fn from(doc: SalesDoc) -> Self {
        match doc {
            SalesDoc::Invoice => LineTables { items: "invoice_items", form: "invoice_form_lines", key: "invoice_id" },
            SalesDoc::SalesReceipt => LineTables { items: "sales_receipt_lines", form: "sales_receipt_form_lines", key: "sales_receipt_id" },
            SalesDoc::Estimate => LineTables { items: "estimate_lines", form: "estimate_form_lines", key: "estimate_id" },
        }
    }
}

fn table(doc: impl Into<LineTables>) -> (&'static str, &'static str) {
    let t = doc.into();
    (t.form, t.key)
}

/// Checks what the form lines refer to: a discount's account is an active income account, its
/// item (if any) a discount item, and its class usable.
pub(crate) async fn check_form_lines(tx: &mut Transaction<'_, Sqlite>, form: &[FormLineInput], layout: &Layout, code: &str) -> Result<(), String> {
    for (n, index, _) in layout.form_rows() {
        let f = &form[index];
        let (what, item_type) = match f.kind {
            FormLineKind::Subtotal => continue,
            FormLineKind::Group => {
                if let Some(group) = f.item_group_id {
                    let found: Option<i64> =
                        sqlx::query_scalar("SELECT id FROM item_groups WHERE id = ?1").bind(group).fetch_optional(&mut **tx).await.map_err(db_err)?;
                    if found.is_none() {
                        return Err(format!("{code}_UNKNOWN_GROUP: the group on line {n} does not exist"));
                    }
                }
                continue;
            }
            FormLineKind::Discount => ("discount", "DISCOUNT"),
            FormLineKind::Charge => ("charge", "OTHER_CHARGE"),
        };
        let account = f.account_id.ok_or_else(|| format!("{code}_NO_ACCOUNT: choose the account the {what} on line {n} posts to"))?;
        let found: Option<(String, bool)> =
            sqlx::query_as("SELECT t.classification, a.is_active FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1")
                .bind(account)
                .fetch_optional(&mut **tx)
                .await
                .map_err(db_err)?;
        match found {
            Some((classification, true)) if classification == "INCOME" => {}
            Some((_, false)) => return Err(format!("{code}_INACTIVE_ACCOUNT: line {n} uses an inactive account")),
            _ => return Err(format!("{code}_NOT_INCOME_ACCOUNT: the {what} on line {n} must use an Income account")),
        }
        if let Some(product) = f.product_service_id {
            let found: Option<String> =
                sqlx::query_scalar("SELECT item_type FROM products_services WHERE id = ?1").bind(product).fetch_optional(&mut **tx).await.map_err(db_err)?;
            if found.as_deref() != Some(item_type) {
                return Err(format!("{code}_NOT_A_{}: the item on line {n} is not a {what} item", what.to_ascii_uppercase()));
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

/// A percent charge as the item row it is written as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChargeRow {
    pub line_number: i64,
    pub description: String,
    pub amount_cents: i64,
    pub percent_ppm: i64,
    pub account_id: i64,
    pub product_service_id: Option<i64>,
    pub class_id: Option<i64>,
    pub is_taxable: bool,
}

/// The percent charges, in line order, to write to the item table and credit.
pub fn charge_rows(form: &[FormLineInput], layout: &Layout) -> Vec<ChargeRow> {
    layout
        .form_rows()
        .filter_map(|(line_number, index, amount_cents)| {
            let f = &form[index];
            (f.kind == FormLineKind::Charge).then(|| ChargeRow {
                line_number,
                description: f.text(),
                amount_cents,
                percent_ppm: f.percent_ppm.unwrap_or(0),
                account_id: f.account_id.unwrap_or(0),
                product_service_id: f.product_service_id,
                class_id: f.class_id,
                is_taxable: f.is_taxable,
            })
        })
        .collect()
}

/// Writes a document's percent charges to its item table (`<table>` keyed by `<key>`).
pub(crate) async fn write_charges(
    tx: &mut Transaction<'_, Sqlite>,
    doc: impl Into<LineTables>,
    id: i64,
    charges: &[ChargeRow],
    taxed: bool,
) -> Result<(), String> {
    let LineTables { items: table, key, .. } = doc.into();
    for c in charges {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {table} ({key}, line_number, description, quantity_e4, rate_cents, amount_cents, income_account_id, product_service_id, class_id, is_taxable, percent_ppm)
             VALUES (?1, ?2, ?3, 10000, ?4, ?4, ?5, ?6, ?7, ?8, ?9)"
        )))
        .bind(id)
        .bind(c.line_number)
        .bind(&c.description)
        .bind(c.amount_cents)
        .bind(c.account_id)
        .bind(c.product_service_id)
        .bind(c.class_id)
        .bind(c.is_taxable && taxed)
        .bind(c.percent_ppm)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    Ok(())
}

/// Writes a document's form lines, after its row is inserted and before it posts. `taxed`: the
/// document has a tax rate (as for item lines, a line is stored taxable only then).
pub(crate) async fn write_form_lines(
    tx: &mut Transaction<'_, Sqlite>,
    doc: impl Into<LineTables>,
    id: i64,
    form: &[FormLineInput],
    layout: &Layout,
    taxed: bool,
) -> Result<(), String> {
    let (table, key) = table(doc);
    for (number, index, amount_cents) in layout.form_rows() {
        let f = &form[index];
        if f.kind == FormLineKind::Charge {
            continue;
        }
        let discount = f.kind == FormLineKind::Discount;
        let is_group = f.kind == FormLineKind::Group;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {table} ({key}, line_number, kind, description, amount_cents, product_service_id, percent_ppm, account_id, class_id, is_taxable,
                                  item_group_id, group_quantity_e4, group_line_count, print_items)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)"
        )))
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
        .bind(f.item_group_id.filter(|_| is_group))
        .bind(f.group_quantity_e4.filter(|_| is_group))
        .bind(is_group.then_some(f.group_line_count as i64))
        .bind(is_group.then_some(f.print_items.unwrap_or(true)))
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
    pub amount_cents: Cents,
    pub product_service_id: Option<i64>,
    pub percent_ppm: Option<i64>,
    pub account_id: Option<i64>,
    pub class_id: Option<i64>,
    pub is_taxable: bool,
    pub item_group_id: Option<i64>,
    pub group_quantity_e4: Option<i64>,
    pub group_line_count: Option<i64>,
    pub print_items: Option<bool>,
}

/// A document's form lines in line order.
pub async fn form_lines_of<'e, E: sqlx::Executor<'e, Database = Sqlite>>(
    executor: E,
    doc: impl Into<LineTables>,
    id: i64,
) -> Result<Vec<FormLineView>, String> {
    let (table, key) = table(doc);
    sqlx::query_as::<_, FormLineView>(sqlx::AssertSqlSafe(format!(
        "SELECT line_number, kind, description, amount_cents, product_service_id, percent_ppm, account_id, class_id, is_taxable,
                item_group_id, group_quantity_e4, group_line_count, print_items FROM {table} WHERE {key} = ?1 ORDER BY line_number"
    )))
    .bind(id)
    .fetch_all(executor)
    .await
    .map_err(db_err)
}

/// A stored percent charge (an item row with a percent) back as a form line input, placed among
/// the plain item lines numbered `item_numbers`.
#[allow(clippy::too_many_arguments)]
pub fn charge_input(
    line_number: i64,
    description: &str,
    percent_ppm: i64,
    account_id: i64,
    product_service_id: Option<i64>,
    class_id: Option<i64>,
    is_taxable: bool,
    item_numbers: &[i64],
) -> FormLineInput {
    FormLineInput {
        after_lines: item_numbers.iter().filter(|n| **n < line_number).count(),
        description: description.to_string(),
        product_service_id,
        class_id,
        ..FormLineInput::charge(0, account_id, percent_ppm, is_taxable)
    }
}

/// Which item lines stay when zero-amount lines are left off (`drop_zero`), as
/// `sales_prefs::without_zero_lines` decides it: all of them when every line is zero.
pub fn kept_lines(amounts: &[i64], drop_zero: bool) -> Vec<bool> {
    let staying = crate::sales_prefs::without_zero_lines((0..amounts.len()).collect(), drop_zero, |i: &usize| amounts[*i]);
    (0..amounts.len()).map(|i| staying.contains(&i)).collect()
}

/// The form lines once some item lines are left out (`kept[i]` false): each stays after the same
/// item lines it followed, and a group counts only its items that stay (a group left with none
/// goes too).
pub fn without_items(form: Vec<FormLineInput>, kept: &[bool]) -> Vec<FormLineInput> {
    let staying = |from: usize, to: usize| kept[from.min(kept.len())..to.min(kept.len())].iter().filter(|k| **k).count();
    form.into_iter()
        .filter_map(|mut f| {
            let start = f.after_lines;
            f.after_lines = staying(0, start);
            if f.kind == FormLineKind::Group {
                f.group_line_count = staying(start, start + f.group_line_count);
                if f.group_line_count == 0 {
                    return None;
                }
            }
            Some(f)
        })
        .collect()
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
                amount_cents: if v.kind == "DISCOUNT" && v.percent_ppm.is_none() { v.amount_cents.checked_neg() } else { None },
                account_id: v.account_id,
                class_id: v.class_id,
                is_taxable: v.is_taxable,
                item_group_id: v.item_group_id,
                group_line_count: v.group_line_count.unwrap_or(0) as usize,
                group_quantity_e4: v.group_quantity_e4,
                print_items: v.print_items,
            })
        })
        .collect()
}

// ------------------------------------------------------------------------------------------------
// Groups
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupMemberInput {
    pub product_id: i64,
    /// Scaled by 10,000.
    pub quantity_e4: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInput {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// False: printed forms show the group as one line.
    #[serde(default = "yes")]
    pub print_items: bool,
    pub members: Vec<GroupMemberInput>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct GroupMember {
    pub product_id: i64,
    pub product_name: String,
    pub quantity_e4: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemGroup {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub print_items: bool,
    pub is_active: bool,
    pub members: Vec<GroupMember>,
}

/// Every group with its items, by name.
pub async fn list_groups(pool: &SqlitePool) -> Result<Vec<ItemGroup>, String> {
    let groups: Vec<(i64, String, Option<String>, bool, bool)> =
        sqlx::query_as("SELECT id, name, description, print_items, is_active FROM item_groups ORDER BY name COLLATE NOCASE")
            .fetch_all(pool)
            .await
            .map_err(db_err)?;
    let members: Vec<(i64, i64, String, i64)> = sqlx::query_as(
        "SELECT m.group_id, m.product_id, p.name, m.quantity_e4 FROM item_group_members m JOIN products_services p ON p.id = m.product_id ORDER BY m.group_id, m.position",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(groups
        .into_iter()
        .map(|(id, name, description, print_items, is_active)| ItemGroup {
            id,
            name,
            description,
            print_items,
            is_active,
            members: members
                .iter()
                .filter(|m| m.0 == id)
                .map(|(_, product_id, product_name, quantity_e4)| GroupMember {
                    product_id: *product_id,
                    product_name: product_name.clone(),
                    quantity_e4: *quantity_e4,
                })
                .collect(),
        })
        .collect())
}

async fn get_group(pool: &SqlitePool, id: i64) -> Result<ItemGroup, String> {
    list_groups(pool).await?.into_iter().find(|g| g.id == id).ok_or_else(|| "ITEM_GROUP_NOT_FOUND: that group does not exist".to_string())
}

/// Adds a group (`id` left out) or changes one. Documents already written keep their lines.
pub async fn save_group(pool: &SqlitePool, id: Option<i64>, input: GroupInput) -> Result<ItemGroup, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("ITEM_GROUP_NO_NAME: enter a name for the group".into());
    }
    if input.members.is_empty() {
        return Err("ITEM_GROUP_EMPTY: add at least one item to the group".into());
    }
    if let Some(m) = input.members.iter().find(|m| !(1..=crate::invoices::MAX_QUANTITY_E4).contains(&m.quantity_e4)) {
        return Err(format!("ITEM_GROUP_BAD_QUANTITY: item {} needs a quantity above zero", m.product_id));
    }
    let description = input.description.as_deref().map(str::trim).filter(|d| !d.is_empty());
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result = async {
        let group_id: i64 = match id {
            None => sqlx::query_scalar("INSERT INTO item_groups (name, description, print_items) VALUES (?1, ?2, ?3) RETURNING id")
                .bind(name)
                .bind(description)
                .bind(input.print_items)
                .fetch_one(&mut *tx)
                .await
                .map_err(group_err)?,
            Some(id) => {
                let done = sqlx::query("UPDATE item_groups SET name = ?1, description = ?2, print_items = ?3 WHERE id = ?4")
                    .bind(name)
                    .bind(description)
                    .bind(input.print_items)
                    .bind(id)
                    .execute(&mut *tx)
                    .await
                    .map_err(group_err)?;
                if done.rows_affected() == 0 {
                    return Err("ITEM_GROUP_NOT_FOUND: that group does not exist".to_string());
                }
                sqlx::query("DELETE FROM item_group_members WHERE group_id = ?1").bind(id).execute(&mut *tx).await.map_err(db_err)?;
                id
            }
        };
        for (i, m) in input.members.iter().enumerate() {
            sqlx::query("INSERT INTO item_group_members (group_id, position, product_id, quantity_e4) VALUES (?1, ?2, ?3, ?4)")
                .bind(group_id)
                .bind(i as i64 + 1)
                .bind(m.product_id)
                .bind(m.quantity_e4)
                .execute(&mut *tx)
                .await
                .map_err(db_err)?;
        }
        Ok(group_id)
    }
    .await;
    match result {
        Ok(group_id) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            get_group(pool, group_id).await
        }
        Err(err) => Err(crate::ledger::rollback(tx, err).await),
    }
}

fn group_err(e: sqlx::Error) -> String {
    let msg = db_err(e);
    if msg.contains("UNIQUE") {
        "ITEM_GROUP_NAME: a group already has that name".into()
    } else {
        msg
    }
}

pub async fn set_group_active(pool: &SqlitePool, id: i64, active: bool) -> Result<ItemGroup, String> {
    sqlx::query("UPDATE item_groups SET is_active = ?1 WHERE id = ?2").bind(active).bind(id).execute(pool).await.map_err(db_err)?;
    get_group(pool, id).await
}

#[cfg(test)]
mod tests;
