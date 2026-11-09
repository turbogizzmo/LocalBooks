//! Combined sales tax rates and the sales tax return per agency (gaps row R11).
//!
//! - **Combined rate.** A rate made of component rates (state, county, city…), each with its own
//!   agency and liability account. Its rate is theirs added up. A sale at a combined rate posts
//!   one tax line per component; the document's tax is still round_half_up(taxable × combined
//!   rate), and the components share it: each gets round_half_up(taxable × its rate), and the
//!   cent or two that rounding leaves over goes to the largest. The split is kept with the entry
//!   (`document_tax_parts`), so changing a combined rate later never changes tax already posted.
//! - **Sales tax return.** For a period, on the accrual or cash basis: for each agency, each of its
//!   rates with the taxable sales and the tax collected, and what was paid to it in the period.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqliteConnection, SqlitePool, Transaction};

use super::{tax_on, Basis, TaxPart, TaxRateView};
use crate::ledger::{db_err, mul_div_round, rollback};

/// A component of a combined rate.
#[derive(Debug, Clone, sqlx::FromRow)]
pub(crate) struct Component {
    pub id: i64,
    pub name: String,
    pub rate_ppm: i64,
    pub liability_account_id: i64,
    pub is_active: bool,
}

pub(crate) async fn components(conn: &mut SqliteConnection, group_id: i64) -> Result<Vec<Component>, String> {
    sqlx::query_as(
        "SELECT r.id, r.name, r.rate_ppm, r.liability_account_id, r.is_active FROM tax_rate_components c JOIN tax_rates r ON r.id = c.component_id
         WHERE c.group_id = ?1 ORDER BY c.position",
    )
    .bind(group_id)
    .fetch_all(conn)
    .await
    .map_err(db_err)
}

/// Splits a combined rate's tax (`total`, already worked out at the combined rate) among its
/// components: each its own rate's share, the rounding left over to the largest. `None` on overflow.
pub(crate) fn split(total: i64, taxable: i64, components: &[Component]) -> Option<Vec<TaxPart>> {
    let mut parts: Vec<TaxPart> = components
        .iter()
        .map(|c| tax_on(taxable, c.rate_ppm).map(|cents| TaxPart { rate_id: c.id, name: c.name.clone(), liability_account_id: c.liability_account_id, cents }))
        .collect::<Option<_>>()?;
    let sum = parts.iter().try_fold(0i64, |s, p| s.checked_add(p.cents))?;
    let left = total.checked_sub(sum)?;
    if left != 0 {
        let largest = parts.iter().enumerate().max_by_key(|(i, p)| (p.cents, std::cmp::Reverse(*i))).map(|(i, _)| i)?;
        parts[largest].cents = parts[largest].cents.checked_add(left)?;
    }
    Some(parts)
}

/// Fills in each combined rate's component ids.
pub(crate) async fn fill_components(pool: &SqlitePool, rates: &mut [TaxRateView]) -> Result<(), String> {
    let rows: Vec<(i64, i64)> = sqlx::query_as("SELECT group_id, component_id FROM tax_rate_components ORDER BY group_id, position").fetch_all(pool).await.map_err(db_err)?;
    for r in rates.iter_mut().filter(|r| r.is_group) {
        r.component_ids = rows.iter().filter(|(g, _)| *g == r.id).map(|(_, c)| *c).collect();
    }
    Ok(())
}

