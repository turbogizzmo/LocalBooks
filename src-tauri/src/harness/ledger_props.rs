//! Ledger invariant property tests.
//!
//! Each case drives a random sequence of operations (journal entries, invoices in home and foreign
//! currency, payments, bills, bill payments, expenses and checks, deposits, transfers, voids, edits
//! and closing-date changes) through the same public functions the app's commands call, on a fresh
//! in-memory company. A small model predicts what every step must do: which steps the ledger has
//! to refuse (closed period, payments applied, already deposited), and the balance every account
//! must end up with. After every step the harness checks:
//!
//! - every posted entry balances, the trial balance sums to zero, and amounts are integer cents
//!   that match the model to the cent (no drift);
//! - assets = liabilities + equity on the balance sheet at several dates, net income on the
//!   balance sheet equals the P&L for the fiscal year, and total equity equals the equity accounts
//!   plus all profit to date; the cash flow statement ends at the bank balances;
//! - nothing posts or voids on or before the closing date, and a refused step changes nothing;
//! - a void reverses its entry exactly (same date, same lines, sides swapped), posted entries never
//!   change, and the audit trail (`verify_ledger_audit`, `audit_chain::verify`) stays intact;
//! - A/R and A/P equal the open balances of the unpaid invoices and bills, in the ledger, in the
//!   aging reports and on the documents; Undeposited Funds equals the payments waiting for deposit.
//!
//! Sequences shrink to the shortest failing one. `PROPTEST_CASES` sets the number of cases
//! (default 16, to keep `cargo test` quick); use e.g. `PROPTEST_CASES=2000` for a long run.

use std::collections::BTreeMap;

use chrono::{Days, NaiveDate};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestCaseError};
use sqlx::SqlitePool;

use super::{account, runtime, system_account};
use crate::banking::{DepositInput, DepositItemRef, DepositOtherLine, TransferInput};
use crate::closing::ClosingInput;
use crate::expenses::{BillInput, BillLineInput, BillPaymentInput, ExpenseInput, SplitLineInput};
use crate::invoices::{InvoiceInput, InvoiceLineInput, PaymentInput};
use crate::ledger::{mul_div_round, to_home_cents, ForeignCurrency, JournalEntry, JournalLine};

/// Days covered by generated dates: 2025-01-01 through 2026-12-31, so fiscal years roll over.
const SPAN: u16 = 730;

fn date(day: u16) -> NaiveDate {
    NaiveDate::from_ymd_opt(2025, 1, 1).unwrap() + Days::new(u64::from(day.min(SPAN - 1)))
}

fn epoch() -> NaiveDate {
    NaiveDate::from_ymd_opt(2025, 1, 1).unwrap()
}

/// Every generated date (payments up to 90 days late included) is on or before this.
fn far_end() -> NaiveDate {
    NaiveDate::from_ymd_opt(2027, 12, 31).unwrap()
}

// ------------------------------------------------------------------------------------------------
// Operations
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Op {
    Journal { day: u16, debit: Pick, credit: Pick, cents: i64 },
    /// Lines are (quantity in quarter units, rate in cents); `fx_rate` makes it a EUR invoice.
    Invoice { day: u16, due_in: u16, lines: Vec<(i64, i64)>, income: Pick, customer: u8, fx_rate: Option<i64> },
    ReceivePayment { invoice: Pick, percent: u8, after: u16, undeposited: bool, fx_rate: i64 },
    Bill { day: u16, due_in: u16, lines: Vec<(Pick, i64)>, vendor: u8 },
    PayBill { bill: Pick, percent: u8, after: u16, from: Pick },
    Expense { day: u16, lines: Vec<(Pick, i64)>, from: Pick, check: bool },
    Deposit { day: u16, items: bool, other: Option<(Pick, i64)>, to: Pick },
    Transfer { day: u16, from: Pick, to: Pick, cents: i64 },
    Void { entry: Pick },
    /// Edits a journal entry, invoice, bill or expense: a new amount, the date moved by `shift` days.
    Replace { entry: Pick, cents: i64, shift: i16 },
    SetClosing { day: Option<u16> },
}

/// Picks an element of whatever list the step works on (accounts, open invoices, live entries...).
/// Mostly small, and shrinks towards 0.
#[derive(Debug, Clone, Copy)]
struct Pick(u8);

impl Pick {
    fn index(self, len: usize) -> usize {
        usize::from(self.0) % len
    }

    fn get<T>(self, items: &[T]) -> &T {
        &items[self.index(items.len())]
    }

    /// The n-th newest item (or the oldest), so dropping a step that ran before the item was
    /// created still picks the same document or entry while a failure is shrunk.
    fn newest<T>(self, items: &[T]) -> &T {
        &items[items.len() - 1 - usize::from(self.0).min(items.len() - 1)]
    }
}

fn pick() -> impl Strategy<Value = Pick> {
    prop_oneof![3 => 0u8..4, 1 => any::<u8>()].prop_map(Pick)
}

fn cents() -> impl Strategy<Value = i64> {
    prop_oneof![3 => 1i64..=100_000, 1 => 1i64..=10_000_000_000, 1 => Just(1i64)]
}

/// Mostly ordinary rates; sometimes a few cents, where foreign-currency rounding shows.
fn line_rate() -> impl Strategy<Value = i64> {
    prop_oneof![4 => 100i64..=200_000, 1 => 4i64..=40]
}

fn percent() -> impl Strategy<Value = u8> {
    prop_oneof![Just(100u8), 1u8..=99]
}

fn fx_rate() -> impl Strategy<Value = i64> {
    500_000i64..=2_000_000
}

