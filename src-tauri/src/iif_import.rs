//! Reads QuickBooks Desktop's Intuit Interchange Format (`.iif`) into a [`Ledger`], so an IIF file
//! goes through the same staging, account mapping and journal or full import as a company file.
//!
//! An IIF file is tab-separated text, normally Windows-1252. A line starting with `!` declares the
//! columns for one keyword (`!ACCNT`, `!CUST`, `!TRNS`, ...); every later line starting with that
//! keyword is read with those columns, so a file can redeclare them part-way through. Lists come as
//! one row per entry (`ACCNT`, `CUST`, `VEND`, `INVITEM`). A transaction is a `TRNS` row, its `SPL`
//! distribution rows and `ENDTRNS`; its amounts are signed (debits positive) and sum to zero. Dates
//! are month-first (`7/16/98`, `7/16/1998`). This follows Intuit's IIF Import Kit (the header
//! reference and sample files); see `docs/guide/desktop-migration.md`.
//!
//! IIF says which invoice a payment paid, or which bill a bill payment paid, nowhere: QuickBooks
//! applies them itself when it imports. So does this reader, the way QuickBooks' "automatically
//! apply payments" does: each payment goes to its customer's oldest open invoices, each bill
//! payment to its vendor's oldest open bills, and a deposit is matched to undeposited payments of
//! the same amount. Whatever does not apply posts unapplied, as the full import does
//! for any payment.

use std::collections::{HashMap, VecDeque};

use chrono::NaiveDate;

use crate::bank::{parse_amount_cents, parse_date};
use crate::qbw_company::{Ledger, LedgerQuality, LineOutcome, SourceItem, SourceLine, SourceTransaction, UnreadableLine};
use crate::qbw_documents::{payment_method, Applied, DocumentLine, SourceDocument};
use crate::text_import::{decode, quantity_e4, terms_days, Accounts, Names};

/// LocalBooks account type for an IIF `ACCNTTYPE`; `None` for non-posting accounts.
fn account_type(code: &str) -> Option<&'static str> {
    Some(match code.trim().to_ascii_uppercase().as_str() {
        "BANK" => "BANK",
        "AR" => "ACCOUNTS_RECEIVABLE",
        "OCASSET" => "OTHER_CURRENT_ASSET",
        "FIXASSET" => "FIXED_ASSET",
        "OASSET" => "OTHER_ASSET",
        "AP" => "ACCOUNTS_PAYABLE",
        "CCARD" => "CREDIT_CARD",
        "OCLIAB" => "OTHER_CURRENT_LIABILITY",
        "LTLIAB" => "LONG_TERM_LIABILITY",
        "EQUITY" => "EQUITY",
        "INC" => "INCOME",
        "COGS" => "COST_OF_GOODS_SOLD",
        "EXP" => "EXPENSE",
        "EXINC" => "OTHER_INCOME",
        "EXEXP" => "OTHER_EXPENSE",
        _ => return None,
    })
}

/// The readable kind for a `TRNSTYPE` (the same names the company-file reader uses).
fn kind(trnstype: &str) -> &'static str {
    match trnstype {
        "INVOICE" => "Invoice",
        "CREDIT MEMO" => "Credit memo",
        "CASH SALE" => "Sales receipt",
        "PAYMENT" => "Payment",
        "DEPOSIT" => "Deposit",
        "TRANSFER" => "Transfer",
        "CHECK" => "Check",
        "CREDIT CARD" => "Credit card charge",
        "CCARD REFUND" => "Credit card credit",
        "BILL" => "Bill",
        "BILL REFUND" => "Vendor credit",
        "BILLPMT" | "BILL PMT" => "Bill payment",
        "BILLCCARD" => "Bill payment (card)",
        "ITEMRECEIPT" | "ITEM RECEIPT" => "Item receipt",
        "CASH REFUND" => "Refund",
        "GENERAL JOURNAL" => "Journal entry",
        "BEGINBALCHECK" => "Opening balance",
        "STATEMENT CHARGE" | "STMT CHG" => "Statement charge",
        "ESTIMATES" | "ESTIMATE" => "Estimate",
        "PURCHORD" => "Purchase order",
        "INVENTORY ADJUST" => "Inventory adjustment",
        "PAYCHECK" | "LIABILITY CHECK" => "Payroll",
        _ => "Other",
    }
}

