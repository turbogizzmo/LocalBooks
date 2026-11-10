//! Merging two customers, vendors, items or accounts (migration 0099).
//!
//! Renaming one entry to another's name offers to merge them. Everything that names the one merged
//! away is pointed at the one kept: documents (their lines too), lists, memorized transactions and
//! settings. The merged entry stays, inactive, with `merged_into_id` set, so old records can still
//! say what it was. Posted journal rows never change: customers, vendors and items are not on
//! journal lines, and an account's posted lines keep it, so merging accounts posts one entry that
//! moves its balance into the account kept. One write transaction does it all, recorded in
//! `name_merges` and the audit trail; it can't be undone.

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::ledger::{db_err, post_entry_in_tx, rollback, EntrySource, JournalEntry, JournalLine};

/// What can be merged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeKind {
    Customer,
    Vendor,
    Item,
    Account,
}

impl MergeKind {
    pub fn parse(kind: &str) -> Result<Self, String> {
        match kind {
            "CUSTOMER" => Ok(Self::Customer),
            "VENDOR" => Ok(Self::Vendor),
            "ITEM" => Ok(Self::Item),
            "ACCOUNT" => Ok(Self::Account),
            other => Err(format!("MERGE_BAD_KIND: '{other}' can't be merged")),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Customer => "CUSTOMER",
            Self::Vendor => "VENDOR",
            Self::Item => "ITEM",
            Self::Account => "ACCOUNT",
        }
    }

    fn table(self) -> &'static str {
        match self {
            Self::Customer => "customers",
            Self::Vendor => "vendors",
            Self::Item => "products_services",
            Self::Account => "chart_of_accounts",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Customer => "customer",
            Self::Vendor => "vendor",
            Self::Item => "item",
            Self::Account => "account",
        }
    }
}

/// The columns that name an entry of each kind, outside its own table: (table, column). Invoices
/// also keep the customer's name, so they move separately.
const CUSTOMER_COLUMNS: &[(&str, &str)] = &[
    ("sales_receipts", "customer_id"),
    ("estimates", "customer_id"),
    ("credit_memos", "customer_id"),
    ("bounced_checks", "customer_id"),
    ("bounced_sales_receipts", "customer_id"),
    ("deposit_lines", "customer_id"),
    ("bill_lines", "customer_id"),
    ("expense_lines", "customer_id"),
    ("vendor_credit_lines", "customer_id"),
    ("time_activities", "customer_id"),
    ("mileage_trips", "customer_id"),
    ("todos", "customer_id"),
    ("purchase_order_lines", "customer_id"),
];
const VENDOR_COLUMNS: &[(&str, &str)] = &[
    ("bills", "vendor_id"),
    ("expenses", "vendor_id"),
    ("vendor_credits", "vendor_id"),
    ("deposit_lines", "vendor_id"),
    ("purchase_orders", "vendor_id"),
    ("ach_entries", "vendor_id"),
    ("tax_rates", "agency_vendor_id"),
    ("products_services", "preferred_vendor_id"),
    ("todos", "vendor_id"),
];
const ITEM_COLUMNS: &[(&str, &str)] = &[
    ("invoice_items", "product_service_id"),
    ("sales_receipt_lines", "product_service_id"),
    ("estimate_lines", "product_service_id"),
    ("credit_memo_lines", "product_service_id"),
    ("bill_lines", "product_service_id"),
    ("purchase_order_lines", "product_service_id"),
    ("time_activities", "product_service_id"),
    ("invoice_form_lines", "product_service_id"),
    ("sales_receipt_form_lines", "product_service_id"),
    ("estimate_form_lines", "product_service_id"),
    ("credit_memo_form_lines", "product_service_id"),
    ("purchase_form_lines", "product_service_id"),
    ("item_group_members", "product_id"),
];
/// An account's lists and settings: not postings, so they follow the merge.
const ACCOUNT_COLUMNS: &[(&str, &str)] = &[
    ("products_services", "income_account_id"),
    ("products_services", "expense_account_id"),
    ("purchase_form_lines", "account_id"),
    ("inventory_items", "asset_account_id"),
    ("inventory_items", "cogs_account_id"),
    ("bank_rules", "account_id"),
    ("tax_rates", "liability_account_id"),
    ("chart_of_accounts", "parent_id"),
    ("estimate_lines", "income_account_id"),
    ("estimate_form_lines", "account_id"),
    ("purchase_order_lines", "account_id"),
];

/// What a merge did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeResult {
    pub from_name: String,
    pub to_name: String,
    /// Rows pointed at the entry kept.
    pub moved: i64,
    /// For an account: the entry that moved its balance, and the amount (debit-positive).
    pub journal_entry_id: Option<i64>,
    pub balance_cents: i64,
}

