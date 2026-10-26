//! Custom reports: one engine over posted transactions, like classic desktop accounting software's
//! Custom Transaction Detail and Summary reports. The detail reports in the Reports menu (P&L
//! Detail, Open Invoices, Check Detail, ...) are presets of it: each is only a [`ReportSettings`],
//! and a customized one can be saved under a name and run again (memorized reports).
//!
//! The engine reads one of two sources:
//! - **Ledger**: every posted journal line, joined to the document that posted it for the name,
//!   number, due date, open balance and rep. Where a line was posted from exactly one item line of
//!   its document (same account and class), it also shows that item, quantity and rate.
//! - **Items**: the item lines of posted sales and purchase documents, in home currency, for the
//!   reports by product/service. Credit memos and vendor credits count negative.
//!
//! Filters apply to lines; the result is either one row per line (detail, optionally grouped with
//! subtotals) or one row per group (summary, optionally with a column per month, quarter or year).
//! Amounts are on each account's normal side unless the settings ask for debit-minus-credit or
//! credit-minus-debit. Money is integer cents throughout.

use std::collections::{BTreeMap, HashSet};

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

use crate::ledger::{db_err, DISPLAY_SOURCE_TYPE};
use crate::preferences::{fiscal_start_month, fiscal_year_bounds};
use crate::report_center::{aging_bucket, AGING_LABELS};

// ------------------------------------------------------------------------------------------------
// Settings
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    #[default]
    Ledger,
    Items,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    #[default]
    Detail,
    Summary,
}

/// The sign of the Amount column (and the running balance).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sign {
    /// Positive on the account's normal side: income and liabilities count credits, assets and
    /// expenses count debits.
    #[default]
    Natural,
    /// Debits minus credits.
    Debit,
    /// Credits minus debits (income positive, expenses negative: net income).
    Credit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Column {
    Type,
    Date,
    Num,
    Name,
    Memo,
    Account,
    Split,
    Class,
    Item,
    Qty,
    Rate,
    Debit,
    Credit,
    Amount,
    Balance,
    OpenBalance,
    DueDate,
    Aging,
    Rep,
}