/// Recomputes the rate of every combined rate `component_id` is part of.
pub(crate) async fn refresh_groups_of(tx: &mut Transaction<'_, Sqlite>, component_id: i64) -> Result<(), String> {
    sqlx::query(
        "UPDATE tax_rates SET rate_ppm = MIN(1000000, (SELECT COALESCE(SUM(r.rate_ppm), 0) FROM tax_rate_components c JOIN tax_rates r ON r.id = c.component_id WHERE c.group_id = tax_rates.id))
         WHERE is_group = 1 AND id IN (SELECT group_id FROM tax_rate_components WHERE component_id = ?1)",
    )
    .bind(component_id)
    .execute(&mut **tx)
    .await
    .map_err(db_err)?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CombinedRateInput {
    pub name: String,
    /// The component rates, in order: at least two ordinary rates.
    pub component_ids: Vec<i64>,
}

/// Saves a combined rate (new when `id` is `None`): its components, and its rate as theirs added up.
pub async fn save_combined_rate(pool: &SqlitePool, id: Option<i64>, input: &CombinedRateInput) -> Result<TaxRateView, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("TAX_NAME_REQUIRED: enter a name for the rate".into());
    }
    let ids = input.component_ids.clone();
    let distinct: std::collections::HashSet<i64> = ids.iter().copied().collect();
    if ids.len() < 2 || distinct.len() != ids.len() {
        return Err("TAX_COMBINED_PARTS: a combined rate is made of two or more different rates".into());
    }
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    let result: Result<i64, String> = async {
        let mut total: i64 = 0;
        let mut first_account = None;
        for c in &ids {
            let row: Option<(i64, i64, bool, bool)> = sqlx::query_as("SELECT rate_ppm, liability_account_id, is_group, is_active FROM tax_rates WHERE id = ?1").bind(c).fetch_optional(&mut *tx).await.map_err(db_err)?;
            match row {
                None => return Err(format!("TAX_NOT_FOUND: tax rate {c} does not exist")),
                Some((_, _, true, _)) => return Err("TAX_COMBINED_PARTS: a combined rate can't include another combined rate".into()),
                Some((_, _, _, false)) => return Err("TAX_COMBINED_PARTS: a combined rate's rates must be active".into()),
                Some((ppm, account, false, true)) => {
                    total = total.checked_add(ppm).ok_or_else(|| "TAX_BAD_RATE: a rate is between 0% and 100%".to_string())?;
                    first_account.get_or_insert(account);
                }
            }
        }
        if id.is_some_and(|g| ids.contains(&g)) {
            return Err("TAX_COMBINED_PARTS: a combined rate can't include itself".into());
        }
        if total > 1_000_000 {
            return Err("TAX_BAD_RATE: a rate is between 0% and 100%".into());
        }
        let group_id = match id {
            Some(g) => {
                let done = sqlx::query("UPDATE tax_rates SET name = ?1, rate_ppm = ?2, liability_account_id = ?3 WHERE id = ?4 AND is_group = 1")
                    .bind(name)
                    .bind(total)
                    .bind(first_account)
                    .bind(g)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| super::unique(e, name))?;
                if done.rows_affected() != 1 {
                    return Err("TAX_NOT_COMBINED: that rate isn't a combined rate".into());
                }
                sqlx::query("DELETE FROM tax_rate_components WHERE group_id = ?1").bind(g).execute(&mut *tx).await.map_err(db_err)?;
                g
            }
            // The group's own liability account and agency are never posted to: its components' are.
            None => sqlx::query_scalar("INSERT INTO tax_rates (name, rate_ppm, liability_account_id, is_group) VALUES (?1, ?2, ?3, 1) RETURNING id")
                .bind(name)
                .bind(total)
                .bind(first_account)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| super::unique(e, name))?,
        };
        for (i, c) in ids.iter().enumerate() {
            sqlx::query("INSERT INTO tax_rate_components (group_id, component_id, position) VALUES (?1, ?2, ?3)").bind(group_id).bind(c).bind(i as i64 + 1).execute(&mut *tx).await.map_err(db_err)?;
        }
        Ok(group_id)
    }
    .await;
    match result {
        Ok(group_id) => {
            crate::ledger::commit_write(tx).await.map_err(db_err)?;
            super::get_rate(pool, group_id).await
        }
        Err(err) => Err(rollback(tx, err).await),
    }
}

// ------------------------------------------------------------------------------------------------
// The sales tax return
// ------------------------------------------------------------------------------------------------