fn op() -> impl Strategy<Value = Op> {
    let day = 0..SPAN;
    prop_oneof![
        3 => (day.clone(), pick(), pick(), cents())
            .prop_map(|(day, debit, credit, cents)| Op::Journal { day, debit, credit, cents }),
        3 => (day.clone(), 0u16..60, prop::collection::vec((1i64..=20, line_rate()), 1..4), pick(), 0u8..3, prop::option::weighted(0.3, fx_rate()))
            .prop_map(|(day, due_in, lines, income, customer, fx_rate)| Op::Invoice { day, due_in, lines, income, customer, fx_rate }),
        3 => (pick(), percent(), 0u16..90, any::<bool>(), fx_rate())
            .prop_map(|(invoice, percent, after, undeposited, fx_rate)| Op::ReceivePayment { invoice, percent, after, undeposited, fx_rate }),
        3 => (day.clone(), 0u16..60, prop::collection::vec((pick(), cents()), 1..4), 0u8..3)
            .prop_map(|(day, due_in, lines, vendor)| Op::Bill { day, due_in, lines, vendor }),
        2 => (pick(), percent(), 0u16..90, pick())
            .prop_map(|(bill, percent, after, from)| Op::PayBill { bill, percent, after, from }),
        2 => (day.clone(), prop::collection::vec((pick(), cents()), 1..3), pick(), any::<bool>())
            .prop_map(|(day, lines, from, check)| Op::Expense { day, lines, from, check }),
        2 => (day.clone(), any::<bool>(), prop::option::of((pick(), cents())), pick())
            .prop_map(|(day, items, other, to)| Op::Deposit { day, items, other, to }),
        2 => (day.clone(), pick(), pick(), cents())
            .prop_map(|(day, from, to, cents)| Op::Transfer { day, from, to, cents }),
        3 => pick().prop_map(|entry| Op::Void { entry }),
        2 => (pick(), cents(), -30i16..30).prop_map(|(entry, cents, shift)| Op::Replace { entry, cents, shift }),
        1 => prop::option::of(day).prop_map(|day| Op::SetClosing { day }),
    ]
}

// ------------------------------------------------------------------------------------------------
// Accounts the operations use
// ------------------------------------------------------------------------------------------------

struct Accounts {
    ar: i64,
    ap: i64,
    undeposited: i64,
    fx_gain_loss: i64,
    banks: Vec<i64>,
    /// Bank and credit card accounts.
    pay_from: Vec<i64>,
    income: Vec<i64>,
    /// Bill and expense categories (expenses and a fixed asset).
    categories: Vec<i64>,
    /// Manual journal entries touch anything except A/R, A/P and Undeposited Funds, whose balances
    /// are owned by documents.
    journal: Vec<i64>,
    deposit_other: Vec<i64>,
    /// account id -> (classification, account type)
    kinds: BTreeMap<i64, (String, String)>,
}

impl Accounts {
    async fn load(pool: &SqlitePool) -> Self {
        let mut ids = BTreeMap::new();
        for number in [
            "1010", "1020", "1500", "2100", "2200", "2500", "3100", "3200", "3900", "4000", "4100", "4900", "6000", "6250", "6300",
            "6400", "7000", "8000",
        ] {
            ids.insert(number, account(pool, number).await);
        }
        let pick = |numbers: &[&str]| numbers.iter().map(|n| ids[n]).collect::<Vec<_>>();
        let kinds = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT a.id, t.classification, a.account_type FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type",
        )
        .fetch_all(pool)
        .await
        .expect("account kinds")
        .into_iter()
        .map(|(id, class, kind)| (id, (class, kind)))
        .collect();
        Accounts {
            ar: system_account(pool, "ACCOUNTS_RECEIVABLE").await,
            ap: system_account(pool, "ACCOUNTS_PAYABLE").await,
            undeposited: system_account(pool, "OTHER_CURRENT_ASSET").await,
            fx_gain_loss: system_account(pool, "OTHER_EXPENSE").await,
            banks: pick(&["1010", "1020"]),
            pay_from: pick(&["1010", "1020", "2100"]),
            income: pick(&["4000", "4100"]),
            categories: pick(&["6000", "6250", "6300", "6400", "1500"]),
            journal: pick(&["1010", "1020", "1500", "2100", "2200", "2500", "3100", "3200", "3900", "4000", "4100", "6000", "6300", "7000", "8000"]),
            deposit_other: pick(&["4000", "4900", "2500", "3100", "7000"]),
            kinds,
        }
    }

    fn of(&self, id: i64) -> (&str, &str) {
        let (class, kind) = &self.kinds[&id];
        (class, kind)
    }
}

// ------------------------------------------------------------------------------------------------
// The model
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Journal,
    Invoice(i64),
    Payment(i64),
    Bill(i64),
    BillPayment(i64),
    Expense(i64),
    Deposit(i64),
    Transfer,
}

/// A posted entry that is not voided, with what it did to each account (debit positive).
#[derive(Debug, Clone)]
struct Live {
    day: u16,
    kind: Kind,
    deltas: Vec<(i64, i64)>,
}

#[derive(Debug, Clone)]
struct InvoiceM {
    day: u16,
    customer: String,
    income: i64,
    fx_rate: Option<i64>,
    total: i64,
    home_total: i64,
    paid: i64,
    relieved: i64,
    voided: bool,
}

#[derive(Debug, Clone)]
struct PaymentM {
    invoice: i64,
    amount: i64,
    relief: i64,
    home: i64,
    undeposited: bool,
    deposit: Option<i64>,
    voided: bool,
}

#[derive(Debug, Clone)]
struct BillM {
    day: u16,
    vendor: String,
    total: i64,
    paid: i64,
    voided: bool,
}

/// An entry as posted: date, lines (account, debit, credit) and the entry it reverses.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Snap {
    date: String,
    status: String,
    reverses: Option<i64>,
    is_voided: bool,
    lines: Vec<(i64, i64, i64)>,
}

#[derive(Default)]
struct Model {
    balances: BTreeMap<i64, i64>,
    live: BTreeMap<i64, Live>,
    invoices: BTreeMap<i64, InvoiceM>,
    payments: BTreeMap<i64, PaymentM>,
    bills: BTreeMap<i64, BillM>,
    bill_payments: BTreeMap<i64, (i64, i64)>,
    /// expense id -> (payment account, is a check)
    expenses: BTreeMap<i64, (i64, bool)>,
    deposits: BTreeMap<i64, Vec<i64>>,
    lock: Option<NaiveDate>,
    /// Every entry ever posted, as it was when first seen.
    posted: BTreeMap<i64, Snap>,
}

impl Model {
    fn closed(&self, day: u16) -> bool {
        self.lock.is_some_and(|lock| date(day) <= lock)
    }

