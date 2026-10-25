//! Documents read from a desktop company file, for the full import: invoices, credit memos, sales
//! receipts, received payments and the deposits that swept them; bills (and vendor credits), bill
//! payments, checks and card charges; transfers; and the non-posting estimates and purchase
//! orders; with the links between them.
//!
//! The file keeps payment applications in `abmc_transaction_link`, the same model as the
//! QuickBooks SDK's `AppliedToTxn`: rows come in sibling pairs, and the row whose
//! `is_t1_source_bool` is set names the paying transaction (`transaction_id`), the transaction it
//! applies to (`transaction_2_id`) and the amount. Link types seen on real files:
//!
//! | `link_type` | paying side → applied to |
//! |---|---|
//! | 4 | received payment → invoice |
//! | 3 | credit memo → invoice |
//! | 9 | received payment → deposit (Undeposited Funds swept to the bank) |
//! | 6 | bill payment or check → bill |
//! | 12 | lines within one invoice (internal; ignored) |
//!
//! Signs follow the ledger lines: negative is a debit, positive a credit.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::qbw_company::{rows_of, text};
use crate::qbw_pages::LogicalPage;
use crate::qbw_records::{Catalog, Row};

/// One line of an invoice or credit memo as it appears on the form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentLine {
    pub item_id: Option<i64>,
    pub description: Option<String>,
    /// Quantity scaled by 10,000, as LocalBooks stores it.
    pub quantity_e4: i64,
    pub rate_cents: i64,
    /// The ledger amount of the line (negative = debit).
    pub cents: i64,
    pub account_id: i64,
}

/// An amount applied to another transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub transaction_id: i64,
    pub cents: i64,
}

/// A sales form's rep and shipping details as the file gives them, by name (the full import finds
/// or creates the rep and shipping method).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SourceSales {
    /// Initials, or "initials:name".
    pub rep: Option<String>,
    pub ship_date: Option<chrono::NaiveDate>,
    pub ship_via: Option<String>,
}