/// A type for an account a transaction uses without the file declaring it: from its name when that
/// says, otherwise from where it appears. QuickBooks itself would make it a bank account.
fn infer_type(name: &str, trnstype: &str, is_trns: bool) -> &'static str {
    let n = name.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| n.contains(w));
    if has(&["receivable"]) {
        return "ACCOUNTS_RECEIVABLE";
    }
    if has(&["payable"]) && !has(&["tax"]) {
        return "ACCOUNTS_PAYABLE";
    }
    if has(&["undeposited", "inventory asset"]) {
        return "OTHER_CURRENT_ASSET";
    }
    if has(&["tax payable", "sales tax"]) {
        return "OTHER_CURRENT_LIABILITY";
    }
    if has(&["equity", "opening bal", "retained earnings", "owner"]) {
        return "EQUITY";
    }
    if has(&["cost of goods"]) {
        return "COST_OF_GOODS_SOLD";
    }
    match (trnstype, is_trns) {
        ("INVOICE" | "CREDIT MEMO" | "STATEMENT CHARGE" | "STMT CHG", true) => "ACCOUNTS_RECEIVABLE",
        ("INVOICE" | "CREDIT MEMO" | "CASH SALE" | "DEPOSIT" | "STATEMENT CHARGE" | "STMT CHG", false) => "INCOME",
        ("PAYMENT", false) => "ACCOUNTS_RECEIVABLE",
        ("BILL" | "BILL REFUND" | "ITEMRECEIPT" | "ITEM RECEIPT", true) => "ACCOUNTS_PAYABLE",
        ("BILLPMT" | "BILL PMT" | "BILLCCARD", false) => "ACCOUNTS_PAYABLE",
        ("BILLCCARD", true) => "CREDIT_CARD",
        ("CREDIT CARD" | "CCARD REFUND", true) => "CREDIT_CARD",
        ("BILL" | "BILL REFUND" | "ITEMRECEIPT" | "ITEM RECEIPT" | "CHECK" | "CREDIT CARD" | "CCARD REFUND", false) => "EXPENSE",
        _ if has(&["card", "visa", "mastercard", "amex"]) => "CREDIT_CARD",
        _ if has(&["income", "sales", "revenue"]) => "INCOME",
        _ if has(&["expense"]) => "EXPENSE",
        _ => "BANK",
    }
}

/// One line of a file, split on tabs, with surrounding quotes removed (Excel quotes a field that
/// contains a comma, and doubles the quotes inside it).
fn fields(line: &str) -> Vec<String> {
    line.split('\t')
        .map(|f| {
            let f = f.trim();
            match f.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                Some(inner) => inner.replace("\"\"", "\""),
                None => f.to_string(),
            }
        })
        .collect()
}

/// A data row read against its keyword's declared columns.
struct Row<'a> {
    columns: &'a [String],
    values: Vec<String>,
}

impl Row<'_> {
    fn get(&self, column: &str) -> Option<&str> {
        let i = self.columns.iter().position(|c| c.eq_ignore_ascii_case(column))?;
        self.values.get(i).map(|v| v.trim()).filter(|v| !v.is_empty())
    }

    fn text(&self, column: &str) -> Option<String> {
        self.get(column).map(str::to_string)
    }

    fn yes(&self, column: &str) -> bool {
        self.get(column).is_some_and(|v| v.eq_ignore_ascii_case("Y"))
    }
}