    fn post(&mut self, entry: i64, day: u16, kind: Kind, deltas: Vec<(i64, i64)>) {
        for (account, delta) in &deltas {
            *self.balances.entry(*account).or_insert(0) += delta;
        }
        self.live.insert(entry, Live { day, kind, deltas });
    }

    /// Why voiding `entry` must be refused, as the error code the ledger has to return.
    fn void_blocker(&self, entry: i64) -> Option<&'static str> {
        let live = &self.live[&entry];
        match live.kind {
            Kind::Invoice(id) if self.invoices[&id].paid > 0 => Some("VOID_HAS_PAYMENTS"),
            Kind::Bill(id) if self.bills[&id].paid > 0 => Some("VOID_HAS_PAYMENTS"),
            Kind::Payment(id) if self.payments[&id].deposit.is_some() => Some("VOID_DEPOSITED"),
            _ if self.closed(live.day) => Some("PERIOD_CLOSED"),
            _ => None,
        }
    }

    fn void(&mut self, entry: i64) {
        let live = self.live.remove(&entry).expect("live entry");
        for (account, delta) in &live.deltas {
            *self.balances.entry(*account).or_insert(0) -= delta;
        }
        match live.kind {
            Kind::Invoice(id) => self.invoices.get_mut(&id).unwrap().voided = true,
            Kind::Bill(id) => self.bills.get_mut(&id).unwrap().voided = true,
            Kind::Payment(id) => {
                let payment = self.payments.get_mut(&id).unwrap();
                payment.voided = true;
                let invoice = self.invoices.get_mut(&payment.invoice).unwrap();
                invoice.paid -= payment.amount;
                invoice.relieved -= payment.relief;
            }
            Kind::BillPayment(id) => {
                let (bill, amount) = self.bill_payments[&id];
                self.bills.get_mut(&bill).unwrap().paid -= amount;
            }
            Kind::Deposit(id) => {
                for payment in &self.deposits[&id] {
                    self.payments.get_mut(payment).unwrap().deposit = None;
                }
            }
            Kind::Journal | Kind::Expense(_) | Kind::Transfer => {}
        }
    }

    fn open_invoices(&self) -> Vec<i64> {
        self.invoices.iter().filter(|(_, i)| !i.voided && i.paid < i.total).map(|(id, _)| *id).collect()
    }

    fn open_bills(&self) -> Vec<i64> {
        self.bills.iter().filter(|(_, b)| !b.voided && b.paid < b.total).map(|(id, _)| *id).collect()
    }

    fn waiting_for_deposit(&self) -> Vec<i64> {
        self.payments.iter().filter(|(_, p)| !p.voided && p.undeposited && p.deposit.is_none()).map(|(id, _)| *id).collect()
    }
}

/// `amount × percent / 100`, at least one cent; 100% is the whole amount.
fn share(amount: i64, percent: u8) -> i64 {
    if percent >= 100 {
        amount
    } else {
        (amount * i64::from(percent) / 100).max(1)
    }
}

fn shifted(day: u16, shift: i16) -> u16 {
    (i32::from(day) + i32::from(shift)).clamp(0, i32::from(SPAN) - 1) as u16
}

/// The outcome a step must have: `Ok` or an error starting with the given code.
fn expect(result: Result<(), String>, expected: Option<&str>) -> Result<bool, String> {
    match (result, expected) {
        (Ok(()), None) => Ok(true),
        (Ok(()), Some(code)) => Err(format!("succeeded but must be refused with {code}")),
        (Err(err), Some(code)) if err.starts_with(code) => Ok(false),
        (Err(err), Some(code)) => Err(format!("refused with '{err}' but expected {code}")),
        (Err(err), None) => Err(format!("refused unexpectedly: {err}")),
    }
}

fn closed_code(closed: bool) -> Option<&'static str> {
    closed.then_some("PERIOD_CLOSED")
}

async fn scalar(pool: &SqlitePool, sql: &str, id: i64) -> Result<i64, String> {
    sqlx::query_scalar(sql).bind(id).fetch_one(pool).await.map_err(|e| format!("{sql}: {e}"))
}

// ------------------------------------------------------------------------------------------------
// Applying an operation to the ledger and the model
// ------------------------------------------------------------------------------------------------

