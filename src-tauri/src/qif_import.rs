//! Reads Quicken Interchange Format (`.qif`) exports into a [`Ledger`], for Quicken users (and
//! the bank, card and other programs that export QIF) moving to LocalBooks.
//!
//! QIF is a register format: each section holds one account's transactions (`!Type:Bank`,
//! `!Type:CCard`, `!Type:Cash`, `!Type:Oth A`, `!Type:Oth L`), preceded by an `!Account` record
//! naming the account when the file holds several. Each record is one field per line, keyed by its
//! first letter (`D` date, `T` amount, `P` payee, `M` memo, `N` number, `L` category), and ends
//! with `^`. A split repeats `S` (category), `E` (memo) and `$` (amount). A category in brackets,
//! `[Savings]`, is a transfer to that account; `Category/Class` adds a class, which is dropped.
//! `!Type:Cat` lists categories with `I` (income) or `E` (expense). Amounts are signed from the
//! account's side (money out is negative). References: Quicken's QIF specification as published
//! with Quicken 99 (W3C mirror), Wikipedia's "Quicken Interchange Format", and GnuCash's QIF
//! importer, which established the conventions below.
//!
//! - Quicken writes a transfer in both accounts; the copy in the second account is dropped when
//!   it is the mirror (same date, opposite amount) of one already read.
//! - An `Opening Balance` record transferring to its own account opens the account against
//!   Opening Balance Equity.
//! - Dates are month-first unless some date can only be day-first; years after an apostrophe
//!   (`1/5'04`) are 2000 and later.
//! - Investment accounts (`!Type:Invst`) hold securities, which LocalBooks does not track; their
//!   transactions are counted and skipped.

use std::collections::HashMap;

use chrono::NaiveDate;

use crate::bank::{detect_date_order, parse_amount_cents, parse_date_ordered, DateOrder};
use crate::qbw_company::{Ledger, LedgerQuality, LineOutcome, SourceLine, SourceTransaction, UnreadableLine};
use crate::qbw_documents::SourceDocument;
use crate::text_import::{decode, Accounts, Names};

const OPENING_BALANCE_EQUITY: &str = "Opening Balance Equity";
pub const SKIPPED_INVESTMENT: &str = "investment transactions (securities are not tracked)";

/// LocalBooks account type for a QIF register or account-list type.
fn register_type(code: &str) -> Option<&'static str> {
    Some(match code.trim().to_ascii_lowercase().as_str() {
        "bank" => "BANK",
        "cash" => "BANK",
        "ccard" => "CREDIT_CARD",
        "oth a" => "OTHER_CURRENT_ASSET",
        "oth l" => "OTHER_CURRENT_LIABILITY",
        "invoice" => "ACCOUNTS_RECEIVABLE",
        _ => return None,
    })
}

fn is_investment(code: &str) -> bool {
    matches!(code.trim().to_ascii_lowercase().as_str(), "invst" | "port" | "401(k)/403(b)" | "mutual" | "invest")
}

/// One record: its fields in order, as (code, value).
type Record = Vec<(char, String)>;

fn first<'a>(record: &'a Record, code: char) -> Option<&'a str> {
    record.iter().find(|(c, _)| *c == code).map(|(_, v)| v.trim()).filter(|v| !v.is_empty())
}

