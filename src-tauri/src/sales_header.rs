//! The header of a sales form beyond the customer's name: the Bill To and Ship To addresses, the
//! customer's P.O. number and the FOB point, and the customer's credit limit.
//!
//! Each invoice, sales receipt, estimate and credit memo keeps its own copy of the two addresses.
//! The copy is filled from the customer when the document is written (a job with no address of
//! its own uses the nearest customer above it that has one) and may be edited on the document
//! itself; editing the customer afterwards leaves documents already written as they were. A
//! document written before these fields existed has no copy (NULL) and prints the customer's
//! current billing address, as it always did.
//!
//! A credit limit never blocks a sale. [`credit_check`] says how far an invoice would take the
//! customer over it, and the invoice form asks before saving.

use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::db_err;

/// Longest P.O. number or FOB point, in characters (as on the printed form's header box).
pub const MAX_SHORT_FIELD: usize = 25;
/// Longest address block, in characters.
pub const MAX_ADDRESS: usize = 500;

/// The header fields a sales form sends. Flattened into each form's input.
///
/// For the addresses, `None` (left out) means "fill from the customer" and `Some("")` means the
/// user cleared it on this document.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SalesHeader {
    pub bill_to: Option<String>,
    pub ship_to: Option<String>,
    pub po_number: Option<String>,
    pub fob: Option<String>,
}

/// What a customer's new sales forms fill in: the address blocks as they will appear.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomerSalesDefaults {
    pub bill_to: String,
    pub ship_to: String,
    /// The credit limit that applies (the customer's own, or the nearest one above a job).
    pub credit_limit_cents: Option<i64>,
}

fn clean(text: Option<&str>) -> Option<String> {
    text.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string)
}

/// An address block as stored: lines trimmed, blank lines at the start and end dropped.
pub fn normalize_address(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let first = lines.iter().position(|l| !l.is_empty());
    let last = lines.iter().rposition(|l| !l.is_empty());
    match (first, last) {
        (Some(a), Some(b)) => lines[a..=b].join("\n"),
        _ => String::new(),
    }
}

/// The nearest non-blank value of each field, walking from the customer up through its parents.
async fn inherited<'e, E: sqlx::SqliteExecutor<'e>>(
    executor: E,
    customer_id: i64,
) -> Result<(String, Option<String>, Option<String>, Option<String>, Option<String>, Option<i64>), String> {
    type HeaderRow = (String, Option<String>, Option<String>, Option<String>, Option<String>, Option<i64>);
    let row: Option<HeaderRow> = sqlx::query_as(
        "WITH RECURSIVE up (id, parent_id, depth) AS (
             SELECT id, parent_id, 0 FROM customers WHERE id = ?1
             UNION ALL SELECT c.id, c.parent_id, up.depth + 1 FROM customers c JOIN up ON c.id = up.parent_id WHERE up.depth < 32
         ), chain AS (SELECT c.*, up.depth FROM up JOIN customers c ON c.id = up.id)
         SELECT (SELECT name FROM customers WHERE id = ?1),
                (SELECT company_name FROM chain WHERE trim(COALESCE(company_name, '')) <> '' ORDER BY depth LIMIT 1),
                (SELECT contact_name FROM chain WHERE trim(COALESCE(contact_name, '')) <> '' ORDER BY depth LIMIT 1),
                (SELECT billing_address FROM chain WHERE trim(COALESCE(billing_address, '')) <> '' ORDER BY depth LIMIT 1),
                (SELECT shipping_address FROM chain WHERE trim(COALESCE(shipping_address, '')) <> '' ORDER BY depth LIMIT 1),
                (SELECT credit_limit_cents FROM chain WHERE credit_limit_cents IS NOT NULL ORDER BY depth LIMIT 1)
         WHERE EXISTS (SELECT 1 FROM customers WHERE id = ?1)",
    )
    .bind(customer_id)
    .fetch_optional(executor)
    .await
    .map_err(db_err)?;
    row.ok_or_else(|| "PARTY_NOT_FOUND: that customer does not exist".to_string())
}

