//! Full import: posts a staged desktop company file as LocalBooks documents rather than journal
//! entries, the way Intuit's own Desktop-to-Online conversion carries transactions over natively.
//!
//! Invoices, credit memos and sales receipts become documents with their form lines; received
//! payments become one LocalBooks payment per invoice they paid (the file's `AppliedToTxn`
//! links); credit memos are applied to the invoices they were applied to; deposits sweep those
//! payments out of Undeposited Funds. Every document posts exactly the source file's own ledger
//! lines, so account balances come out the same as a journal import.
//!
//! A transaction that cannot be a document (its form does not add up, it applies to something that
//! did not import as a document, a database rule refuses it) falls back to a journal entry, and the
//! reason is kept so the review screen can list it. Each document is written under a savepoint, so
//! a refused one leaves nothing half-written behind. Everything else posts as a journal entry, as
//! in a journal import.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use serde::Serialize;
use sqlx::{Sqlite, Transaction};

use crate::invoices::line_amount;
use crate::ledger::{self, db_err, EntrySource, JournalEntry, JournalLine};
use crate::qbw_documents::{DocumentLine, SourceDocument};

/// One staged transaction, with its lines already mapped to LocalBooks accounts.
pub(crate) struct Staged {
    pub source_id: i64,
    pub kind: String,
    pub date: NaiveDate,
    pub doc_num: Option<String>,
    pub name: Option<String>,
    pub memo: Option<String>,
    /// (LocalBooks account, cents: negative = debit, memo)
    pub lines: Vec<(i64, i64, Option<String>)>,
    pub document: Option<SourceDocument>,
}

/// What one staged transaction became.
pub(crate) struct Outcome {
    pub source_id: i64,
    /// The transaction's entry (the first, when a payment split into several).
    pub entry_id: i64,
    pub posted_as: &'static str,
    pub fallback_reason: Option<String>,
    pub entries: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FullImportCounts {
    pub invoices: i64,
    pub credit_memos: i64,
    pub sales_receipts: i64,
    /// Payment rows, one per invoice paid.
    pub payments: i64,
    pub deposits: i64,
    pub credits_applied: i64,
    pub journal_entries: i64,
    /// Documents that had to post as journal entries instead.
    pub fell_back: i64,
}

/// The LocalBooks records the staged lists became.
struct Books {
    /// account → (type, is_system)
    accounts: HashMap<i64, (String, bool)>,
    /// source customer → (customer, name)
    customers: HashMap<i64, (i64, String)>,
    /// source item → (product, name)
    products: HashMap<i64, (i64, String)>,
}

impl Books {
    fn kind(&self, account: i64) -> &str {
        self.accounts.get(&account).map_or("", |(t, _)| t.as_str())
    }

    fn is_undeposited(&self, account: i64) -> bool {
        self.accounts.get(&account).is_some_and(|(t, system)| t == "OTHER_CURRENT_ASSET" && *system)
    }