impl Column {
    pub fn label(self) -> &'static str {
        match self {
            Column::Type => "Type",
            Column::Date => "Date",
            Column::Num => "Num",
            Column::Name => "Name",
            Column::Memo => "Memo",
            Column::Account => "Account",
            Column::Split => "Split",
            Column::Class => "Class",
            Column::Item => "Item",
            Column::Qty => "Qty",
            Column::Rate => "Rate",
            Column::Debit => "Debit",
            Column::Credit => "Credit",
            Column::Amount => "Amount",
            Column::Balance => "Balance",
            Column::OpenBalance => "Open Balance",
            Column::DueDate => "Due Date",
            Column::Aging => "Aging",
            Column::Rep => "Rep",
        }
    }

    fn kind(self) -> ColumnKind {
        match self {
            Column::Date | Column::DueDate => ColumnKind::Date,
            Column::Qty => ColumnKind::Quantity,
            Column::Rate | Column::Debit | Column::Credit | Column::Amount | Column::Balance | Column::OpenBalance => ColumnKind::Money,
            Column::Aging => ColumnKind::Number,
            _ => ColumnKind::Text,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GroupBy {
    Account,
    Name,
    Item,
    Class,
    Type,
    Rep,
    /// One group per transaction (Journal, Check Detail).
    Transaction,
    Month,
    Quarter,
    Year,
    /// Current, 1–30, 31–60, 61–90 and 91+ days past due, as of the end date.
    Aging,
}

impl GroupBy {
    fn label(self) -> &'static str {
        match self {
            GroupBy::Account => "Account",
            GroupBy::Name => "Name",
            GroupBy::Item => "Product/Service",
            GroupBy::Class => "Class",
            GroupBy::Type => "Type",
            GroupBy::Rep => "Rep",
            GroupBy::Transaction => "Transaction",
            GroupBy::Month => "Month",
            GroupBy::Quarter => "Quarter",
            GroupBy::Year => "Year",
            GroupBy::Aging => "Aging",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Period {
    Month,
    Quarter,
    Year,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PaidStatus {
    /// Invoices, bills and credits with a balance left as of the end date.
    Open,
    /// Invoices, bills and credits fully paid or used by the end date.
    Paid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Cleared {
    /// Marked cleared in a reconciliation, finished or not.
    Cleared,
    Uncleared,
    /// In a finished (locked) reconciliation.
    Reconciled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NameKind {
    Customer,
    Vendor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameRef {
    pub kind: NameKind,
    pub id: i64,
}

/// Which lines a report includes. Empty lists and `None` mean "any".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Filters {
    /// Accounts, each with all of its subaccounts.
    pub account_ids: Vec<i64>,
    /// Account type codes (`BANK`, `ACCOUNTS_RECEIVABLE`, `INCOME`, ...).
    pub account_types: Vec<String>,
    /// `ASSET`, `LIABILITY`, `EQUITY`, `INCOME` or `EXPENSE`.
    pub classifications: Vec<String>,
    /// Only lines whose transaction names a customer, or a vendor.
    pub name_kind: Option<NameKind>,
    pub names: Vec<NameRef>,
    /// Only vendors marked for a 1099.
    pub vendors_1099: bool,
    /// Transaction types as journal entries show them (`INVOICE`, `CHECK`, `CARD_CREDIT`, ...).
    pub transaction_types: Vec<String>,
    /// Classes, each with all of its subclasses.
    pub class_ids: Vec<i64>,
    /// Also (or, with no `class_ids`, only) lines without a class.
    pub unclassified: bool,
    pub item_ids: Vec<i64>,
    /// Only lines with a product or service.
    pub items_only: bool,
    pub rep_ids: Vec<i64>,
    pub paid_status: Option<PaidStatus>,
    pub cleared: Option<Cleared>,
    /// Bounds on the line's amount, whichever side it is on.
    pub amount_min_cents: Option<i64>,
    pub amount_max_cents: Option<i64>,
    /// Text the memo contains, ignoring case.
    pub memo: Option<String>,
    /// Leave out voided transactions and the entries that void them.
    pub exclude_voided: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReportSettings {
    pub source: Source,
    /// First day of the report; `None` for all dates.
    pub start_date: Option<NaiveDate>,
    /// Last day of the report, and the date open balances and aging are figured at; `None` for today.
    pub end_date: Option<NaiveDate>,
    pub filters: Filters,
    pub mode: Mode,
    /// Detail mode's columns, in order.
    pub columns: Vec<Column>,
    /// Detail: the groups that get a heading and a subtotal. Summary: the rows.
    pub group_by: Option<GroupBy>,
    /// Summary: a column per period, plus a total.
    pub column_by: Option<Period>,
    pub sign: Sign,
    /// Detail grouped by account: start each account at its balance before the start date (income
    /// and expense accounts at their fiscal-year-to-date amount), as the General Ledger does.
    pub opening_balances: bool,
    /// One row per transaction (its first matching line) instead of one per line; Amount is the
    /// transaction's total.
    pub per_transaction: bool,
    /// Summary: a quantity column before the amount (reports by product/service).
    pub show_qty: bool,
    /// Summary: keep rows that total zero.
    pub show_zero_rows: bool,
}

// ------------------------------------------------------------------------------------------------
// Output
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ColumnKind {
    Text,
    Date,
    /// Integer cents.
    Money,
    /// Quantity scaled by 10,000.
    Quantity,
    Number,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnHead {
    pub label: String,
    pub kind: ColumnKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RowKind {
    Section,
    Detail,
    Subtotal,
    Total,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportRow {
    pub kind: RowKind,
    pub depth: u32,
    /// One value per column: text, a `YYYY-MM-DD` date, integer cents or quantity, or null. The
    /// first column holds group headings and total labels.
    pub cells: Vec<Value>,
    /// The transaction a detail row opens.
    pub journal_entry_id: Option<i64>,
    /// The account a heading opens (its register), when grouped by account.
    pub account_id: Option<i64>,
    /// Summary rows: the filters (and dates) that show this row's lines as a detail report.
    pub drill: Option<Drill>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Drill {
    pub account_ids: Vec<i64>,
    pub names: Vec<NameRef>,
    pub item_ids: Vec<i64>,
    pub class_ids: Vec<i64>,
    pub rep_ids: Vec<i64>,
    pub transaction_types: Vec<String>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomReport {
    pub start_date: Option<NaiveDate>,
    pub end_date: NaiveDate,
    pub columns: Vec<ColumnHead>,
    pub rows: Vec<ReportRow>,
    /// Detail rows (lines or transactions) in the report.
    pub line_count: usize,
}

/// Labels of the transaction types (as `JournalEntries.jsx` shows them).
pub const TRANSACTION_TYPES: [(&str, &str); 14] = [
    ("JOURNAL", "Journal Entry"),
    ("INVOICE", "Invoice"),
    ("RECEIVE_PAYMENT", "Payment"),
    ("SALES_RECEIPT", "Sales Receipt"),
    ("CREDIT_MEMO", "Credit Memo"),
    ("BILL", "Bill"),
    ("BILL_PAYMENT", "Bill Payment"),
    ("VENDOR_CREDIT", "Vendor Credit"),
    ("CHECK", "Check"),
    ("EXPENSE", "Expense"),
    ("CARD_CREDIT", "Credit Card Credit"),
    ("DEPOSIT", "Deposit"),
    ("TRANSFER", "Transfer"),
    ("OPENING_BALANCE", "Opening Balance"),
];

pub fn type_label(code: &str) -> &str {
    TRANSACTION_TYPES.iter().find(|(c, _)| *c == code).map_or(code, |(_, label)| label)
}

// ------------------------------------------------------------------------------------------------
// Reading lines
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, sqlx::FromRow)]
struct Line {
    journal_entry_id: i64,
    entry_number: i64,
    date: NaiveDate,
    source_type: String,
    is_reversal: bool,
    is_voided: bool,
    party_kind: Option<String>,
    party_id: Option<i64>,
    name: Option<String>,
    is_1099: bool,
    num: Option<String>,
    due_date: Option<NaiveDate>,
    open_cents: Option<i64>,
    rep_id: Option<i64>,
    rep_initials: Option<String>,
    rep_name: Option<String>,
    memo: Option<String>,
    account_id: i64,
    account_name: String,
    account_number: Option<String>,
    account_type: String,
    classification: String,
    debit_normal: bool,
    type_sort: i64,
    split: Option<String>,
    class_id: Option<i64>,
    class_name: Option<String>,
    item_id: Option<i64>,
    item_name: Option<String>,
    /// Every item behind the line, comma-separated (for the item filter).
    items: Option<String>,
    qty_e4: Option<i64>,
    rate_cents: Option<i64>,
    debit_cents: i64,
    credit_cents: i64,
    /// Items source: the line's signed amount.
    item_amount_cents: Option<i64>,
    entry_total_cents: i64,
    cleared: bool,
    reconciled: bool,
}

impl Line {
    fn party(&self) -> Option<NameRef> {
        let kind = match self.party_kind.as_deref()? {
            "customer" => NameKind::Customer,
            _ => NameKind::Vendor,
        };
        Some(NameRef { kind, id: self.party_id? })
    }

    /// The Amount column, by the settings' sign.
    fn amount(&self, sign: Sign) -> i64 {
        if let Some(amount) = self.item_amount_cents {
            return amount;
        }
        signed(self.debit_cents - self.credit_cents, self.debit_normal, sign)
    }

    fn type_text(&self) -> &str {
        if self.is_reversal {
            "Void"
        } else {
            type_label(&self.source_type)
        }
    }

    fn rep_label(&self) -> Option<String> {
        match (&self.rep_initials, &self.rep_name) {
            (Some(i), Some(n)) => Some(format!("{i} ({n})")),
            (Some(i), None) => Some(i.clone()),
            _ => None,
        }
    }

    fn days_past_due(&self, as_of: NaiveDate) -> Option<i64> {
        match (self.due_date, self.open_cents) {
            (Some(due), Some(open)) if open != 0 => Some((as_of - due).num_days()),
            _ => None,
        }
    }
}

/// Debits minus credits (`net`) on the chosen side.
fn signed(net: i64, debit_normal: bool, sign: Sign) -> i64 {
    match sign {
        Sign::Debit => net,
        Sign::Credit => -net,
        Sign::Natural if debit_normal => net,
        Sign::Natural => -net,
    }
}

/// The document behind each entry: name, number, due date, open balance as of `?2` and rep. Names
/// follow `v_entry_party`, so reports by name agree with the existing summaries.
const ENTRY_JOINS: &str = "
    LEFT JOIN invoices inv ON e.source_type = 'INVOICE' AND inv.id = e.source_id
    LEFT JOIN invoice_payments ip ON e.source_type = 'RECEIVE_PAYMENT' AND ip.id = e.source_id
    LEFT JOIN invoices ipi ON ipi.id = ip.invoice_id
    LEFT JOIN sales_receipts sr ON e.source_type = 'SALES_RECEIPT' AND sr.id = e.source_id
    LEFT JOIN credit_memos cm ON e.source_type = 'CREDIT_MEMO' AND cm.id = e.source_id
    LEFT JOIN bills bl ON e.source_type = 'BILL' AND bl.id = e.source_id
    LEFT JOIN bill_payments bp ON e.source_type = 'BILL_PAYMENT' AND bp.id = e.source_id
    LEFT JOIN bills bpb ON bpb.id = bp.bill_id
    LEFT JOIN vendor_credits vc ON e.source_type = 'VENDOR_CREDIT' AND vc.id = e.source_id
    LEFT JOIN expenses x ON e.source_type IN ('EXPENSE', 'CHECK') AND x.id = e.source_id
    LEFT JOIN credit_refunds crf ON e.source_type = 'JOURNAL' AND crf.journal_entry_id = e.id
    LEFT JOIN credit_memos crm ON crm.id = crf.credit_memo_id
    LEFT JOIN vendor_credit_refunds vrf ON e.source_type = 'JOURNAL' AND vrf.journal_entry_id = e.id
    LEFT JOIN vendor_credits vrc ON vrc.id = vrf.vendor_credit_id
    LEFT JOIN customers pc ON pc.id = COALESCE(inv.customer_id, ipi.customer_id, sr.customer_id, cm.customer_id, crm.customer_id)
    LEFT JOIN vendors pv ON pv.id = COALESCE(bl.vendor_id, bpb.vendor_id, vc.vendor_id, x.vendor_id, vrc.vendor_id)
    LEFT JOIN sales_reps rep ON rep.id = COALESCE(inv.sales_rep_id, sr.sales_rep_id)";

/// Open balances use the same arithmetic as `report_center::aging`; only the document's current
/// entry carries one (an edited document's earlier entries show zero).
const ENTRY_COLUMNS: &str = "
    CASE WHEN pc.id IS NOT NULL THEN 'customer' WHEN pv.id IS NOT NULL THEN 'vendor' END AS party_kind,
    COALESCE(pc.id, pv.id) AS party_id,
    COALESCE(pc.name, pv.name) AS name,
    COALESCE(pv.is_1099, 0) AS is_1099,
    COALESCE(CAST(inv.invoice_number AS TEXT), ip.reference, CAST(sr.receipt_number AS TEXT), CAST(cm.credit_number AS TEXT),
             bl.bill_number, bp.reference, vc.reference, x.check_number,
             CASE WHEN e.source_type IN ('JOURNAL', 'OPENING_BALANCE') THEN CAST(e.entry_number AS TEXT) END) AS num,
    COALESCE(inv.due_date, bl.due_date, cm.credit_date, vc.credit_date) AS due_date,
    CASE
        WHEN inv.id IS NOT NULL THEN CASE WHEN inv.journal_entry_id IS NOT e.id OR inv.is_voided = 1 THEN 0 ELSE
            COALESCE(inv.home_total_cents, inv.total_cents)
            - COALESCE((SELECT SUM(COALESCE(p.ar_relief_cents, p.amount_cents)) FROM invoice_payments p
                        WHERE p.invoice_id = inv.id AND p.is_voided = 0 AND p.payment_date <= ?2), 0)
            - COALESCE((SELECT SUM(a.amount_cents) FROM credit_applications a WHERE a.invoice_id = inv.id AND a.is_voided = 0 AND a.applied_date <= ?2), 0) END
        WHEN cm.id IS NOT NULL AND cm.refund_account_id IS NULL THEN CASE WHEN cm.journal_entry_id IS NOT e.id OR cm.is_voided = 1 THEN 0 ELSE
            -(cm.total_cents
              - COALESCE((SELECT SUM(a.amount_cents) FROM credit_applications a WHERE a.credit_memo_id = cm.id AND a.is_voided = 0 AND a.applied_date <= ?2), 0)
              - COALESCE((SELECT SUM(r.amount_cents) FROM credit_refunds r JOIN journal_entries re ON re.id = r.journal_entry_id
                          WHERE r.credit_memo_id = cm.id AND re.is_voided = 0 AND r.refund_date <= ?2), 0)) END
        WHEN bl.id IS NOT NULL THEN CASE WHEN bl.journal_entry_id IS NOT e.id OR bl.is_voided = 1 THEN 0 ELSE
            bl.total_cents
            - COALESCE((SELECT SUM(p.amount_cents) FROM bill_payments p WHERE p.bill_id = bl.id AND p.is_voided = 0 AND p.payment_date <= ?2), 0)
            - COALESCE((SELECT SUM(a.amount_cents) FROM vendor_credit_applications a WHERE a.bill_id = bl.id AND a.is_voided = 0 AND a.applied_date <= ?2), 0) END
        WHEN vc.id IS NOT NULL THEN CASE WHEN vc.journal_entry_id IS NOT e.id OR vc.is_voided = 1 THEN 0 ELSE
            -(vc.total_cents
              - COALESCE((SELECT SUM(a.amount_cents) FROM vendor_credit_applications a WHERE a.vendor_credit_id = vc.id AND a.is_voided = 0 AND a.applied_date <= ?2), 0)
              - COALESCE((SELECT SUM(r.amount_cents) FROM vendor_credit_refunds r JOIN journal_entries re ON re.id = r.journal_entry_id
                          WHERE r.vendor_credit_id = vc.id AND re.is_voided = 0 AND r.refund_date <= ?2), 0)) END
    END AS open_cents,
    rep.id AS rep_id, rep.initials AS rep_initials, rep.name AS rep_name";

/// The item lines of every posted sales and purchase document, with the entry that posted them.
/// Amounts are in home currency (prorated as `report_center::sales_by_product` does) and negative
/// for credits.
const ITEM_LINES: &str = "
    SELECT i.journal_entry_id AS je, it.line_number, it.description AS memo, it.income_account_id AS account_id, it.class_id,
           it.product_service_id AS item_id, it.quantity_e4 AS qty_e4, it.rate_cents,
           it.amount_cents * COALESCE(i.home_total_cents, i.total_cents) / i.total_cents AS amount
    FROM invoice_items it JOIN invoices i ON i.id = it.invoice_id
    WHERE i.is_voided = 0 AND i.journal_entry_id IS NOT NULL
    UNION ALL
    SELECT r.journal_entry_id, sl.line_number, sl.description, sl.income_account_id, sl.class_id,
           sl.product_service_id, sl.quantity_e4, sl.rate_cents, sl.amount_cents
    FROM sales_receipt_lines sl JOIN sales_receipts r ON r.id = sl.sales_receipt_id
    WHERE r.is_voided = 0 AND r.journal_entry_id IS NOT NULL
    UNION ALL
    SELECT m.journal_entry_id, cl.line_number, cl.description, cl.income_account_id, cl.class_id,
           cl.product_service_id, -cl.quantity_e4, cl.rate_cents, -cl.amount_cents
    FROM credit_memo_lines cl JOIN credit_memos m ON m.id = cl.credit_memo_id
    WHERE m.is_voided = 0 AND m.journal_entry_id IS NOT NULL
    UNION ALL
    SELECT b.journal_entry_id, bt.line_number, bt.memo, bt.account_id, bt.class_id,
           bt.product_service_id, bt.quantity_e4, bt.rate_cents, bt.amount_cents
    FROM bill_lines bt JOIN bills b ON b.id = bt.bill_id
    WHERE b.is_voided = 0 AND b.journal_entry_id IS NOT NULL
    UNION ALL
    SELECT c.journal_entry_id, vl.line_number, vl.memo, vl.account_id, vl.class_id, NULL, NULL, NULL, -vl.amount_cents
    FROM vendor_credit_lines vl JOIN vendor_credits c ON c.id = vl.vendor_credit_id
    WHERE c.is_voided = 0 AND c.journal_entry_id IS NOT NULL
    UNION ALL
    SELECT x.journal_entry_id, el.line_number, el.memo, el.account_id, el.class_id, NULL, NULL, NULL,
           CASE WHEN x.is_credit = 1 THEN -1 ELSE 1 END * el.amount_cents * COALESCE(x.home_total_cents, x.total_cents) / x.total_cents
    FROM expense_lines el JOIN expenses x ON x.id = el.expense_id
    WHERE x.is_voided = 0 AND x.journal_entry_id IS NOT NULL";

const ACCOUNT_COLUMNS: &str = "a.id AS account_id, a.full_name AS account_name, a.account_number, a.account_type, a.classification,
    a.normal_balance = 'DEBIT' AS debit_normal, a.type_sort_order AS type_sort";

async fn ledger_lines(pool: &SqlitePool, start: Option<NaiveDate>, end: NaiveDate) -> Result<Vec<Line>, String> {
    // Item details for a line come from the document lines posted to the same account and class.
    let sql = format!(
        "SELECT e.id AS journal_entry_id, e.entry_number, e.entry_date AS date, {DISPLAY_SOURCE_TYPE} AS source_type,
                e.reverses_entry_id IS NOT NULL AS is_reversal, e.is_voided, l.line_number, {ENTRY_COLUMNS},
                COALESCE(l.memo, e.memo) AS memo, {ACCOUNT_COLUMNS},
                (SELECT CASE WHEN COUNT(*) = 1 THEN MAX(o_a.full_name) WHEN COUNT(*) > 1 THEN '-Split-' END
                 FROM journal_lines o JOIN v_chart_of_accounts o_a ON o_a.id = o.account_id
                 WHERE o.journal_entry_id = e.id AND o.account_id <> l.account_id) AS split,
                l.class_id, k.full_name AS class_name,
                CASE WHEN g.n = 1 THEN g.item_id END AS item_id, ps.name AS item_name, g.items,
                CASE WHEN g.n = 1 THEN g.qty_e4 END AS qty_e4, CASE WHEN g.n = 1 THEN g.rate_cents END AS rate_cents,
                l.debit_cents, l.credit_cents, NULL AS item_amount_cents,
                (SELECT SUM(t.debit_cents) FROM journal_lines t WHERE t.journal_entry_id = e.id) AS entry_total_cents,
                l.reconciliation_id IS NOT NULL AS cleared, l.reconciled_at IS NOT NULL AS reconciled
         FROM journal_lines l
         JOIN journal_entries e ON e.id = l.journal_entry_id
         JOIN v_chart_of_accounts a ON a.id = l.account_id
         LEFT JOIN v_classes k ON k.id = l.class_id
         {ENTRY_JOINS}
         LEFT JOIN (SELECT je, account_id, class_id, COUNT(*) AS n, MAX(item_id) AS item_id, MAX(qty_e4) AS qty_e4,
                           MAX(rate_cents) AS rate_cents, group_concat(DISTINCT item_id) AS items
                    FROM ({ITEM_LINES}) GROUP BY je, account_id, class_id) g
                ON g.je = e.id AND g.account_id = l.account_id AND g.class_id IS l.class_id
         LEFT JOIN products_services ps ON ps.id = CASE WHEN g.n = 1 THEN g.item_id END
         WHERE e.status = 'POSTED' AND (?1 IS NULL OR e.entry_date >= ?1) AND e.entry_date <= ?2
         ORDER BY e.entry_date, e.entry_number, l.line_number"
    );
    sqlx::query_as(&sql).bind(start).bind(end).fetch_all(pool).await.map_err(db_err)
}

async fn item_lines(pool: &SqlitePool, start: Option<NaiveDate>, end: NaiveDate) -> Result<Vec<Line>, String> {
    let sql = format!(
        "SELECT e.id AS journal_entry_id, e.entry_number, e.entry_date AS date, {DISPLAY_SOURCE_TYPE} AS source_type,
                0 AS is_reversal, e.is_voided, d.line_number, {ENTRY_COLUMNS},
                COALESCE(d.memo, e.memo) AS memo, {ACCOUNT_COLUMNS},
                NULL AS split, d.class_id, k.full_name AS class_name,
                d.item_id, ps.name AS item_name, CAST(d.item_id AS TEXT) AS items, d.qty_e4, d.rate_cents,
                0 AS debit_cents, 0 AS credit_cents, d.amount AS item_amount_cents,
                (SELECT SUM(t.debit_cents) FROM journal_lines t WHERE t.journal_entry_id = e.id) AS entry_total_cents,
                0 AS cleared, 0 AS reconciled
         FROM ({ITEM_LINES}) d
         JOIN journal_entries e ON e.id = d.je
         JOIN v_chart_of_accounts a ON a.id = d.account_id
         LEFT JOIN v_classes k ON k.id = d.class_id
         LEFT JOIN products_services ps ON ps.id = d.item_id
         {ENTRY_JOINS}
         WHERE e.status = 'POSTED' AND (?1 IS NULL OR e.entry_date >= ?1) AND e.entry_date <= ?2
         ORDER BY e.entry_date, e.entry_number, d.line_number"
    );
    sqlx::query_as(&sql).bind(start).bind(end).fetch_all(pool).await.map_err(db_err)
}

/// The ids themselves and everything below them in `view` (`v_account_ancestry` or `v_class_ancestry`).
async fn with_descendants(pool: &SqlitePool, view: &str, column: &str, ids: &[i64]) -> Result<HashSet<i64>, String> {
    if ids.is_empty() {
        return Ok(HashSet::new());
    }
    let json = serde_json::to_string(ids).map_err(|e| e.to_string())?;
    let rows: Vec<i64> = sqlx::query_scalar(&format!("SELECT DISTINCT a.{column} FROM {view} a JOIN json_each(?1) j ON j.value = a.ancestor_id"))
        .bind(json)
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    Ok(rows.into_iter().chain(ids.iter().copied()).collect())
}

struct LineFilter<'a> {
    f: &'a Filters,
    accounts: HashSet<i64>,
    classes: HashSet<i64>,
    memo: Option<String>,
}

impl LineFilter<'_> {
    fn account_matches(&self, account_id: i64, account_type: &str, classification: &str) -> bool {
        let f = self.f;
        (f.account_ids.is_empty() || self.accounts.contains(&account_id))
            && (f.account_types.is_empty() || f.account_types.iter().any(|t| t == account_type))
            && (f.classifications.is_empty() || f.classifications.iter().any(|c| c == classification))
    }

    fn matches(&self, l: &Line) -> bool {
        let f = self.f;
        if !self.account_matches(l.account_id, &l.account_type, &l.classification) {
            return false;
        }
        let party = l.party();
        if let Some(kind) = f.name_kind {
            if party.map(|p| p.kind) != Some(kind) {
                return false;
            }
        }
        if !f.names.is_empty() && !party.is_some_and(|p| f.names.contains(&p)) {
            return false;
        }
        if f.vendors_1099 && !(l.is_1099 && party.is_some_and(|p| p.kind == NameKind::Vendor)) {
            return false;
        }
        if !f.transaction_types.is_empty() && (l.is_reversal || !f.transaction_types.contains(&l.source_type)) {
            return false;
        }
        if !f.class_ids.is_empty() || f.unclassified {
            let ok = match l.class_id {
                Some(c) => self.classes.contains(&c),
                None => f.unclassified,
            };
            if !ok {
                return false;
            }
        }
        let items: Vec<i64> = l.items.as_deref().unwrap_or("").split(',').filter_map(|s| s.trim().parse().ok()).collect();
        if f.items_only && items.is_empty() {
            return false;
        }
        if !f.item_ids.is_empty() && !items.iter().any(|i| f.item_ids.contains(i)) {
            return false;
        }
        if !f.rep_ids.is_empty() && !l.rep_id.is_some_and(|r| f.rep_ids.contains(&r)) {
            return false;
        }
        match (f.paid_status, l.open_cents) {
            (None, _) => {}
            (Some(_), None) => return false,
            (Some(PaidStatus::Open), Some(open)) if open == 0 => return false,
            (Some(PaidStatus::Paid), Some(open)) if open != 0 => return false,
            _ => {}
        }
        match f.cleared {
            Some(Cleared::Cleared) if !l.cleared => return false,
            Some(Cleared::Uncleared) if l.cleared => return false,
            Some(Cleared::Reconciled) if !l.reconciled => return false,
            _ => {}
        }
        let size = l.item_amount_cents.map_or(l.debit_cents + l.credit_cents, i64::abs);
        if f.amount_min_cents.is_some_and(|min| size < min) || f.amount_max_cents.is_some_and(|max| size > max) {
            return false;
        }
        if let Some(text) = &self.memo {
            if !l.memo.as_deref().is_some_and(|m| m.to_lowercase().contains(text)) {
                return false;
            }
        }
        if f.exclude_voided && (l.is_voided || l.is_reversal) {
            return false;
        }
        true
    }
}

// ------------------------------------------------------------------------------------------------
// Grouping
// ------------------------------------------------------------------------------------------------

/// Sort key of a group: rank, then text, then a tiebreaking id.
type GroupKey = (i64, String, String, i64);

fn period_key(date: NaiveDate, period: Period) -> (i64, String) {
    match period {
        Period::Month => (i64::from(date.year()) * 100 + i64::from(date.month()), date.format("%b %Y").to_string()),
        Period::Quarter => {
            let q = (date.month() - 1) / 3 + 1;
            (i64::from(date.year()) * 10 + i64::from(q), format!("Q{q} {}", date.year()))
        }
        Period::Year => (i64::from(date.year()), date.year().to_string()),
    }
}

/// First and last day of the period holding `date`.
fn period_bounds(date: NaiveDate, period: Period) -> (NaiveDate, NaiveDate) {
    let (y, first_month, months) = match period {
        Period::Month => (date.year(), date.month(), 1),
        Period::Quarter => (date.year(), (date.month() - 1) / 3 * 3 + 1, 3),
        Period::Year => (date.year(), 1, 12),
    };
    let start = NaiveDate::from_ymd_opt(y, first_month, 1).expect("first of month");
    let end = start.checked_add_months(chrono::Months::new(months)).expect("in range").pred_opt().expect("in range");
    (start, end)
}

/// Sorts names as SQLite's NOCASE does (ASCII letters only).
fn nocase(s: &str) -> String {
    s.to_ascii_lowercase()
}

fn group_of(by: GroupBy, l: &Line, as_of: NaiveDate) -> (GroupKey, String) {
    let missing = |v: Option<&String>| i64::from(v.is_none());
    match by {
        GroupBy::Account => (
            (l.type_sort, l.account_number.clone().unwrap_or_default(), l.account_name.clone(), l.account_id),
            l.account_name.clone(),
        ),
        GroupBy::Name => ((missing(l.name.as_ref()), l.name.as_deref().map(nocase).unwrap_or_default(), String::new(), 0), l.name.clone().unwrap_or_else(|| "No name".into())),
        GroupBy::Item => (
            (missing(l.item_name.as_ref()), l.item_name.as_deref().map(nocase).unwrap_or_default(), String::new(), l.item_id.unwrap_or(0)),
            l.item_name.clone().unwrap_or_else(|| "No product/service".into()),
        ),
        GroupBy::Class => (
            (missing(l.class_name.as_ref()), l.class_name.as_deref().map(nocase).unwrap_or_default(), String::new(), l.class_id.unwrap_or(0)),
            l.class_name.clone().unwrap_or_else(|| "Unclassified".into()),
        ),
        GroupBy::Type => ((0, l.type_text().to_string(), String::new(), 0), l.type_text().to_string()),
        GroupBy::Rep => {
            let label = l.rep_label();
            ((missing(label.as_ref()), label.as_deref().map(nocase).unwrap_or_default(), String::new(), l.rep_id.unwrap_or(0)), label.unwrap_or_else(|| "No rep".into()))
        }
        GroupBy::Transaction => {
            let label = match &l.num {
                Some(num) => format!("{} {num}", l.type_text()),
                None => l.type_text().to_string(),
            };
            ((i64::from(l.date.num_days_from_ce()), String::new(), String::new(), l.entry_number), format!("{label} · {}", l.date))
        }
        GroupBy::Month | GroupBy::Quarter | GroupBy::Year => {
            let period = match by {
                GroupBy::Month => Period::Month,
                GroupBy::Quarter => Period::Quarter,
                _ => Period::Year,
            };
            let (rank, label) = period_key(l.date, period);
            ((rank, String::new(), String::new(), 0), label)
        }
        GroupBy::Aging => {
            let bucket = l.days_past_due(as_of).map_or(0, aging_bucket);
            ((bucket as i64, String::new(), String::new(), 0), AGING_LABELS[bucket].to_string())
        }
    }
}

struct Group<'a> {
    label: String,
    lines: Vec<&'a Line>,
    /// Beginning balance (grouped by account with `opening_balances`).
    opening: Option<i64>,
    account_id: Option<i64>,
}

fn drill_for(by: GroupBy, group: &Group<'_>, start: Option<NaiveDate>, end: NaiveDate) -> Option<Drill> {
    let first = group.lines.first()?;
    let mut drill = Drill::default();
    match by {
        GroupBy::Account => drill.account_ids = vec![first.account_id],
        GroupBy::Name => {
            let mut names: Vec<NameRef> = group.lines.iter().filter_map(|l| l.party()).collect();
            if names.is_empty() {
                return None;
            }
            names.sort_by_key(|n| (n.kind as u8, n.id));
            names.dedup();
            drill.names = names;
        }
        GroupBy::Item => drill.item_ids = vec![first.item_id?],
        GroupBy::Class => drill.class_ids = vec![first.class_id?],
        GroupBy::Rep => drill.rep_ids = vec![first.rep_id?],
        GroupBy::Type if !first.is_reversal => drill.transaction_types = vec![first.source_type.clone()],
        GroupBy::Month | GroupBy::Quarter | GroupBy::Year => {
            let period = match by {
                GroupBy::Month => Period::Month,
                GroupBy::Quarter => Period::Quarter,
                _ => Period::Year,
            };
            let (from, to) = period_bounds(first.date, period);
            drill.start_date = Some(start.map_or(from, |s| s.max(from)));
            drill.end_date = Some(to.min(end));
        }
        _ => return None,
    }
    Some(drill)
}

// ------------------------------------------------------------------------------------------------
// Running a report
// ------------------------------------------------------------------------------------------------

/// Runs a report; `today` stands in for a missing end date.
pub async fn run(pool: &SqlitePool, settings: &ReportSettings, today: NaiveDate) -> Result<CustomReport, String> {
    let end = settings.end_date.unwrap_or(today);
    if settings.start_date.is_some_and(|s| s > end) {
        return Err("RPT_BAD_RANGE: the end date is before the start date".into());
    }
    if settings.mode == Mode::Detail && settings.columns.is_empty() {
        return Err("RPT_NO_COLUMNS: choose at least one column".into());
    }
    let f = &settings.filters;
    let filter = LineFilter {
        f,
        accounts: with_descendants(pool, "v_account_ancestry", "account_id", &f.account_ids).await?,
        classes: with_descendants(pool, "v_class_ancestry", "class_id", &f.class_ids).await?,
        memo: f.memo.as_deref().map(str::trim).filter(|m| !m.is_empty()).map(str::to_lowercase),
    };
    let all = match settings.source {
        Source::Ledger => ledger_lines(pool, settings.start_date, end).await?,
        Source::Items => item_lines(pool, settings.start_date, end).await?,
    };
    let mut lines: Vec<Line> = all.into_iter().filter(|l| filter.matches(l)).collect();
    if settings.per_transaction {
        let mut seen = HashSet::new();
        lines.retain(|l| seen.insert(l.journal_entry_id));
    }

    let openings = if settings.mode == Mode::Detail && settings.group_by == Some(GroupBy::Account) && settings.opening_balances && settings.source == Source::Ledger {
        Some(opening_balances(pool, &filter, settings.start_date, end, settings.sign).await?)
    } else {
        None
    };

    let mut groups: BTreeMap<GroupKey, Group<'_>> = BTreeMap::new();
    let by = settings.group_by.unwrap_or(GroupBy::Account);
    let grouped = settings.group_by.is_some() || settings.mode == Mode::Summary;
    if let Some(openings) = &openings {
        for o in openings {
            groups.insert(o.key.clone(), Group { label: o.label.clone(), lines: Vec::new(), opening: Some(o.cents), account_id: Some(o.account_id) });
        }
    }
    for l in &lines {
        let (key, label) = if grouped { group_of(by, l, end) } else { ((0, String::new(), String::new(), 0), String::new()) };
        let group = groups.entry(key).or_insert_with(|| Group {
            label,
            lines: Vec::new(),
            opening: openings.as_ref().map(|_| 0),
            account_id: (by == GroupBy::Account).then_some(l.account_id),
        });
        group.lines.push(l);
    }
    // As in the General Ledger: an account shows when it has activity or a beginning balance.
    groups.retain(|_, g| !g.lines.is_empty() || g.opening.is_some_and(|o| o != 0));

    let (columns, rows) = match settings.mode {
        Mode::Detail => detail(settings, grouped, &groups, end),
        Mode::Summary => summary(settings, by, &groups, &lines, end),
    };
    Ok(CustomReport { start_date: settings.start_date, end_date: end, columns, rows, line_count: lines.len() })
}

struct Opening {
    key: GroupKey,
    label: String,
    account_id: i64,
    cents: i64,
}

/// Each account's balance before `start` on the report's sign: balance sheet accounts from the
/// beginning, income and expense accounts from the start of the fiscal year (as the General
/// Ledger begins them). Only account filters apply.
async fn opening_balances(pool: &SqlitePool, filter: &LineFilter<'_>, start: Option<NaiveDate>, end: NaiveDate, sign: Sign) -> Result<Vec<Opening>, String> {
    let Some(start) = start else { return Ok(Vec::new()) };
    let (fiscal_start, _) = fiscal_year_bounds(start, fiscal_start_month(pool).await?);
    let rows: Vec<(i64, String, Option<String>, String, String, bool, i64, i64)> = sqlx::query_as(
        "SELECT a.id, a.full_name, a.account_number, a.account_type, a.classification, a.normal_balance = 'DEBIT', a.type_sort_order,
                COALESCE((SELECT SUM(p.debit_cents - p.credit_cents) FROM v_posted_lines p
                          WHERE p.account_id = a.id AND p.entry_date < ?1 AND (a.is_balance_sheet = 1 OR p.entry_date >= ?2)), 0)
         FROM v_chart_of_accounts a
         WHERE EXISTS (SELECT 1 FROM v_posted_lines l WHERE l.account_id = a.id AND l.entry_date <= ?3)",
    )
    .bind(start)
    .bind(fiscal_start)
    .bind(end)
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    Ok(rows
        .into_iter()
        .filter(|(id, _, _, account_type, classification, ..)| filter.account_matches(*id, account_type, classification))
        .map(|(id, name, number, _, _, debit_normal, sort, net)| Opening {
            key: (sort, number.unwrap_or_default(), name.clone(), id),
            label: name,
            account_id: id,
            cents: signed(net, debit_normal, sign),
        })
        .collect())
}

fn money(cents: i64) -> Value {
    json!(cents)
}

fn opt<T: Into<Value>>(v: Option<T>) -> Value {
    v.map_or(Value::Null, Into::into)
}

fn cell(column: Column, l: &Line, settings: &ReportSettings, balance: i64, as_of: NaiveDate) -> Value {
    let nonzero = |c: i64| if c == 0 { Value::Null } else { money(c) };
    match column {
        Column::Type => json!(l.type_text()),
        Column::Date => json!(l.date.to_string()),
        Column::Num => opt(l.num.clone()),
        Column::Name => opt(l.name.clone()),
        Column::Memo => opt(l.memo.clone()),
        Column::Account => json!(l.account_name),
        Column::Split => opt(l.split.clone()),
        Column::Class => opt(l.class_name.clone()),
        Column::Item => opt(l.item_name.clone()),
        Column::Qty => opt(l.qty_e4),
        Column::Rate => opt(l.rate_cents),
        Column::Debit => nonzero(l.debit_cents),
        Column::Credit => nonzero(l.credit_cents),
        Column::Amount if settings.per_transaction => money(l.entry_total_cents),
        Column::Amount => money(l.amount(settings.sign)),
        Column::Balance => money(balance),
        Column::OpenBalance => opt(l.open_cents),
        Column::DueDate => opt(l.due_date.map(|d| d.to_string())),
        Column::Aging => opt(l.days_past_due(as_of).filter(|d| *d > 0)),
        Column::Rep => opt(l.rep_label()),
    }
}

/// Sums for a subtotal or total row.
#[derive(Default)]
struct Sums {
    debit: i64,
    credit: i64,
    amount: i64,
    open: i64,
    qty: i64,
}

impl Sums {
    fn add(&mut self, l: &Line, settings: &ReportSettings) {
        self.debit += l.debit_cents;
        self.credit += l.credit_cents;
        self.amount += if settings.per_transaction { l.entry_total_cents } else { l.amount(settings.sign) };
        self.open += l.open_cents.unwrap_or(0);
        self.qty += l.qty_e4.unwrap_or(0);
    }

    fn merge(&mut self, other: &Sums) {
        self.debit += other.debit;
        self.credit += other.credit;
        self.amount += other.amount;
        self.open += other.open;
        self.qty += other.qty;
    }

    fn cells(&self, label: String, columns: &[Column], balance: Option<i64>) -> Vec<Value> {
        let mut cells = vec![json!(label)];
        cells.extend(columns.iter().map(|c| match c {
            Column::Debit => money(self.debit),
            Column::Credit => money(self.credit),
            Column::Amount => money(self.amount),
            Column::OpenBalance => money(self.open),
            Column::Qty => json!(self.qty),
            Column::Balance => opt(balance),
            _ => Value::Null,
        }));
        cells
    }
}

fn blank_row(kind: RowKind, depth: u32, label: String, width: usize) -> ReportRow {
    let mut cells = vec![json!(label)];
    cells.resize(width + 1, Value::Null);
    ReportRow { kind, depth, cells, journal_entry_id: None, account_id: None, drill: None }
}

fn detail(settings: &ReportSettings, grouped: bool, groups: &BTreeMap<GroupKey, Group<'_>>, as_of: NaiveDate) -> (Vec<ColumnHead>, Vec<ReportRow>) {
    let cols = &settings.columns;
    let mut heads = vec![ColumnHead { label: String::new(), kind: ColumnKind::Text }];
    heads.extend(cols.iter().map(|c| ColumnHead { label: c.label().into(), kind: c.kind() }));
    let mut rows = Vec::new();
    let mut total = Sums::default();
    let mut last_balance = 0;
    let depth = u32::from(grouped);
    for group in groups.values() {
        if grouped {
            let mut heading = blank_row(RowKind::Section, 0, group.label.clone(), cols.len());
            heading.account_id = group.account_id;
            rows.push(heading);
        }
        let mut balance = group.opening.unwrap_or(0);
        if let Some(opening) = group.opening {
            let mut row = blank_row(RowKind::Detail, depth, "Beginning balance".into(), cols.len());
            if let Some(i) = cols.iter().position(|c| *c == Column::Balance) {
                row.cells[i + 1] = money(opening);
            }
            rows.push(row);
        }
        let mut sums = Sums::default();
        for l in &group.lines {
            balance += if settings.per_transaction { l.entry_total_cents } else { l.amount(settings.sign) };
            sums.add(l, settings);
            let mut cells = vec![Value::Null];
            cells.extend(cols.iter().map(|c| cell(*c, l, settings, balance, as_of)));
            rows.push(ReportRow { kind: RowKind::Detail, depth, cells, journal_entry_id: Some(l.journal_entry_id), account_id: None, drill: None });
        }
        if grouped {
            rows.push(ReportRow { kind: RowKind::Subtotal, depth: 0, cells: sums.cells(format!("Total {}", group.label), cols, Some(balance)), journal_entry_id: None, account_id: None, drill: None });
        }
        total.merge(&sums);
        last_balance = balance;
    }
    // A running balance across groups means nothing, so the grand total shows one only ungrouped.
    let balance = (!grouped).then_some(last_balance);
    rows.push(ReportRow { kind: RowKind::Total, depth: 0, cells: total.cells("TOTAL".into(), cols, balance), journal_entry_id: None, account_id: None, drill: None });
    (heads, rows)
}

fn summary(settings: &ReportSettings, by: GroupBy, groups: &BTreeMap<GroupKey, Group<'_>>, lines: &[Line], end: NaiveDate) -> (Vec<ColumnHead>, Vec<ReportRow>) {
    let amount = |l: &Line| if settings.per_transaction { l.entry_total_cents } else { l.amount(settings.sign) };
    let mut heads = vec![ColumnHead { label: by.label().into(), kind: ColumnKind::Text }];
    // Periods from the start date (or the first line) to the end date.
    let periods: Vec<(i64, String)> = match settings.column_by {
        Some(period) => {
            let mut out: Vec<(i64, String)> = Vec::new();
            let first = settings.start_date.or_else(|| lines.iter().map(|l| l.date).min()).unwrap_or(end);
            let mut day = first;
            while day <= end {
                out.push(period_key(day, period));
                day = period_bounds(day, period).1.succ_opt().expect("in range");
            }
            out
        }
        None => Vec::new(),
    };
    let show_qty = settings.show_qty && settings.column_by.is_none();
    if show_qty {
        heads.push(ColumnHead { label: "Qty".into(), kind: ColumnKind::Quantity });
    }
    heads.extend(periods.iter().map(|(_, label)| ColumnHead { label: label.clone(), kind: ColumnKind::Money }));
    heads.push(ColumnHead { label: if periods.is_empty() { "Amount".into() } else { "Total".into() }, kind: ColumnKind::Money });

    let mut rows = Vec::new();
    let mut totals = vec![0i64; periods.len() + 1];
    let mut total_qty = 0;
    for group in groups.values() {
        let mut amounts = vec![0i64; periods.len() + 1];
        let mut qty = 0;
        for l in &group.lines {
            let a = amount(l);
            if let Some(period) = settings.column_by {
                let rank = period_key(l.date, period).0;
                if let Some(i) = periods.iter().position(|(r, _)| *r == rank) {
                    amounts[i] += a;
                }
            }
            *amounts.last_mut().expect("a total") += a;
            qty += l.qty_e4.unwrap_or(0);
        }
        if !settings.show_zero_rows && amounts.iter().all(|a| *a == 0) && (!show_qty || qty == 0) {
            continue;
        }
        let mut cells = vec![json!(group.label)];
        if show_qty {
            cells.push(json!(qty));
        }
        cells.extend(amounts.iter().map(|a| money(*a)));
        for (t, a) in totals.iter_mut().zip(&amounts) {
            *t += a;
        }
        total_qty += qty;
        rows.push(ReportRow { kind: RowKind::Detail, depth: 0, cells, journal_entry_id: None, account_id: group.account_id, drill: drill_for(by, group, settings.start_date, end) });
    }
    let mut cells = vec![json!("TOTAL")];
    if show_qty {
        cells.push(json!(total_qty));
    }
    cells.extend(totals.iter().map(|a| money(*a)));
    rows.push(ReportRow { kind: RowKind::Total, depth: 0, cells, journal_entry_id: None, account_id: None, drill: None });
    (heads, rows)
}

// ------------------------------------------------------------------------------------------------
// Presets
// ------------------------------------------------------------------------------------------------

/// How a preset's dates are chosen on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DateMode {
    /// From and to, starting at `period`.
    Range,
    /// All dates through an as-of date (balances, aging).
    AsOf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preset {
    pub id: &'static str,
    pub title: &'static str,
    /// The Reports menu group it is listed under.
    pub group: &'static str,
    pub date_mode: DateMode,
    /// The report period it opens with (an id from `ReportParts.jsx`'s `PERIODS`).
    pub period: &'static str,
    pub settings: ReportSettings,
}

fn ledger(columns: &[Column], group_by: Option<GroupBy>, filters: Filters) -> ReportSettings {
    ReportSettings { columns: columns.to_vec(), group_by, filters, ..ReportSettings::default() }
}

fn types(codes: &[&str]) -> Vec<String> {
    codes.iter().map(|c| (*c).to_string()).collect()
}

/// The detail and summary reports built on the engine.
pub fn presets() -> Vec<Preset> {
    use Column::*;
    let classes = |c: &[&str]| Filters { classifications: types(c), ..Filters::default() };
    let account_type = |t: &str| Filters { account_types: types(&[t]), ..Filters::default() };
    let open_docs = |t: &str, docs: &[&str]| Filters { account_types: types(&[t]), transaction_types: types(docs), paid_status: Some(PaidStatus::Open), ..Filters::default() };
    let sales = types(&["INVOICE", "SALES_RECEIPT", "CREDIT_MEMO"]);
    let purchases = types(&["BILL", "VENDOR_CREDIT", "CHECK", "EXPENSE", "CARD_CREDIT"]);
    let items = |types: &[String], items_only: bool, columns: &[Column], mode: Mode| ReportSettings {
        source: Source::Items,
        mode,
        columns: columns.to_vec(),
        group_by: Some(GroupBy::Item),
        filters: Filters { transaction_types: types.to_vec(), items_only, ..Filters::default() },
        show_qty: true,
        ..ReportSettings::default()
    };
    let summary = |mut s: ReportSettings| {
        s.mode = Mode::Summary;
        s.columns.clear();
        s
    };
    let preset = |id, title, group, date_mode, period, settings| Preset { id, title, group, date_mode, period, settings };
    let range = DateMode::Range;
    let as_of = DateMode::AsOf;
    let gl_columns = [Type, Date, Num, Name, Memo, Split, Debit, Credit, Balance];
    let customer_balance = ledger(&[Type, Date, Num, Memo, Account, Amount, Balance], Some(GroupBy::Name), account_type("ACCOUNTS_RECEIVABLE"));
    let vendor_balance = ledger(&[Type, Date, Num, Memo, Account, Amount, Balance], Some(GroupBy::Name), account_type("ACCOUNTS_PAYABLE"));
    let income_by_customer = ReportSettings {
        sign: Sign::Credit,
        ..ledger(&[Type, Date, Num, Memo, Account, Amount, Balance], Some(GroupBy::Name), classes(&["INCOME", "EXPENSE"]))
    };
    // Debits minus credits: the bank line is negative and the lines it paid for positive.
    let by_transaction = |codes: &[&str]| ReportSettings {
        sign: Sign::Debit,
        ..ledger(&[Type, Date, Num, Name, Memo, Account, Class, Amount], Some(GroupBy::Transaction), Filters { transaction_types: types(codes), ..Filters::default() })
    };
    let purchases_by_vendor = ReportSettings { group_by: Some(GroupBy::Name), ..items(&purchases, true, &[Type, Date, Num, Memo, Item, Qty, Rate, Amount, Balance], Mode::Detail) };
    vec![
        preset("pl-detail", "Profit & Loss Detail", "Business overview", range, "fiscalYearToDate", ledger(&[Type, Date, Num, Name, Memo, Class, Split, Amount, Balance], Some(GroupBy::Account), classes(&["INCOME", "EXPENSE"]))),
        preset(
            "bs-detail",
            "Balance Sheet Detail",
            "Business overview",
            range,
            "thisMonth",
            ReportSettings { opening_balances: true, ..ledger(&gl_columns, Some(GroupBy::Account), classes(&["ASSET", "LIABILITY", "EQUITY"])) },
        ),
        preset("ar-aging-detail", "A/R Aging Detail", "Who owes you", as_of, "today", ledger(&[Type, Date, Num, Name, DueDate, Aging, OpenBalance], Some(GroupBy::Aging), open_docs("ACCOUNTS_RECEIVABLE", &["INVOICE", "CREDIT_MEMO"]))),
        preset("open-invoices", "Open Invoices", "Who owes you", as_of, "today", ledger(&[Type, Date, Num, Memo, DueDate, Aging, OpenBalance], Some(GroupBy::Name), open_docs("ACCOUNTS_RECEIVABLE", &["INVOICE", "CREDIT_MEMO"]))),
        preset("customer-balance-summary", "Customer Balance Summary", "Who owes you", as_of, "today", summary(customer_balance.clone())),
        preset("customer-balance-detail", "Customer Balance Detail", "Who owes you", as_of, "today", customer_balance),
        preset("sales-customer-detail", "Sales by Customer Detail", "Sales and customers", range, "fiscalYearToDate", ledger(&[Type, Date, Num, Memo, Item, Qty, Rate, Account, Amount, Balance], Some(GroupBy::Name), classes(&["INCOME"]))),
        preset("sales-product-detail", "Sales by Product/Service Detail", "Sales and customers", range, "fiscalYearToDate", items(&sales, false, &[Type, Date, Num, Name, Memo, Qty, Rate, Amount, Balance], Mode::Detail)),
        preset("income-customer-summary", "Income by Customer Summary", "Sales and customers", range, "fiscalYearToDate", summary(income_by_customer.clone())),
        preset("income-customer-detail", "Income by Customer Detail", "Sales and customers", range, "fiscalYearToDate", income_by_customer),
        preset("ap-aging-detail", "A/P Aging Detail", "What you owe", as_of, "today", ledger(&[Type, Date, Num, Name, DueDate, Aging, OpenBalance], Some(GroupBy::Aging), open_docs("ACCOUNTS_PAYABLE", &["BILL", "VENDOR_CREDIT"]))),
        preset("unpaid-bills", "Unpaid Bills", "What you owe", as_of, "today", ledger(&[Type, Date, Num, Memo, DueDate, Aging, OpenBalance], Some(GroupBy::Name), open_docs("ACCOUNTS_PAYABLE", &["BILL", "VENDOR_CREDIT"]))),
        preset("vendor-balance-summary", "Vendor Balance Summary", "What you owe", as_of, "today", summary(vendor_balance.clone())),
        preset("vendor-balance-detail", "Vendor Balance Detail", "What you owe", as_of, "today", vendor_balance),
        preset("expenses-vendor-detail", "Expenses by Vendor Detail", "Expenses and vendors", range, "fiscalYearToDate", ledger(&[Type, Date, Num, Memo, Account, Class, Amount, Balance], Some(GroupBy::Name), classes(&["EXPENSE"]))),
        preset("purchases-vendor-summary", "Purchases by Vendor Summary", "Expenses and vendors", range, "fiscalYearToDate", summary(purchases_by_vendor.clone())),
        preset("purchases-vendor-detail", "Purchases by Vendor Detail", "Expenses and vendors", range, "fiscalYearToDate", purchases_by_vendor),
        preset("purchases-item-summary", "Purchases by Product/Service Summary", "Expenses and vendors", range, "fiscalYearToDate", summary(items(&purchases, true, &[], Mode::Summary))),
        preset("purchases-item-detail", "Purchases by Product/Service Detail", "Expenses and vendors", range, "fiscalYearToDate", items(&purchases, true, &[Type, Date, Num, Name, Memo, Qty, Rate, Amount, Balance], Mode::Detail)),
        preset(
            "1099-detail",
            "1099 Detail",
            "Expenses and vendors",
            range,
            "lastYear",
            ReportSettings {
                sign: Sign::Credit,
                ..ledger(
                    &[Type, Date, Num, Memo, Account, Amount, Balance],
                    Some(GroupBy::Name),
                    Filters { account_types: types(&["BANK"]), transaction_types: types(&["CHECK", "EXPENSE", "BILL_PAYMENT"]), vendors_1099: true, exclude_voided: true, ..Filters::default() },
                )
            },
        ),
        preset("check-detail", "Check Detail", "Banking", range, "thisMonth", by_transaction(&["CHECK", "BILL_PAYMENT"])),
        preset("deposit-detail", "Deposit Detail", "Banking", range, "thisMonth", by_transaction(&["DEPOSIT"])),
        preset(
            "transaction-list-date",
            "Transaction List by Date",
            "For my accountant",
            range,
            "thisMonth",
            ReportSettings { per_transaction: true, ..ledger(&[Type, Date, Num, Name, Memo, Account, Split, Amount], None, Filters::default()) },
        ),
        preset("journal", "Journal", "For my accountant", range, "thisMonth", ledger(&[Type, Date, Num, Name, Memo, Account, Debit, Credit], Some(GroupBy::Transaction), Filters::default())),
    ]
}

// ------------------------------------------------------------------------------------------------
// Saved (memorized) reports
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedReport {
    pub id: i64,
    pub name: String,
    /// The preset it was customized from, if any.
    pub preset: Option<String>,
    /// The screen's report settings as saved: [`ReportSettings`] plus the screen's own fields
    /// (such as the report period).
    pub settings: Value,
    pub updated_at: String,
}

type SavedRow = (i64, String, Option<String>, String, String);

fn saved(row: SavedRow) -> Result<SavedReport, String> {
    let (id, name, preset, settings, updated_at) = row;
    let settings = serde_json::from_str(&settings).map_err(|e| format!("RPT_SAVED_CORRUPT: saved report {id}: {e}"))?;
    Ok(SavedReport { id, name, preset, settings, updated_at })
}

pub async fn list_saved(pool: &SqlitePool) -> Result<Vec<SavedReport>, String> {
    let rows: Vec<SavedRow> = sqlx::query_as("SELECT id, name, preset, settings, updated_at FROM saved_reports ORDER BY name COLLATE NOCASE, id")
        .fetch_all(pool)
        .await
        .map_err(db_err)?;
    rows.into_iter().map(saved).collect()
}

/// Saves `settings` under `name`, replacing a saved report of that name when `replace` is set.
pub async fn save(pool: &SqlitePool, name: &str, preset: Option<&str>, settings: &Value, replace: bool) -> Result<SavedReport, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err("RPT_SAVE_NAME: give the report a name of up to 100 characters".into());
    }
    serde_json::from_value::<ReportSettings>(settings.clone()).map_err(|e| format!("RPT_SAVE_SETTINGS: {e}"))?;
    let text = serde_json::to_string(settings).map_err(|e| e.to_string())?;
    let existing: Option<i64> = sqlx::query_scalar("SELECT id FROM saved_reports WHERE name = ?1").bind(name).fetch_optional(pool).await.map_err(db_err)?;
    let id = match existing {
        Some(_) if !replace => return Err(format!("RPT_SAVE_EXISTS: a saved report named '{name}' already exists")),
        Some(id) => {
            sqlx::query("UPDATE saved_reports SET preset = ?1, settings = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?3")
                .bind(preset)
                .bind(&text)
                .bind(id)
                .execute(pool)
                .await
                .map_err(db_err)?;
            id
        }
        None => sqlx::query_scalar("INSERT INTO saved_reports (name, preset, settings) VALUES (?1, ?2, ?3) RETURNING id")
            .bind(name)
            .bind(preset)
            .bind(&text)
            .fetch_one(pool)
            .await
            .map_err(db_err)?,
    };
    saved(sqlx::query_as("SELECT id, name, preset, settings, updated_at FROM saved_reports WHERE id = ?1").bind(id).fetch_one(pool).await.map_err(db_err)?)
}

pub async fn delete_saved(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let done = sqlx::query("DELETE FROM saved_reports WHERE id = ?1").bind(id).execute(pool).await.map_err(db_err)?;
    if done.rows_affected() == 0 {
        return Err(format!("RPT_SAVED_NOT_FOUND: saved report {id} does not exist"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