/// One rate on an agency's return.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReturnLine {
    pub tax_rate_id: i64,
    pub name: String,
    pub rate_ppm: i64,
    pub taxable_sales_cents: i64,
    pub tax_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgencyReturn {
    pub agency_vendor_id: Option<i64>,
    /// The agency's name, or "No agency" for rates without one.
    pub agency_name: String,
    pub lines: Vec<ReturnLine>,
    pub tax_cents: i64,
    /// Sales tax paid to this agency in the period.
    pub paid_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SalesTaxReturn {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub basis: Basis,
    pub agencies: Vec<AgencyReturn>,
    pub tax_cents: i64,
    pub paid_cents: i64,
}

/// A document with tax: its entry, rate, sign (−1 for a credit memo), tax, taxable amount, total,
/// and how much of it counts in the period (the total, or on cash what was settled in it).
type TaxedDoc = (i64, i64, i64, i64, i64, i64, i64);

const TAXABLE_INVOICE: &str = "(SELECT COALESCE(SUM(amount_cents), 0) FROM invoice_items WHERE invoice_id = i.id AND is_taxable = 1)
    + (SELECT COALESCE(SUM(amount_cents), 0) FROM invoice_form_lines WHERE invoice_id = i.id AND kind = 'DISCOUNT' AND is_taxable = 1)";

/// The sales tax return for a period, per agency, on `basis`. Sales receipts and credit memos count
/// by their date; invoices by theirs (accrual) or, on cash, in proportion to what was paid or
/// credited on them in the period, less checks that bounced. Foreign-currency invoices are left
/// out, as on the liability report.
pub async fn sales_tax_return(pool: &SqlitePool, start: NaiveDate, end: NaiveDate, basis: Basis) -> Result<SalesTaxReturn, String> {
    if start > end {
        return Err("RPT_BAD_RANGE: the start date is after the end date".into());
    }
    let invoices = match basis {
        Basis::Accrual => format!(
            "SELECT i.journal_entry_id, i.tax_rate_id, 1, i.tax_cents, {TAXABLE_INVOICE}, i.total_cents, i.total_cents
             FROM invoices i WHERE i.is_voided = 0 AND i.journal_entry_id IS NOT NULL AND i.currency_code IS NULL AND i.tax_rate_id IS NOT NULL AND i.issue_date BETWEEN ?1 AND ?2"
        ),
        Basis::Cash => format!(
            "SELECT i.journal_entry_id, i.tax_rate_id, 1, i.tax_cents, {TAXABLE_INVOICE}, i.total_cents,
                    COALESCE((SELECT SUM(p.amount_cents) FROM invoice_payments p WHERE p.invoice_id = i.id AND p.is_voided = 0 AND p.payment_date BETWEEN ?1 AND ?2), 0)
                  + COALESCE((SELECT SUM(a.amount_cents) FROM credit_applications a WHERE a.invoice_id = i.id AND a.is_voided = 0 AND a.applied_date BETWEEN ?1 AND ?2), 0)
                  - COALESCE((SELECT SUM(b.amount_cents) FROM bounced_checks b WHERE b.invoice_id = i.id AND b.is_voided = 0 AND b.bounce_date BETWEEN ?1 AND ?2), 0)
             FROM invoices i WHERE i.is_voided = 0 AND i.journal_entry_id IS NOT NULL AND i.currency_code IS NULL AND i.tax_rate_id IS NOT NULL AND i.total_cents > 0 AND i.issue_date <= ?2"
        ),
    };
    let sql = format!(
        "{invoices}
         UNION ALL
         SELECT r.journal_entry_id, r.tax_rate_id, 1, r.tax_cents,
                (SELECT COALESCE(SUM(amount_cents), 0) FROM sales_receipt_lines WHERE sales_receipt_id = r.id AND is_taxable = 1)
                + (SELECT COALESCE(SUM(amount_cents), 0) FROM sales_receipt_form_lines WHERE sales_receipt_id = r.id AND kind = 'DISCOUNT' AND is_taxable = 1),
                1, 1
         FROM sales_receipts r WHERE r.is_voided = 0 AND r.journal_entry_id IS NOT NULL AND r.tax_rate_id IS NOT NULL AND r.receipt_date BETWEEN ?1 AND ?2
         UNION ALL
         SELECT m.journal_entry_id, m.tax_rate_id, -1, m.tax_cents,
                (SELECT COALESCE(SUM(amount_cents), 0) FROM credit_memo_lines WHERE credit_memo_id = m.id AND is_taxable = 1),
                1, 1
         FROM credit_memos m WHERE m.is_voided = 0 AND m.journal_entry_id IS NOT NULL AND m.tax_rate_id IS NOT NULL AND m.credit_date BETWEEN ?1 AND ?2"
    );
    let docs: Vec<TaxedDoc> = sqlx::query_as(&sql).bind(start).bind(end).fetch_all(pool).await.map_err(db_err)?;
    // Each combined-rate entry's split, by entry.
    let mut parts: std::collections::HashMap<i64, Vec<(i64, i64)>> = std::collections::HashMap::new();
    for (entry, rate_id, cents) in sqlx::query_as::<_, (i64, i64, i64)>("SELECT journal_entry_id, tax_rate_id, cents FROM document_tax_parts").fetch_all(pool).await.map_err(db_err)? {
        parts.entry(entry).or_default().push((rate_id, cents));
    }
    let rates: Vec<(i64, String, i64, Option<i64>, Option<String>)> =
        sqlx::query_as("SELECT r.id, r.name, r.rate_ppm, r.agency_vendor_id, v.name FROM tax_rates r LEFT JOIN vendors v ON v.id = r.agency_vendor_id").fetch_all(pool).await.map_err(db_err)?;
    let rate = |id: i64| rates.iter().find(|r| r.0 == id);
    let overflow = || "TAX_OVERFLOW: the sales tax return is out of range".to_string();
    let share = |amount: i64, counted: i64, total: i64| -> Result<i64, String> {
        if counted == total {
            return Ok(amount);
        }
        if total == 0 {
            return Ok(0);
        }
        mul_div_round(amount, counted, total).ok_or_else(overflow)
    };

    // Per rate (component): taxable sales and tax.
    let mut sums: std::collections::BTreeMap<i64, (i64, i64)> = std::collections::BTreeMap::new();
    for (entry, rate_id, sign, tax, taxable, total, counted) in docs {
        if counted == 0 {
            continue;
        }
        let split: Vec<(i64, i64)> = parts.get(&entry).cloned().unwrap_or_else(|| vec![(rate_id, tax)]);
        let taxable = share(taxable, counted, total)?.checked_mul(sign).ok_or_else(overflow)?;
        for (component, cents) in split {
            let cents = share(cents, counted, total)?.checked_mul(sign).ok_or_else(overflow)?;
            let e = sums.entry(component).or_default();
            e.0 = e.0.checked_add(taxable).ok_or_else(overflow)?;
            e.1 = e.1.checked_add(cents).ok_or_else(overflow)?;
        }
    }

    // Grouped by agency.
    let mut agencies: Vec<AgencyReturn> = Vec::new();
    for (rate_id, (taxable, tax)) in sums {
        let (name, ppm, agency_id, agency_name) = rate(rate_id).map_or_else(|| (format!("Rate {rate_id}"), 0, None, None), |r| (r.1.clone(), r.2, r.3, r.4.clone()));
        let at = match agencies.iter().position(|a| a.agency_vendor_id == agency_id) {
            Some(i) => i,
            None => {
                agencies.push(AgencyReturn { agency_vendor_id: agency_id, agency_name: agency_name.unwrap_or_else(|| "No agency".into()), lines: Vec::new(), tax_cents: 0, paid_cents: 0 });
                agencies.len() - 1
            }
        };
        let a = &mut agencies[at];
        a.tax_cents = a.tax_cents.checked_add(tax).ok_or_else(overflow)?;
        a.lines.push(ReturnLine { tax_rate_id: rate_id, name, rate_ppm: ppm, taxable_sales_cents: taxable, tax_cents: tax });
    }

    // Payments in the period, by the agency of the rate they were paid for.
    let paid: Vec<(Option<i64>, Option<String>, i64)> = sqlx::query_as(
        "SELECT r.agency_vendor_id, v.name, SUM(p.amount_cents) FROM sales_tax_payments p JOIN journal_entries e ON e.id = p.journal_entry_id
         LEFT JOIN tax_rates r ON r.id = p.tax_rate_id LEFT JOIN vendors v ON v.id = r.agency_vendor_id
         WHERE e.is_voided = 0 AND p.payment_date BETWEEN ?1 AND ?2 GROUP BY r.agency_vendor_id",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    for (agency_id, agency_name, cents) in paid {
        match agencies.iter_mut().find(|a| a.agency_vendor_id == agency_id) {
            Some(a) => a.paid_cents = cents,
            None => agencies.push(AgencyReturn { agency_vendor_id: agency_id, agency_name: agency_name.unwrap_or_else(|| "No agency".into()), lines: Vec::new(), tax_cents: 0, paid_cents: cents }),
        }
    }
    agencies.sort_by(|a, b| (a.agency_vendor_id.is_none(), a.agency_name.to_lowercase()).cmp(&(b.agency_vendor_id.is_none(), b.agency_name.to_lowercase())));
    for a in &mut agencies {
        a.lines.sort_by(|x, y| x.name.to_lowercase().cmp(&y.name.to_lowercase()));
    }
    let tax = agencies.iter().try_fold(0i64, |s, a| s.checked_add(a.tax_cents)).ok_or_else(overflow)?;
    let paid = agencies.iter().try_fold(0i64, |s, a| s.checked_add(a.paid_cents)).ok_or_else(overflow)?;
    Ok(SalesTaxReturn { start_date: start, end_date: end, basis, agencies, tax_cents: tax, paid_cents: paid })
}

#[cfg(test)]
mod tests;
