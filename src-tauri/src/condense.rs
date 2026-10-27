//! Condense Data: archive closed years and continue in a smaller company file.
//!
//! LocalBooks never rewrites or deletes posted entries in the company file that is open. Condensing
//! builds a new file next to it and switches to it only once the new file has been checked against
//! the old one:
//!
//! 1. **Archive.** A verified snapshot of the books becomes the archive
//!    (`archive-through-<cut-off>-<time>.db` in the backups folder), marked so that LocalBooks opens
//!    it read-only. Its history and audit trail are exactly the books' as they were. A plain
//!    `pre-condense-<time>.db` copy is kept as well, to undo the condense by restoring it.
//! 2. **Condense a copy.** Transactions are grouped by what links them: an invoice, its lines, its
//!    payments, the deposit that took those payments to the bank, and so on, following every foreign
//!    key between the [`TRANSACTION_TABLES`] (plus the few links stored without one). A group whose
//!    entries are all dated on or before the cut-off is replaced by one summary journal entry per
//!    month, with a line per account and class, unless anything in it is still open: an unpaid
//!    invoice or bill, credit not used up, a payment not yet deposited (any balance left in A/R, A/P
//!    or Undeposited Funds), a line not cleared in a completed reconciliation of a bank or credit card
//!    account, a draft, a foreign-currency entry, anything dated after the cut-off, or stock whose
//!    movements could not be summed in order. Open groups are kept whole, at their original dates.
//!    Lists, settings, users, reconciliations and every other table are copied unchanged.
//! 3. **Audit trail.** The copy keeps every audit record, including the posting snapshot of each
//!    summarized entry, and gains one `CONDENSE` record naming the archive and listing what each
//!    summary replaced. [`crate::ledger::verify_ledger_audit`] accepts a summarized entry's absence
//!    only when that record lists it, and checks that its snapshots add up to the summary. The copy
//!    gets a new audit-trail identity (the archive keeps the old one, so both verify).
//! 4. **Verify, then switch.** The copy must pass File Dentist's check-up and match the archive:
//!    balances per account, class and month; A/R and A/P aging and inventory valuation at the
//!    cut-off and today; cleared balances and uncleared lines; every list. Only then is it swapped in,
//!    and only if the books did not change while it was being built.
//!
//! The copy is a private file nobody has open while it is condensed. Its protecting triggers are
//! dropped for the removal and recreated, from the definitions it had, before the same transaction
//! commits; the check-up then confirms they match this build's.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{Datelike, Local, NaiveDate};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Connection, SqliteConnection, SqlitePool};

use crate::backup::{self, BackupFile};
use crate::db::Db;
use crate::file_dentist::{self, CheckStatus};
use crate::ledger::{self, db_err};

/// Tables of transactions, each with the columns that date a row. A row dated after the cut-off
/// keeps its whole group.
pub const TRANSACTION_TABLES: &[(&str, &[&str])] = &[
    ("ach_entries", &[]),
    ("ach_entry_payments", &[]),
    ("ach_files", &["payment_date", "effective_date"]),
    ("attachments", &[]),
    ("audit_log", &[]),
    ("bank_transactions", &["txn_date"]),
    ("bill_lines", &[]),
    ("bill_payments", &["payment_date"]),
    ("bills", &["bill_date"]),
    ("class_reclassifications", &[]),
    ("credit_applications", &["applied_date"]),
    ("credit_memo_lines", &[]),
    ("credit_memos", &["credit_date"]),
    ("credit_refunds", &["refund_date"]),
    ("deposit_lines", &[]),
    ("deposits", &["deposit_date"]),
    ("estimate_lines", &[]),
    ("estimates", &["estimate_date"]),
    ("expense_lines", &[]),
    ("expenses", &["expense_date"]),
    ("finance_charge_basis", &["from_date", "to_date"]),
    ("finance_charge_invoices", &[]),
    ("form_email_log", &[]),
    ("form_email_queue", &[]),
    ("fx_revaluations", &["as_of"]),
    ("invoice_items", &[]),
    ("invoice_payments", &["payment_date"]),
    ("invoices", &["issue_date"]),
    ("journal_entries", &["entry_date"]),
    ("journal_lines", &[]),
    ("ledger_import_transactions", &["date"]),
    ("legacy_status_voids", &[]),
    ("purchase_order_lines", &[]),
    ("purchase_orders", &["po_date"]),
    ("qbw_posted_invoices", &[]),
    ("recurring_runs", &["run_date"]),
    ("sales_document_fields", &[]),
    ("sales_receipt_lines", &[]),
    ("sales_receipts", &["receipt_date"]),
    ("sales_tax_payments", &["payment_date"]),
    ("stock_ledger_entries", &["movement_date"]),
    ("transaction_edits", &[]),
    ("transfers", &["transfer_date"]),
    ("undeposited_fund_matches", &[]),
    ("vendor_credit_applications", &["applied_date"]),
    ("vendor_credit_lines", &[]),
    ("vendor_credit_refunds", &["refund_date"]),
    ("vendor_credits", &["credit_date"]),
];

/// Tables copied unchanged: lists, settings, users and keys, reconciliations, import staging and the
/// audit trail. A row here that points at a transaction keeps that transaction's group.
pub const KEPT_TABLES: &[&str] = &[
    "_sqlx_migrations",
    "account_types",
    "ach_originators",
    "bank_format_templates",
    "bank_rules",
    "budget_amounts",
    "budgets",
    "chart_of_accounts",
    "classes",
    "closing_locks",
    "custom_field_values",
    "custom_fields",
    "customers",
    "data_key",
    "employees",
    "exchange_rate_cache",
    "form_styles",
    "form_templates",
    "inventory_items",
    "key_slots",
    "ledger_audit_log",
    "ledger_chain",
    "ledger_import_accounts",
    "ledger_import_batches",
    "ledger_import_items",
    "ledger_import_lines",
    "ledger_import_parties",
    "ledger_import_price_levels",
    "ledger_import_sales_lists",
    "ledger_import_unreadable",
    "mileage_rates",
    "mileage_trips",
    "price_level_items",
    "price_levels",
    "products_services",
    "reconciliation_snapshot_lines",
    "reconciliation_snapshots",
    "reconciliations",
    "recurring_templates",
    "reminder_settings",
    "sales_reps",
    "saved_reports",
    "ship_methods",
    "simplefin_accounts",
    "simplefin_connections",
    "staging_account_map",
    "staging_import_batches",
    "staging_raw_transactions",
    "system_preferences",
    "tax_rates",
    "time_activities",
    "todos",
    "user_reset_in_progress",
    "users",
    "vehicles",
    "vendor_bank_accounts",
    "vendors",
];

/// Links to a form stored without a foreign key: (table, kind column, id column). A link from a
/// kept table (a to-do) keeps the form's group, like a foreign key from a list.
const FORM_LINKS: &[(&str, &str, &str)] = &[
    ("sales_document_fields", "document_kind", "document_id"),
    ("form_email_log", "form_type", "document_id"),
    ("form_email_queue", "form_type", "document_id"),
    ("todos", "link_kind", "link_id"),
];

