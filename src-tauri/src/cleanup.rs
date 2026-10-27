//! Accounting › Clean Up Books: the Client Data Review tools an accountant runs before closing a
//! client's year.
//!
//! * **Unapplied credits.** Credit memos and vendor credits with credit left, beside the same
//!   customer's open invoices or vendor's open bills, applied in one step (oldest first is suggested).
//!   A received payment or bill payment is always tied to its invoice or bill in LocalBooks, so the
//!   only payments that can be "unapplied" are journal entries that credit A/R (or debit A/P) without
//!   an invoice or bill, as imports write for money QuickBooks held unapplied. Those are listed for
//!   review only: a journal entry names no customer and cannot count toward an invoice's amount paid.
//! * **Undeposited Funds.** A payment sitting in Undeposited Funds whose money a deposit already
//!   banked on another line (usually straight to income: the double deposit) is matched to that line.
//!   The match posts Dr the line's account / Cr Undeposited Funds and records the link, so the payment
//!   counts as deposited. The deposit itself is not changed (it may be reconciled); voiding the
//!   correcting entry, or Undo, removes the match.
//! * **Write off invoices.** Each selected invoice gets a credit memo for its open balance to a chosen
//!   income (discounts) or expense (bad debt) account, applied to it at once: Dr that account / Cr A/R.
//! * **List changes.** Accounts, items, customers and vendors added, edited, renamed or made inactive in
//!   a date range: additions from each list's created_at, edits from the ledger audit trail
//!   (ACCOUNT_CHANGE and LIST_CHANGE events). LocalBooks has no list merge, so there are none to show.
//! * **Inventory.** The inventory asset accounts' balances against the stock valuation as of a date,
//!   with every transaction whose amount on those accounts differs from the stock it moved.
//!
//! Every fix runs in one transaction through the normal posting rules (balanced entries, the closing
//! date, the database triggers) and writes one CLEANUP event in the ledger audit trail naming what it
//! changed. Nothing is fixed without the caller asking for that exact change.

use std::collections::{BTreeMap, HashMap};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::credits::{apply_customer_credit_in_tx, apply_vendor_credit_in_tx, write_credit_memo, ApplyCreditInput, CreditMemoInput};
use crate::ledger::{db_err, format_cents, post_entry_in_tx, rollback, EntrySource, JournalEntry, JournalLine};
use crate::sales::SalesLineInput;

async fn run<T>(pool: &SqlitePool, work: impl for<'t> AsyncFnOnce(&mut Transaction<'t, Sqlite>) -> Result<T, String>) -> Result<T, String> {
    let mut tx = crate::ledger::begin_write(pool).await.map_err(db_err)?;
    match work(&mut tx).await {
        Ok(value) => crate::ledger::commit_write(tx).await.map(|()| value).map_err(db_err),
        Err(err) => Err(rollback(tx, err).await),
    }
}

/// Writes the CLEANUP event for one fix and returns its id.
async fn audit(tx: &mut Transaction<'_, Sqlite>, count: usize, payload: serde_json::Value) -> Result<i64, String> {
    sqlx::query_scalar("INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot) VALUES ('CLEANUP', ?1, ?2) RETURNING id")
        .bind(count as i64)
        .bind(payload.to_string())
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)
}

/// What a fix did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub count: usize,
    pub total_cents: i64,
    /// The CLEANUP record in the ledger audit trail.
    pub audit_record_id: i64,
}

// ------------------------------------------------------------------------------------------------
// 1. Unapplied credits
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PartyKind {
    Customer,
    Vendor,
}

/// A credit memo or vendor credit with credit left.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OpenCredit {
    pub id: i64,
    pub label: String,
    pub date: String,
    pub memo: Option<String>,
    pub total_cents: i64,
    pub remaining_cents: i64,
}