    fn customer(&self, source: i64) -> Result<&(i64, String), String> {
        self.customers.get(&source).ok_or_else(|| "its customer was not imported".to_string())
    }
}

async fn load_books(tx: &mut Transaction<'_, Sqlite>, batch_id: i64) -> Result<Books, String> {
    let accounts = sqlx::query_as::<_, (i64, String, bool)>("SELECT id, account_type, is_system FROM chart_of_accounts")
        .fetch_all(&mut **tx)
        .await
        .map_err(db_err)?
        .into_iter()
        .map(|(id, t, s)| (id, (t, s)))
        .collect();
    let customers = sqlx::query_as::<_, (i64, i64, String)>(
        "SELECT p.source_id, c.id, c.name FROM ledger_import_parties p JOIN customers c ON c.id = p.party_id
         WHERE p.batch_id = ?1 AND p.kind = 'CUSTOMER'",
    )
    .bind(batch_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?
    .into_iter()
    .map(|(s, id, name)| (s, (id, name)))
    .collect();
    let products = sqlx::query_as::<_, (i64, i64, String)>(
        "SELECT i.source_id, p.id, p.name FROM ledger_import_items i JOIN products_services p ON p.id = i.product_id WHERE i.batch_id = ?1",
    )
    .bind(batch_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(db_err)?
    .into_iter()
    .map(|(s, id, name)| (s, (id, name)))
    .collect();
    Ok(Books { accounts, customers, products })
}

fn entry_lines(lines: &[(i64, i64, Option<String>)]) -> Vec<JournalLine> {
    lines
        .iter()
        .map(|(account, cents, memo)| {
            let line = if *cents < 0 { JournalLine::debit(*account, -cents) } else { JournalLine::credit(*account, *cents) };
            line.with_memo(memo.as_deref())
        })
        .collect()
}

fn describe(t: &Staged) -> String {
    crate::qbw_ledger_import::describe(&t.kind, t.doc_num.as_deref(), t.name.as_deref(), t.memo.as_deref())
}

async fn post(tx: &mut Transaction<'_, Sqlite>, t: &Staged, description: String, lines: &[(i64, i64, Option<String>)], source: EntrySource) -> Result<i64, String> {
    let entry = JournalEntry { id: None, date: t.date, description, lines: entry_lines(lines), currency: None };
    ledger::post_entry_in_tx(tx, &entry, source).await
}

async fn exec(tx: &mut Transaction<'_, Sqlite>, sql: &str) -> Result<(), String> {
    sqlx::query(sql).execute(&mut **tx).await.map(|_| ()).map_err(db_err)
}

/// A form line ready for a document row.
struct FormLine {
    product_id: Option<i64>,
    description: String,
    quantity_e4: i64,
    rate_cents: i64,
    amount_cents: i64,
    account_id: i64,
}

/// A document's form: its item lines, the discount and the sales tax.
struct Form {
    lines: Vec<FormLine>,
    discount: i64,
    tax: i64,
}

/// Sorts a document's lines into what LocalBooks keeps on the form. `sign` is 1 when item lines
/// are credits (invoices, receipts) and -1 when they are debits (credit memos). Lines to
/// liability accounts are the sales tax; item lines the other way are the discount; cost of
/// goods sold and the inventory it came out of stay in the entry only, and must net to zero.
/// The form must add up to `total`.
fn form(lines: &[DocumentLine], map: &HashMap<i64, i64>, books: &Books, sign: i64, total: i64) -> Result<Form, String> {
    let mut out = Form { lines: Vec::new(), discount: 0, tax: 0 };
    let mut cost = 0;
    for l in lines {
        let account = *map.get(&l.account_id).ok_or("a line's account is not mapped")?;
        let cents = l.cents * sign;
        match books.kind(account) {
            "COST_OF_GOODS_SOLD" | "OTHER_CURRENT_ASSET" | "FIXED_ASSET" | "OTHER_ASSET" => cost += l.cents,
            "OTHER_CURRENT_LIABILITY" => out.tax += cents,
            _ if cents > 0 => {
                let (quantity_e4, rate_cents) = match line_amount(l.quantity_e4, l.rate_cents) {
                    Ok(a) if a == cents => (l.quantity_e4, l.rate_cents),
                    _ => (10_000, cents),
                };
                let product = l.item_id.and_then(|i| books.products.get(&i));
                let description = l.description.clone().or_else(|| product.map(|p| p.1.clone())).unwrap_or_default();
                out.lines.push(FormLine { product_id: product.map(|p| p.0), description, quantity_e4, rate_cents, amount_cents: cents, account_id: account });
            }
            _ => out.discount -= cents,
        }
    }
    if cost != 0 {
        return Err("a line posts to an asset account outside the form".into());
    }
    if out.tax < 0 {
        return Err("its sales tax is negative".into());
    }
    if out.lines.is_empty() {
        return Err("it has no item lines".into());
    }
    let subtotal: i64 = out.lines.iter().map(|l| l.amount_cents).sum();
    if out.discount > subtotal || subtotal - out.discount + out.tax != total {
        return Err("its form lines do not add up to its total".into());
    }
    Ok(out)
}

/// The document number the file used when it is a free positive number, otherwise the next one.
async fn number(tx: &mut Transaction<'_, Sqlite>, table: &str, column: &str, base: i64, doc_num: Option<&str>) -> Result<i64, String> {
    if let Some(n) = doc_num.and_then(|d| d.trim().parse::<i64>().ok()).filter(|n| (1..1_000_000_000_000).contains(n)) {
        let taken: bool = sqlx::query_scalar(&format!("SELECT EXISTS (SELECT 1 FROM {table} WHERE {column} = ?1)"))
            .bind(n)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_err)?;
        if !taken {
            return Ok(n);
        }
    }
    sqlx::query_scalar(&format!("SELECT COALESCE(MAX({column}), {base}) + 1 FROM {table}")).fetch_one(&mut **tx).await.map_err(db_err)
}

/// Lines on receivables accounts, and their account when there is just one.
fn receivable(t: &Staged, books: &Books) -> (i64, Option<i64>) {
    let ar: Vec<&(i64, i64, Option<String>)> = t.lines.iter().filter(|l| books.kind(l.0) == "ACCOUNTS_RECEIVABLE").collect();
    let accounts: HashSet<i64> = ar.iter().map(|l| l.0).collect();
    (ar.iter().map(|l| l.1).sum(), if accounts.len() == 1 { accounts.into_iter().next() } else { None })
}

async fn insert_lines(tx: &mut Transaction<'_, Sqlite>, table: &str, fk: &str, id: i64, lines: &[FormLine]) -> Result<(), String> {
    for (n, l) in lines.iter().enumerate() {
        sqlx::query(&format!(
            "INSERT INTO {table} ({fk}, line_number, description, quantity_e4, rate_cents, amount_cents, income_account_id, product_service_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
        ))
        .bind(id)
        .bind(n as i64 + 1)
        .bind(l.description.trim())
        .bind(l.quantity_e4)
        .bind(l.rate_cents)
        .bind(l.amount_cents)
        .bind(l.account_id)
        .bind(l.product_id)
        .execute(&mut **tx)
        .await
        .map_err(db_err)?;
    }
    Ok(())
}

/// The documents written so far, by source transaction.
#[derive(Default)]
struct Written {
    /// invoice → (LocalBooks invoice, issue date)
    invoices: HashMap<i64, (i64, NaiveDate)>,
    /// credit memo → (LocalBooks credit memo, date)
    credit_memos: HashMap<i64, (i64, NaiveDate)>,
    /// receipt or payment → its rows: (payment row or receipt, amount, deposit account, is a receipt)
    deposited: HashMap<i64, Vec<(i64, i64, i64, bool)>>,
}

async fn write_invoice(
    tx: &mut Transaction<'_, Sqlite>,
    t: &Staged,
    doc: &SourceDocument,
    map: &HashMap<i64, i64>,
    books: &Books,
) -> Result<(i64, i64), String> {
    let SourceDocument::Invoice { customer_id, number: doc_num, due_date, lines } = doc else { unreachable!() };
    let (customer, customer_name) = books.customer(*customer_id)?.clone();
    let total = -receivable(t, books).0;
    if total <= 0 {
        return Err("it does not charge the customer".into());
    }
    let form = form(lines, map, books, 1, total)?;
    let number = number(tx, "invoices", "invoice_number", 1000, doc_num.as_deref()).await?;
    let due = due_date.filter(|d| *d >= t.date).unwrap_or(t.date);
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO invoices (invoice_number, customer_name, issue_date, due_date, total_cents, status, customer_id, tax_cents, discount_cents)
         VALUES (?1, ?2, ?3, ?4, ?5, 'DRAFT', ?6, ?7, ?8) RETURNING id",
    )
    .bind(number)
    .bind(&customer_name)
    .bind(t.date)
    .bind(due)
    .bind(total)
    .bind(customer)
    .bind(form.tax)
    .bind(form.discount)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    insert_lines(tx, "invoice_items", "invoice_id", id, &form.lines).await?;
    let entry = post(tx, t, format!("Invoice #{number} - {customer_name}"), &t.lines, EntrySource::Invoice(id)).await?;
    sqlx::query("UPDATE invoices SET status = 'UNPAID', journal_entry_id = ?1 WHERE id = ?2").bind(entry).bind(id).execute(&mut **tx).await.map_err(db_err)?;
    Ok((entry, id))
}

async fn write_credit_memo(
    tx: &mut Transaction<'_, Sqlite>,
    t: &Staged,
    doc: &SourceDocument,
    map: &HashMap<i64, i64>,
    books: &Books,
) -> Result<(i64, i64), String> {
    let SourceDocument::CreditMemo { customer_id, number: doc_num, lines, .. } = doc else { unreachable!() };
    let (customer, customer_name) = books.customer(*customer_id)?.clone();
    let total = receivable(t, books).0;
    if total <= 0 {
        return Err("it does not credit the customer".into());
    }
    let form = form(lines, map, books, -1, total)?;
    if form.discount != 0 {
        return Err("it has a line the other way (credit memos have no discount)".into());
    }
    let number = number(tx, "credit_memos", "credit_number", 0, doc_num.as_deref()).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO credit_memos (credit_number, customer_id, credit_date, memo, total_cents, tax_cents) VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING id",
    )
    .bind(number)
    .bind(customer)
    .bind(t.date)
    .bind(&t.memo)
    .bind(total)
    .bind(form.tax)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    insert_lines(tx, "credit_memo_lines", "credit_memo_id", id, &form.lines).await?;
    let entry = post(tx, t, format!("Credit memo #{number} - {customer_name}"), &t.lines, EntrySource::CreditMemo(id)).await?;
    sqlx::query("UPDATE credit_memos SET journal_entry_id = ?1 WHERE id = ?2").bind(entry).bind(id).execute(&mut **tx).await.map_err(db_err)?;
    Ok((entry, id))
}