/// The table a form kind in [`FORM_LINKS`] names; other kinds (statements) name no transaction.
fn form_table(kind: &str) -> Option<&'static str> {
    match kind {
        "INVOICE" => Some("invoices"),
        "SALES_RECEIPT" => Some("sales_receipts"),
        "ESTIMATE" => Some("estimates"),
        "BILL" => Some("bills"),
        _ => None,
    }
}

/// Preferences that mark a file made or left by condensing.
pub const ROLE_KEY: &str = "company_file_role";
pub const ARCHIVE_ROLE: &str = "ARCHIVE";
const ARCHIVE_THROUGH_KEY: &str = "archive_through";
const CONDENSED_THROUGH_KEY: &str = "condensed_through";
const CONDENSE_ARCHIVE_KEY: &str = "condense_archive_path";

/// Refused because the open file is an archive.
pub const ARCHIVE_READ_ONLY: &str =
    "ARCHIVE_READ_ONLY: this is an archive of condensed books and opens read-only; open your current company file to make changes";

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

// ------------------------------------------------------------------------------------------------
// The plan: which groups are summarized, and what replaces them
// ------------------------------------------------------------------------------------------------

/// Why a group dated on or before the cut-off is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum KeepReason {
    /// Something in the group is dated after the cut-off (a later payment, credit or deposit).
    AfterCutoff,
    /// An invoice or bill not fully paid, credit not used up, or a payment not yet deposited.
    OpenBalance,
    /// A bank or credit card line not cleared in a completed reconciliation.
    Uncleared,
    /// A draft, or an entry in a foreign currency.
    NotSummarizable,
    /// Stock movements that could not be summed in the order they were recorded.
    Inventory,
    /// The newest entry, kept so a new entry never reuses a summarized entry's id.
    Newest,
    /// A list or setting points at it.
    Referenced,
}

impl KeepReason {
    fn describe(self) -> &'static str {
        match self {
            KeepReason::AfterCutoff => "linked to a transaction dated after the cut-off",
            KeepReason::OpenBalance => "still open (unpaid, credit left, or not yet deposited)",
            KeepReason::Uncleared => "not cleared in a completed reconciliation",
            KeepReason::NotSummarizable => "a draft or a foreign-currency entry",
            KeepReason::Inventory => "stock movements that cannot be summed in order",
            KeepReason::Newest => "the newest entry in the file",
            KeepReason::Referenced => "used by a list or setting",
        }
    }
}

#[derive(Debug, Clone, Default)]
struct Line {
    net: i64,
    reconciliation_id: Option<i64>,
    reconciled_at: Option<String>,
}

/// One month's summary entry.
#[derive(Debug, Clone)]
struct Summary {
    date: NaiveDate,
    /// The posted entries it replaces.
    entries: BTreeSet<i64>,
    /// (account, class, reconciled) → net debit.
    lines: BTreeMap<(i64, Option<i64>, bool), Line>,
}

/// The stock an item had after its last summarized movement, recorded as one movement.
#[derive(Debug, Clone)]
struct StockSummary {
    movement_id: i64,
    item_id: i64,
    date: String,
    /// The summarized movements' total change, which is also the quantity and value after them
    /// (tracking starts at zero, and they are the item's first movements).
    qty_e4: i64,
    value_cents: i64,
}

#[derive(Debug, Clone)]
struct Plan {
    cutoff: NaiveDate,
    /// Rowids to remove, per transaction table.
    removed: BTreeMap<&'static str, Vec<i64>>,
    /// Posted entries dated on or before the cut-off that are kept, by reason.
    kept: BTreeMap<KeepReason, i64>,
    entries_before_cutoff: i64,
    summaries: Vec<Summary>,
    stock: Vec<StockSummary>,
}

impl Plan {
    fn removed_entries(&self) -> &[i64] {
        self.removed.get("journal_entries").map(Vec::as_slice).unwrap_or(&[])
    }
}

struct UnionFind(Vec<usize>);

impl UnionFind {
    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }
}

/// Every transaction row as a node: (table, rowid) ↔ index.
struct Nodes {
    tables: Vec<&'static str>,
    base: Vec<usize>,
    rowids: Vec<Vec<i64>>,
}

impl Nodes {
    fn table(&self, name: &str) -> Option<usize> {
        self.tables.iter().position(|t| *t == name)
    }

    fn node(&self, table: usize, rowid: i64) -> Option<usize> {
        self.rowids[table].binary_search(&rowid).ok().map(|i| self.base[table] + i)
    }

    fn len(&self) -> usize {
        self.base.last().copied().unwrap_or(0) + self.rowids.last().map_or(0, Vec::len)
    }

    fn locate(&self, node: usize) -> (usize, i64) {
        let table = self.base.partition_point(|b| *b <= node) - 1;
        (table, self.rowids[table][node - self.base[table]])
    }
}

async fn all_tables(conn: &mut SqliteConnection) -> Result<Vec<String>, String> {
    sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
        .fetch_all(&mut *conn)
        .await
        .map_err(db_err)
}

/// Fails when the file has a table this build does not know how to condense, so a new table is
/// never silently treated as a list or a transaction.
async fn check_tables_known(conn: &mut SqliteConnection) -> Result<(), String> {
    let unknown: Vec<String> = all_tables(conn)
        .await?
        .into_iter()
        .filter(|t| !KEPT_TABLES.contains(&t.as_str()) && !TRANSACTION_TABLES.iter().any(|(name, _)| name == t))
        .collect();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(format!("CONDENSE_UNKNOWN_TABLE: this version of LocalBooks does not know how to condense {}", unknown.join(", ")))
    }
}

fn last_of_month(date: NaiveDate) -> NaiveDate {
    let (y, m) = if date.month() == 12 { (date.year() + 1, 1) } else { (date.year(), date.month() + 1) };
    NaiveDate::from_ymd_opt(y, m, 1).and_then(|d| d.pred_opt()).unwrap_or(date)
}

async fn closing_date(conn: &mut SqliteConnection) -> Result<Option<NaiveDate>, String> {
    let date: Option<Option<String>> =
        sqlx::query_scalar("SELECT lock_date FROM closing_locks ORDER BY id DESC LIMIT 1").fetch_optional(&mut *conn).await.map_err(db_err)?;
    Ok(date.flatten().and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok()))
}