async fn apply(pool: &SqlitePool, a: &Accounts, m: &mut Model, op: &Op) -> Result<(), String> {
    match op {
        Op::Journal { day, debit, credit, cents } => {
            let (debit, credit) = (*debit.get(&a.journal), *credit.get(&a.journal));
            post_journal(pool, m, *day, debit, credit, *cents, None).await
        }
        Op::Invoice { day, due_in, lines, income, customer, fx_rate } => {
            let lines: Vec<(i64, i64)> = lines.iter().map(|(quarters, rate)| (quarters * 2_500, *rate)).collect();
            let customer = format!("Customer {customer}");
            post_invoice(pool, a, m, *day, *due_in, &lines, *income.get(&a.income), &customer, *fx_rate, None).await
        }
        Op::ReceivePayment { invoice, percent, after, undeposited, fx_rate } => {
            let open = m.open_invoices();
            if open.is_empty() {
                return Ok(());
            }
            let invoice_id = *invoice.newest(&open);
            let inv = m.invoices[&invoice_id].clone();
            let amount = share(inv.total - inv.paid, *percent);
            let day = shifted(inv.day, *after as i16);
            let deposit_account = if *undeposited { a.undeposited } else { a.banks[0] };
            let rate = inv.fx_rate.map(|_| *fx_rate);
            let result = crate::invoices::record_payment(
                pool,
                PaymentInput {
                    invoice_id,
                    deposit_account_id: deposit_account,
                    payment_date: date(day),
                    amount_cents: amount,
                    payment_method: "CHECK".into(),
                    reference: None,
                    exchange_rate_e6: rate,
                },
            )
            .await
            .map(|_| ());
            if !expect(result, closed_code(m.closed(day)))? {
                return Ok(());
            }
            let payment_id = scalar(pool, "SELECT MAX(id) FROM invoice_payments WHERE invoice_id = ?1", invoice_id).await?;
            let entry = scalar(pool, "SELECT journal_entry_id FROM invoice_payments WHERE id = ?1", payment_id).await?;
            // Home currency: cash received at the payment's rate; A/R relieved in proportion to the
            // amount paid so far at the invoice's rate (the last payment clears it exactly); the
            // difference is a realized exchange gain or loss.
            let new_paid = inv.paid + amount;
            let (home, relief) = match rate {
                None => (amount, amount),
                Some(rate) => {
                    let home = to_home_cents(amount, rate).unwrap();
                    // Never backwards, never past the home total (voided payments can leave the A/R
                    // relieved so far a little off the proportional amount).
                    let relieved_after = if new_paid == inv.total {
                        inv.home_total
                    } else {
                        mul_div_round(inv.home_total, new_paid, inv.total).unwrap().clamp(inv.relieved, inv.home_total)
                    };
                    (home, relieved_after - inv.relieved)
                }
            };
            let mut deltas = vec![(deposit_account, home), (a.ar, -relief)];
            if home != relief {
                deltas.push((a.fx_gain_loss, relief - home));
            }
            m.post(entry, day, Kind::Payment(payment_id), deltas);
            let invoice = m.invoices.get_mut(&invoice_id).unwrap();
            invoice.paid = new_paid;
            invoice.relieved += relief;
            m.payments.insert(
                payment_id,
                PaymentM { invoice: invoice_id, amount, relief, home, undeposited: *undeposited, deposit: None, voided: false },
            );
            Ok(())
        }
        Op::Bill { day, due_in, lines, vendor } => {
            let lines: Vec<(i64, i64)> = lines.iter().map(|(account, cents)| (*account.get(&a.categories), *cents)).collect();
            post_bill(pool, a, m, *day, *due_in, &lines, &format!("Vendor {vendor}"), None).await
        }
        Op::PayBill { bill, percent, after, from } => {
            let open = m.open_bills();
            if open.is_empty() {
                return Ok(());
            }
            let bill_id = *bill.newest(&open);
            let b = m.bills[&bill_id].clone();
            let amount = share(b.total - b.paid, *percent);
            let day = shifted(b.day, *after as i16);
            let from = *from.get(&a.pay_from);
            let result = crate::expenses::pay_vendor_bill(
                pool,
                BillPaymentInput { bill_id, payment_account_id: from, payment_date: date(day), amount_cents: amount, reference: None },
            )
            .await
            .map(|_| ());
            if !expect(result, closed_code(m.closed(day)))? {
                return Ok(());
            }
            let payment_id = scalar(pool, "SELECT MAX(id) FROM bill_payments WHERE bill_id = ?1", bill_id).await?;
            let entry = scalar(pool, "SELECT journal_entry_id FROM bill_payments WHERE id = ?1", payment_id).await?;
            m.post(entry, day, Kind::BillPayment(payment_id), vec![(a.ap, amount), (from, -amount)]);
            m.bills.get_mut(&bill_id).unwrap().paid += amount;
            m.bill_payments.insert(payment_id, (bill_id, amount));
            Ok(())
        }
        Op::Expense { day, lines, from, check } => {
            let from = *from.get(&a.pay_from);
            let check = *check && a.banks.contains(&from);
            let lines: Vec<(i64, i64)> = lines.iter().map(|(account, cents)| (*account.get(&a.categories), *cents)).collect();
            post_expense(pool, m, *day, from, check, &lines, None).await
        }
        Op::Deposit { day, items, other, to } => {
            let payments = if *items { m.waiting_for_deposit() } else { Vec::new() };
            let other = match other {
                Some((account, cents)) => Some((*account.get(&a.deposit_other), *cents)),
                None if payments.is_empty() => Some((a.deposit_other[0], 1)),
                None => None,
            };
            let to = *to.get(&a.banks);
            let result = crate::banking::post_bank_deposit(
                pool,
                DepositInput {
                    bank_account_id: to,
                    deposit_date: date(*day),
                    memo: None,
                    items: payments.iter().map(|id| DepositItemRef { kind: "PAYMENT".into(), id: *id }).collect(),
                    other_lines: other
                        .iter()
                        .map(|(account_id, amount_cents)| DepositOtherLine { account_id: *account_id, amount_cents: *amount_cents, memo: None, class_id: None })
                        .collect(),
                },
            )
            .await;
            let deposit_id = match result {
                Ok(id) => {
                    expect(Ok(()), closed_code(m.closed(*day)))?;
                    id
                }
                Err(err) => {
                    expect(Err(err), closed_code(m.closed(*day)))?;
                    return Ok(());
                }
            };
            let entry = scalar(pool, "SELECT journal_entry_id FROM deposits WHERE id = ?1", deposit_id).await?;
            let items: i64 = payments.iter().map(|p| m.payments[p].home).sum();
            let other_cents = other.map_or(0, |o| o.1);
            let mut deltas = vec![(to, items + other_cents)];
            if items != 0 {
                deltas.push((a.undeposited, -items));
            }
            if let Some((account, cents)) = other {
                deltas.push((account, -cents));
            }
            m.post(entry, *day, Kind::Deposit(deposit_id), deltas);
            for payment in &payments {
                m.payments.get_mut(payment).unwrap().deposit = Some(deposit_id);
            }
            m.deposits.insert(deposit_id, payments);
            Ok(())
        }
        Op::Transfer { day, from, to, cents } => {
            let from_index = from.index(a.pay_from.len());
            let mut to_index = to.index(a.pay_from.len());
            if to_index == from_index {
                to_index = (to_index + 1) % a.pay_from.len();
            }
            let (from, to) = (a.pay_from[from_index], a.pay_from[to_index]);
            let result = crate::banking::post_transfer(
                pool,
                TransferInput { from_account_id: from, to_account_id: to, transfer_date: date(*day), amount_cents: *cents, memo: None },
            )
            .await;
            let transfer_id = match result {
                Ok(id) => {
                    expect(Ok(()), closed_code(m.closed(*day)))?;
                    id
                }
                Err(err) => {
                    expect(Err(err), closed_code(m.closed(*day)))?;
                    return Ok(());
                }
            };
            let entry = scalar(pool, "SELECT journal_entry_id FROM transfers WHERE id = ?1", transfer_id).await?;
            m.post(entry, *day, Kind::Transfer, vec![(to, *cents), (from, -*cents)]);
            Ok(())
        }
        Op::Void { entry } => {
            let live: Vec<i64> = m.live.keys().copied().collect();
            if live.is_empty() {
                return Ok(());
            }
            let entry = *entry.newest(&live);
            let result = crate::ledger::void_transaction(pool, entry).await;
            if expect(result, m.void_blocker(entry))? {
                m.void(entry);
            }
            Ok(())
        }
        Op::Replace { entry, cents, shift } => {
            let editable: Vec<i64> = m
                .live
                .iter()
                .filter(|(_, l)| matches!(l.kind, Kind::Journal | Kind::Invoice(_) | Kind::Bill(_) | Kind::Expense(_)))
                .map(|(id, _)| *id)
                .collect();
            if editable.is_empty() {
                return Ok(());
            }
            let old = *entry.newest(&editable);
            let live = m.live[&old].clone();
            let day = shifted(live.day, *shift);
            // The original is voided first (its own rules apply), then the edit posts on its new date.
            let expected = m.void_blocker(old).or(closed_code(m.closed(day)));
            match live.kind {
                Kind::Journal => {
                    let (debit, credit) = (live.deltas[0].0, live.deltas[1].0);
                    post_journal(pool, m, day, debit, credit, *cents, Some((old, expected))).await
                }
                Kind::Invoice(id) => {
                    let inv = m.invoices[&id].clone();
                    // One line of quantity 1 at the new amount (scaled down to keep FX totals in range).
                    post_invoice(pool, a, m, day, 0, &[(10_000, *cents)], inv.income, &inv.customer, inv.fx_rate, Some((old, expected))).await
                }
                Kind::Bill(id) => {
                    let vendor = m.bills[&id].vendor.clone();
                    post_bill(pool, a, m, day, 0, &[(live.deltas[0].0, *cents)], &vendor, Some((old, expected))).await
                }
                Kind::Expense(id) => {
                    let (from, check) = m.expenses[&id];
                    post_expense(pool, m, day, from, check, &[(live.deltas[0].0, *cents)], Some((old, expected))).await
                }
                _ => unreachable!("only editable kinds are picked"),
            }
        }
        Op::SetClosing { day } => {
            let lock = day.map(date);
            let result = crate::closing::set_closing_date(pool, &ClosingInput { lock_date: lock, current_passcode: None, new_passcode: None })
                .await
                .map(|_| ());
            if expect(result, (lock == m.lock).then_some("CLOSE_UNCHANGED"))? {
                m.lock = lock;
            }
            Ok(())
        }
    }
}