/// The one debit of a receipt, payment or deposit: the account the money went into.
fn single_debit(t: &Staged) -> Result<(i64, i64), String> {
    let debits: Vec<&(i64, i64, Option<String>)> = t.lines.iter().filter(|l| l.1 < 0).collect();
    match debits.as_slice() {
        [(account, cents, _)] => Ok((*account, -cents)),
        _ => Err("the money went into more than one account".into()),
    }
}

async fn write_sales_receipt(
    tx: &mut Transaction<'_, Sqlite>,
    t: &Staged,
    doc: &SourceDocument,
    map: &HashMap<i64, i64>,
    books: &Books,
) -> Result<(i64, i64, i64, i64), String> {
    let SourceDocument::SalesReceipt { customer_id, number: doc_num, method, lines } = doc else { unreachable!() };
    let (into, total) = single_debit(t)?;
    if !matches!(books.kind(into), "BANK" | "OTHER_CURRENT_ASSET") {
        return Err("the money did not go into a bank or Undeposited Funds account".into());
    }
    let customer = match customer_id {
        Some(c) => Some(books.customer(*c)?.clone()),
        None => None,
    };
    let lines: Vec<DocumentLine> = lines.iter().filter(|l| !(l.cents < 0 && map.get(&l.account_id) == Some(&into))).cloned().collect();
    let form = form(&lines, map, books, 1, total)?;
    let number = number(tx, "sales_receipts", "receipt_number", 0, doc_num.as_deref()).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO sales_receipts (receipt_number, customer_id, receipt_date, deposit_account_id, payment_method, reference, memo, total_cents, tax_cents, discount_cents)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) RETURNING id",
    )
    .bind(number)
    .bind(customer.as_ref().map(|c| c.0))
    .bind(t.date)
    .bind(into)
    .bind(method)
    .bind(doc_num)
    .bind(&t.memo)
    .bind(total)
    .bind(form.tax)
    .bind(form.discount)
    .fetch_one(&mut **tx)
    .await
    .map_err(db_err)?;
    insert_lines(tx, "sales_receipt_lines", "sales_receipt_id", id, &form.lines).await?;
    let who = customer.map(|c| format!(" - {}", c.1)).unwrap_or_default();
    let entry = post(tx, t, format!("Sales receipt #{number}{who}"), &t.lines, EntrySource::SalesReceipt(id)).await?;
    sqlx::query("UPDATE sales_receipts SET journal_entry_id = ?1 WHERE id = ?2").bind(entry).bind(id).execute(&mut **tx).await.map_err(db_err)?;
    Ok((entry, id, total, into))
}