/// The Bill To and Ship To a new form for this customer starts with, and its credit limit.
///
/// Bill To is the company name (unless it only repeats the customer's name), the contact and the
/// billing address; Ship To is the company name, the contact and the shipping address, or blank
/// when there is no shipping address.
pub async fn customer_defaults<'e, E: sqlx::SqliteExecutor<'e>>(executor: E, customer_id: i64) -> Result<CustomerSalesDefaults, String> {
    let (name, company, contact, billing, shipping, credit_limit_cents) = inherited(executor, customer_id).await?;
    let leaf = name.rsplit(':').next().unwrap_or(&name).trim().to_string();
    let company = company.map(|c| c.trim().to_string()).filter(|c| !c.eq_ignore_ascii_case(&name) && !c.eq_ignore_ascii_case(&leaf));
    let block = |address: Option<&String>| {
        let Some(address) = address.map(|a| normalize_address(a)).filter(|a| !a.is_empty()) else {
            return String::new();
        };
        let mut lines: Vec<String> = company.iter().cloned().collect();
        lines.extend(contact.iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty() && !c.eq_ignore_ascii_case(&name)));
        lines.push(address);
        lines.join("\n")
    };
    let mut bill_to = block(billing.as_ref());
    if bill_to.is_empty() {
        // No billing address: still name the company on the form.
        bill_to = company.clone().unwrap_or_default();
    }
    Ok(CustomerSalesDefaults { bill_to, ship_to: block(shipping.as_ref()), credit_limit_cents })
}

pub async fn customer_sales_defaults(pool: &SqlitePool, customer_id: i64) -> Result<CustomerSalesDefaults, String> {
    customer_defaults(pool, customer_id).await
}

fn check_short(value: Option<&str>, what: &str, code: &str) -> Result<Option<String>, String> {
    let value = clean(value);
    if value.as_ref().is_some_and(|v| v.chars().count() > MAX_SHORT_FIELD) {
        return Err(format!("{code}_TOO_LONG: the {what} is at most {MAX_SHORT_FIELD} characters"));
    }
    Ok(value)
}

/// The header as it will be stored: addresses filled from the customer where left out.
pub(crate) async fn resolve(
    tx: &mut Transaction<'_, Sqlite>,
    customer_id: Option<i64>,
    header: &SalesHeader,
) -> Result<(String, String, Option<String>, Option<String>), String> {
    let defaults = match (customer_id, header.bill_to.is_none() || header.ship_to.is_none()) {
        (Some(id), true) => customer_defaults(&mut **tx, id).await?,
        _ => CustomerSalesDefaults::default(),
    };
    let bill_to = header.bill_to.as_deref().map_or(defaults.bill_to, normalize_address);
    let ship_to = header.ship_to.as_deref().map_or(defaults.ship_to, normalize_address);
    for (address, label) in [(&bill_to, "Bill To"), (&ship_to, "Ship To")] {
        if address.chars().count() > MAX_ADDRESS {
            return Err(format!("SALES_ADDRESS_TOO_LONG: the {label} address is at most {MAX_ADDRESS} characters"));
        }
    }
    let po_number = check_short(header.po_number.as_deref(), "P.O. number", "SALES_PO")?;
    let fob = check_short(header.fob.as_deref(), "FOB", "SALES_FOB")?;
    Ok((bill_to, ship_to, po_number, fob))
}

/// Writes a document's header copy, after its row is inserted (before a sales receipt posts).
/// `table` is `invoices`, `sales_receipts`, `estimates` or `credit_memos`.
pub(crate) async fn write(tx: &mut Transaction<'_, Sqlite>, table: &str, id: i64, header: &SalesHeader) -> Result<(), String> {
    debug_assert!(matches!(table, "invoices" | "sales_receipts" | "estimates" | "credit_memos" | "sales_orders"));
    let customer_id: Option<i64> =
        sqlx::query_scalar(&format!("SELECT customer_id FROM {table} WHERE id = ?1")).bind(id).fetch_one(&mut **tx).await.map_err(db_err)?;
    let (bill_to, ship_to, po_number, fob) = resolve(tx, customer_id, header).await?;
    sqlx::query(&format!("UPDATE {table} SET bill_to = ?1, ship_to = ?2, po_number = ?3, fob = ?4 WHERE id = ?5"))
        .bind(bill_to)
        .bind(ship_to)
        .bind(po_number)
        .bind(fob)
        .bind(id)
        .execute(&mut **tx)
        .await
        .map(|_| ())
        .map_err(db_err)
}