/// A `TRNS` or `SPL` row as read.
struct RawLine {
    trnstype: String,
    date: Option<String>,
    account: Option<String>,
    name: Option<String>,
    amount: Option<String>,
    doc_num: Option<String>,
    memo: Option<String>,
    quantity: Option<String>,
    price: Option<String>,
    item: Option<String>,
    due_date: Option<String>,
    pay_method: Option<String>,
}

impl RawLine {
    fn from(row: &Row) -> Self {
        RawLine {
            trnstype: row.get("TRNSTYPE").unwrap_or_default().to_ascii_uppercase(),
            date: row.text("DATE"),
            account: row.text("ACCNT"),
            name: row.text("NAME"),
            amount: row.text("AMOUNT"),
            doc_num: row.text("DOCNUM"),
            memo: row.text("MEMO"),
            quantity: row.text("QNTY"),
            price: row.text("PRICE"),
            item: row.text("INVITEM"),
            due_date: row.text("DUEDATE"),
            pay_method: row.text("PAYMETH"),
        }
    }
}

/// A transaction block as read, before its accounts and names are resolved.
struct Block {
    trns: RawLine,
    splits: Vec<RawLine>,
    file_line: usize,
}

fn item_kind(code: &str) -> Option<&'static str> {
    match code.trim().to_ascii_uppercase().as_str() {
        "SERV" => Some("Service"),
        "INVENTORY" | "INVENTORYASSEMBLY" | "ASSEMBLY" => Some("Inventory part"),
        "PART" => Some("Non-inventory part"),
        "OTHC" => Some("Other charge"),
        // Discounts, subtotals, groups, payments and sales tax items are not things LocalBooks sells.
        _ => None,
    }
}