/// Works out what condensing through `cutoff` would summarize and keep. Reads only.
async fn plan(conn: &mut SqliteConnection, cutoff: NaiveDate) -> Result<Plan, String> {
    check_tables_known(conn).await?;
    let cut = cutoff.format("%Y-%m-%d").to_string();

    let mut nodes = Nodes { tables: Vec::new(), base: Vec::new(), rowids: Vec::new() };
    let mut total = 0;
    for (table, _) in TRANSACTION_TABLES {
        let rowids: Vec<i64> =
            sqlx::query_scalar(&format!("SELECT rowid FROM {} ORDER BY rowid", quote_ident(table))).fetch_all(&mut *conn).await.map_err(db_err)?;
        nodes.tables.push(table);
        nodes.base.push(total);
        total += rowids.len();
        nodes.rowids.push(rowids);
    }
    let mut groups = UnionFind((0..nodes.len()).collect());
    let mut kept: Vec<(usize, KeepReason)> = Vec::new();

    // Foreign keys between transactions join their groups; one from a list keeps the group.
    let mut targets: HashMap<(String, String), HashMap<i64, i64>> = HashMap::new();
    for child in all_tables(conn).await? {
        let keys: Vec<(String, String, Option<String>)> =
            sqlx::query_as("SELECT \"table\", \"from\", \"to\" FROM pragma_foreign_key_list(?1)").bind(&child).fetch_all(&mut *conn).await.map_err(db_err)?;
        for (parent, from, to) in keys {
            let Some(parent_table) = nodes.table(&parent) else { continue };
            let to = to.unwrap_or_else(|| "rowid".into());
            let target = match targets.entry((parent.clone(), to)) {
                std::collections::hash_map::Entry::Occupied(known) => known.into_mut(),
                std::collections::hash_map::Entry::Vacant(slot) => {
                    let (parent, to) = slot.key();
                    let pairs: Vec<(i64, i64)> = sqlx::query_as(&format!("SELECT {}, rowid FROM {} WHERE {0} IS NOT NULL", quote_ident(to), quote_ident(parent)))
                        .fetch_all(&mut *conn)
                        .await
                        .map_err(db_err)?;
                    slot.insert(pairs.into_iter().collect())
                }
            };
            let links: Vec<(i64, i64)> = sqlx::query_as(&format!("SELECT rowid, {} FROM {} WHERE {0} IS NOT NULL", quote_ident(&from), quote_ident(&child)))
                .fetch_all(&mut *conn)
                .await
                .map_err(db_err)?;
            let child_table = nodes.table(&child);
            for (rowid, value) in links {
                let Some(parent_node) = target.get(&value).and_then(|r| nodes.node(parent_table, *r)) else {
                    return Err(format!("CONDENSE_BROKEN_LINK: a row in {child} points to a missing row in {parent}; run File Dentist first"));
                };
                match child_table.and_then(|t| nodes.node(t, rowid)) {
                    Some(child_node) => groups.union(child_node, parent_node),
                    None => kept.push((parent_node, KeepReason::Referenced)),
                }
            }
        }
    }
    for (table, kind_column, id_column) in FORM_LINKS {
        let links: Vec<(i64, String, i64)> =
            sqlx::query_as(&format!("SELECT rowid, {0}, {1} FROM {2} WHERE {0} IS NOT NULL AND {1} IS NOT NULL", quote_ident(kind_column), quote_ident(id_column), quote_ident(table)))
                .fetch_all(&mut *conn)
                .await
                .map_err(db_err)?;
        let from = nodes.table(table);
        for (rowid, kind, id) in links {
            let Some(target) = form_table(&kind).and_then(|t| nodes.table(t)).and_then(|t| nodes.node(t, id)) else { continue };
            match from.and_then(|t| nodes.node(t, rowid)) {
                Some(a) => groups.union(a, target),
                None => kept.push((target, KeepReason::Referenced)),
            }
        }
    }

    // Rows that keep their group.
    let keep_rows = |table: &str, rowids: Vec<i64>, reason: KeepReason, kept: &mut Vec<(usize, KeepReason)>| {
        let t = nodes.table(table).expect("a transaction table");
        kept.extend(rowids.into_iter().filter_map(|r| nodes.node(t, r)).map(|n| (n, reason)));
    };
    for (table, dates) in TRANSACTION_TABLES.iter().filter(|(_, dates)| !dates.is_empty()) {
        let after = dates.iter().map(|c| format!("date({}) > ?1", quote_ident(c))).collect::<Vec<_>>().join(" OR ");
        let rows: Vec<i64> =
            sqlx::query_scalar(&format!("SELECT rowid FROM {} WHERE {after}", quote_ident(table))).bind(&cut).fetch_all(&mut *conn).await.map_err(db_err)?;
        keep_rows(table, rows, KeepReason::AfterCutoff, &mut kept);
    }
    for (table, sql, reason) in [
        ("journal_entries", "SELECT id FROM journal_entries WHERE status = 'DRAFT' OR foreign_currency_code IS NOT NULL", KeepReason::NotSummarizable),
        ("invoices", "SELECT id FROM invoices WHERE is_voided = 0 AND (status = 'DRAFT' OR total_cents <> amount_paid_cents)", KeepReason::OpenBalance),
        ("bills", "SELECT id FROM bills WHERE is_voided = 0 AND (status = 'DRAFT' OR total_cents <> amount_paid_cents)", KeepReason::OpenBalance),
        (
            "journal_lines",
            "SELECT l.id FROM journal_lines l
             JOIN journal_entries e ON e.id = l.journal_entry_id
             JOIN chart_of_accounts a ON a.id = l.account_id
             WHERE e.status = 'POSTED' AND l.reconciled_at IS NULL
               AND (a.account_type IN ('BANK', 'CREDIT_CARD') OR EXISTS (SELECT 1 FROM reconciliations r WHERE r.account_id = l.account_id))",
            KeepReason::Uncleared,
        ),
        ("journal_entries", "SELECT id FROM journal_entries ORDER BY id DESC LIMIT 1", KeepReason::Newest),
        ("journal_lines", "SELECT id FROM journal_lines ORDER BY id DESC LIMIT 1", KeepReason::Newest),
    ] {
        let rows: Vec<i64> = sqlx::query_scalar(sql).fetch_all(&mut *conn).await.map_err(db_err)?;
        keep_rows(table, rows, reason, &mut kept);
    }

    let mut reasons: HashMap<usize, KeepReason> = HashMap::new();
    for (node, reason) in kept {
        let root = groups.find(node);
        reasons.entry(root).and_modify(|r| *r = (*r).min(reason)).or_insert(reason);
    }

    // A group that leaves a balance in A/R, A/P or Undeposited Funds holds an open item.
    let je = nodes.table("journal_entries").expect("journal entries");
    let lines: Vec<(i64, String, i64, Option<i64>, i64, i64, Option<i64>, Option<String>, bool)> = sqlx::query_as(
        "SELECT l.journal_entry_id, e.entry_date, l.account_id, l.class_id, l.debit_cents, l.credit_cents, l.reconciliation_id, l.reconciled_at,
                a.account_type IN ('ACCOUNTS_RECEIVABLE', 'ACCOUNTS_PAYABLE') OR (a.is_system = 1 AND a.account_type = 'OTHER_CURRENT_ASSET')
         FROM journal_lines l
         JOIN journal_entries e ON e.id = l.journal_entry_id
         JOIN chart_of_accounts a ON a.id = l.account_id
         WHERE e.status = 'POSTED' AND e.entry_date <= ?1
         ORDER BY e.entry_date, l.journal_entry_id, l.line_number",
    )
    .bind(&cut)
    .fetch_all(&mut *conn)
    .await
    .map_err(db_err)?;
    let mut clearing: HashMap<(usize, i64), i64> = HashMap::new();
    for (entry, _, account, _, debit, credit, _, _, is_clearing) in &lines {
        if *is_clearing {
            let root = groups.find(nodes.node(je, *entry).expect("a journal entry"));
            *clearing.entry((root, *account)).or_default() += debit - credit;
        }
    }
    for ((root, _), net) in clearing {
        if net != 0 {
            reasons.entry(root).and_modify(|r| *r = (*r).min(KeepReason::OpenBalance)).or_insert(KeepReason::OpenBalance);
        }
    }

    // Groups that hold an entry and nothing that keeps them.
    let mut has_entry: HashSet<usize> = HashSet::new();
    for i in 0..nodes.rowids[je].len() {
        has_entry.insert(groups.find(nodes.base[je] + i));
    }

    // Stock: an item's summarized movements must come before its kept ones, so the kept movements
    // continue from the summary's quantity and value. Otherwise its groups are kept too.
    let stock_table = nodes.table("stock_ledger_entries").expect("stock ledger");
    let movements: Vec<(i64, i64, String, i64, i64)> =
        sqlx::query_as("SELECT id, item_id, movement_date, qty_change_e4, value_change_cents FROM stock_ledger_entries ORDER BY item_id, id")
            .fetch_all(&mut *conn)
            .await
            .map_err(db_err)?;
    let mut by_item: BTreeMap<i64, Vec<(usize, &(i64, i64, String, i64, i64))>> = BTreeMap::new();
    for m in &movements {
        let root = groups.find(nodes.node(stock_table, m.0).expect("a stock movement"));
        by_item.entry(m.1).or_default().push((root, m));
    }
    loop {
        let mut out_of_order = Vec::new();
        for moves in by_item.values() {
            let removable = |root: &usize| has_entry.contains(root) && !reasons.contains_key(root);
            let first_kept = moves.iter().position(|(root, _)| !removable(root)).unwrap_or(moves.len());
            if moves[first_kept..].iter().any(|(root, _)| removable(root)) {
                out_of_order.extend(moves.iter().map(|(root, _)| *root).filter(|root| removable(root)));
            }
        }
        if out_of_order.is_empty() {
            break;
        }
        for root in out_of_order {
            reasons.insert(root, KeepReason::Inventory);
        }
    }

    let mut removed: BTreeMap<&'static str, Vec<i64>> = BTreeMap::new();
    let mut removed_nodes: HashSet<usize> = HashSet::new();
    for node in 0..nodes.len() {
        let root = groups.find(node);
        if has_entry.contains(&root) && !reasons.contains_key(&root) {
            let (table, rowid) = nodes.locate(node);
            removed.entry(nodes.tables[table]).or_default().push(rowid);
            removed_nodes.insert(node);
        }
    }

    // What the summarized entries add up to, month by month.
    let mut months: BTreeMap<NaiveDate, Summary> = BTreeMap::new();
    let mut kept_counts: BTreeMap<KeepReason, i64> = BTreeMap::new();
    let mut counted: HashSet<i64> = HashSet::new();
    let mut entries_before_cutoff = 0;
    for (entry, date, account, class, debit, credit, rec_id, rec_at, _) in lines {
        let node = nodes.node(je, entry).expect("a journal entry");
        let first = counted.insert(entry);
        if first {
            entries_before_cutoff += 1;
        }
        if !removed_nodes.contains(&node) {
            if first {
                let reason = reasons.get(&groups.find(node)).copied().unwrap_or(KeepReason::Referenced);
                *kept_counts.entry(reason).or_default() += 1;
            }
            continue;
        }
        let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| format!("CONDENSE_BAD_DATE: {e}"))?;
        let bucket = last_of_month(date).min(cutoff);
        let summary = months.entry(bucket).or_insert_with(|| Summary { date: bucket, entries: BTreeSet::new(), lines: BTreeMap::new() });
        summary.entries.insert(entry);
        let line = summary.lines.entry((account, class, rec_at.is_some())).or_default();
        line.net += debit - credit;
        if rec_at.is_some() {
            line.reconciliation_id = line.reconciliation_id.max(rec_id);
            line.reconciled_at = line.reconciled_at.clone().max(rec_at);
        }
    }
    let summaries: Vec<Summary> = months
        .into_values()
        .map(|mut s| {
            s.lines.retain(|_, l| l.net != 0);
            s
        })
        .collect();

    // Each item's summarized movements (the first ones recorded) become one, dated with the last.
    let mut stock = Vec::new();
    for (item, moves) in &by_item {
        let summarized: Vec<_> = moves.iter().map(|(_, m)| *m).filter(|m| removed_nodes.contains(&nodes.node(stock_table, m.0).unwrap_or(usize::MAX))).collect();
        let (qty_e4, value_cents) = summarized.iter().fold((0, 0), |(q, v), m| (q + m.3, v + m.4));
        if let Some(last) = summarized.last() {
            if qty_e4 != 0 || value_cents != 0 {
                stock.push(StockSummary { movement_id: last.0, item_id: *item, date: last.2.clone(), qty_e4, value_cents });
            }
        }
    }

    Ok(Plan { cutoff, removed, kept: kept_counts, entries_before_cutoff, summaries, stock })
}

