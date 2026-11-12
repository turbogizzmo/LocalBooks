//! Transaction History: the transactions linked to one posted transaction (an invoice and the
//! payments that paid it, a payment and the deposit it went into, a bill and its bill payments,
//! a credit and what it was applied to).

use serde::Serialize;
use sqlx::SqlitePool;

use crate::ledger::{db_err, DISPLAY_SOURCE_TYPE};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub journal_entry_id: i64,
    pub entry_number: i64,
    pub entry_date: String,
    /// As journal entries show it (`INVOICE`, `CHECK`, `CARD_CREDIT`, ...).
    pub source_type: String,
    pub source_id: Option<i64>,
    pub memo: Option<String>,
    pub is_voided: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedTransaction {
    /// What the linked transaction is to this one: "Payment", "Deposit", "Invoice", ...
    pub relation: String,
    /// The amount the link carries (the payment applied, the line deposited, the credit used).
    pub amount_cents: i64,
    /// The link itself was voided (a voided payment or deposit line), even if the entry stands.
    pub link_voided: bool,
    #[serde(flatten)]
    pub entry: HistoryEntry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionHistory {
    #[serde(flatten)]
    pub entry: HistoryEntry,
    pub links: Vec<LinkedTransaction>,
}

fn select_entry(condition: &str) -> String {
    format!(
        "SELECT e.id AS journal_entry_id, e.entry_number, e.entry_date, {DISPLAY_SOURCE_TYPE} AS source_type, e.source_id,
                e.memo, e.is_voided
         FROM journal_entries e WHERE {condition}"
    )
}

/// The links of a document, by its stored source type: (linked source type, its id, relation, amount, link voided).
fn link_query(source_type: &str) -> Option<&'static str> {
    Some(match source_type {
        "INVOICE" => {
            "SELECT 'RECEIVE_PAYMENT', p.id, 'Payment', p.amount_cents, p.is_voided FROM invoice_payments p WHERE p.invoice_id = ?1
             UNION ALL
             SELECT 'CREDIT_MEMO', a.credit_memo_id, 'Credit applied', a.amount_cents, a.is_voided FROM credit_applications a WHERE a.invoice_id = ?1"
        }
        "RECEIVE_PAYMENT" => {
            "SELECT 'INVOICE', p.invoice_id, 'Invoice', p.amount_cents, p.is_voided FROM invoice_payments p WHERE p.id = ?1
             UNION ALL
             SELECT 'DEPOSIT', d.deposit_id, 'Deposit', d.amount_cents, d.is_voided FROM deposit_lines d WHERE d.invoice_payment_id = ?1"
        }
        "SALES_RECEIPT" => "SELECT 'DEPOSIT', d.deposit_id, 'Deposit', d.amount_cents, d.is_voided FROM deposit_lines d WHERE d.sales_receipt_id = ?1",
        "DEPOSIT" => {
            "SELECT 'RECEIVE_PAYMENT', d.invoice_payment_id, 'Payment', d.amount_cents, d.is_voided FROM deposit_lines d WHERE d.deposit_id = ?1 AND d.invoice_payment_id IS NOT NULL
             UNION ALL
             SELECT 'SALES_RECEIPT', d.sales_receipt_id, 'Sales receipt', d.amount_cents, d.is_voided FROM deposit_lines d WHERE d.deposit_id = ?1 AND d.sales_receipt_id IS NOT NULL"
        }
        "BILL" => {
            "SELECT 'BILL_PAYMENT', p.id, 'Bill payment', p.amount_cents, p.is_voided FROM bill_payments p WHERE p.bill_id = ?1
             UNION ALL
             SELECT 'VENDOR_CREDIT', a.vendor_credit_id, 'Credit applied', a.amount_cents, a.is_voided FROM vendor_credit_applications a WHERE a.bill_id = ?1"
        }
        "BILL_PAYMENT" => "SELECT 'BILL', p.bill_id, 'Bill', p.amount_cents, p.is_voided FROM bill_payments p WHERE p.id = ?1",
        "CREDIT_MEMO" => "SELECT 'INVOICE', a.invoice_id, 'Applied to invoice', a.amount_cents, a.is_voided FROM credit_applications a WHERE a.credit_memo_id = ?1",
        "VENDOR_CREDIT" => "SELECT 'BILL', a.bill_id, 'Applied to bill', a.amount_cents, a.is_voided FROM vendor_credit_applications a WHERE a.vendor_credit_id = ?1",
        _ => return None,
    })
}

/// The transaction behind `journal_entry_id` and the transactions linked to it, in date order.
/// Manual journal entries, checks, expenses and transfers have no links; the list is then empty.
pub async fn transaction_history(pool: &SqlitePool, journal_entry_id: i64) -> Result<TransactionHistory, String> {
    let entry: HistoryEntry = sqlx::query_as(&select_entry("e.id = ?1"))
        .bind(journal_entry_id)
        .fetch_optional(pool)
        .await
        .map_err(db_err)?
        .ok_or_else(|| format!("JE_NOT_FOUND: journal entry {journal_entry_id} does not exist"))?;
    let stored: (String, Option<i64>) =
        sqlx::query_as("SELECT source_type, source_id FROM journal_entries WHERE id = ?1").bind(journal_entry_id).fetch_one(pool).await.map_err(db_err)?;
    let mut links = Vec::new();
    if let (Some(query), Some(source_id)) = (link_query(&stored.0), stored.1) {
        let found: Vec<(String, i64, String, i64, bool)> = sqlx::query_as(query).bind(source_id).fetch_all(pool).await.map_err(db_err)?;
        let linked_entry =
            select_entry("e.source_type = ?1 AND e.source_id = ?2 AND e.reverses_entry_id IS NULL AND e.status = 'POSTED' ORDER BY e.id LIMIT 1");
        for (source_type, id, relation, amount_cents, link_voided) in found {
            // A document saved but never posted has no entry to show.
            let linked: Option<HistoryEntry> = sqlx::query_as(&linked_entry).bind(&source_type).bind(id).fetch_optional(pool).await.map_err(db_err)?;
            if let Some(linked) = linked {
                links.push(LinkedTransaction { relation, amount_cents, link_voided, entry: linked });
            }
        }
    }
    links.sort_by(|a, b| (&a.entry.entry_date, a.entry.entry_number).cmp(&(&b.entry.entry_date, b.entry.entry_number)));
    Ok(TransactionHistory { entry, links })
}