/// Reads an IIF file. Lists the file declares are kept even when no transaction uses them.
pub fn read(bytes: &[u8]) -> Result<Ledger, String> {
    let text = decode(bytes);
    let mut headers: HashMap<String, Vec<String>> = HashMap::new();
    let mut accounts = Accounts::default();
    let mut nonposting: Vec<String> = Vec::new();
    let mut names = Names::default();
    let mut items: Vec<SourceItem> = Vec::new();
    let mut blocks: Vec<Block> = Vec::new();
    let mut open: Option<Block> = None;
    let mut problems: Vec<String> = Vec::new();

    for (n, line) in text.lines().enumerate() {
        let line_no = n + 1;
        let values = fields(line);
        let Some(keyword) = values.first().map(|k| k.to_ascii_uppercase()) else { continue };
        if keyword.is_empty() {
            continue;
        }
        if let Some(declared) = keyword.strip_prefix('!') {
            headers.insert(declared.to_string(), values);
            continue;
        }
        if keyword == "ENDTRNS" {
            match open.take() {
                Some(block) => blocks.push(block),
                None => problems.push(format!("line {line_no}: ENDTRNS without a TRNS")),
            }
            continue;
        }
        let Some(columns) = headers.get(&keyword) else {
            if matches!(keyword.as_str(), "TRNS" | "SPL" | "ACCNT" | "CUST" | "VEND" | "INVITEM") {
                problems.push(format!("line {line_no}: {keyword} row before its !{keyword} header"));
            }
            continue;
        };
        let row = Row { columns, values };
        match keyword.as_str() {
            "ACCNT" => {
                let Some(name) = row.get("NAME") else { continue };
                let code = row.get("ACCNTTYPE").unwrap_or_default();
                match account_type(code) {
                    Some(t) => {
                        let t = match row.get("EXTRA").map(str::to_ascii_uppercase).as_deref() {
                            Some("UNDEPOSIT") => "OTHER_CURRENT_ASSET",
                            _ => t,
                        };
                        accounts.declare(name, t, !row.yes("HIDDEN"));
                    }
                    None => nonposting.push(name.to_lowercase()),
                }
            }
            "CUST" => {
                let Some(name) = row.get("NAME") else { continue };
                let id = names.ensure_customer(name);
                let address: Vec<&str> = (1..=5).filter_map(|i| row.get(&format!("BADDR{i}"))).collect();
                let person = [row.get("FIRSTNAME"), row.get("LASTNAME")].into_iter().flatten().collect::<Vec<_>>().join(" ");
                let p = names.customers.iter_mut().find(|p| p.id == id).expect("just ensured");
                p.company_name = row.text("COMPANYNAME").or_else(|| (!person.is_empty()).then_some(person));
                p.email = row.text("EMAIL");
                p.phone = row.text("PHONE1");
                p.address = (!address.is_empty()).then(|| address.join("\n"));
                p.terms_days = row.get("TERMS").and_then(terms_days);
                p.active = !row.yes("HIDDEN");
                p.notes = row.text("NOTEPAD").or_else(|| row.text("NOTE"));
            }
            "VEND" => {
                let Some(name) = row.get("NAME") else { continue };
                let id = names.ensure_vendor(name);
                let address: Vec<&str> = (1..=5).filter_map(|i| row.get(&format!("ADDR{i}"))).collect();
                let person = [row.get("FIRSTNAME"), row.get("LASTNAME")].into_iter().flatten().collect::<Vec<_>>().join(" ");
                let p = names.vendors.iter_mut().find(|p| p.id == id).expect("just ensured");
                p.company_name = row.text("COMPANYNAME").or_else(|| (!person.is_empty()).then_some(person));
                p.email = row.text("EMAIL");
                p.phone = row.text("PHONE1");
                p.address = (!address.is_empty()).then(|| address.join("\n"));
                p.terms_days = row.get("TERMS").and_then(terms_days);
                p.tax_id = row.text("TAXID");
                p.is_1099 = row.yes("1099");
                p.active = !row.yes("HIDDEN");
                p.notes = row.text("NOTEPAD").or_else(|| row.text("NOTE"));
            }
            "INVITEM" => {
                let (Some(name), Some(kind)) = (row.get("NAME"), row.get("INVITEMTYPE").and_then(item_kind)) else { continue };
                let income_account_id = row.get("ACCNT").map(|a| accounts.ensure(a, "INCOME"));
                let rate_cents = row.get("PRICE").filter(|p| !p.ends_with('%')).and_then(|p| parse_amount_cents(p).ok()).map_or(0, i64::abs);
                items.push(SourceItem {
                    id: items.len() as i64 + 1,
                    name: name.to_string(),
                    description: row.text("DESC"),
                    rate_cents,
                    income_account_id,
                    kind,
                    taxable: row.yes("TAXABLE"),
                    active: !row.yes("HIDDEN"),
                });
            }
            "TRNS" => {
                if let Some(unclosed) = open.replace(Block { trns: RawLine::from(&row), splits: Vec::new(), file_line: line_no }) {
                    problems.push(format!("line {}: TRNS without ENDTRNS", unclosed.file_line));
                    blocks.push(unclosed);
                }
            }
            "SPL" => match open.as_mut() {
                Some(block) => block.splits.push(RawLine::from(&row)),
                None => problems.push(format!("line {line_no}: SPL outside a transaction")),
            },
            _ => {}
        }
    }
    if let Some(unclosed) = open.take() {
        problems.push(format!("line {}: TRNS without ENDTRNS", unclosed.file_line));
        blocks.push(unclosed);
    }
    if blocks.is_empty() && accounts.list.is_empty() && names.customers.is_empty() && names.vendors.is_empty() && items.is_empty() {
        return Err(match problems.first() {
            Some(p) => format!("IIF_EMPTY: no lists or transactions could be read ({p})"),
            None => "IIF_EMPTY: this file has no lists or transactions in IIF format".into(),
        });
    }

    let item_ids: HashMap<String, (i64, Option<i64>)> = items.iter().map(|i| (i.name.to_lowercase(), (i.id, i.income_account_id))).collect();
    let mut reader = Transactions { accounts, nonposting, names, item_ids, unreadable: Vec::new() };
    let mut transactions: Vec<SourceTransaction> = Vec::new();
    for (n, block) in blocks.iter().enumerate() {
        if let Some(t) = reader.transaction(n as i64 + 1, block) {
            transactions.push(t);
        }
    }
    transactions.sort_by_key(|t| (t.date, t.id));
    apply_payments(&mut transactions, &reader.accounts);

    let (transactions, unbalanced): (Vec<_>, Vec<_>) = transactions.into_iter().partition(|t| t.imbalance_cents() == 0);
    let quality = LedgerQuality { unreadable_lines: reader.unreadable.len(), ..Default::default() };
    let Transactions { accounts, mut names, unreadable, .. } = reader;
    names.sort();
    items.sort_by_key(|i| i.name.to_lowercase());
    Ok(Ledger { accounts: accounts.list, customers: names.customers, vendors: names.vendors, items, transactions, unbalanced, unreadable, quality })
}