// ------------------------------------------------------------------------------------------------
// Preview
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeptCount {
    pub reason: KeepReason,
    pub description: &'static str,
    pub entries: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CondensePreview {
    pub cutoff: NaiveDate,
    pub closing_date: Option<NaiveDate>,
    /// Posted entries dated on or before the cut-off.
    pub entries_before_cutoff: i64,
    /// Of those, the ones replaced by summaries.
    pub entries_summarized: i64,
    pub kept: Vec<KeptCount>,
    /// Summary entries that replace them (one per month with any change).
    pub summary_entries: i64,
    /// Rows removed per table, for the record.
    pub rows_removed: BTreeMap<String, i64>,
    /// Why condensing cannot run now; empty when it can.
    pub blockers: Vec<String>,
}

fn validate_cutoff(cutoff: NaiveDate, closing: Option<NaiveDate>, today: NaiveDate) -> Vec<String> {
    let mut blockers = Vec::new();
    match closing {
        None => blockers.push("Set a closing date first (Company › Set Closing Date): only closed periods can be condensed.".into()),
        Some(closing) if cutoff > closing => {
            blockers.push(format!("The cut-off must be on or before the closing date ({closing}): only closed periods can be condensed."))
        }
        Some(_) => {}
    }
    if cutoff >= today {
        blockers.push("The cut-off must be in the past.".into());
    }
    blockers
}

async fn preview_pool(pool: &SqlitePool, cutoff: NaiveDate, today: NaiveDate) -> Result<(CondensePreview, Plan), String> {
    let mut conn = pool.acquire().await.map_err(db_err)?;
    if is_archive(&mut conn).await? {
        return Err(ARCHIVE_READ_ONLY.into());
    }
    let closing = closing_date(&mut conn).await?;
    let plan = plan(&mut conn, cutoff).await?;
    let mut blockers = validate_cutoff(cutoff, closing, today);
    if plan.removed_entries().is_empty() && blockers.is_empty() {
        blockers.push("Nothing on or before the cut-off can be summarized: every transaction there is still open, uncleared or linked to a later one.".into());
    }
    let preview = CondensePreview {
        cutoff,
        closing_date: closing,
        entries_before_cutoff: plan.entries_before_cutoff,
        entries_summarized: plan.summaries.iter().map(|s| s.entries.len() as i64).sum(),
        kept: plan.kept.iter().map(|(reason, n)| KeptCount { reason: *reason, description: reason.describe(), entries: *n }).collect(),
        summary_entries: plan.summaries.iter().filter(|s| !s.lines.is_empty()).count() as i64,
        rows_removed: plan.removed.iter().map(|(t, r)| (t.to_string(), r.len() as i64)).collect(),
        blockers,
    };
    Ok((preview, plan))
}

/// What condensing the open company file through `cutoff` would do. Reads only.
pub async fn preview(pool: &SqlitePool, cutoff: NaiveDate) -> Result<CondensePreview, String> {
    preview_pool(pool, cutoff, Local::now().date_naive()).await.map(|(preview, _)| preview)
}

// ------------------------------------------------------------------------------------------------
// Condensing the copy
// ------------------------------------------------------------------------------------------------

/// Where the copy came from, as recorded in its CONDENSE audit record.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ArchiveRecord {
    file_name: String,
    file_id: String,
    audit_head_id: Option<i64>,
    audit_head_hash: Option<String>,
    content_sha256: String,
}