/// `replacing`: the entry being edited and the outcome the edit must have.
type Replacing<'a> = Option<(i64, Option<&'a str>)>;

/// Applies a document post or an edit's outcome: `Ok(None)` when the ledger refused it as expected.
fn outcome<T>(m: &Model, day: u16, replacing: Replacing<'_>, result: Result<T, String>) -> Result<Option<T>, String> {
    let expected = match replacing {
        Some((_, expected)) => expected,
        None => closed_code(m.closed(day)),
    };
    match result {
        Ok(value) => expect(Ok(()), expected).map(|_| Some(value)),
        Err(err) => expect(Err(err), expected).map(|_| None),
    }
}

async fn post_journal(pool: &SqlitePool, m: &mut Model, day: u16, debit: i64, credit: i64, cents: i64, replacing: Replacing<'_>) -> Result<(), String> {
    let entry = JournalEntry {
        id: None,
        date: date(day),
        description: "Property test entry".into(),
        lines: vec![JournalLine::debit(debit, cents), JournalLine::credit(credit, cents)],
        currency: None,
    };
    let result = match replacing {
        Some((old, _)) => crate::edits::replace_journal_entry(pool, old, entry).await,
        None => crate::ledger::post_journal_entry(pool, entry).await,
    };
    let Some(id) = outcome(m, day, replacing, result)? else { return Ok(()) };
    if let Some((old, _)) = replacing {
        m.void(old);
    }
    m.post(id, day, Kind::Journal, vec![(debit, cents), (credit, -cents)]);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn post_invoice(
    pool: &SqlitePool,
    a: &Accounts,
    m: &mut Model,
    day: u16,
    due_in: u16,
    lines: &[(i64, i64)],
    income: i64,
    customer: &str,
    fx_rate: Option<i64>,
    replacing: Replacing<'_>,
) -> Result<(), String> {
    let input = InvoiceInput {
        customer_name: customer.into(),
        issue_date: date(day),
        due_date: date(day) + Days::new(u64::from(due_in)),
        lines: lines
            .iter()
            .map(|(quantity_e4, rate_cents)| InvoiceLineInput {
                description: "Work".into(),
                quantity_e4: *quantity_e4,
                rate_cents: *rate_cents,
                income_account_id: income,
                product_service_id: None,
                class_id: None,
                is_taxable: false,
            })
            .collect(),
        currency: fx_rate.map(|rate| ForeignCurrency { code: "EUR".into(), exchange_rate_e6: rate }),
        tax_rate_id: None,
        discount_cents: 0,
    };
    let result = match replacing {
        Some((old, _)) => crate::edits::replace_invoice(pool, live_doc(m, old), input).await,
        None => crate::invoices::create_and_post_invoice(pool, input).await.map(|s| s.id),
    };
    let Some(invoice_id) = outcome(m, day, replacing, result)? else { return Ok(()) };
    if let Some((old, _)) = replacing {
        m.void(old);
    }
    let total: i64 = lines.iter().map(|(q, r)| crate::invoices::line_amount(*q, *r).unwrap()).sum();
    // One income line, so the converted entry needs no rounding adjustment: both sides convert alike.
    let home_total = fx_rate.map_or(total, |rate| to_home_cents(total, rate).unwrap());
    let entry = scalar(pool, "SELECT journal_entry_id FROM invoices WHERE id = ?1", invoice_id).await?;
    m.post(entry, day, Kind::Invoice(invoice_id), vec![(a.ar, home_total), (income, -home_total)]);
    m.invoices.insert(
        invoice_id,
        InvoiceM { day, customer: customer.into(), income, fx_rate, total, home_total, paid: 0, relieved: 0, voided: false },
    );
    Ok(())
}

/// The document id behind a live entry.
fn live_doc(m: &Model, entry: i64) -> i64 {
    match m.live[&entry].kind {
        Kind::Invoice(id) | Kind::Bill(id) | Kind::Expense(id) => id,
        other => unreachable!("not an editable document: {other:?}"),
    }
}

#[allow(clippy::too_many_arguments)]
async fn post_bill(
    pool: &SqlitePool,
    a: &Accounts,
    m: &mut Model,
    day: u16,
    due_in: u16,
    lines: &[(i64, i64)],
    vendor: &str,
    replacing: Replacing<'_>,
) -> Result<(), String> {
    let total: i64 = lines.iter().map(|l| l.1).sum();
    let input = BillInput {
        vendor_name: vendor.into(),
        bill_number: None,
        bill_date: date(day),
        due_date: date(day) + Days::new(u64::from(due_in)),
        memo: None,
        total_cents: total,
        lines: lines
            .iter()
            .map(|(account_id, amount_cents)| BillLineInput {
                account_id: *account_id,
                amount_cents: *amount_cents,
                memo: None,
                quantity_e4: None,
                rate_cents: None,
                product_service_id: None,
                class_id: None,
            })
            .collect(),
    };
    let result = match replacing {
        Some((old, _)) => crate::edits::replace_bill(pool, live_doc(m, old), input).await,
        None => crate::expenses::post_vendor_bill(pool, input).await.map(|b| b.id),
    };
    let Some(bill_id) = outcome(m, day, replacing, result)? else { return Ok(()) };
    if let Some((old, _)) = replacing {
        m.void(old);
    }
    let entry = scalar(pool, "SELECT journal_entry_id FROM bills WHERE id = ?1", bill_id).await?;
    let mut deltas: Vec<(i64, i64)> = lines.to_vec();
    deltas.push((a.ap, -total));
    m.post(entry, day, Kind::Bill(bill_id), deltas);
    m.bills.insert(bill_id, BillM { day, vendor: vendor.into(), total, paid: 0, voided: false });
    Ok(())
}

async fn post_expense(
    pool: &SqlitePool,
    m: &mut Model,
    day: u16,
    from: i64,
    check: bool,
    lines: &[(i64, i64)],
    replacing: Replacing<'_>,
) -> Result<(), String> {
    let total: i64 = lines.iter().map(|l| l.1).sum();
    let input = ExpenseInput {
        kind: if check { "CHECK" } else { "EXPENSE" }.into(),
        payee_name: Some("Payee".into()),
        payment_account_id: from,
        expense_date: date(day),
        check_number: check.then(|| "101".into()),
        payment_method: None,
        memo: None,
        lines: lines
            .iter()
            .map(|(account_id, amount_cents)| SplitLineInput { account_id: *account_id, amount_cents: *amount_cents, memo: None, class_id: None })
            .collect(),
        currency: None,
    };
    let result = match replacing {
        Some((old, _)) => crate::edits::replace_expense(pool, live_doc(m, old), input).await,
        None => crate::expenses::post_expense(pool, input).await.map(|x| x.id),
    };
    let Some(expense_id) = outcome(m, day, replacing, result)? else { return Ok(()) };
    if let Some((old, _)) = replacing {
        m.void(old);
    }
    let entry = scalar(pool, "SELECT journal_entry_id FROM expenses WHERE id = ?1", expense_id).await?;
    let mut deltas: Vec<(i64, i64)> = lines.to_vec();
    deltas.push((from, -total));
    m.post(entry, day, Kind::Expense(expense_id), deltas);
    m.expenses.insert(expense_id, (from, check));
    Ok(())
}

// ------------------------------------------------------------------------------------------------
// Invariants
// ------------------------------------------------------------------------------------------------

macro_rules! ensure {
    ($cond:expr, $($msg:tt)+) => {
        if !$cond {
            return Err(format!($($msg)+));
        }
    };
}

macro_rules! ensure_eq {
    ($left:expr, $right:expr, $($msg:tt)+) => {{
        let (left, right) = (&$left, &$right);
        if left != right {
            return Err(format!("{}: {:?} != {:?}", format!($($msg)+), left, right));
        }
    }};
}

async fn snapshot(pool: &SqlitePool) -> Result<BTreeMap<i64, Snap>, String> {
    let headers: Vec<(i64, String, String, Option<i64>, bool)> =
        sqlx::query_as("SELECT id, entry_date, status, reverses_entry_id, is_voided FROM journal_entries ORDER BY id")
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
    let lines: Vec<(i64, i64, i64, i64)> =
        sqlx::query_as("SELECT journal_entry_id, account_id, debit_cents, credit_cents FROM journal_lines ORDER BY journal_entry_id, line_number")
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
    let mut snaps: BTreeMap<i64, Snap> = headers
        .into_iter()
        .map(|(id, date, status, reverses, is_voided)| (id, Snap { date, status, reverses, is_voided, lines: Vec::new() }))
        .collect();
    for (entry, account, debit, credit) in lines {
        snaps.get_mut(&entry).ok_or("line without an entry")?.lines.push((account, debit, credit));
    }
    Ok(snaps)
}

/// Checks every invariant. The statements are checked at year end of the last year and at
/// `dates` (each date costs a balance sheet, two P&Ls and a trial balance).
async fn check(pool: &SqlitePool, a: &Accounts, m: &mut Model, lock_before: Option<NaiveDate>, dates: &[&str]) -> Result<(), String> {
    // Amounts are stored as integers: a REAL anywhere would mean cents can drift.
    let non_integer: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM journal_lines WHERE typeof(debit_cents) <> 'integer' OR typeof(credit_cents) <> 'integer'",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    ensure_eq!(non_integer, 0, "journal lines with non-integer amounts");

    // Posted entries never change, never disappear; new ones respect the closing date; each balances.
    let snaps = snapshot(pool).await?;
    for (id, before) in &m.posted {
        let now = snaps.get(id).ok_or_else(|| format!("entry {id} disappeared"))?;
        let unchanged = Snap { is_voided: before.is_voided, ..now.clone() };
        ensure_eq!(unchanged, *before, "entry {id} changed after posting");
        ensure!(!before.is_voided || now.is_voided, "entry {id} was un-voided");
    }
    for (id, snap) in &snaps {
        ensure_eq!(snap.status, "POSTED", "entry {id} status");
        let debits: i64 = snap.lines.iter().map(|l| l.1).sum();
        let credits: i64 = snap.lines.iter().map(|l| l.2).sum();
        ensure_eq!(debits, credits, "entry {id} does not balance");
        if !m.posted.contains_key(id) {
            if let Some(lock) = lock_before {
                ensure!(snap.date.as_str() > lock.to_string().as_str(), "entry {id} posted on {} with the books closed through {lock}", snap.date);
            }
        }
    }

    // A void reverses its entry exactly: same date, same accounts, sides swapped, line by line.
    for (id, snap) in &snaps {
        let reversals: Vec<(&i64, &Snap)> = snaps.iter().filter(|(_, s)| s.reverses == Some(*id)).collect();
        if snap.is_voided {
            ensure_eq!(reversals.len(), 1, "reversals of voided entry {id}");
            let (rid, reversal) = reversals[0];
            ensure_eq!(reversal.date, snap.date, "date of reversal {rid} of entry {id}");
            let swapped: Vec<(i64, i64, i64)> = snap.lines.iter().map(|(acct, d, c)| (*acct, *c, *d)).collect();
            ensure_eq!(reversal.lines, swapped, "lines of reversal {rid} of entry {id}");
        } else {
            ensure!(reversals.is_empty(), "entry {id} has a reversal but is not voided");
        }
        let is_live = !snap.is_voided && snap.reverses.is_none();
        ensure_eq!(is_live, m.live.contains_key(id), "entry {id} live in the ledger vs the model");
    }
    for (id, snap) in snaps {
        m.posted.entry(id).or_insert(snap);
    }

    // The audit trail is intact.
    let audit = crate::ledger::verify_ledger_audit(pool).await?;
    ensure!(audit.intact, "ledger audit findings: {:?}", audit.findings);
    let chain = crate::audit_chain::verify(pool).await?;
    ensure!(chain.problems.is_empty(), "audit chain problems: {:?}", chain.problems);

    // Every account holds exactly what the model says, to the cent.
    let actual: BTreeMap<i64, i64> = sqlx::query_as::<_, (i64, i64)>(
        "SELECT l.account_id, SUM(l.debit_cents - l.credit_cents) FROM journal_lines l
         JOIN journal_entries e ON e.id = l.journal_entry_id WHERE e.status = 'POSTED'
         GROUP BY l.account_id HAVING SUM(l.debit_cents - l.credit_cents) <> 0",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?
    .into_iter()
    .collect();
    let expected: BTreeMap<i64, i64> = m.balances.iter().filter(|(_, b)| **b != 0).map(|(k, v)| (*k, *v)).collect();
    ensure_eq!(actual, expected, "account balances (debit positive)");

    let tb = crate::reports::trial_balance(pool).await?;
    ensure_eq!(tb.total_debit_cents, tb.total_credit_cents, "trial balance totals");
    ensure_eq!(tb.total_debit_cents, expected.values().filter(|b| **b > 0).sum::<i64>(), "trial balance debits");

    // Statements at several dates, across the fiscal year boundary.
    let sum_of = |class: &str| -> i64 { expected.iter().filter(|(id, _)| a.of(**id).0 == class).map(|(_, b)| *b).sum() };
    // The far horizon comes from `far_end()`, the same date the model-vs-report checks below key on.
    let as_of_dates = dates.iter().map(|d| d.parse::<NaiveDate>().unwrap()).chain([far_end()]);
    for as_of in as_of_dates {
        let bs = crate::statements::balance_sheet(pool, as_of).await?;
        ensure!(bs.is_balanced && bs.difference_cents == 0, "balance sheet {as_of} out of balance by {}", bs.difference_cents);
        ensure_eq!(bs.total_assets_cents, bs.total_liabilities_cents + bs.total_equity_cents, "assets = liabilities + equity at {as_of}");
        let year = crate::statements::profit_and_loss(pool, bs.fiscal_year_start, as_of).await?;
        ensure_eq!(year.net_income_cents, bs.net_income_cents, "P&L for the fiscal year vs balance sheet net income at {as_of}");
        let to_date = crate::statements::profit_and_loss(pool, epoch(), as_of).await?;
        let equity_accounts: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(l.credit_cents - l.debit_cents), 0) FROM journal_lines l
             JOIN journal_entries e ON e.id = l.journal_entry_id
             JOIN chart_of_accounts c ON c.id = l.account_id JOIN account_types t ON t.code = c.account_type
             WHERE e.status = 'POSTED' AND t.classification = 'EQUITY' AND e.entry_date <= ?1",
        )
        .bind(as_of)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
        ensure_eq!(bs.total_equity_cents, equity_accounts + to_date.net_income_cents, "equity = equity accounts + profit to date at {as_of}");
        let tb = crate::statements::trial_balance_as_of(pool, as_of).await?;
        ensure_eq!(tb.total_debit_cents, tb.total_credit_cents, "trial balance as of {as_of}");
        if as_of == far_end() {
            ensure_eq!(bs.total_assets_cents, sum_of("ASSET"), "total assets vs the model");
            ensure_eq!(bs.total_liabilities_cents, -sum_of("LIABILITY"), "total liabilities vs the model");
        }
    }
    let cash: i64 = expected.iter().filter(|(id, _)| a.of(**id).1 == "BANK").map(|(_, b)| *b).sum();
    let flow = crate::statements::cash_flow(pool, epoch(), far_end()).await?;
    ensure_eq!((flow.cash_start_cents, flow.net_change_cents, flow.cash_end_cents), (0, cash, cash), "cash flow vs the bank balances");

    // A/R: the ledger, the invoices and the aging report agree on what is still owed.
    let open_ar: i64 = m.invoices.values().filter(|i| !i.voided).map(|i| i.home_total - i.relieved).sum();
    ensure_eq!(m.balances.get(&a.ar).copied().unwrap_or(0), open_ar, "A/R in the model vs its open invoices");
    let ar_aging = crate::report_center::aging(pool, false, far_end()).await?;
    ensure_eq!(ar_aging.total_cents, open_ar, "A/R aging total");
    let view_ar: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(b.home_open_cents), 0) FROM v_invoice_balances b JOIN invoices i ON i.id = b.invoice_id WHERE i.is_voided = 0",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    ensure_eq!(view_ar, open_ar, "open invoice balances");
    ensure_eq!(scalar(pool, "SELECT COUNT(*) FROM invoices WHERE ?1 = ?1", 0).await?, m.invoices.len() as i64, "invoice rows");
    for (id, inv) in &m.invoices {
        let (paid, status, voided): (i64, String, bool) =
            sqlx::query_as("SELECT amount_paid_cents, status, is_voided FROM invoices WHERE id = ?1")
                .bind(id)
                .fetch_one(pool)
                .await
                .map_err(|e| e.to_string())?;
        ensure_eq!((paid, voided), (inv.paid, inv.voided), "invoice {id} paid and voided");
        if !inv.voided {
            let expected = if inv.paid == 0 { "UNPAID" } else if inv.paid < inv.total { "PARTIAL" } else { "PAID" };
            ensure_eq!(status, expected, "invoice {id} status");
        }
    }

    // A/P likewise.
    let open_ap: i64 = m.bills.values().filter(|b| !b.voided).map(|b| b.total - b.paid).sum();
    ensure_eq!(-m.balances.get(&a.ap).copied().unwrap_or(0), open_ap, "A/P in the model vs its open bills");
    let ap_aging = crate::report_center::aging(pool, true, far_end()).await?;
    ensure_eq!(ap_aging.total_cents, open_ap, "A/P aging total");
    ensure_eq!(scalar(pool, "SELECT COUNT(*) FROM bills WHERE ?1 = ?1", 0).await?, m.bills.len() as i64, "bill rows");
    for (id, bill) in &m.bills {
        let (paid, status, voided): (i64, String, bool) = sqlx::query_as("SELECT amount_paid_cents, status, is_voided FROM bills WHERE id = ?1")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
        ensure_eq!((paid, voided), (bill.paid, bill.voided), "bill {id} paid and voided");
        if !bill.voided {
            let expected = if bill.paid == 0 { "UNPAID" } else if bill.paid < bill.total { "PARTIAL" } else { "PAID" };
            ensure_eq!(status, expected, "bill {id} status");
        }
    }

    // Undeposited Funds holds exactly the payments waiting to be deposited.
    let waiting: i64 = m.waiting_for_deposit().iter().map(|p| m.payments[p].home).sum();
    ensure_eq!(m.balances.get(&a.undeposited).copied().unwrap_or(0), waiting, "Undeposited Funds in the model");
    let listed: i64 = crate::banking::list_undeposited(pool).await?.iter().map(|i| i.amount_cents).sum();
    ensure_eq!(listed, waiting, "payments listed as undeposited");
    Ok(())
}