/// Resolves transaction blocks against the lists.
struct Transactions {
    accounts: Accounts,
    nonposting: Vec<String>,
    names: Names,
    /// Item name → (item, its income account).
    item_ids: HashMap<String, (i64, Option<i64>)>,
    unreadable: Vec<UnreadableLine>,
}

impl Transactions {
    fn transaction(&mut self, id: i64, block: &Block) -> Option<SourceTransaction> {
        let trnstype = block.trns.trnstype.as_str();
        let kind = kind(trnstype);
        let date = block.trns.date.as_deref().and_then(|d| parse_date(d).ok());
        let mut lines = Vec::new();
        let mut form = Vec::new();
        for (i, raw) in std::iter::once(&block.trns).chain(&block.splits).enumerate() {
            let Some(account_name) = raw.account.as_deref() else { continue };
            let amount = raw.amount.as_deref().map(parse_amount_cents);
            let Some(Ok(amount)) = amount else {
                self.unreadable.push(UnreadableLine {
                    kind,
                    transaction_id: Some(id),
                    date,
                    doc_num: block.trns.doc_num.clone(),
                    name: block.trns.name.clone(),
                    account_id: self.accounts.id(account_name),
                    cents: None,
                    outcome: if raw.amount.is_none() { LineOutcome::NoAmount } else { LineOutcome::Unreadable },
                });
                continue;
            };
            if self.nonposting.contains(&account_name.trim().to_lowercase()) {
                // Estimates and purchase orders post to non-posting accounts: keep the form line only.
                if i > 0 {
                    form.push(self.form_line(raw, None, amount.abs()));
                }
                continue;
            }
            let account = match self.accounts.id(account_name) {
                Some(a) => a,
                None => {
                    let inferred = infer_type(account_name, trnstype, i == 0);
                    self.accounts.ensure(account_name, inferred)
                }
            };
            // IIF debits are positive; the ledger's are negative.
            let cents = -amount;
            lines.push(SourceLine { account_id: account, cents, memo: raw.memo.clone() });
            if i > 0 {
                form.push(self.form_line(raw, Some(account), cents));
            }
        }
        let Some(date) = date else {
            self.unreadable.extend(lines.iter().map(|l| UnreadableLine {
                kind,
                transaction_id: Some(id),
                date: None,
                doc_num: block.trns.doc_num.clone(),
                name: block.trns.name.clone(),
                account_id: Some(l.account_id),
                cents: Some(l.cents),
                outcome: LineOutcome::Unreadable,
            }));
            return None;
        };
        let document = self.document(trnstype, block, date, &lines, form);
        if lines.is_empty() && !matches!(document, Some(SourceDocument::Estimate { .. } | SourceDocument::PurchaseOrder { .. })) {
            return None;
        }
        Some(SourceTransaction {
            document,
            id,
            kind,
            date,
            doc_num: block.trns.doc_num.clone(),
            name: block.trns.name.clone(),
            memo: block.trns.memo.clone().or_else(|| block.splits.iter().find_map(|s| s.memo.clone())),
            lines,
        })
    }