/// A received payment becomes one payment per invoice it paid; whatever it did not apply to an
/// imported invoice posts as a journal entry. Returns the entries and the payment rows.
async fn write_payment(
    tx: &mut Transaction<'_, Sqlite>,
    t: &Staged,
    doc: &SourceDocument,
    books: &Books,
    written: &Written,
) -> Result<(Vec<i64>, Vec<(i64, i64)>, i64, Option<String>), String> {
    let SourceDocument::Payment { reference, method, applied_to, .. } = doc else { unreachable!() };
    let (into, total) = single_debit(t)?;
    if !matches!(books.kind(into), "BANK" | "OTHER_CURRENT_ASSET") {
        return Err("the money did not go into a bank or Undeposited Funds account".into());
    }
    let (ar_cents, ar) = receivable(t, books);
    let ar = ar.ok_or("it does not credit one receivables account")?;
    if ar_cents != total || t.lines.len() != t.lines.iter().filter(|l| l.0 == into || l.0 == ar).count() {
        return Err("it posts to accounts other than receivables and the deposit account".into());
    }
    let (mut entries, mut rows, mut applied) = (Vec::new(), Vec::new(), 0);
    let mut skipped: Vec<&str> = Vec::new();
    for a in applied_to {
        let Some(&(invoice, _)) = written.invoices.get(&a.transaction_id) else {
            skipped.push("what it paid did not import as an invoice");
            continue;
        };
        let (number, customer, open): (i64, String, i64) =
            sqlx::query_as("SELECT invoice_number, customer_name, total_cents - amount_paid_cents FROM invoices WHERE id = ?1")
                .bind(invoice)
                .fetch_one(&mut **tx)
                .await
                .map_err(db_err)?;
        // A payment may predate its invoice (a prepayment), as QuickBooks allows. What it applies
        // beyond the invoice's open balance (the file can over-apply) stays unapplied.
        let cents = a.cents.min(open).min(total - applied);
        if cents < a.cents {
            skipped.push(if cents < open { "its applications add up to more than the payment" } else { "it pays more than the invoice's open balance" });
        }
        if cents <= 0 {
            continue;
        }
        let row: i64 = sqlx::query_scalar(
            "INSERT INTO invoice_payments (invoice_id, payment_date, amount_cents, payment_method, reference, deposit_account_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING id",
        )
        .bind(invoice)
        .bind(t.date)
        .bind(cents)
        .bind(method)
        .bind(reference)
        .bind(into)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)?;
        let lines = [(into, -cents, None), (ar, cents, None)];
        let entry = post(tx, t, format!("Payment - Invoice #{number} - {customer}"), &lines, EntrySource::ReceivePayment(row)).await?;
        sqlx::query("UPDATE invoice_payments SET journal_entry_id = ?1 WHERE id = ?2").bind(entry).bind(row).execute(&mut **tx).await.map_err(db_err)?;
        ledger::recompute_paid(tx, "invoices", "invoice_payments", "invoice_id", invoice).await?;
        entries.push(entry);
        rows.push((row, cents));
        applied += cents;
    }
    skipped.dedup();
    if rows.is_empty() {
        return Err(if skipped.is_empty() { "it was not applied to an invoice".into() } else { skipped.join("; ") });
    }
    let unapplied = total - applied;
    let mut note = None;
    if unapplied > 0 {
        let lines = [(into, -unapplied, None), (ar, unapplied, None)];
        entries.push(post(tx, t, format!("{} (unapplied)", describe(t)), &lines, EntrySource::Journal).await?);
        if !skipped.is_empty() {
            note = Some(format!("{} of it posted as a journal entry: {}", ledger::format_cents(unapplied), skipped.join("; ")));
        }
    }
    Ok((entries, rows, into, note))
}