/// The entry of `kind` with this name, other than `except`: what renaming `except` to `name` would
/// merge into. Accounts compare names among siblings, as the chart does.
pub async fn merge_target(pool: &SqlitePool, kind: MergeKind, except: i64, name: &str) -> Result<Option<(i64, String)>, String> {
    let name = name.trim();
    let sql = match kind {
        MergeKind::Account => "SELECT a.id, a.name FROM chart_of_accounts a JOIN chart_of_accounts e ON e.id = ?2
             WHERE a.name = ?1 COLLATE NOCASE AND a.id <> ?2 AND a.merged_into_id IS NULL AND a.parent_id IS e.parent_id"
            .to_string(),
        _ => format!("SELECT id, name FROM {} WHERE name = ?1 COLLATE NOCASE AND id <> ?2 AND merged_into_id IS NULL", kind.table()),
    };
    sqlx::query_as(&sql).bind(name).bind(except).fetch_optional(pool).await.map_err(db_err)
}

async fn exists(tx: &mut Transaction<'_, Sqlite>, table: &str) -> Result<bool, String> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)")
        .bind(table)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)
}

async fn repoint(tx: &mut Transaction<'_, Sqlite>, columns: &[(&str, &str)], from: i64, to: i64) -> Result<i64, String> {
    let mut moved = 0;
    for (table, column) in columns {
        if !exists(tx, table).await? {
            continue;
        }
        let done = sqlx::query(&format!("UPDATE {table} SET {column} = ?2 WHERE {column} = ?1"))
            .bind(from)
            .bind(to)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
        moved += i64::try_from(done.rows_affected()).unwrap_or(i64::MAX);
    }
    Ok(moved)
}

/// Memorized transactions name parties by name and lines by id; point those at the entry kept.
async fn repoint_memorized(tx: &mut Transaction<'_, Sqlite>, kind: MergeKind, from: (i64, &str), to: (i64, &str)) -> Result<i64, String> {
    let rows: Vec<(i64, String)> = sqlx::query_as("SELECT id, payload FROM recurring_templates").fetch_all(&mut **tx).await.map_err(db_err)?;
    let (name_keys, id_keys): (&[&str], &[&str]) = match kind {
        MergeKind::Customer => (&["customerName"], &["customerId"]),
        MergeKind::Vendor => (&["vendorName", "payeeName"], &["vendorId"]),
        MergeKind::Item => (&[], &["productServiceId", "productId"]),
        MergeKind::Account => (&[], &["accountId", "incomeAccountId", "paymentAccountId", "depositAccountId", "bankAccountId"]),
    };
    fn walk(v: &mut serde_json::Value, names: &[&str], ids: &[&str], from: (i64, &str), to: (i64, &str)) -> bool {
        let mut changed = false;
        match v {
            serde_json::Value::Object(map) => {
                for (k, value) in map.iter_mut() {
                    if names.contains(&k.as_str()) && value.as_str().is_some_and(|s| s.trim().eq_ignore_ascii_case(from.1.trim())) {
                        *value = serde_json::Value::String(to.1.to_string());
                        changed = true;
                    } else if ids.contains(&k.as_str()) && value.as_i64() == Some(from.0) {
                        *value = serde_json::Value::from(to.0);
                        changed = true;
                    } else {
                        changed |= walk(value, names, ids, from, to);
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    changed |= walk(item, names, ids, from, to);
                }
            }
            _ => {}
        }
        changed
    }
    let mut moved = 0;
    for (id, payload) in rows {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&payload) else { continue };
        if walk(&mut value, name_keys, id_keys, from, to) {
            sqlx::query("UPDATE recurring_templates SET payload = ?1 WHERE id = ?2")
                .bind(value.to_string())
                .bind(id)
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
            moved += 1;
        }
    }
    Ok(moved)
}