async fn run(ops: Vec<Op>) -> Result<(), String> {
    let pool = crate::db::test_pool().await;
    let accounts = Accounts::load(&pool).await;
    let mut model = Model::default();
    // Mid-year and year-end dates on both sides of the fiscal year boundary: one per step in turn,
    // and all of them after the last step.
    const DATES: [&str; 4] = ["2025-06-30", "2025-12-31", "2026-06-30", "2026-12-31"];
    check(&pool, &accounts, &mut model, None, &DATES).await.map_err(|e| format!("on the empty company: {e}"))?;
    for (step, op) in ops.iter().enumerate() {
        let lock_before = model.lock;
        apply(&pool, &accounts, &mut model, op).await.map_err(|e| format!("step {step} {op:?}: {e}"))?;
        let dates = if step + 1 == ops.len() { &DATES[..] } else { &DATES[step % DATES.len()..][..1] };
        check(&pool, &accounts, &mut model, lock_before, dates).await.map_err(|e| format!("after step {step} {op:?}: {e}"))?;
    }
    Ok(())
}

fn config() -> Config {
    let cases = std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(16);
    // Shrinking replays whole sequences, so it gets a real budget (the default is 4 tries per case).
    Config { cases, max_shrink_iters: 10_000, max_shrink_time: 300_000, ..Config::default() }
}