    /// A form line: its item, quantity and rate as the file gives them. When they do not multiply
    /// out to the amount, the full import uses one unit at the amount instead. A line on a
    /// non-posting account (an estimate's or purchase order's) takes its item's income account.
    fn form_line(&self, raw: &RawLine, account: Option<i64>, cents: i64) -> DocumentLine {
        let item = raw.item.as_deref().and_then(|i| self.item_ids.get(&i.trim().to_lowercase()).copied());
        let quantity_e4 = raw.quantity.as_deref().and_then(quantity_e4).map(i64::abs).filter(|q| *q > 0).unwrap_or(10_000);
        let rate_cents = raw.price.as_deref().and_then(|p| parse_amount_cents(p).ok()).map_or(cents.abs(), i64::abs);
        DocumentLine {
            item_id: item.map(|i| i.0),
            description: raw.memo.clone(),
            quantity_e4,
            rate_cents,
            cents,
            account_id: account.or(item.and_then(|i| i.1)).unwrap_or(0),
        }
    }

    fn has_kind(&self, lines: &[SourceLine], account_type: &str) -> bool {
        lines.iter().any(|l| self.accounts.kind(l.account_id) == account_type)
    }

    fn document(&mut self, trnstype: &str, block: &Block, date: NaiveDate, lines: &[SourceLine], form: Vec<DocumentLine>) -> Option<SourceDocument> {
        let name = block.trns.name.clone();
        let number = block.trns.doc_num.clone();
        let due_date = block.trns.due_date.as_deref().and_then(|d| parse_date(d).ok());
        let customer = |names: &mut Names| name.as_deref().map(|n| names.ensure_customer(n));
        let vendor = |names: &mut Names| name.as_deref().map(|n| names.ensure_vendor(n));
        let method = || payment_method(block.trns.pay_method.as_deref().or(block.splits.iter().find_map(|s| s.pay_method.as_deref()))).to_string();
        Some(match trnstype {
            "INVOICE" => SourceDocument::Invoice { customer_id: customer(&mut self.names)?, number, due_date, lines: form },
            "CREDIT MEMO" => SourceDocument::CreditMemo { customer_id: customer(&mut self.names)?, number, lines: form, applied_to: Vec::new() },
            "CASH SALE" => SourceDocument::SalesReceipt { customer_id: customer(&mut self.names), number, method: method(), lines: form },
            "PAYMENT" => SourceDocument::Payment { customer_id: customer(&mut self.names)?, reference: number, method: method(), applied_to: Vec::new() },
            "DEPOSIT" => SourceDocument::Deposit { payments: Vec::new() },
            // An item receipt is a bill for goods received before the bill itself arrived.
            "BILL" | "BILL REFUND" | "ITEMRECEIPT" | "ITEM RECEIPT" => SourceDocument::Bill { vendor_id: vendor(&mut self.names)?, number, due_date, lines: form },
            "BILLPMT" | "BILL PMT" | "BILLCCARD" => SourceDocument::BillPayment { vendor_id: vendor(&mut self.names), reference: number, applied_to: Vec::new() },
            "CHECK" | "CREDIT CARD" | "CCARD REFUND" => {
                // A check that pays bills names a vendor; any other payee stays on the memo line.
                let pays_bills = self.has_kind(lines, "ACCOUNTS_PAYABLE");
                let vendor_id = match name.as_deref() {
                    Some(n) if pays_bills => Some(self.names.ensure_vendor(n)),
                    Some(n) => self.names.vendor(n),
                    None => None,
                };
                SourceDocument::Expense { vendor_id, number, applied_to: Vec::new() }
            }
            "TRANSFER" => SourceDocument::Transfer,
            "ESTIMATES" | "ESTIMATE" => SourceDocument::Estimate {
                customer_id: customer(&mut self.names)?,
                date,
                number,
                expires: due_date,
                memo: block.trns.memo.clone(),
                lines: form.into_iter().map(|l| DocumentLine { cents: l.cents.abs(), ..l }).collect(),
            },
            "PURCHORD" => SourceDocument::PurchaseOrder {
                vendor_id: vendor(&mut self.names)?,
                date,
                number,
                expected: due_date,
                memo: block.trns.memo.clone(),
                received: false,
                lines: form.into_iter().map(|l| DocumentLine { cents: l.cents.abs(), ..l }).collect(),
            },
            _ => return None,
        })
    }
}