/// A deposit sweeps the imported payments and receipts it was linked to out of Undeposited Funds.
async fn write_deposit(tx: &mut Transaction<'_, Sqlite>, t: &Staged, doc: &SourceDocument, books: &Books, written: &Written) -> Result<(i64, usize), String> {
    let SourceDocument::Deposit { payments } = doc else { unreachable!() };
    // The bank account is debited with what arrived; other debits are fees or cash back taken out
    // of the deposit. LocalBooks deposit lines are the money put in, so the deposit's total is
    // the gross amount and its entry (the file's own lines) carries the deductions.
    let banks: Vec<&(i64, i64, Option<String>)> = t.lines.iter().filter(|l| l.1 < 0 && matches!(books.kind(l.0), "BANK" | "CREDIT_CARD")).collect();
    let [&(bank, _, _)] = banks.as_slice() else {
        return Err("it was not deposited into exactly one bank account".into());
    };
    let total: i64 = t.lines.iter().filter(|l| l.1 > 0).map(|l| l.1).sum();
    let deducted: i64 = -t.lines.iter().filter(|l| l.1 < 0 && l.0 != bank).map(|l| l.1).sum::<i64>();
    if t.lines.iter().any(|l| l.1 < 0 && l.0 != bank && books.is_undeposited(l.0)) {
        return Err("it puts money back into Undeposited Funds".into());
    }
    if t.lines.iter().any(|l| l.0 != bank && matches!(books.kind(l.0), "ACCOUNTS_RECEIVABLE" | "ACCOUNTS_PAYABLE")) {
        return Err("it posts to receivables or payables".into());
    }
    if t.lines.iter().any(|l| l.0 == bank && l.1 > 0) {
        return Err("it takes money back out of the bank account".into());
    }
    let memo = match (deducted, t.memo.as_deref()) {
        (0, m) => m.map(str::to_string),
        (d, Some(m)) => Some(format!("{m} ({} taken out of the deposit)", ledger::format_cents(d))),
        (d, None) => Some(format!("{} taken out of the deposit", ledger::format_cents(d))),
    };
    let id: i64 = sqlx::query_scalar("INSERT INTO deposits (bank_account_id, deposit_date, memo, total_cents) VALUES (?1, ?2, ?3, ?4) RETURNING id")
        .bind(bank)
        .bind(t.date)
        .bind(memo)
        .bind(total)
        .fetch_one(&mut **tx)
        .await
        .map_err(db_err)?;
    // The file gives each deposited payment its own Undeposited Funds line, but not which one;
    // the linked payments are matched against the deposit's Undeposited Funds total instead.
    let mut pooled: Vec<(i64, i64)> = Vec::new();
    for (account, cents, _) in t.lines.iter().filter(|l| l.1 > 0 && books.is_undeposited(l.0)) {
        match pooled.iter_mut().find(|(a, _)| a == account) {
            Some((_, sum)) => *sum += cents,
            None => pooled.push((*account, *cents)),
        }
    }
    let (mut n, mut linked) = (0, 0);
    for p in payments {
        let Some(rows) = written.deposited.get(&p.transaction_id) else { continue };
        let sum: i64 = rows.iter().map(|r| r.1).sum();
        let Some((account, left)) = pooled.iter_mut().find(|(a, _)| rows.iter().all(|r| r.2 == *a)) else { continue };
        if sum > p.cents || sum > *left {
            continue;
        }
        for &(row, amount, _, receipt) in rows {
            n += 1;
            let (payment, receipt) = if receipt { (None, Some(row)) } else { (Some(row), None) };
            sqlx::query(
                "INSERT INTO deposit_lines (deposit_id, line_number, invoice_payment_id, sales_receipt_id, account_id, amount_cents) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .bind(id)
            .bind(n)
            .bind(payment)
            .bind(receipt)
            .bind(*account)
            .bind(amount)
            .execute(&mut **tx)
            .await
            .map_err(db_err)?;
            *left -= amount;
            linked += 1;
        }
    }
    // What is left: the Undeposited Funds not matched to a payment, and every other line as it was.
    let others = t.lines.iter().filter(|l| l.1 > 0 && !books.is_undeposited(l.0)).map(|(a, c, m)| (*a, *c, m.clone()));
    for (account, cents, memo) in pooled.into_iter().map(|(a, c)| (a, c, None)).chain(others) {
        if cents > 0 {
            n += 1;
            sqlx::query("INSERT INTO deposit_lines (deposit_id, line_number, account_id, amount_cents, memo) VALUES (?1, ?2, ?3, ?4, ?5)")
                .bind(id)
                .bind(n)
                .bind(account)
                .bind(cents)
                .bind(memo)
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
        }
    }
    let entry = post(tx, t, describe(t), &t.lines, EntrySource::Deposit(id)).await?;
    sqlx::query("UPDATE deposits SET journal_entry_id = ?1 WHERE id = ?2").bind(entry).bind(id).execute(&mut **tx).await.map_err(db_err)?;
    Ok((entry, linked))
}

/// Posts the staged transactions: documents first, then the payments that pay them, the credits
/// applied to them and the deposits that sweep the payments. Fails (and the caller rolls back)
/// only when a transaction cannot be posted even as a journal entry.
pub(crate) async fn post_all(
    tx: &mut Transaction<'_, Sqlite>,
    batch_id: i64,
    map: &HashMap<i64, i64>,
    staged: &[Staged],
) -> Result<(Vec<Outcome>, FullImportCounts), String> {
    let books = load_books(tx, batch_id).await?;
    let mut written = Written::default();
    let mut counts = FullImportCounts::default();
    let mut outcomes = Vec::with_capacity(staged.len());

    // Runs one document under a savepoint; a refusal rolls it back and posts the transaction as a
    // journal entry with the reason.
    macro_rules! attempt {
        ($write:expr) => {{
            exec(tx, "SAVEPOINT full_import_doc").await?;
            match $write {
                Ok(v) => {
                    exec(tx, "RELEASE full_import_doc").await?;
                    Ok(v)
                }
                Err(reason) => {
                    exec(tx, "ROLLBACK TO full_import_doc").await?;
                    exec(tx, "RELEASE full_import_doc").await?;
                    Err(reason)
                }
            }
        }};
    }
    async fn fallback(tx: &mut Transaction<'_, Sqlite>, t: &Staged, reason: Option<String>, counts: &mut FullImportCounts) -> Result<Outcome, String> {
        let entry = post(tx, t, describe(t), &t.lines, EntrySource::Journal).await.map_err(|e| format!("{e} (imported {})", describe(t)))?;
        counts.journal_entries += 1;
        counts.fell_back += reason.is_some() as i64;
        Ok(Outcome { source_id: t.source_id, entry_id: entry, posted_as: "JOURNAL", fallback_reason: reason, entries: 1 })
    }

    let phase = |t: &Staged| match &t.document {
        Some(SourceDocument::Payment { .. }) => 1,
        Some(SourceDocument::Deposit { .. }) => 3,
        _ => 0,
    };
    for pass in [0, 1, 3] {
        for t in staged.iter().filter(|t| phase(t) == pass) {
            let outcome = match &t.document {
                None => fallback(tx, t, None, &mut counts).await?,
                Some(doc @ SourceDocument::Invoice { .. }) => match attempt!(write_invoice(tx, t, doc, map, &books).await) {
                    Ok((entry, id)) => {
                        written.invoices.insert(t.source_id, (id, t.date));
                        counts.invoices += 1;
                        Outcome { source_id: t.source_id, entry_id: entry, posted_as: "INVOICE", fallback_reason: None, entries: 1 }
                    }
                    Err(reason) => fallback(tx, t, Some(reason), &mut counts).await?,
                },
                Some(doc @ SourceDocument::CreditMemo { .. }) => match attempt!(write_credit_memo(tx, t, doc, map, &books).await) {
                    Ok((entry, id)) => {
                        written.credit_memos.insert(t.source_id, (id, t.date));
                        counts.credit_memos += 1;
                        Outcome { source_id: t.source_id, entry_id: entry, posted_as: "CREDIT_MEMO", fallback_reason: None, entries: 1 }
                    }
                    Err(reason) => fallback(tx, t, Some(reason), &mut counts).await?,
                },
                Some(doc @ SourceDocument::SalesReceipt { .. }) => match attempt!(write_sales_receipt(tx, t, doc, map, &books).await) {
                    Ok((entry, id, total, into)) => {
                        written.deposited.insert(t.source_id, vec![(id, total, into, true)]);
                        counts.sales_receipts += 1;
                        Outcome { source_id: t.source_id, entry_id: entry, posted_as: "SALES_RECEIPT", fallback_reason: None, entries: 1 }
                    }
                    Err(reason) => fallback(tx, t, Some(reason), &mut counts).await?,
                },
                Some(doc @ SourceDocument::Payment { .. }) => match attempt!(write_payment(tx, t, doc, &books, &written).await) {
                    Ok((entries, rows, into, note)) => {
                        counts.payments += rows.len() as i64;
                        counts.journal_entries += entries.len() as i64 - rows.len() as i64;
                        written.deposited.insert(t.source_id, rows.into_iter().map(|(row, cents)| (row, cents, into, false)).collect());
                        Outcome { source_id: t.source_id, entry_id: entries[0], posted_as: "PAYMENT", fallback_reason: note, entries: entries.len() as i64 }
                    }
                    Err(reason) => fallback(tx, t, Some(reason), &mut counts).await?,
                },
                Some(doc @ SourceDocument::Deposit { .. }) => match attempt!(write_deposit(tx, t, doc, &books, &written).await) {
                    Ok((entry, _linked)) => {
                        counts.deposits += 1;
                        Outcome { source_id: t.source_id, entry_id: entry, posted_as: "DEPOSIT", fallback_reason: None, entries: 1 }
                    }
                    Err(reason) => fallback(tx, t, Some(reason), &mut counts).await?,
                },
            };
            outcomes.push(outcome);
        }
        if pass == 1 {
            counts.credits_applied += apply_credits(tx, staged, &written).await?;
        }
    }
    Ok((outcomes, counts))
}

/// Applies the imported credit memos to the imported invoices they were applied to in the file.
async fn apply_credits(tx: &mut Transaction<'_, Sqlite>, staged: &[Staged], written: &Written) -> Result<i64, String> {
    let mut applied = 0;
    for t in staged {
        let Some(SourceDocument::CreditMemo { applied_to, .. }) = &t.document else { continue };
        let Some(&(memo, memo_date)) = written.credit_memos.get(&t.source_id) else { continue };
        for a in applied_to {
            let Some(&(invoice, issued)) = written.invoices.get(&a.transaction_id) else { continue };
            let (open, remaining): (i64, i64) = sqlx::query_as(
                "SELECT (SELECT total_cents - amount_paid_cents FROM invoices WHERE id = ?1), (SELECT remaining_cents FROM v_credit_memo_balances WHERE credit_memo_id = ?2)",
            )
            .bind(invoice)
            .bind(memo)
            .fetch_one(&mut **tx)
            .await
            .map_err(db_err)?;
            if a.cents > open || a.cents > remaining {
                continue;
            }
            sqlx::query("INSERT INTO credit_applications (credit_memo_id, invoice_id, applied_date, amount_cents) VALUES (?1, ?2, ?3, ?4)")
                .bind(memo)
                .bind(invoice)
                .bind(memo_date.max(issued))
                .bind(a.cents)
                .execute(&mut **tx)
                .await
                .map_err(db_err)?;
            ledger::recompute_paid(tx, "invoices", "invoice_payments", "invoice_id", invoice).await?;
            applied += 1;
        }
    }
    Ok(applied)
}