/// An open invoice or bill.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OpenDocument {
    pub id: i64,
    pub label: String,
    pub date: String,
    pub due_date: String,
    pub total_cents: i64,
    pub open_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreditApplication {
    pub kind: PartyKind,
    pub credit_id: i64,
    pub document_id: i64,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnappliedGroup {
    pub kind: PartyKind,
    pub party_id: i64,
    pub party_name: String,
    pub credits: Vec<OpenCredit>,
    pub documents: Vec<OpenDocument>,
    /// Oldest credit to the oldest due document first.
    pub suggested: Vec<CreditApplication>,
}

/// A journal entry that credits A/R or debits A/P with no invoice or bill behind it.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UnlinkedEntry {
    pub journal_entry_id: i64,
    pub entry_number: i64,
    pub entry_date: String,
    pub memo: Option<String>,
    /// `AR` (an unapplied customer payment or credit) or `AP` (an unapplied vendor payment or credit).
    pub side: String,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnappliedReview {
    pub groups: Vec<UnappliedGroup>,
    pub unlinked: Vec<UnlinkedEntry>,
}

/// Allocates credits (in order) to documents (in order) until one side runs out.
fn suggest(kind: PartyKind, credits: &[OpenCredit], documents: &[OpenDocument]) -> Vec<CreditApplication> {
    let mut open: Vec<i64> = documents.iter().map(|d| d.open_cents).collect();
    let mut out = Vec::new();
    for credit in credits {
        let mut left = credit.remaining_cents;
        for (doc, owed) in documents.iter().zip(open.iter_mut()) {
            if left == 0 {
                break;
            }
            let amount = left.min(*owed);
            if amount > 0 {
                out.push(CreditApplication { kind, credit_id: credit.id, document_id: doc.id, amount_cents: amount });
                left -= amount;
                *owed -= amount;
            }
        }
    }
    out
}

pub async fn unapplied(pool: &SqlitePool) -> Result<UnappliedReview, String> {
    let mut groups = Vec::new();
    let customers: Vec<(i64, String)> = sqlx::query_as(
        "SELECT DISTINCT c.id, c.name FROM credit_memos m JOIN customers c ON c.id = m.customer_id
         JOIN v_credit_memo_balances b ON b.credit_memo_id = m.id
         WHERE b.remaining_cents > 0 AND m.journal_entry_id IS NOT NULL ORDER BY c.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    for (party_id, party_name) in customers {
        let credits: Vec<OpenCredit> = sqlx::query_as(
            "SELECT m.id, 'Credit memo #' || m.credit_number AS label, m.credit_date AS date, m.memo, m.total_cents, b.remaining_cents
             FROM credit_memos m JOIN v_credit_memo_balances b ON b.credit_memo_id = m.id
             WHERE m.customer_id = ?1 AND b.remaining_cents > 0 AND m.journal_entry_id IS NOT NULL ORDER BY m.credit_date, m.id",
        )
        .bind(party_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
        let documents: Vec<OpenDocument> = sqlx::query_as(
            "SELECT id, 'Invoice #' || invoice_number AS label, issue_date AS date, due_date, total_cents, total_cents - amount_paid_cents AS open_cents
             FROM invoices WHERE customer_id = ?1 AND is_voided = 0 AND status IN ('UNPAID', 'PARTIAL') AND currency_code IS NULL
             ORDER BY due_date, issue_date, id",
        )
        .bind(party_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
        let suggested = suggest(PartyKind::Customer, &credits, &documents);
        groups.push(UnappliedGroup { kind: PartyKind::Customer, party_id, party_name, credits, documents, suggested });
    }
    let vendors: Vec<(i64, String)> = sqlx::query_as(
        "SELECT DISTINCT v.id, v.name FROM vendor_credits c JOIN vendors v ON v.id = c.vendor_id
         JOIN v_vendor_credit_balances b ON b.vendor_credit_id = c.id
         WHERE b.remaining_cents > 0 AND c.journal_entry_id IS NOT NULL ORDER BY v.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    for (party_id, party_name) in vendors {
        let credits: Vec<OpenCredit> = sqlx::query_as(
            "SELECT c.id, 'Vendor credit' || COALESCE(' #' || c.reference, ' of ' || c.credit_date) AS label, c.credit_date AS date, c.memo,
                    c.total_cents, b.remaining_cents
             FROM vendor_credits c JOIN v_vendor_credit_balances b ON b.vendor_credit_id = c.id
             WHERE c.vendor_id = ?1 AND b.remaining_cents > 0 AND c.journal_entry_id IS NOT NULL ORDER BY c.credit_date, c.id",
        )
        .bind(party_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
        let documents: Vec<OpenDocument> = sqlx::query_as(
            "SELECT id, 'Bill' || COALESCE(' #' || bill_number, ' of ' || bill_date) AS label, bill_date AS date, due_date, total_cents,
                    total_cents - amount_paid_cents AS open_cents
             FROM bills WHERE vendor_id = ?1 AND is_voided = 0 AND status IN ('UNPAID', 'PARTIAL') ORDER BY due_date, bill_date, id",
        )
        .bind(party_id)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
        let suggested = suggest(PartyKind::Vendor, &credits, &documents);
        groups.push(UnappliedGroup { kind: PartyKind::Vendor, party_id, party_name, credits, documents, suggested });
    }

    // Live journal entries (not voids, not voided) whose net effect on A/R is a credit or on A/P a debit.
    let unlinked: Vec<UnlinkedEntry> = sqlx::query_as(
        "SELECT e.id AS journal_entry_id, e.entry_number, e.entry_date, e.memo, x.side, x.amount_cents
         FROM (SELECT l.journal_entry_id,
                      CASE a.account_type WHEN 'ACCOUNTS_RECEIVABLE' THEN 'AR' ELSE 'AP' END AS side,
                      CASE a.account_type WHEN 'ACCOUNTS_RECEIVABLE' THEN SUM(l.credit_cents - l.debit_cents)
                                          ELSE SUM(l.debit_cents - l.credit_cents) END AS amount_cents
               FROM journal_lines l JOIN chart_of_accounts a ON a.id = l.account_id
               WHERE a.account_type IN ('ACCOUNTS_RECEIVABLE', 'ACCOUNTS_PAYABLE')
               GROUP BY l.journal_entry_id, a.account_type) x
         JOIN journal_entries e ON e.id = x.journal_entry_id
         WHERE x.amount_cents > 0 AND e.source_type = 'JOURNAL' AND e.status = 'POSTED' AND e.is_voided = 0
           AND e.reverses_entry_id IS NULL
           AND NOT EXISTS (SELECT 1 FROM fx_revaluations r WHERE e.id IN (r.journal_entry_id, r.reversal_entry_id))
         ORDER BY x.side, e.entry_date, e.entry_number",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(UnappliedReview { groups, unlinked })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyCreditsInput {
    pub applied_date: NaiveDate,
    pub applications: Vec<CreditApplication>,
}

/// Applies every listed credit, all or nothing, and records them as one CLEANUP event.
pub async fn apply_credits(pool: &SqlitePool, input: &ApplyCreditsInput) -> Result<CleanupResult, String> {
    if input.applications.is_empty() {
        return Err("CLEANUP_NOTHING: choose at least one credit to apply".into());
    }
    run(pool, async |tx| {
        // Applying moves no money, but it changes what is owed as of that date, so a closed period stays closed.
        crate::closing::check_open(tx, input.applied_date).await?;
        let mut total: i64 = 0;
        let mut rows = Vec::with_capacity(input.applications.len());
        for a in &input.applications {
            let apply = ApplyCreditInput { credit_id: a.credit_id, document_id: a.document_id, amount_cents: a.amount_cents, applied_date: input.applied_date };
            match a.kind {
                PartyKind::Customer => apply_customer_credit_in_tx(tx, &apply).await?,
                PartyKind::Vendor => apply_vendor_credit_in_tx(tx, &apply).await?,
            }
            total += a.amount_cents;
            rows.push(serde_json::json!({ "kind": a.kind, "creditId": a.credit_id, "documentId": a.document_id, "amountCents": a.amount_cents }));
        }
        let payload = serde_json::json!({ "event": "APPLY_CREDITS", "appliedDate": input.applied_date, "totalCents": total, "applications": rows });
        let audit_record_id = audit(tx, rows.len(), payload).await?;
        Ok(CleanupResult { count: rows.len(), total_cents: total, audit_record_id })
    })
    .await
}

// ------------------------------------------------------------------------------------------------
// 2. Undeposited Funds
// ------------------------------------------------------------------------------------------------

/// A deposit line that put money in the bank without a payment from Undeposited Funds.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DepositCandidate {
    pub deposit_id: i64,
    pub deposit_line_id: i64,
    pub deposit_date: String,
    pub bank_account_name: String,
    pub deposit_memo: Option<String>,
    pub line_memo: Option<String>,
    pub line_account_id: i64,
    pub line_account_name: String,
    pub line_amount_cents: i64,
    /// What is left of the line after earlier matches.
    pub available_cents: i64,
    /// The deposit's or line's memo names this customer.
    #[sqlx(skip)]
    pub names_customer: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndepositedCustomer {
    pub customer_name: Option<String>,
    pub payments: Vec<crate::banking::UndepositedItem>,
    pub total_cents: i64,
    /// Lines that name this customer first, then lines that name none of the customers listed.
    pub candidates: Vec<DepositCandidate>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct UndepositedMatch {
    pub id: i64,
    /// `PAYMENT` or `SALES_RECEIPT`
    pub kind: String,
    pub item_id: i64,
    pub item_date: String,
    pub customer_name: Option<String>,
    pub deposit_id: i64,
    pub deposit_date: String,
    pub line_account_name: String,
    pub amount_cents: i64,
    pub journal_entry_id: Option<i64>,
    pub entry_number: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndepositedReview {
    pub customers: Vec<UndepositedCustomer>,
    pub matches: Vec<UndepositedMatch>,
}

const CANDIDATES_SELECT: &str = "SELECT d.id AS deposit_id, l.id AS deposit_line_id, d.deposit_date, b.name AS bank_account_name,
        d.memo AS deposit_memo, l.memo AS line_memo, l.account_id AS line_account_id, a.name AS line_account_name,
        l.amount_cents AS line_amount_cents,
        l.amount_cents - COALESCE((SELECT SUM(m.amount_cents) FROM undeposited_fund_matches m WHERE m.deposit_line_id = l.id AND m.is_voided = 0), 0)
            AS available_cents
 FROM deposit_lines l JOIN deposits d ON d.id = l.deposit_id
 JOIN chart_of_accounts b ON b.id = d.bank_account_id JOIN chart_of_accounts a ON a.id = l.account_id
 WHERE l.is_voided = 0 AND d.is_voided = 0 AND d.journal_entry_id IS NOT NULL
   AND l.invoice_payment_id IS NULL AND l.sales_receipt_id IS NULL";

const MATCHES_SELECT: &str = "SELECT m.id, CASE WHEN m.invoice_payment_id IS NULL THEN 'SALES_RECEIPT' ELSE 'PAYMENT' END AS kind,
        COALESCE(m.invoice_payment_id, m.sales_receipt_id) AS item_id,
        COALESCE(p.payment_date, r.receipt_date) AS item_date, COALESCE(i.customer_name, c.name) AS customer_name,
        m.deposit_id, d.deposit_date, a.name AS line_account_name, m.amount_cents, m.journal_entry_id, e.entry_number
 FROM undeposited_fund_matches m
 JOIN deposits d ON d.id = m.deposit_id JOIN deposit_lines l ON l.id = m.deposit_line_id JOIN chart_of_accounts a ON a.id = l.account_id
 LEFT JOIN invoice_payments p ON p.id = m.invoice_payment_id LEFT JOIN invoices i ON i.id = p.invoice_id
 LEFT JOIN sales_receipts r ON r.id = m.sales_receipt_id LEFT JOIN customers c ON c.id = r.customer_id
 LEFT JOIN journal_entries e ON e.id = m.journal_entry_id";

fn mentions(candidate: &DepositCandidate, name: &str) -> bool {
    let name = name.to_lowercase();
    [&candidate.deposit_memo, &candidate.line_memo].iter().any(|m| m.as_deref().is_some_and(|m| m.to_lowercase().contains(&name)))
}

pub async fn undeposited(pool: &SqlitePool) -> Result<UndepositedReview, String> {
    let items = crate::banking::list_undeposited(pool).await?;
    let lines: Vec<DepositCandidate> = sqlx::query_as::<_, DepositCandidate>(&format!("{CANDIDATES_SELECT} ORDER BY d.deposit_date, l.id"))
        .fetch_all(pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .filter(|l| l.available_cents > 0)
        .collect();
    let mut by_customer: BTreeMap<Option<String>, Vec<crate::banking::UndepositedItem>> = BTreeMap::new();
    for item in items {
        by_customer.entry(item.customer_name.clone()).or_default().push(item);
    }
    let names: Vec<&String> = by_customer.keys().flatten().collect();
    let unnamed: Vec<&DepositCandidate> = lines.iter().filter(|l| !names.iter().any(|n| mentions(l, n))).collect();
    let customers = by_customer
        .iter()
        .map(|(name, payments)| {
            let smallest = payments.iter().map(|p| p.amount_cents).min().unwrap_or(0);
            let named = lines.iter().filter(|l| name.as_deref().is_some_and(|n| mentions(l, n)));
            let candidates = named
                .map(|l| DepositCandidate { names_customer: true, ..l.clone() })
                .chain(unnamed.iter().map(|l| (*l).clone()))
                .filter(|l| l.available_cents >= smallest)
                .collect();
            UndepositedCustomer { customer_name: name.clone(), total_cents: payments.iter().map(|p| p.amount_cents).sum(), payments: payments.clone(), candidates }
        })
        .collect();
    let matches = sqlx::query_as::<_, UndepositedMatch>(&format!("{MATCHES_SELECT} WHERE m.is_voided = 0 ORDER BY m.id DESC"))
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    Ok(UndepositedReview { customers, matches })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchItem {
    /// `PAYMENT` or `SALES_RECEIPT`
    pub kind: String,
    pub id: i64,
    pub deposit_line_id: i64,
    /// Date of the correcting entry; the deposit's date is usual, as that is when the money was counted twice.
    pub entry_date: NaiveDate,
}

/// Matches payments in Undeposited Funds to the deposit lines that already banked them.
pub async fn match_undeposited(pool: &SqlitePool, items: &[MatchItem]) -> Result<CleanupResult, String> {
    if items.is_empty() {
        return Err("CLEANUP_NOTHING: choose at least one payment to match".into());
    }
    run(pool, async |tx| {
        let mut total = 0;
        let mut rows = Vec::with_capacity(items.len());
        for item in items {
            let row: Option<(i64, i64, bool, Option<String>)> = match item.kind.as_str() {
                "PAYMENT" => sqlx::query_as(
                    "SELECT p.deposit_account_id, COALESCE(p.home_amount_cents, p.amount_cents), a.is_system = 1 AND a.account_type = 'OTHER_CURRENT_ASSET', i.customer_name
                     FROM invoice_payments p JOIN invoices i ON i.id = p.invoice_id JOIN chart_of_accounts a ON a.id = p.deposit_account_id
                     WHERE p.id = ?1 AND p.is_voided = 0 AND p.journal_entry_id IS NOT NULL",
                ),
                "SALES_RECEIPT" => sqlx::query_as(
                    "SELECT r.deposit_account_id, r.total_cents, a.is_system = 1 AND a.account_type = 'OTHER_CURRENT_ASSET', c.name
                     FROM sales_receipts r LEFT JOIN customers c ON c.id = r.customer_id JOIN chart_of_accounts a ON a.id = r.deposit_account_id
                     WHERE r.id = ?1 AND r.is_voided = 0 AND r.journal_entry_id IS NOT NULL",
                ),
                other => return Err(format!("UF_BAD_ITEM: unknown kind '{other}'")),
            }
            .bind(item.id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
            let (undeposited, amount, in_undeposited, customer) = row.ok_or("UF_NOT_FOUND: that payment does not exist or was voided")?;
            if !in_undeposited {
                return Err("UF_NOT_UNDEPOSITED: that payment went straight to a bank account, not Undeposited Funds".into());
            }
            let column = if item.kind == "PAYMENT" { "invoice_payment_id" } else { "sales_receipt_id" };
            let matched: bool = sqlx::query_scalar(&format!("SELECT EXISTS (SELECT 1 FROM undeposited_fund_matches WHERE {column} = ?1 AND is_voided = 0)"))
                .bind(item.id)
                .fetch_one(&mut **tx)
                .await
                .map_err(db_err)?;
            if matched {
                return Err("UF_ALREADY_MATCHED: that payment is already matched to a deposit".into());
            }
            let line: Option<(i64, i64, Option<i64>, String)> = sqlx::query_as(
                "SELECT l.deposit_id, l.account_id, l.class_id, d.deposit_date FROM deposit_lines l JOIN deposits d ON d.id = l.deposit_id WHERE l.id = ?1",
            )
            .bind(item.deposit_line_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
            let (deposit_id, line_account, class_id, deposit_date) = line.ok_or("UF_BAD_LINE: that deposit line does not exist")?;
            let who = customer.as_deref().unwrap_or("customer");
            // A line that already took the money out of Undeposited Funds needs only the link.
            let entry_id = if line_account == undeposited {
                None
            } else {
                let entry = JournalEntry {
                    id: None,
                    date: item.entry_date,
                    description: format!("Undeposited Funds cleanup: {who} payment was already in the deposit of {deposit_date}"),
                    lines: vec![JournalLine::debit(line_account, amount).with_class(class_id), JournalLine::credit(undeposited, amount)],
                    currency: None,
                };
                Some(post_entry_in_tx(tx, &entry, EntrySource::Journal).await?)
            };
            let (payment, receipt) = if item.kind == "PAYMENT" { (Some(item.id), None) } else { (None, Some(item.id)) };
            let match_id: i64 = sqlx::query_scalar(
                "INSERT INTO undeposited_fund_matches (invoice_payment_id, sales_receipt_id, deposit_id, deposit_line_id, amount_cents, journal_entry_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING id",
            )
            .bind(payment)
            .bind(receipt)
            .bind(deposit_id)
            .bind(item.deposit_line_id)
            .bind(amount)
            .bind(entry_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(|e| {
                let msg = db_err(e);
                if msg.contains("UNIQUE") { "UF_ALREADY_MATCHED: that payment is already matched to a deposit".into() } else { msg }
            })?;
            total += amount;
            rows.push(serde_json::json!({
                "matchId": match_id, "kind": item.kind, "id": item.id, "customer": customer, "depositId": deposit_id,
                "depositLineId": item.deposit_line_id, "amountCents": amount, "journalEntryId": entry_id,
            }));
        }
        let payload = serde_json::json!({ "event": "MATCH_UNDEPOSITED", "totalCents": total, "matches": rows });
        let audit_record_id = audit(tx, rows.len(), payload).await?;
        Ok(CleanupResult { count: rows.len(), total_cents: total, audit_record_id })
    })
    .await
}

/// Undoes a match: voids its correcting entry (on that entry's date) and puts the payment back in
/// Undeposited Funds.
pub async fn unmatch_undeposited(pool: &SqlitePool, match_id: i64) -> Result<CleanupResult, String> {
    run(pool, async |tx| {
        let row: Option<(Option<i64>, i64)> =
            sqlx::query_as("SELECT journal_entry_id, amount_cents FROM undeposited_fund_matches WHERE id = ?1 AND is_voided = 0")
                .bind(match_id)
                .fetch_optional(&mut **tx)
                .await
                .map_err(db_err)?;
        let (entry, amount) = row.ok_or("UF_MATCH_NOT_FOUND: that match was already undone")?;
        match entry {
            // The trigger on the entry's void marks the match undone.
            Some(entry) => crate::ledger::void_in_tx(tx, entry).await?,
            None => {
                sqlx::query("UPDATE undeposited_fund_matches SET is_voided = 1 WHERE id = ?1").bind(match_id).execute(&mut **tx).await.map_err(db_err)?;
            }
        }
        let payload = serde_json::json!({ "event": "UNMATCH_UNDEPOSITED", "matchId": match_id, "journalEntryId": entry, "amountCents": amount });
        let audit_record_id = audit(tx, 1, payload).await?;
        Ok(CleanupResult { count: 1, total_cents: amount, audit_record_id })
    })
    .await
}

// ------------------------------------------------------------------------------------------------
// 3. Write off invoices
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteOffFilter {
    pub as_of: NaiveDate,
    /// Days past the due date, as of `as_of`.
    #[serde(default)]
    pub min_days_overdue: i64,
    /// Only balances of at most this much (small leftover balances).
    #[serde(default)]
    pub max_open_cents: Option<i64>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct WriteOffCandidate {
    pub invoice_id: i64,
    pub invoice_number: i64,
    pub customer_name: String,
    pub issue_date: String,
    pub due_date: String,
    pub days_overdue: i64,
    pub total_cents: i64,
    pub open_cents: i64,
}

/// Open home-currency invoices matching the filter, oldest due first.
pub async fn write_off_candidates(pool: &SqlitePool, filter: &WriteOffFilter) -> Result<Vec<WriteOffCandidate>, String> {
    sqlx::query_as(
        "SELECT i.id AS invoice_id, i.invoice_number, c.name AS customer_name, i.issue_date, i.due_date,
                CAST(julianday(?1) - julianday(i.due_date) AS INTEGER) AS days_overdue,
                i.total_cents, i.total_cents - i.amount_paid_cents AS open_cents
         FROM invoices i JOIN customers c ON c.id = i.customer_id
         WHERE i.is_voided = 0 AND i.status IN ('UNPAID', 'PARTIAL') AND i.currency_code IS NULL
           AND i.issue_date <= ?1
           AND julianday(?1) - julianday(i.due_date) >= ?2
           AND (?3 IS NULL OR i.total_cents - i.amount_paid_cents <= ?3)
         ORDER BY i.due_date, i.invoice_number",
    )
    .bind(filter.as_of)
    .bind(filter.min_days_overdue)
    .bind(filter.max_open_cents)
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteOffInput {
    pub invoice_ids: Vec<i64>,
    /// An income account (discounts given) or expense account (bad debt).
    pub account_id: i64,
    pub write_off_date: NaiveDate,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteOffResult {
    pub credit_memo_ids: Vec<i64>,
    pub count: usize,
    pub total_cents: i64,
    pub audit_record_id: i64,
}

/// Writes off each invoice's open balance with a credit memo to the chosen account, applied to it.
pub async fn write_off(pool: &SqlitePool, input: &WriteOffInput) -> Result<WriteOffResult, String> {
    if input.invoice_ids.is_empty() {
        return Err("CLEANUP_NOTHING: choose at least one invoice to write off".into());
    }
    let mut seen = std::collections::HashSet::new();
    if !input.invoice_ids.iter().all(|id| seen.insert(*id)) {
        return Err("WO_DUPLICATE: an invoice is listed twice".into());
    }
    let memo = input.memo.as_deref().map(str::trim).filter(|m| !m.is_empty());
    run(pool, async |tx| {
        let account: Option<(String, bool, bool)> = sqlx::query_as(
            "SELECT t.classification, a.is_active, a.is_system FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type WHERE a.id = ?1",
        )
        .bind(input.account_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(db_err)?;
        match account {
            Some((class, true, false)) if class == "INCOME" || class == "EXPENSE" => {}
            Some((_, false, _)) => return Err("WO_INACTIVE_ACCOUNT: that account is inactive".into()),
            _ => return Err("WO_BAD_ACCOUNT: write invoices off to an Income account (discounts) or an Expense account (bad debt)".into()),
        }
        let mut memos = Vec::with_capacity(input.invoice_ids.len());
        let mut rows = Vec::with_capacity(input.invoice_ids.len());
        let mut total = 0;
        for &invoice_id in &input.invoice_ids {
            let invoice: Option<(i64, String, i64, String, bool, Option<String>)> = sqlx::query_as(
                "SELECT i.invoice_number, c.name, i.total_cents - i.amount_paid_cents, i.status, i.is_voided, i.currency_code
                 FROM invoices i JOIN customers c ON c.id = i.customer_id WHERE i.id = ?1",
            )
            .bind(invoice_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db_err)?;
            let (number, customer, open, status, voided, currency) = invoice.ok_or_else(|| format!("WO_NOT_FOUND: invoice {invoice_id} does not exist"))?;
            if voided || !(status == "UNPAID" || status == "PARTIAL") {
                return Err(format!("WO_NOT_OPEN: invoice #{number} has nothing left to write off"));
            }
            if currency.is_some() {
                return Err(format!("WO_FOREIGN: invoice #{number} is in {}; write-offs are in the home currency", currency.unwrap_or_default()));
            }
            let description = format!("Write-off of invoice #{number}");
            let credit = CreditMemoInput {
                customer_name: customer.clone(),
                credit_date: input.write_off_date,
                memo: Some(memo.map_or_else(|| description.clone(), |m| format!("{description}: {m}"))),
                refund_account_id: None,
                lines: vec![SalesLineInput {
                    product_service_id: None,
                    description,
                    quantity_e4: crate::ledger::QUANTITY_SCALE,
                    rate_cents: open,
                    income_account_id: input.account_id,
                    class_id: None,
                    is_taxable: false,
                }],
                tax_rate_id: None,
            };
            let memo_id = write_credit_memo(tx, &credit, true).await?;
            apply_customer_credit_in_tx(tx, &ApplyCreditInput { credit_id: memo_id, document_id: invoice_id, amount_cents: open, applied_date: input.write_off_date }).await?;
            total += open;
            memos.push(memo_id);
            rows.push(serde_json::json!({ "invoiceId": invoice_id, "invoiceNumber": number, "customer": customer, "creditMemoId": memo_id, "amountCents": open }));
        }
        let payload = serde_json::json!({
            "event": "WRITE_OFF_INVOICES", "accountId": input.account_id, "date": input.write_off_date, "totalCents": total, "invoices": rows,
        });
        let audit_record_id = audit(tx, rows.len(), payload).await?;
        Ok(WriteOffResult { count: memos.len(), credit_memo_ids: memos, total_cents: total, audit_record_id })
    })
    .await
}

// ------------------------------------------------------------------------------------------------
// 4. List changes
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListChange {
    /// `ACCOUNT`, `ITEM`, `CUSTOMER` or `VENDOR`
    pub list: String,
    pub record_id: i64,
    pub name: String,
    /// `ADDED`, `RENAMED`, `MADE_INACTIVE`, `MADE_ACTIVE` or `EDITED`
    pub change: String,
    pub occurred_at: String,
    pub actor: Option<String>,
    pub details: String,
}

fn describe_edit(before: &serde_json::Value, after: &serde_json::Value, changed: Vec<String>) -> (String, String) {
    let field = |v: &serde_json::Value, k: &str| v.get(k).cloned().unwrap_or(serde_json::Value::Null);
    let text = |v: serde_json::Value| match v {
        serde_json::Value::String(s) => s,
        serde_json::Value::Null => "none".into(),
        other => other.to_string(),
    };
    let mut details = Vec::new();
    let (old_name, new_name) = (field(before, "name"), field(after, "name"));
    if old_name != new_name {
        details.push(format!("name {} → {}", text(old_name), text(new_name)));
    }
    let (old_active, new_active) = (field(before, "isActive"), field(after, "isActive"));
    let flag = |v: &serde_json::Value| v.as_i64().map(|n| n != 0).or_else(|| v.as_bool());
    let change = match (flag(&old_active), flag(&new_active)) {
        (Some(true), Some(false)) => "MADE_INACTIVE",
        (Some(false), Some(true)) => "MADE_ACTIVE",
        _ if !details.is_empty() => "RENAMED",
        _ => "EDITED",
    };
    for key in ["accountNumber", "accountType", "parentId"] {
        if field(before, key) != field(after, key) {
            let label = match key {
                "accountNumber" => "number",
                "accountType" => "type",
                _ => "parent",
            };
            if key == "parentId" {
                details.push("parent account".into());
            } else {
                details.push(format!("{label} {} → {}", text(field(before, key)), text(field(after, key))));
            }
        }
    }
    let (old_rate, new_rate) = (field(before, "rateCents"), field(after, "rateCents"));
    if old_rate != new_rate {
        if let (Some(o), Some(n)) = (old_rate.as_i64(), new_rate.as_i64()) {
            details.push(format!("price {} → {}", format_cents(o), format_cents(n)));
        }
    }
    details.extend(changed.into_iter().filter(|c| c != "price"));
    (change.to_string(), details.join("; "))
}

/// Additions and edits to the four lists dated `start` to `end` (local dates).
pub async fn list_changes(pool: &SqlitePool, start: NaiveDate, end: NaiveDate) -> Result<Vec<ListChange>, String> {
    if end < start {
        return Err("CLEANUP_BAD_RANGE: the end date is before the start date".into());
    }
    let added: Vec<(String, i64, String, String)> = sqlx::query_as(
        "SELECT 'ACCOUNT', id, COALESCE(account_number || ' ', '') || name, created_at FROM chart_of_accounts WHERE date(created_at, 'localtime') BETWEEN ?1 AND ?2
         UNION ALL SELECT 'ITEM', id, name, created_at FROM products_services WHERE date(created_at, 'localtime') BETWEEN ?1 AND ?2
         UNION ALL SELECT 'CUSTOMER', id, name, created_at FROM customers WHERE date(created_at, 'localtime') BETWEEN ?1 AND ?2
         UNION ALL SELECT 'VENDOR', id, name, created_at FROM vendors WHERE date(created_at, 'localtime') BETWEEN ?1 AND ?2",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut out: Vec<ListChange> = added
        .into_iter()
        .map(|(list, record_id, name, occurred_at)| ListChange { list, record_id, name, change: "ADDED".into(), occurred_at, actor: None, details: String::new() })
        .collect();

    let edits: Vec<(String, i64, String, Option<String>, String)> = sqlx::query_as(
        "SELECT action_type, target_entity_id, timestamp, actor, payload_snapshot FROM ledger_audit_log
         WHERE action_type IN ('ACCOUNT_CHANGE', 'LIST_CHANGE') AND date(timestamp, 'localtime') BETWEEN ?1 AND ?2 ORDER BY id",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    for (action, record_id, occurred_at, actor, payload) in edits {
        let payload: serde_json::Value = serde_json::from_str(&payload).map_err(|e| format!("CLEANUP_BAD_AUDIT_ROW: {e}"))?;
        let list = if action == "ACCOUNT_CHANGE" { "ACCOUNT".to_string() } else { payload["list"].as_str().unwrap_or("ITEM").to_string() };
        let (before, after) = (&payload["before"], &payload["after"]);
        let changed: Vec<String> = match &payload["changed"] {
            serde_json::Value::Array(items) => items.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
            serde_json::Value::String(s) => serde_json::from_str(s).unwrap_or_default(),
            _ => Vec::new(),
        };
        let (change, details) = describe_edit(before, after, changed);
        let name = after["name"].as_str().unwrap_or_default();
        let name = match after["accountNumber"].as_str() {
            Some(number) if list == "ACCOUNT" => format!("{number} {name}"),
            _ => name.to_string(),
        };
        out.push(ListChange { list, record_id, name, change, occurred_at, actor, details });
    }
    out.sort_by(|a, b| b.occurred_at.cmp(&a.occurred_at));
    Ok(out)
}

// ------------------------------------------------------------------------------------------------
// 5. Balance Sheet against inventory
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryAccountComparison {
    pub account_id: i64,
    pub account_name: String,
    pub balance_cents: i64,
    pub valuation_cents: i64,
    pub difference_cents: i64,
}

/// A transaction whose amount on an inventory asset account differs from the stock value it moved.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryDifference {
    pub journal_entry_id: i64,
    pub entry_number: i64,
    pub entry_date: String,
    pub source_type: String,
    pub memo: Option<String>,
    pub party_name: Option<String>,
    pub account_id: i64,
    pub account_name: String,
    pub ledger_cents: i64,
    pub stock_cents: i64,
    pub difference_cents: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryComparison {
    pub as_of: NaiveDate,
    pub accounts: Vec<InventoryAccountComparison>,
    pub total_balance_cents: i64,
    pub total_valuation_cents: i64,
    pub total_difference_cents: i64,
    pub differences: Vec<InventoryDifference>,
}

pub async fn compare_inventory(pool: &SqlitePool, as_of: NaiveDate) -> Result<InventoryComparison, String> {
    // The accounts tracked items use, plus any other asset account named for inventory.
    let accounts: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, COALESCE(account_number || ' ', '') || name FROM chart_of_accounts
         WHERE id IN (SELECT asset_account_id FROM inventory_items)
            OR (account_type = 'OTHER_CURRENT_ASSET' AND is_system = 0 AND name LIKE '%inventory%')
         ORDER BY account_number, name",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let names: HashMap<i64, String> = accounts.iter().cloned().collect();

    let ledger: Vec<(i64, i64, i64)> = sqlx::query_as(
        "SELECT l.journal_entry_id, l.account_id, SUM(l.debit_cents - l.credit_cents)
         FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
         WHERE e.status = 'POSTED' AND e.entry_date <= ?1
           AND l.account_id IN (SELECT asset_account_id FROM inventory_items
                                UNION SELECT id FROM chart_of_accounts WHERE account_type = 'OTHER_CURRENT_ASSET' AND is_system = 0 AND name LIKE '%inventory%')
         GROUP BY l.journal_entry_id, l.account_id",
    )
    .bind(as_of)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let stock: Vec<(i64, i64, i64)> = sqlx::query_as(
        "SELECT s.journal_entry_id, i.asset_account_id, SUM(s.value_change_cents)
         FROM stock_ledger_entries s JOIN inventory_items i ON i.id = s.item_id
         WHERE s.movement_date <= ?1 GROUP BY s.journal_entry_id, i.asset_account_id",
    )
    .bind(as_of)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;

    // (entry, account) → (ledger, stock)
    let mut pairs: BTreeMap<(i64, i64), (i64, i64)> = BTreeMap::new();
    for (entry, account, cents) in ledger {
        pairs.entry((entry, account)).or_default().0 += cents;
    }
    for (entry, account, cents) in stock {
        pairs.entry((entry, account)).or_default().1 += cents;
    }
    let mut totals: HashMap<i64, (i64, i64)> = HashMap::new();
    for (&(_, account), &(l, s)) in &pairs {
        let t = totals.entry(account).or_default();
        t.0 += l;
        t.1 += s;
    }
    let accounts: Vec<InventoryAccountComparison> = accounts
        .into_iter()
        .map(|(account_id, account_name)| {
            let (balance_cents, valuation_cents) = totals.get(&account_id).copied().unwrap_or_default();
            InventoryAccountComparison { account_id, account_name, balance_cents, valuation_cents, difference_cents: balance_cents - valuation_cents }
        })
        .collect();

    let differing: Vec<(i64, i64, i64, i64)> = pairs.iter().filter(|(_, (l, s))| l != s).map(|(&(e, a), &(l, s))| (e, a, l, s)).collect();
    // Every differing entry's header in one query.
    let ids = serde_json::to_string(&differing.iter().map(|d| d.0).collect::<Vec<_>>()).map_err(|e| e.to_string())?;
    let headers: HashMap<i64, (i64, String, String, Option<String>, Option<String>)> =
        sqlx::query_as::<_, (i64, i64, String, String, Option<String>, Option<String>)>(
            "SELECT e.id, e.entry_number, e.entry_date, e.source_type, e.memo, p.party_name
             FROM journal_entries e LEFT JOIN v_entry_party p ON p.journal_entry_id = e.id
             WHERE e.id IN (SELECT value FROM json_each(?1))",
        )
        .bind(ids)
        .fetch_all(pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|(id, number, date, source, memo, party)| (id, (number, date, source, memo, party)))
        .collect();
    let mut differences = Vec::with_capacity(differing.len());
    for (entry, account, l, s) in differing {
        let (entry_number, entry_date, source_type, memo, party_name) = headers.get(&entry).cloned().ok_or_else(|| format!("CLEANUP_NO_ENTRY: entry {entry} is missing"))?;
        differences.push(InventoryDifference {
            journal_entry_id: entry,
            entry_number,
            entry_date,
            source_type,
            memo,
            party_name,
            account_id: account,
            account_name: names.get(&account).cloned().unwrap_or_default(),
            ledger_cents: l,
            stock_cents: s,
            difference_cents: l - s,
        });
    }
    differences.sort_by(|a, b| (&a.entry_date, a.entry_number).cmp(&(&b.entry_date, b.entry_number)));
    let total_balance_cents = accounts.iter().map(|a| a.balance_cents).sum();
    let total_valuation_cents = accounts.iter().map(|a| a.valuation_cents).sum();
    Ok(InventoryComparison {
        as_of,
        total_difference_cents: total_balance_cents - total_valuation_cents,
        total_balance_cents,
        total_valuation_cents,
        accounts,
        differences,
    })
}

#[cfg(test)]
mod tests;