/// Applies payments to the oldest open invoices of their customer and bill payments (and checks
/// that pay bills) to the oldest open bills of their vendor, and matches deposits to the payments
/// and sales receipts they swept out of Undeposited Funds. `transactions` is in date order.
fn apply_payments(transactions: &mut [SourceTransaction], accounts: &Accounts) {
    let sum = |t: &SourceTransaction, account_type: &str| -> i64 {
        t.lines.iter().filter(|l| accounts.kind(l.account_id) == account_type).map(|l| l.cents).sum()
    };
    let undeposited = |id: i64| accounts.list.iter().any(|a| a.id == id && a.full_name.to_lowercase().contains("undeposited"));

    let mut invoices: HashMap<i64, VecDeque<(i64, i64)>> = HashMap::new();
    let mut bills: HashMap<i64, VecDeque<(i64, i64)>> = HashMap::new();
    // Payments and receipts waiting in Undeposited Funds: (transaction, amount).
    let mut waiting: Vec<(i64, i64)> = Vec::new();

    fn take(queue: &mut VecDeque<(i64, i64)>, mut amount: i64) -> Vec<Applied> {
        let mut applied = Vec::new();
        while amount > 0 {
            let Some(front) = queue.front_mut() else { break };
            let cents = front.1.min(amount);
            applied.push(Applied { transaction_id: front.0, cents });
            front.1 -= cents;
            amount -= cents;
            if front.1 == 0 {
                queue.pop_front();
            }
        }
        applied
    }

    for t in transactions.iter_mut() {
        let id = t.id;
        let receivable = sum(t, "ACCOUNTS_RECEIVABLE");
        let payable = sum(t, "ACCOUNTS_PAYABLE");
        let into_undeposited: i64 = t.lines.iter().filter(|l| l.cents < 0 && undeposited(l.account_id)).map(|l| -l.cents).sum();
        match &mut t.document {
            Some(SourceDocument::Invoice { customer_id, .. }) if receivable < 0 => invoices.entry(*customer_id).or_default().push_back((id, -receivable)),
            Some(SourceDocument::Payment { customer_id, applied_to, .. }) => {
                if receivable > 0 {
                    *applied_to = take(invoices.entry(*customer_id).or_default(), receivable);
                }
                if into_undeposited > 0 {
                    waiting.push((id, into_undeposited));
                }
            }
            Some(SourceDocument::SalesReceipt { .. }) if into_undeposited > 0 => waiting.push((id, into_undeposited)),
            Some(SourceDocument::Bill { vendor_id, .. }) if payable > 0 => bills.entry(*vendor_id).or_default().push_back((id, payable)),
            Some(SourceDocument::BillPayment { vendor_id: Some(vendor), applied_to, .. } | SourceDocument::Expense { vendor_id: Some(vendor), applied_to, .. })
                if payable < 0 =>
            {
                *applied_to = take(bills.entry(*vendor).or_default(), -payable);
            }
            Some(SourceDocument::Deposit { payments }) => {
                // Each Undeposited Funds line is one payment taken out; the oldest waiting payment
                // of that amount is the one.
                for line in t.lines.iter().filter(|l| l.cents > 0 && undeposited(l.account_id)) {
                    if let Some(i) = waiting.iter().position(|w| w.1 == line.cents) {
                        let (payment, cents) = waiting.remove(i);
                        payments.push(Applied { transaction_id: payment, cents });
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