/// A document's header copy, to load into its form, print, or carry to another document. The
/// addresses are `None` only on documents written before they were kept.
pub async fn read<'e, E: sqlx::SqliteExecutor<'e>>(executor: E, table: &str, id: i64) -> Result<SalesHeader, String> {
    debug_assert!(matches!(table, "invoices" | "sales_receipts" | "estimates" | "credit_memos" | "sales_orders"));
    type AddressRow = (Option<String>, Option<String>, Option<String>, Option<String>);
    let row: Option<AddressRow> = sqlx::query_as(&format!("SELECT bill_to, ship_to, po_number, fob FROM {table} WHERE id = ?1"))
        .bind(id)
        .fetch_optional(executor)
        .await
        .map_err(db_err)?;
    let (bill_to, ship_to, po_number, fob) = row.unwrap_or_default();
    Ok(SalesHeader { bill_to, ship_to, po_number, fob })
}

// ------------------------------------------------------------------------------------------------
// Credit limit
// ------------------------------------------------------------------------------------------------

/// An invoice would take a customer over their credit limit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditLimitWarning {
    /// The customer whose limit applies (the job's parent when the limit is set there).
    pub customer_name: String,
    pub credit_limit_cents: i64,
    /// What they owe now (open invoices less unused credits, theirs and their jobs').
    pub balance_cents: i64,
    /// What they would owe with this invoice.
    pub new_balance_cents: i64,
}

/// Would an invoice of `amount_cents` (home currency) for `customer_name` take them over their
/// credit limit? `replacing_invoice_id` is the invoice an edit replaces; its open balance is left
/// out. `None` when there is no limit, the customer is new, or the invoice stays within it.
pub async fn credit_check(
    pool: &SqlitePool,
    customer_name: &str,
    amount_cents: i64,
    replacing_invoice_id: Option<i64>,
) -> Result<Option<CreditLimitWarning>, String> {
    let customer: Option<i64> =
        sqlx::query_scalar("SELECT id FROM customers WHERE name = ?1 COLLATE NOCASE").bind(customer_name.trim()).fetch_optional(pool).await.map_err(db_err)?;
    let Some(customer) = customer else { return Ok(None) };
    // The nearest customer, from this one upward, that has a limit.
    let holder: Option<(i64, String, i64)> = sqlx::query_as(
        "WITH RECURSIVE up (id, parent_id, depth) AS (
             SELECT id, parent_id, 0 FROM customers WHERE id = ?1
             UNION ALL SELECT c.id, c.parent_id, up.depth + 1 FROM customers c JOIN up ON c.id = up.parent_id WHERE up.depth < 32
         )
         SELECT c.id, c.name, c.credit_limit_cents FROM up JOIN customers c ON c.id = up.id
         WHERE c.credit_limit_cents IS NOT NULL ORDER BY up.depth LIMIT 1",
    )
    .bind(customer)
    .fetch_optional(pool)
    .await
    .map_err(db_err)?;
    let Some((holder, name, limit)) = holder else { return Ok(None) };
    // What the holder and every job under it owe, in the home currency.
    let balance: i64 = sqlx::query_scalar(
        "WITH RECURSIVE down (id, depth) AS (
             SELECT id, 0 FROM customers WHERE id = ?1
             UNION ALL SELECT c.id, down.depth + 1 FROM customers c JOIN down ON c.parent_id = down.id WHERE down.depth < 32
         )
         SELECT COALESCE((SELECT SUM(b.home_open_cents) FROM invoices i JOIN v_invoice_balances b ON b.invoice_id = i.id
                          WHERE i.customer_id IN (SELECT id FROM down) AND i.status IN ('UNPAID', 'PARTIAL') AND i.is_voided = 0
                            AND i.id IS NOT ?2), 0)
              - COALESCE((SELECT SUM(remaining_cents) FROM v_credit_memo_balances WHERE customer_id IN (SELECT id FROM down)), 0)",
    )
    .bind(holder)
    .bind(replacing_invoice_id)
    .fetch_one(pool)
    .await
    .map_err(db_err)?;
    let new_balance = balance.checked_add(amount_cents).ok_or("SALES_AMOUNT_TOO_LARGE: that amount is out of range")?;
    Ok((new_balance > limit).then_some(CreditLimitWarning {
        customer_name: name,
        credit_limit_cents: limit,
        balance_cents: balance,
        new_balance_cents: new_balance,
    }))
}

#[cfg(test)]
mod tests;