/// What a section of the file holds.
#[derive(Clone, PartialEq)]
enum Section {
    AccountList,
    Categories,
    /// Transactions of the current account, with the register type it declared.
    Register(&'static str),
    Investments,
    Ignored,
}

/// A transaction as read, before categories and transfers are resolved.
struct RawTxn {
    account: String,
    account_type: &'static str,
    date: String,
    amount: Option<String>,
    payee: Option<String>,
    memo: Option<String>,
    number: Option<String>,
    category: Option<String>,
    splits: Vec<(Option<String>, Option<String>, Option<String>)>,
}

/// A category or transfer target: `Cat:Sub/Class` → `Cat:Sub`; `[Account]/Class` → account.
enum Target {
    None,
    Category(String),
    Transfer(String),
}

fn target(raw: Option<&str>) -> Target {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else { return Target::None };
    if let Some(rest) = raw.strip_prefix('[') {
        let name = rest.split(']').next().unwrap_or_default().trim();
        return if name.is_empty() { Target::None } else { Target::Transfer(name.to_string()) };
    }
    let name = raw.split('/').next().unwrap_or_default().trim();
    if name.is_empty() {
        Target::None
    } else {
        Target::Category(name.to_string())
    }
}

/// Rewrites Quicken's year forms for the bank date parser: `1/ 5'04` → `1/5/2004`, `12/21' 7` →
/// `12/21/2007`. Two-digit years after `/` keep the usual 1970-2069 window.
fn normalize_date(raw: &str) -> String {
    let compact: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    match compact.split_once('\'') {
        Some((month_day, year)) => match year.parse::<i32>() {
            Ok(y) if year.len() <= 2 => format!("{month_day}/{}", 2000 + y),
            _ => format!("{month_day}/{year}"),
        },
        None => compact,
    }
}

/// Reads a QIF file. `default_account` names the account when the file has transactions but no
/// `!Account` record (a single-account export, as banks provide); the file name is a good choice.
pub fn read(bytes: &[u8], default_account: &str) -> Result<Ledger, String> {
    let text = decode(bytes);
    let mut accounts = Accounts::default();
    let mut categories: HashMap<String, &'static str> = HashMap::new();
    let mut raw: Vec<RawTxn> = Vec::new();
    let mut skipped_investment = 0usize;

    let mut section = Section::Ignored;
    let mut auto_switch = false;
    let mut current: Option<(String, &'static str)> = None;
    let mut record: Record = Vec::new();
    let mut saw_header = false;

    let mut finish = |section: &Section, record: &mut Record, current: &mut Option<(String, &'static str)>, accounts: &mut Accounts| {
        if record.is_empty() {
            return;
        }
        let r = std::mem::take(record);
        match section {
            Section::AccountList => {
                if let Some(name) = first(&r, 'N') {
                    let code = first(&r, 'T').unwrap_or("Bank");
                    match register_type(code) {
                        Some(t) => {
                            accounts.declare(name, t, true);
                            *current = Some((name.to_string(), t));
                        }
                        None => *current = is_investment(code).then(|| (name.to_string(), "INVESTMENT")),
                    }
                }
            }
            Section::Categories => {
                if let Some(name) = first(&r, 'N') {
                    let income = r.iter().any(|(c, _)| *c == 'I');
                    categories.insert(name.to_lowercase(), if income { "INCOME" } else { "EXPENSE" });
                }
            }
            Section::Register(register) => {
                let (account, account_type) = match current.clone() {
                    Some((name, t)) if t != "INVESTMENT" => (name, t),
                    _ => (default_account.to_string(), *register),
                };
                let mut splits = Vec::new();
                for (code, value) in &r {
                    match code {
                        'S' => splits.push((Some(value.clone()), None, None)),
                        'E' => match splits.last_mut() {
                            Some(s) if s.1.is_none() => s.1 = Some(value.clone()),
                            _ => splits.push((None, Some(value.clone()), None)),
                        },
                        '$' => match splits.last_mut() {
                            Some(s) if s.2.is_none() => s.2 = Some(value.clone()),
                            _ => splits.push((None, None, Some(value.clone()))),
                        },
                        _ => {}
                    }
                }
                raw.push(RawTxn {
                    account,
                    account_type,
                    date: first(&r, 'D').unwrap_or_default().to_string(),
                    amount: first(&r, 'T').or(first(&r, 'U')).map(str::to_string),
                    payee: first(&r, 'P').map(str::to_string),
                    memo: first(&r, 'M').map(str::to_string),
                    number: first(&r, 'N').map(str::to_string),
                    category: first(&r, 'L').map(str::to_string),
                    splits,
                });
            }
            Section::Investments => skipped_investment += 1,
            Section::Ignored => {}
        }
    };

    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        if let Some(header) = line.strip_prefix('!') {
            finish(&section, &mut record, &mut current, &mut accounts);
            saw_header = true;
            let header = header.trim();
            let lower = header.to_ascii_lowercase();
            section = if lower == "option:autoswitch" {
                auto_switch = true;
                Section::AccountList
            } else if lower == "clear:autoswitch" {
                auto_switch = false;
                Section::Ignored
            } else if lower == "account" {
                Section::AccountList
            } else if let Some(kind) = header.strip_prefix("Type:").or_else(|| header.strip_prefix("type:")) {
                match kind.trim().to_ascii_lowercase().as_str() {
                    "cat" => Section::Categories,
                    k if is_investment(k) => Section::Investments,
                    k => match register_type(k) {
                        Some(t) if !auto_switch => Section::Register(t),
                        _ => Section::Ignored,
                    },
                }
            } else {
                Section::Ignored
            };
            continue;
        }
        if line.starts_with('^') {
            finish(&section, &mut record, &mut current, &mut accounts);
            continue;
        }
        let mut chars = line.chars();
        if let Some(code) = chars.next() {
            record.push((code, chars.as_str().to_string()));
        }
    }
    finish(&section, &mut record, &mut current, &mut accounts);
    if !saw_header {
        return Err("QIF_NOT_QIF: this file has no QIF headers (!Type:Bank, !Account, ...)".into());
    }
    if raw.is_empty() && skipped_investment == 0 && accounts.list.is_empty() && categories.is_empty() {
        return Err("QIF_EMPTY: this file has no accounts, categories or transactions".into());
    }

    let normalized: Vec<String> = raw.iter().map(|t| normalize_date(&t.date)).collect();
    let (order, _) = detect_date_order(normalized.iter().map(String::as_str).filter(|d| !d.is_empty()));
    let mut builder = Builder { accounts, categories, order, unreadable: Vec::new(), mirrors: Vec::new() };
    let mut transactions = Vec::new();
    for (n, (t, date)) in raw.iter().zip(&normalized).enumerate() {
        if let Some(t) = builder.transaction(n as i64 + 1, t, date) {
            transactions.push(t);
        }
    }
    transactions.sort_by_key(|t| (t.date, t.id));
    let (transactions, unbalanced): (Vec<_>, Vec<_>) = transactions.into_iter().partition(|t| t.imbalance_cents() == 0);

    let mut quality = LedgerQuality { unreadable_lines: builder.unreadable.len(), rejected_lines: skipped_investment, ..Default::default() };
    if skipped_investment > 0 {
        quality.rejected_by_reason.insert(SKIPPED_INVESTMENT, skipped_investment);
    }
    let Builder { accounts, unreadable, .. } = builder;
    let names = Names::default();
    Ok(Ledger {
        accounts: accounts.list,
        customers: names.customers,
        vendors: names.vendors,
        items: Vec::new(),
        transactions,
        unbalanced,
        unreadable,
        quality,
        sales_lists: Default::default(),
    })
}

struct Builder {
    accounts: Accounts,
    categories: HashMap<String, &'static str>,
    order: DateOrder,
    unreadable: Vec<UnreadableLine>,
    /// Transfers already read, waiting for their copy in the other account:
    /// (date, account the copy will be in, the account it transfers to, amount in that account).
    mirrors: Vec<(NaiveDate, i64, i64, i64)>,
}

impl Builder {
    fn category(&mut self, name: &str, inflow: bool) -> i64 {
        let known = self.categories.get(&name.to_lowercase()).copied();
        // A subcategory takes its top category's type.
        let top = name.split(':').next().unwrap_or(name).to_lowercase();
        let kind = known.or_else(|| self.categories.get(&top).copied()).unwrap_or(if inflow { "INCOME" } else { "EXPENSE" });
        self.accounts.ensure(name, kind)
    }