/// Merges `from_id` into `to_id`; the entry kept keeps its name and details.
pub async fn merge(pool: &SqlitePool, kind: MergeKind, from_id: i64, to_id: i64, today: NaiveDate) -> Result<MergeResult, String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    match write(&mut tx, kind, from_id, to_id, today).await {
        Ok(result) => crate::ledger::commit_write(tx).await.map(|()| result).map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

async fn write(tx: &mut Transaction<'_, Sqlite>, kind: MergeKind, from_id: i64, to_id: i64, today: NaiveDate) -> Result<MergeResult, String> {
    let noun = kind.noun();
    if from_id == to_id {
        return Err(format!("MERGE_SAME: a {noun} can't be merged into itself"));
    }
    let table = kind.table();
    let load = |id: i64| format!("SELECT name, merged_into_id IS NOT NULL FROM {table} WHERE id = {id}");
    let from: Option<(String, bool)> = sqlx::query_as(&load(from_id)).fetch_optional(&mut **tx).await.map_err(db_err)?;
    let to: Option<(String, bool)> = sqlx::query_as(&load(to_id)).fetch_optional(&mut **tx).await.map_err(db_err)?;
    let ((from_name, from_merged), (to_name, to_merged)) = match (from, to) {
        (Some(f), Some(t)) => (f, t),
        _ => return Err(format!("MERGE_NOT_FOUND: that {noun} does not exist")),
    };
    if from_merged || to_merged {
        return Err(format!("MERGE_ALREADY: that {noun} was already merged into another"));
    }

    match kind {
        MergeKind::Customer => {
            let jobs: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM customers WHERE parent_id = ?1").bind(from_id).fetch_one(&mut **tx).await.map_err(db_err)?;
            if jobs > 0 {
                return Err(format!("MERGE_HAS_JOBS: {from_name} has jobs; merge or move its jobs first"));
            }
        }
        MergeKind::Item => {
            let (from_type, to_type, stocked): (String, String, bool) = sqlx::query_as(
                "SELECT f.item_type, t.item_type, EXISTS (SELECT 1 FROM inventory_items WHERE product_id IN (?1, ?2))
                 FROM products_services f, products_services t WHERE f.id = ?1 AND t.id = ?2",
            )
            .bind(from_id)
            .bind(to_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_err)?;
            if from_type != to_type {
                return Err("MERGE_ITEM_TYPES: only items of the same type can be merged".into());
            }
            if stocked {
                return Err("MERGE_STOCK_ITEM: items tracked in stock can't be merged; their quantities and costs are kept per item".into());
            }
        }
        MergeKind::Account => {
            let (from_type, to_type, from_system, to_system, cycle): (String, String, bool, bool, bool) = sqlx::query_as(
                "WITH RECURSIVE up(id) AS (SELECT parent_id FROM chart_of_accounts WHERE id = ?2
                                         UNION SELECT a.parent_id FROM chart_of_accounts a JOIN up ON a.id = up.id WHERE a.parent_id IS NOT NULL)
                 SELECT f.account_type, t.account_type, f.is_system, t.is_system, EXISTS (SELECT 1 FROM up WHERE id = ?1)
                 FROM chart_of_accounts f, chart_of_accounts t WHERE f.id = ?1 AND t.id = ?2",
            )
            .bind(from_id)
            .bind(to_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_err)?;
            if from_type != to_type {
                return Err("MERGE_ACCOUNT_TYPES: only accounts of the same type can be merged".into());
            }
            if from_system || to_system {
                return Err("MERGE_SYSTEM_ACCOUNT: LocalBooks' own accounts (such as Accounts Receivable or Undeposited Funds) can't be merged".into());
            }
            if matches!(from_type.as_str(), "BANK" | "CREDIT_CARD") {
                return Err("MERGE_BANK_ACCOUNT: bank and credit card accounts keep their reconciliations, so they can't be merged; make the one you don't use inactive".into());
            }
            if cycle {
                return Err(format!("MERGE_SUBACCOUNT: {to_name} is a subaccount of {from_name}; merge the other way, or move it first"));
            }
        }
        MergeKind::Vendor => {}
    }

    // An account's posted lines keep it: one entry moves its balance to the account kept.
    let mut entry = None;
    let mut balance: i64 = 0;
    if kind == MergeKind::Account {
        balance = sqlx::query_scalar(
            "SELECT COALESCE(SUM(l.debit_cents - l.credit_cents), 0) FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
             WHERE l.account_id = ?1 AND e.status = 'POSTED'",
        )
        .bind(from_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)?;
        if balance != 0 {
            let amount = balance.checked_abs().ok_or("MERGE_TOO_LARGE: the balance is out of range")?;
            let description = format!("Merge account {from_name} into {to_name}");
            let lines = if balance > 0 {
                vec![JournalLine::debit(to_id, amount).with_memo(Some(&description)), JournalLine::credit(from_id, amount).with_memo(Some(&description))]
            } else {
                vec![JournalLine::debit(from_id, amount).with_memo(Some(&description)), JournalLine::credit(to_id, amount).with_memo(Some(&description))]
            };
            let journal = JournalEntry { id: None, date: today, description, lines, currency: None };
            entry = Some(post_entry_in_tx(tx, &journal, EntrySource::Journal).await?);
        }
    }

    let merge_id: i64 = sqlx::query_scalar(
        "INSERT INTO name_merges (kind, from_id, to_id, from_name, to_name, journal_entry_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING id",
    )
    .bind(kind.key())
    .bind(from_id)
    .bind(to_id)
    .bind(&from_name)
    .bind(&to_name)
    .bind(entry)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;

    let mut moved = 0;
    match kind {
        MergeKind::Customer => {
            // An invoice also keeps the customer's name: it moves with the id.
            let done = sqlx::query("UPDATE invoices SET customer_id = ?2, customer_name = ?3 WHERE customer_id = ?1")
                .bind(from_id)
                .bind(to_id)
                .bind(&to_name)
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
            moved += i64::try_from(done.rows_affected()).unwrap_or(i64::MAX);
            moved += repoint(tx, CUSTOMER_COLUMNS, from_id, to_id).await?;
            // Customer groups: the kept customer joins each group the merged one was in.
            sqlx::query("INSERT OR IGNORE INTO customer_group_members (group_id, customer_id) SELECT group_id, ?2 FROM customer_group_members WHERE customer_id = ?1")
                .bind(from_id)
                .bind(to_id)
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
            sqlx::query("DELETE FROM customer_group_members WHERE customer_id = ?1").bind(from_id).execute(&mut **tx).await.map_err(db_err)?;
        }
        MergeKind::Vendor => {
            moved += repoint(tx, VENDOR_COLUMNS, from_id, to_id).await?;
            // ACH bank details: the vendor kept keeps its own; it takes the merged one's otherwise.
            sqlx::query("UPDATE vendor_bank_accounts SET vendor_id = ?2 WHERE vendor_id = ?1 AND NOT EXISTS (SELECT 1 FROM vendor_bank_accounts WHERE vendor_id = ?2)")
                .bind(from_id)
                .bind(to_id)
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
        }
        MergeKind::Item => {
            moved += repoint(tx, ITEM_COLUMNS, from_id, to_id).await?;
            // Price levels: the item kept keeps its own price on a level; it takes the merged one's otherwise.
            sqlx::query(
                "UPDATE price_level_items SET product_id = ?2 WHERE product_id = ?1
                 AND NOT EXISTS (SELECT 1 FROM price_level_items k WHERE k.product_id = ?2 AND k.price_level_id = price_level_items.price_level_id)",
            )
            .bind(from_id)
            .bind(to_id)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
            sqlx::query("DELETE FROM price_level_items WHERE product_id = ?1").bind(from_id).execute(&mut **tx).await.map_err(db_err)?;
        }
        MergeKind::Account => {
            moved += repoint(tx, ACCOUNT_COLUMNS, from_id, to_id).await?;
            // Budgets: amounts for the same budget and period add up.
            sqlx::query(
                "INSERT INTO budget_amounts (budget_id, account_id, period, amount_cents)
                 SELECT budget_id, ?2, period, amount_cents FROM budget_amounts WHERE account_id = ?1
                 ON CONFLICT (budget_id, account_id, period) DO UPDATE SET amount_cents = amount_cents + excluded.amount_cents",
            )
            .bind(from_id)
            .bind(to_id)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
            sqlx::query("DELETE FROM budget_amounts WHERE account_id = ?1").bind(from_id).execute(&mut **tx).await.map_err(db_err)?;
            // Settings that name the account (discount, finance charge, reimbursed income...).
            sqlx::query("UPDATE system_preferences SET value = ?2 WHERE key LIKE '%account_id' AND value = ?1")
                .bind(from_id.to_string())
                .bind(to_id.to_string())
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
        }
    }
    moved += repoint_memorized(tx, kind, (from_id, &from_name), (to_id, &to_name)).await?;
    // Files kept with the record (the attachments library) move to the one kept, deleted ones too.
    let record_kind = match kind {
        MergeKind::Customer => "CUSTOMER",
        MergeKind::Vendor => "VENDOR",
        MergeKind::Item => "ITEM",
        MergeKind::Account => "ACCOUNT",
    };
    let files = sqlx::query("UPDATE record_attachments SET record_id = ?3 WHERE record_kind = ?1 AND record_id = ?2")
        .bind(record_kind)
        .bind(from_id)
        .bind(to_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    moved += i64::try_from(files.rows_affected()).unwrap_or(i64::MAX);

    sqlx::query(&format!("UPDATE {table} SET is_active = 0, merged_into_id = ?2 WHERE id = ?1"))
        .bind(from_id)
        .bind(to_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    sqlx::query("UPDATE name_merges SET applying = 0 WHERE id = ?1").bind(merge_id).execute(&mut **tx).await.map_err(db_err)?;
    Ok(MergeResult { from_name, to_name, moved, journal_entry_id: entry, balance_cents: balance })
}

#[cfg(test)]
mod tests;