proptest! {
    #![proptest_config(config())]

    #[test]
    fn ledger_invariants_hold_after_every_step(ops in prop::collection::vec(op(), 1..40)) {
        runtime().block_on(run(ops)).map_err(TestCaseError::fail)?;
    }
}

/// Runs a hand-written sequence (a shrunk failure, or a scenario worth keeping) through the same checks.
fn replay(ops: Vec<Op>) {
    if let Err(err) = runtime().block_on(run(ops)) {
        panic!("{err}");
    }
}

#[test]
fn voiding_a_payment_reopens_its_invoice() {
    replay(vec![
        Op::Invoice { day: 10, due_in: 30, lines: vec![(4, 10_000)], income: Pick(0), customer: 0, fx_rate: None },
        Op::ReceivePayment { invoice: Pick(0), percent: 100, after: 5, undeposited: true, fx_rate: 1_000_000 },
        Op::Void { entry: Pick(0) },
        Op::ReceivePayment { invoice: Pick(0), percent: 40, after: 6, undeposited: false, fx_rate: 1_000_000 },
    ]);
}

#[test]
fn foreign_payment_after_voided_partial_payments() {
    let pay = |percent| Op::ReceivePayment { invoice: Pick(0), percent, after: 1, undeposited: false, fx_rate: 757_900 };
    replay(vec![
        Op::Invoice { day: 10, due_in: 30, lines: vec![(4, 27)], income: Pick(0), customer: 0, fx_rate: Some(757_900) },
        pay(78),
        pay(1),
        pay(60),
        pay(50),
        pay(100),
        Op::Void { entry: Pick(1) },
        Op::Void { entry: Pick(2) },
        pay(50),
    ]);
}