    fn unreadable(&mut self, id: i64, date: Option<NaiveDate>, t: &RawTxn, cents: Option<i64>) {
        self.unreadable.push(UnreadableLine {
            kind: "Transaction",
            transaction_id: Some(id),
            date,
            doc_num: t.number.clone(),
            name: t.payee.clone(),
            account_id: self.accounts.id(&t.account),
            cents,
            outcome: LineOutcome::Unreadable,
        });
    }

    fn transaction(&mut self, id: i64, t: &RawTxn, date: &str) -> Option<SourceTransaction> {
        let date = parse_date_ordered(date, self.order).ok();
        let amount = t.amount.as_deref().map(parse_amount_cents);
        let (Some(date), Some(Ok(amount))) = (date, amount) else {
            self.unreadable(id, date, t, None);
            return None;
        };
        let account = self.accounts.ensure(&t.account, t.account_type);
        // Money into the account is a debit (negative in the ledger) for an asset and a payment
        // (also a debit) for a card or liability: either way, the account line is -amount.
        let mut lines = vec![SourceLine { account_id: account, cents: -amount, memo: None }];
        let inflow = amount > 0;
        let mut transfers: Vec<(i64, i64)> = Vec::new();
        let mut opening = false;

        let parts: Vec<(Target, Option<String>, i64)> = if t.splits.is_empty() {
            vec![(target(t.category.as_deref()), None, amount)]
        } else {
            let mut parts = Vec::new();
            for (category, memo, cents) in &t.splits {
                match cents.as_deref().map(parse_amount_cents) {
                    Some(Ok(c)) => parts.push((target(category.as_deref()), memo.clone(), c)),
                    _ => self.unreadable(id, Some(date), t, None),
                }
            }
            parts
        };
        for (target, memo, cents) in parts {
            let other = match target {
                Target::Transfer(name) if name.eq_ignore_ascii_case(&t.account) => {
                    opening = true;
                    self.accounts.ensure(OPENING_BALANCE_EQUITY, "EQUITY")
                }
                Target::Transfer(name) => {
                    // The other account keeps its declared type; an undeclared one is a bank account.
                    let other = self.accounts.id(&name).unwrap_or_else(|| self.accounts.ensure(&name, "BANK"));
                    transfers.push((other, cents));
                    other
                }
                Target::Category(name) => self.category(&name, cents > 0),
                Target::None => self.accounts.ensure(if cents > 0 { "Uncategorized Income" } else { "Uncategorized Expense" }, if cents > 0 { "INCOME" } else { "EXPENSE" }),
            };
            lines.push(SourceLine { account_id: other, cents, memo });
        }

        // The copy of a transfer already read from the other account.
        if let [(other, cents)] = transfers.as_slice() {
            if lines.len() == 2 {
                if let Some(i) = self.mirrors.iter().position(|m| *m == (date, account, *other, -*cents)) {
                    self.mirrors.remove(i);
                    return None;
                }
            }
        }
        for (other, cents) in &transfers {
            self.mirrors.push((date, *other, account, *cents));
        }

        let account_type = self.accounts.kind(account);
        let only_transfers = !transfers.is_empty() && transfers.len() == lines.len() - 1;
        let kind = if opening {
            "Opening balance"
        } else if only_transfers {
            "Transfer"
        } else {
            match (account_type, inflow) {
                ("BANK", false) if t.number.as_deref().is_some_and(|n| n.trim().parse::<u64>().is_ok()) => "Check",
                ("BANK", false) => "Withdrawal",
                ("BANK", true) => "Deposit",
                ("CREDIT_CARD", false) => "Credit card charge",
                ("CREDIT_CARD", true) => "Credit card credit",
                _ => "Journal entry",
            }
        };
        let document = match kind {
            "Transfer" if lines.len() == 2 => Some(SourceDocument::Transfer),
            "Check" | "Withdrawal" | "Credit card charge" | "Credit card credit" => {
                Some(SourceDocument::Expense { vendor_id: None, number: t.number.clone().filter(|_| kind == "Check"), applied_to: Vec::new() })
            }
            "Deposit" => Some(SourceDocument::Deposit { payments: Vec::new() }),
            _ => None,
        };
        let memo = match (&t.payee, &t.memo) {
            (Some(p), Some(m)) if !m.is_empty() => Some(format!("{p} - {m}")),
            (Some(p), _) => Some(p.clone()),
            (None, m) => m.clone(),
        };
        Some(SourceTransaction { document, id, kind, date, doc_num: t.number.clone(), name: t.payee.clone(), memo, lines })
    }
}

#[cfg(test)]
mod tests;