/// Opens `path` on one connection with foreign-key enforcement off: the condense removes whole
/// groups, and `PRAGMA foreign_key_check` (File Dentist's LINKS check) confirms nothing dangles.
async fn open_private(path: &Path) -> Result<SqliteConnection, String> {
    let options = SqliteConnectOptions::new().filename(path).foreign_keys(false).create_if_missing(false);
    SqliteConnection::connect_with(&options).await.map_err(|e| format!("CONDENSE_OPEN: cannot open {}: {e}", path.display()))
}

/// Replaces the planned groups in the file on `conn` with summary entries, in one transaction.
async fn condense_copy(conn: &mut SqliteConnection, plan: &Plan, archive: &ArchiveRecord, archive_path: &Path) -> Result<(), String> {
    let mut tx = conn.begin_with("BEGIN IMMEDIATE").await.map_err(db_err)?;

    let triggers: Vec<(String, String)> =
        sqlx::query_as("SELECT name, sql FROM sqlite_schema WHERE type = 'trigger' ORDER BY rowid").fetch_all(&mut *tx).await.map_err(db_err)?;
    for (name, _) in &triggers {
        sqlx::query(&format!("DROP TRIGGER {}", quote_ident(name))).execute(&mut *tx).await.map_err(db_err)?;
    }

    sqlx::query("CREATE TEMP TABLE condense_removed (tbl TEXT NOT NULL, rid INTEGER NOT NULL, PRIMARY KEY (tbl, rid))").execute(&mut *tx).await.map_err(db_err)?;
    for (table, rowids) in &plan.removed {
        for rowid in rowids {
            sqlx::query("INSERT INTO temp.condense_removed (tbl, rid) VALUES (?1, ?2)").bind(table).bind(rowid).execute(&mut *tx).await.map_err(db_err)?;
        }
        sqlx::query(&format!("DELETE FROM main.{} WHERE rowid IN (SELECT rid FROM temp.condense_removed WHERE tbl = ?1)", quote_ident(table)))
            .bind(table)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
    }
    sqlx::query("DROP TABLE temp.condense_removed").execute(&mut *tx).await.map_err(db_err)?;

    // Summary entries take ids and numbers above any the file has used, so no audit record ever
    // describes two entries.
    let (mut next_id, mut next_number): (i64, i64) = sqlx::query_as(
        "SELECT MAX(COALESCE((SELECT MAX(id) FROM journal_entries), 0),
                    COALESCE((SELECT MAX(target_entity_id) FROM ledger_audit_log WHERE action_type IN ('POST', 'VOID')), 0)) + 1,
                MAX(COALESCE((SELECT MAX(entry_number) FROM journal_entries), 0),
                    COALESCE((SELECT MAX(json_extract(payload_snapshot, '$.entryNumber')) FROM ledger_audit_log WHERE action_type = 'POST'), 0)) + 1",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(db_err)?;
    let archive_name = archive_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut recorded_summaries = Vec::new();
    let mut summary_for_month: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    for summary in &plan.summaries {
        if summary.lines.is_empty() {
            recorded_summaries.push(serde_json::json!({ "entryId": null, "date": summary.date, "summarizes": summary.entries }));
            continue;
        }
        let (debits, credits) = summary.lines.values().fold((0i64, 0i64), |(d, c), l| if l.net > 0 { (d + l.net, c) } else { (d, c - l.net) });
        if debits != credits || summary.lines.len() < 2 {
            return Err(format!("CONDENSE_UNBALANCED: the summary for {} does not balance; nothing was changed", summary.date));
        }
        let id = next_id;
        next_id += 1;
        let memo = format!(
            "Condensed: {} transactions in {} (the detail is in the archive {archive_name})",
            summary.entries.len(),
            summary.date.format("%B %Y")
        );
        sqlx::query("INSERT INTO journal_entries (id, entry_number, entry_date, memo, source_type, status) VALUES (?1, ?2, ?3, ?4, 'JOURNAL', 'DRAFT')")
            .bind(id)
            .bind(next_number)
            .bind(summary.date)
            .bind(&memo)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        next_number += 1;
        for (n, ((account, class, _), line)) in summary.lines.iter().enumerate() {
            sqlx::query(
                "INSERT INTO journal_lines (journal_entry_id, line_number, account_id, debit_cents, credit_cents, class_id, reconciliation_id, reconciled_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .bind(id)
            .bind(n as i64 + 1)
            .bind(account)
            .bind(line.net.max(0))
            .bind((-line.net).max(0))
            .bind(class)
            .bind(line.reconciliation_id)
            .bind(&line.reconciled_at)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        }
        sqlx::query("UPDATE journal_entries SET status = 'POSTED', posted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        // What the posting triggers (dropped for now) write for every posted entry.
        sqlx::query("INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot) SELECT 'POST', entry_id, snapshot FROM v_entry_snapshot WHERE entry_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(db_err)?;
        sqlx::query(
            "INSERT INTO audit_log (action, journal_entry_id, source_type, amount_cents, detail)
             SELECT 'POST', e.id, e.source_type, (SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = e.id),
                    'Posted entry #' || e.entry_number || ' dated ' || e.entry_date || ': ' || e.memo
             FROM journal_entries e WHERE e.id = ?1",
        )
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        summary_for_month.insert(summary.date, id);
        recorded_summaries.push(serde_json::json!({ "entryId": id, "date": summary.date, "summarizes": summary.entries }));
    }

    let mut recorded_stock = Vec::new();
    for stock in &plan.stock {
        let date = NaiveDate::parse_from_str(&stock.date, "%Y-%m-%d").map_err(|e| format!("CONDENSE_BAD_DATE: {e}"))?;
        let bucket = last_of_month(date).min(plan.cutoff);
        let entry = summary_for_month
            .range(..=bucket)
            .next_back()
            .or_else(|| summary_for_month.iter().next())
            .map(|(_, id)| *id)
            .ok_or_else(|| "CONDENSE_STOCK: summarized stock needs a summary entry to belong to; nothing was changed".to_string())?;
        sqlx::query(
            "INSERT INTO stock_ledger_entries (id, item_id, movement_date, kind, qty_change_e4, value_change_cents, qty_after_e4, value_after_cents, journal_entry_id)
             VALUES (?1, ?2, ?3, 'ADJUSTMENT', ?4, ?5, ?4, ?5, ?6)",
        )
        .bind(stock.movement_id)
        .bind(stock.item_id)
        .bind(&stock.date)
        .bind(stock.qty_e4)
        .bind(stock.value_cents)
        .bind(entry)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
        recorded_stock.push(serde_json::json!({ "movementId": stock.movement_id, "itemId": stock.item_id, "qtyE4": stock.qty_e4, "valueCents": stock.value_cents }));
    }

    // A new audit-trail identity: the archive keeps the old one, and this computer's record of it.
    sqlx::query("UPDATE ledger_chain SET file_id = lower(hex(randomblob(16))) WHERE id = 1").execute(&mut *tx).await.map_err(db_err)?;
    let cut = plan.cutoff.format("%Y-%m-%d").to_string();
    for (key, value) in [(CONDENSED_THROUGH_KEY, cut.as_str()), (CONDENSE_ARCHIVE_KEY, &archive_path.display().to_string())] {
        sqlx::query(
            "INSERT INTO system_preferences (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        )
        .bind(key)
        .bind(value)
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;
    }

    let payload = serde_json::json!({
        "event": "CONDENSE",
        "cutoff": plan.cutoff,
        "archive": archive,
        "removed": {
            "journalEntries": plan.removed_entries(),
            "rows": plan.removed.iter().map(|(t, r)| (t.to_string(), r.len())).collect::<BTreeMap<_, _>>(),
        },
        "summaries": recorded_summaries,
        "stock": recorded_stock,
        "keptBeforeCutoff": plan.kept,
    });
    sqlx::query("INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot) VALUES ('CONDENSE', 0, ?1)")
        .bind(payload.to_string())
        .execute(&mut *tx)
        .await
        .map_err(db_err)?;

    for (name, sql) in &triggers {
        sqlx::query(sql).execute(&mut *tx).await.map_err(|e| format!("CONDENSE_TRIGGERS: could not restore trigger {name} ({e}); nothing was changed"))?;
    }
    crate::audit_chain::seal(&mut tx).await?;
    tx.commit().await.map_err(db_err)?;
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Verification against the archive
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyCheck {
    pub id: &'static str,
    pub title: &'static str,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CondenseVerification {
    pub passed: bool,
    pub checks: Vec<VerifyCheck>,
}

fn verify_check(id: &'static str, title: &'static str, problems: Vec<String>, ok: impl Into<String>) -> VerifyCheck {
    let passed = problems.is_empty();
    let detail = if passed {
        ok.into()
    } else {
        let mut shown: Vec<String> = problems.iter().take(10).cloned().collect();
        if problems.len() > shown.len() {
            shown.push(format!("… and {} more", problems.len() - shown.len()));
        }
        shown.join("; ")
    };
    VerifyCheck { id, title, passed, detail }
}

/// Net debit per account, class and month of every posted line.
async fn monthly_balances(pool: &SqlitePool) -> Result<BTreeMap<(i64, Option<i64>, String), i64>, String> {
    let rows: Vec<(i64, Option<i64>, String, i64)> = sqlx::query_as(
        "SELECT l.account_id, l.class_id, substr(e.entry_date, 1, 7), SUM(l.debit_cents) - SUM(l.credit_cents)
         FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
         WHERE e.status = 'POSTED' GROUP BY 1, 2, 3",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().filter(|r| r.3 != 0).map(|(a, c, m, n)| ((a, c, m), n)).collect())
}

/// Net debit per account through `date`.
async fn balances_through(pool: &SqlitePool, date: NaiveDate) -> Result<BTreeMap<i64, i64>, String> {
    let rows: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT l.account_id, SUM(l.debit_cents) - SUM(l.credit_cents)
         FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
         WHERE e.status = 'POSTED' AND e.entry_date <= ?1 GROUP BY 1",
    )
    .bind(date)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows.into_iter().filter(|r| r.1 != 0).collect())
}

fn differences<K: Ord + std::fmt::Debug, V: PartialEq + std::fmt::Debug>(archive: &BTreeMap<K, V>, condensed: &BTreeMap<K, V>) -> Vec<String> {
    let keys: BTreeSet<&K> = archive.keys().chain(condensed.keys()).collect();
    keys.into_iter()
        .filter(|k| archive.get(k) != condensed.get(k))
        .map(|k| format!("{k:?}: archive {:?}, condensed {:?}", archive.get(k), condensed.get(k)))
        .collect()
}

fn same_json<T: Serialize>(label: &str, archive: &T, condensed: &T) -> Result<Vec<String>, String> {
    let a = serde_json::to_value(archive).map_err(|e| e.to_string())?;
    let b = serde_json::to_value(condensed).map_err(|e| e.to_string())?;
    Ok(if a == b { Vec::new() } else { vec![format!("{label} differs")] })
}

/// Cleared balance and uncleared lines per account (what the next reconciliation starts from).
async fn reconcile_state(pool: &SqlitePool) -> Result<BTreeMap<i64, (i64, Vec<i64>)>, String> {
    let accounts: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM chart_of_accounts a
         WHERE a.account_type IN ('BANK', 'CREDIT_CARD') OR EXISTS (SELECT 1 FROM reconciliations r WHERE r.account_id = a.id) ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    let mut state = BTreeMap::new();
    for account in accounts {
        let cleared = crate::reconcile::account_state(pool, account).await?.reconciled_balance_cents;
        let open = crate::reconcile::unreconciled_lines(pool, account).await?.into_iter().map(|l| l.line_id).collect();
        state.insert(account, (cleared, open));
    }
    Ok(state)
}

/// Every list and setting the condense leaves alone, digested table by table.
async fn kept_tables_digest(pool: &SqlitePool) -> Result<BTreeMap<String, String>, String> {
    let mut conn = pool.acquire().await.map_err(db_err)?;
    let mut digests = BTreeMap::new();
    for table in KEPT_TABLES.iter().filter(|t| !matches!(**t, "ledger_audit_log" | "ledger_chain" | "system_preferences")) {
        digests.insert(table.to_string(), table_digest(&mut conn, table).await?);
    }
    let preferences: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM system_preferences WHERE key NOT IN (?1, ?2, ?3, ?4) ORDER BY key")
        .bind(ROLE_KEY)
        .bind(ARCHIVE_THROUGH_KEY)
        .bind(CONDENSED_THROUGH_KEY)
        .bind(CONDENSE_ARCHIVE_KEY)
        .fetch_all(&mut *conn)
        .await
        .map_err(db_err)?;
    digests.insert("system_preferences".into(), format!("{:x}", Sha256::digest(serde_json::to_string(&preferences).unwrap_or_default())));
    Ok(digests)
}

async fn table_digest(conn: &mut SqliteConnection, table: &str) -> Result<String, String> {
    let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info(?1) ORDER BY cid").bind(table).fetch_all(&mut *conn).await.map_err(db_err)?;
    let row = columns.iter().map(|c| format!("quote({})", quote_ident(c))).collect::<Vec<_>>().join(" || ',' || ");
    let rows: Vec<String> = sqlx::query_scalar(&format!("SELECT {row} FROM {} ORDER BY rowid", quote_ident(table))).fetch_all(&mut *conn).await.map_err(db_err)?;
    let mut hash = Sha256::new();
    for r in rows {
        hash.update(r.as_bytes());
        hash.update(b"\n");
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// A digest of every row of every table: equal digests mean the books did not change.
pub async fn content_digest(pool: &SqlitePool) -> Result<String, String> {
    let mut conn = pool.acquire().await.map_err(db_err)?;
    let mut hash = Sha256::new();
    for table in all_tables(&mut conn).await? {
        hash.update(table.as_bytes());
        hash.update(table_digest(&mut conn, &table).await?.as_bytes());
    }
    Ok(format!("{:x}", hash.finalize()))
}

/// Compares the condensed copy with the archive it was made from.
pub async fn verify(archive: &SqlitePool, condensed: &SqlitePool, condensed_size: u64, cutoff: NaiveDate, today: NaiveDate) -> Result<CondenseVerification, String> {
    let mut checks = Vec::new();

    let report = file_dentist::checkup(condensed, condensed_size).await?;
    let failing: Vec<String> = report.checks.iter().filter(|c| c.status == CheckStatus::Fail).map(|c| format!("{}: {}", c.title, c.summary)).collect();
    checks.push(verify_check("CHECKUP", "File Dentist check-up of the condensed file", failing, "Passed"));

    for (id, title, pool) in [("AUDIT_CONDENSED", "Audit trail of the condensed file", condensed), ("AUDIT_ARCHIVE", "Audit trail of the archive", archive)] {
        let audit = ledger::verify_ledger_audit(pool).await?;
        let problems = audit.findings.iter().map(|f| format!("entry {}: {}", f.entry_id, f.problem)).collect();
        checks.push(verify_check(id, title, problems, format!("{} entries and {} audit records verified", audit.entries_checked, audit.audit_records_sealed)));
    }

    let problems = differences(&balances_through(archive, cutoff).await?, &balances_through(condensed, cutoff).await?);
    checks.push(verify_check("TRIAL_BALANCE", "Trial balance at the cut-off", problems, "Every account matches"));
    let problems = differences(&monthly_balances(archive).await?, &monthly_balances(condensed).await?);
    checks.push(verify_check("MONTHLY", "Balances by account, class and month", problems, "Every month matches"));

    for (id, title, payable) in [("AR_AGING", "A/R aging", false), ("AP_AGING", "A/P aging", true)] {
        let mut problems = Vec::new();
        for date in [cutoff, today] {
            let a = crate::report_center::aging(archive, payable, date).await?;
            let b = crate::report_center::aging(condensed, payable, date).await?;
            problems.extend(same_json(&format!("as of {date}"), &a, &b)?);
        }
        checks.push(verify_check(id, title, problems, "Matches at the cut-off and today"));
    }
    let mut problems = Vec::new();
    for date in [cutoff, today] {
        let a = crate::report_center::inventory_valuation(archive, date).await?;
        let b = crate::report_center::inventory_valuation(condensed, date).await?;
        problems.extend(same_json(&format!("as of {date}"), &a, &b)?);
    }
    checks.push(verify_check("INVENTORY", "Inventory valuation", problems, "Matches at the cut-off and today"));

    let problems = differences(&reconcile_state(archive).await?, &reconcile_state(condensed).await?);
    checks.push(verify_check("RECONCILE", "Cleared balances and uncleared lines", problems, "Every bank and card account matches"));

    let problems = differences(&kept_tables_digest(archive).await?, &kept_tables_digest(condensed).await?);
    checks.push(verify_check("LISTS", "Lists, settings and reconciliation history", problems, "Unchanged"));

    Ok(CondenseVerification { passed: checks.iter().all(|c| c.passed), checks })
}

// ------------------------------------------------------------------------------------------------
// The whole run
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CondenseResult {
    pub archive: BackupFile,
    pub safety_backup: BackupFile,
    pub preview: CondensePreview,
    pub verification: CondenseVerification,
    pub size_before: u64,
    pub size_after: u64,
}

async fn is_archive(conn: &mut SqliteConnection) -> Result<bool, String> {
    let role: Option<String> =
        sqlx::query_scalar("SELECT value FROM system_preferences WHERE key = ?1").bind(ROLE_KEY).fetch_optional(&mut *conn).await.map_err(db_err)?;
    Ok(role.as_deref() == Some(ARCHIVE_ROLE))
}

/// Whether the file on `pool` is an archive left by condensing (opened read-only).
pub async fn pool_is_archive(pool: &SqlitePool) -> bool {
    match pool.acquire().await {
        Ok(mut conn) => is_archive(&mut conn).await.unwrap_or(false),
        Err(_) => false,
    }
}

async fn open_checked(path: &Path) -> Result<SqlitePool, String> {
    let options = SqliteConnectOptions::new().filename(path).foreign_keys(true).create_if_missing(false);
    SqlitePoolOptions::new().max_connections(1).connect_with(options).await.map_err(|e| format!("CONDENSE_OPEN: cannot open {}: {e}", path.display()))
}

fn size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn remove_all(paths: &[&Path]) {
    for path in paths {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }
}

/// Condenses the open company file through `cutoff`. See the module documentation for the steps.
/// The books are left as they were unless every step and check succeeds.
pub async fn condense_live(db: &Db, cutoff: NaiveDate) -> Result<CondenseResult, String> {
    condense_live_on(db, cutoff, Local::now().date_naive()).await
}

pub(crate) async fn condense_live_on(db: &Db, cutoff: NaiveDate, today: NaiveDate) -> Result<CondenseResult, String> {
    let pool = db.pool().await;
    let live_path = db.path().await?;
    let size_before = size(&live_path);
    let (preview, _) = preview_pool(&pool, cutoff, today).await?;
    if let Some(blocker) = preview.blockers.first() {
        return Err(format!("CONDENSE_NOT_READY: {blocker}"));
    }
    // Only books that verify today can vouch for the archive and the condensed copy.
    let audit = ledger::verify_ledger_audit(&pool).await?;
    if !audit.intact {
        return Err(format!(
            "CONDENSE_AUDIT: the ledger audit trail reports {} problem(s), so the books cannot be archived as verified; open Ledger Integrity to review them",
            audit.findings.len()
        ));
    }

    let dir = db.backups_dir().to_path_buf();
    let stamp = Local::now().format("%Y%m%d-%H%M%S");
    let archive_path = dir.join(format!("archive-through-{cutoff}-{stamp}.db"));
    let safety_path = dir.join(format!("pre-condense-{stamp}.db"));
    let staging_path = dir.join(format!(".condense-staging-{}.db", std::process::id()));
    remove_all(&[&staging_path]);

    let result = build(&pool, &live_path, &archive_path, &safety_path, &staging_path, cutoff, today).await;
    let (digest, verification) = match result {
        Ok(done) => done,
        Err(e) => {
            remove_all(&[&staging_path, &archive_path, &safety_path]);
            return Err(e);
        }
    };
    if !verification.passed {
        remove_all(&[&staging_path, &archive_path, &safety_path]);
        let failed: Vec<String> = verification.checks.iter().filter(|c| !c.passed).map(|c| format!("{}: {}", c.title, c.detail)).collect();
        return Err(format!("CONDENSE_VERIFY: the condensed file did not match the archive, so your books were left as they were ({})", failed.join("; ")));
    }

    if let Err(e) = db.swap_in_unchanged(&staging_path, &safety_path, &digest).await {
        remove_all(&[&staging_path]);
        if e.starts_with("CONDENSE_BOOKS_CHANGED") {
            remove_all(&[&archive_path, &safety_path]);
        }
        return Err(e);
    }
    Ok(CondenseResult {
        archive: backup::describe_kind(&archive_path, "ARCHIVE")?,
        safety_backup: backup::describe_kind(&safety_path, "PRE_CONDENSE")?,
        preview,
        verification,
        size_before,
        size_after: size(&live_path),
    })
}

/// Writes and checks the archive, the safety copy and the condensed copy. Returns the books'
/// content digest (to confirm nothing changed before the swap) and the verification.
async fn build(
    pool: &SqlitePool,
    live_path: &Path,
    archive_path: &Path,
    safety_path: &Path,
    staging_path: &Path,
    cutoff: NaiveDate,
    today: NaiveDate,
) -> Result<(String, CondenseVerification), String> {
    // 1. The archive: a verified snapshot, its digest, and its audit-trail head.
    backup::snapshot(pool, live_path, archive_path).await?;
    backup::inspect(archive_path).await.map_err(|e| format!("CONDENSE_ARCHIVE: the archive copy could not be verified ({e}); nothing was changed"))?;
    let archive_pool = open_checked(archive_path).await?;
    let digest = content_digest(&archive_pool).await?;
    let archive_audit = ledger::verify_ledger_audit(&archive_pool).await?;
    let (file_id, head): (String, Option<(i64, String)>) = (
        sqlx::query_scalar("SELECT file_id FROM ledger_chain WHERE id = 1").fetch_one(&archive_pool).await.map_err(db_err)?,
        sqlx::query_as("SELECT id, row_hash FROM ledger_audit_log WHERE row_hash IS NOT NULL ORDER BY id DESC LIMIT 1")
            .fetch_optional(&archive_pool)
            .await
            .map_err(db_err)?,
    );
    crate::db::close_pool(&archive_pool).await;
    if !archive_audit.intact {
        return Err("CONDENSE_ARCHIVE: the archive copy's audit trail did not verify; nothing was changed".into());
    }

    // 2. A plain copy to undo with, and the copy to condense, both before the archive is marked.
    std::fs::copy(archive_path, safety_path).map_err(|e| format!("CONDENSE_BACKUP: cannot write {}: {e}", safety_path.display()))?;
    std::fs::copy(archive_path, staging_path).map_err(|e| format!("CONDENSE_STAGING: cannot write {}: {e}", staging_path.display()))?;
    let archive_pool = open_checked(archive_path).await?;
    for (key, value) in [(ROLE_KEY, ARCHIVE_ROLE.to_string()), (ARCHIVE_THROUGH_KEY, cutoff.to_string())] {
        sqlx::query("INSERT INTO system_preferences (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value")
            .bind(key)
            .bind(value)
            .execute(&archive_pool)
            .await
            .map_err(db_err)?;
    }

    // 3. Condense the copy.
    let record = ArchiveRecord {
        file_name: archive_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        file_id,
        audit_head_id: head.as_ref().map(|h| h.0),
        audit_head_hash: head.map(|h| h.1),
        content_sha256: digest.clone(),
    };
    let mut conn = open_private(staging_path).await?;
    let plan = plan(&mut conn, cutoff).await?;
    let condensed = condense_copy(&mut conn, &plan, &record, archive_path).await;
    let _ = conn.close().await;
    condensed?;
    let staging = open_checked(staging_path).await?;
    sqlx::query("VACUUM").execute(&staging).await.map_err(db_err)?;

    // 4. Check it against the archive.
    let verification = verify(&archive_pool, &staging, size(staging_path), cutoff, today).await;
    crate::db::close_pool(&staging).await;
    crate::db::close_pool(&archive_pool).await;
    Ok((digest, verification?))
}

/// What the open file says about condensing: the archive a condensed file came from, or the cut-off
/// an archive was made at.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CondenseStatus {
    pub is_archive: bool,
    pub archive_through: Option<String>,
    pub condensed_through: Option<String>,
    pub archive_path: Option<String>,
    pub archive_exists: bool,
}

pub async fn status(pool: &SqlitePool) -> Result<CondenseStatus, String> {
    let prefs: HashMap<String, String> = sqlx::query_as("SELECT key, value FROM system_preferences WHERE key IN (?1, ?2, ?3, ?4)")
        .bind(ROLE_KEY)
        .bind(ARCHIVE_THROUGH_KEY)
        .bind(CONDENSED_THROUGH_KEY)
        .bind(CONDENSE_ARCHIVE_KEY)
        .fetch_all(pool)
        .await
        .map_err(db_err)?
        .into_iter()
        .collect();
    let archive_path = prefs.get(CONDENSE_ARCHIVE_KEY).cloned();
    Ok(CondenseStatus {
        is_archive: prefs.get(ROLE_KEY).map(String::as_str) == Some(ARCHIVE_ROLE),
        archive_through: prefs.get(ARCHIVE_THROUGH_KEY).cloned(),
        condensed_through: prefs.get(CONDENSED_THROUGH_KEY).cloned(),
        archive_exists: archive_path.as_ref().is_some_and(|p| PathBuf::from(p).is_file()),
        archive_path,
    })
}

#[cfg(test)]
mod tests;