impl SourceSales {
    pub fn is_empty(&self) -> bool {
        self.rep.is_none() && self.ship_date.is_none() && self.ship_via.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SourceDocument {
    #[serde(rename_all = "camelCase")]
    Invoice {
        customer_id: i64,
        number: Option<String>,
        due_date: Option<chrono::NaiveDate>,
        lines: Vec<DocumentLine>,
        #[serde(default, skip_serializing_if = "SourceSales::is_empty")]
        sales: SourceSales,
    },
    #[serde(rename_all = "camelCase")]
    CreditMemo { customer_id: i64, number: Option<String>, lines: Vec<DocumentLine>, applied_to: Vec<Applied> },
    /// Lines include the debit to the account the money went into.
    #[serde(rename_all = "camelCase")]
    SalesReceipt {
        customer_id: Option<i64>,
        number: Option<String>,
        method: String,
        lines: Vec<DocumentLine>,
        #[serde(default, skip_serializing_if = "SourceSales::is_empty")]
        sales: SourceSales,
    },
    #[serde(rename_all = "camelCase")]
    Payment { customer_id: i64, reference: Option<String>, method: String, applied_to: Vec<Applied> },
    /// The payments a deposit swept out of Undeposited Funds (its other lines are in the ledger lines).
    #[serde(rename_all = "camelCase")]
    Deposit { payments: Vec<Applied> },
    /// A bill, or a vendor credit when it debits payables (the file keeps both in one table).
    #[serde(rename_all = "camelCase")]
    Bill { vendor_id: i64, number: Option<String>, due_date: Option<chrono::NaiveDate>, lines: Vec<DocumentLine> },
    #[serde(rename_all = "camelCase")]
    BillPayment { vendor_id: Option<i64>, reference: Option<String>, applied_to: Vec<Applied> },
    /// A check or card charge, with the bills it paid when it paid any.
    #[serde(rename_all = "camelCase")]
    Expense { vendor_id: Option<i64>, number: Option<String>, applied_to: Vec<Applied> },
    /// Money moved between two balance-sheet accounts (its lines are the ledger lines).
    Transfer,
    /// Estimates and purchase orders post nothing, so they carry their own date and memo; their
    /// line amounts are positive.
    #[serde(rename_all = "camelCase")]
    Estimate {
        customer_id: i64,
        date: chrono::NaiveDate,
        number: Option<String>,
        expires: Option<chrono::NaiveDate>,
        memo: Option<String>,
        lines: Vec<DocumentLine>,
        #[serde(default, skip_serializing_if = "SourceSales::is_empty")]
        sales: SourceSales,
    },
    #[serde(rename_all = "camelCase")]
    PurchaseOrder {
        vendor_id: i64,
        date: chrono::NaiveDate,
        number: Option<String>,
        expected: Option<chrono::NaiveDate>,
        memo: Option<String>,
        received: bool,
        lines: Vec<DocumentLine>,
    },
}

/// LocalBooks' payment method for the name the file uses.
pub fn payment_method(name: Option<&str>) -> &'static str {
    let n = name.unwrap_or_default().to_ascii_lowercase();
    if n.contains("cash") {
        "CASH"
    } else if n.contains("check") {
        "CHECK"
    } else if n.contains("wire") {
        "WIRE"
    } else if n.contains("bank") || n.contains("direct deposit") || n.contains("ach") {
        "ACH"
    } else if ["visa", "master", "discover", "american express", "amex", "credit card", "square"].iter().any(|k| n.contains(k)) {
        "CREDIT_CARD"
    } else {
        "OTHER"
    }
}

/// Quantity and rate for a form line, consistent with LocalBooks' rule that the amount is
/// `round(quantity × rate)`; when the file's own pair does not round to the amount (or is
/// missing), the line becomes quantity 1 at the line amount.
fn quantity_and_rate(row: &Row, amount_cents: i64) -> (i64, i64) {
    let amount = amount_cents.abs();
    let quantity = row.get("quantity_qnty").as_f64().map(f64::abs).filter(|q| *q > 0.0);
    let rate = row.get("unit_price_amt").as_cents().map(i64::abs).filter(|r| *r > 0);
    if let (Some(q), Some(r)) = (quantity, rate) {
        let q_e4 = (q * 10_000.0).round() as i64;
        if q_e4 > 0 && (q_e4 as i128 * r as i128 + 5_000) / 10_000 == amount as i128 {
            return (q_e4, r);
        }
    }
    (10_000, amount)
}

/// Form lines of a document: the lines with an amount that do not post to receivables or
/// payables, in their order on the form.
fn form_lines(rows: &[&Row], arap: &dyn Fn(i64) -> bool) -> Vec<DocumentLine> {
    let mut sorted: Vec<&&Row> = rows.iter().collect();
    sorted.sort_by_key(|r| (r.get("txn_line_order").as_int().unwrap_or(i64::MAX), r.get("target_id").as_int().unwrap_or(0)));
    sorted
        .into_iter()
        .filter_map(|r| {
            let account_id = r.get("account_id").as_int()?;
            let cents = r.get("amount_amt").as_cents().filter(|c| *c != 0)?;
            if arap(account_id) {
                return None;
            }
            let (quantity_e4, rate_cents) = quantity_and_rate(r, cents);
            Some(DocumentLine { item_id: r.get("item_id").as_int(), description: text(r, "memo"), quantity_e4, rate_cents, cents, account_id })
        })
        .collect()
}

/// Reads the documents, keyed by transaction id. `arap` says whether a source account is a
/// receivables or payables account.
pub fn read_documents(pages: &[LogicalPage], catalog: &Catalog, arap: &dyn Fn(i64) -> bool) -> HashMap<i64, SourceDocument> {
    let lines_of = |table: &str| -> BTreeMap<i64, Vec<Row>> {
        // Estimates and purchase orders are non-posting throughout; elsewhere a non-posting row
        // is a memo or subtotal line.
        let non_posting = table.contains("estimate") || table.contains("purchase_order");
        let mut by: BTreeMap<i64, Vec<Row>> = BTreeMap::new();
        for r in rows_of(pages, catalog, table) {
            if !non_posting && r.get("is_no_post_bool").as_int() == Some(1) {
                continue;
            }
            if let Some(t) = r.get("transaction_id").as_int() {
                by.entry(t).or_default().push(r);
            }
        }
        by
    };
    let headers_of = |table: &str| -> HashMap<i64, Row> {
        rows_of(pages, catalog, table).into_iter().filter_map(|r| Some((r.get("transaction_id").as_int()?, r))).collect()
    };
    let methods: HashMap<i64, String> =
        rows_of(pages, catalog, "abmc_payment_method").iter().filter_map(|r| Some((r.get("payment_method_id").as_int()?, text(r, "name")?))).collect();

    // Applications, from the paying side of each link pair.
    let mut applied: HashMap<(i64, i64), Vec<Applied>> = HashMap::new();
    for r in rows_of(pages, catalog, "abmc_transaction_link") {
        if r.get("is_t1_source_bool").as_int() != Some(1) {
            continue;
        }
        let (Some(kind), Some(from), Some(to), Some(cents)) =
            (r.get("link_type").as_int(), r.get("transaction_id").as_int(), r.get("transaction_2_id").as_int(), r.get("amount_amt").as_cents())
        else {
            continue;
        };
        if from != to && cents > 0 {
            applied.entry((kind, from)).or_default().push(Applied { transaction_id: to, cents });
        }
    }

    // A sales form's rep (`sales_rep_id`), shipping method (`ship_via_id`; estimates have none) and ship date.
    let (reps, ships) = crate::qbw_company::read_sales_lists(pages, catalog);
    let sales_of = |h: &Row| SourceSales {
        rep: h.get("sales_rep_id").as_int().and_then(|r| reps.get(&r)).map(|r| r.0.clone()),
        ship_date: h.get("ship_date").as_date(),
        ship_via: h.get("ship_via_id").as_int().and_then(|s| ships.get(&s)).map(|s| s.0.clone()),
    };
    let mut out = HashMap::new();
    let invoice_headers = headers_of("abmc_invoice_header");
    for (tid, rows) in lines_of("abmc_invoice_lineitem") {
        let Some(h) = invoice_headers.get(&tid) else { continue };
        let Some(customer_id) = h.get("customer_id").as_int() else { continue };
        let refs: Vec<&Row> = rows.iter().collect();
        out.insert(
            tid,
            SourceDocument::Invoice { customer_id, number: text(h, "doc_num"), due_date: h.get("due_date").as_date(), lines: form_lines(&refs, arap), sales: sales_of(h) },
        );
    }
    let memo_headers = headers_of("abmc_credit_memo_header");
    for (tid, rows) in lines_of("abmc_credit_memo_lineitem") {
        let Some(h) = memo_headers.get(&tid) else { continue };
        let Some(customer_id) = h.get("customer_id").as_int() else { continue };
        let refs: Vec<&Row> = rows.iter().collect();
        out.insert(
            tid,
            SourceDocument::CreditMemo {
                customer_id,
                number: text(h, "doc_num"),
                lines: form_lines(&refs, arap),
                applied_to: applied.remove(&(3, tid)).unwrap_or_default(),
            },
        );
    }
    let method_of = |h: &Row| payment_method(h.get("payment_method_id").as_int().and_then(|m| methods.get(&m)).map(String::as_str)).to_string();
    let receipt_headers = headers_of("abmc_sales_receipt_header");
    for (tid, rows) in lines_of("abmc_sales_receipt_lineitem") {
        let Some(h) = receipt_headers.get(&tid) else { continue };
        let refs: Vec<&Row> = rows.iter().collect();
        out.insert(
            tid,
            SourceDocument::SalesReceipt {
                customer_id: h.get("customer_id").as_int(),
                number: text(h, "doc_num"),
                method: method_of(h),
                lines: form_lines(&refs, arap),
                sales: sales_of(h),
            },
        );
    }
    for (tid, h) in headers_of("abmc_received_payment_header") {
        let Some(customer_id) = h.get("customer_id").as_int() else { continue };
        out.insert(
            tid,
            SourceDocument::Payment { customer_id, reference: text(&h, "doc_num"), method: method_of(&h), applied_to: applied.remove(&(4, tid)).unwrap_or_default() },
        );
    }
    let bill_headers = headers_of("abmc_bill_header");
    for (tid, rows) in lines_of("abmc_bill_lineitem") {
        let Some(h) = bill_headers.get(&tid) else { continue };
        let Some(vendor_id) = h.get("vendor_id").as_int() else { continue };
        let refs: Vec<&Row> = rows.iter().collect();
        out.insert(tid, SourceDocument::Bill { vendor_id, number: text(h, "doc_num"), due_date: h.get("due_date").as_date(), lines: form_lines(&refs, arap) });
    }
    for table in ["abmc_bill_check_header", "abmc_bill_ccard_header"] {
        for (tid, h) in headers_of(table) {
            let applied_to = applied.remove(&(6, tid)).unwrap_or_default();
            out.insert(tid, SourceDocument::BillPayment { vendor_id: h.get("vendor_id").as_int(), reference: text(&h, "doc_num"), applied_to });
        }
    }
    for table in ["abmc_check_header", "abmc_credit_cardcharge_header"] {
        for (tid, h) in headers_of(table) {
            let applied_to = applied.remove(&(6, tid)).unwrap_or_default();
            out.insert(tid, SourceDocument::Expense { vendor_id: h.get("vendor_id").as_int(), number: text(&h, "doc_num"), applied_to });
        }
    }
    for (tid, _) in headers_of("abmc_transfer_header") {
        out.insert(tid, SourceDocument::Transfer);
    }
    // Lines of non-posting documents, as positive amounts.
    let positive = |rows: &[Row]| -> Vec<DocumentLine> {
        let refs: Vec<&Row> = rows.iter().collect();
        form_lines(&refs, &|_| false).into_iter().map(|l| DocumentLine { cents: l.cents.abs(), ..l }).collect()
    };
    let estimate_headers = headers_of("abmc_estimate_header");
    for (tid, rows) in lines_of("abmc_estimate_lineitem") {
        let Some(h) = estimate_headers.get(&tid) else { continue };
        let (Some(customer_id), Some(date)) = (h.get("customer_id").as_int(), h.get("transaction_date").as_date()) else { continue };
        out.insert(
            tid,
            SourceDocument::Estimate { customer_id, date, number: text(h, "doc_num"), expires: h.get("due_date").as_date(), memo: text(h, "memo"), lines: positive(&rows), sales: sales_of(h) },
        );
    }
    let order_headers = headers_of("abmc_purchase_order_header");
    for (tid, rows) in lines_of("abmc_purchase_order_lineitem") {
        let Some(h) = order_headers.get(&tid) else { continue };
        let (Some(vendor_id), Some(date)) = (h.get("vendor_id").as_int(), h.get("transaction_date").as_date()) else { continue };
        out.insert(
            tid,
            SourceDocument::PurchaseOrder {
                vendor_id,
                date,
                number: text(h, "doc_num"),
                expected: h.get("expected_date").as_date(),
                memo: text(h, "memo"),
                received: h.get("is_received_bool").as_int() == Some(1),
                lines: positive(&rows),
            },
        );
    }
    // Deposits: group the payment → deposit links by deposit.
    let mut deposits: BTreeMap<i64, Vec<Applied>> = BTreeMap::new();
    for ((kind, payment), links) in &applied {
        if *kind == 9 {
            for l in links {
                deposits.entry(l.transaction_id).or_default().push(Applied { transaction_id: *payment, cents: l.cents });
            }
        }
    }
    for (tid, mut payments) in deposits {
        payments.sort_by_key(|p| p.transaction_id);
        out.insert(tid, SourceDocument::Deposit { payments });
    }
    out
}

#[cfg(test)]
mod tests;
