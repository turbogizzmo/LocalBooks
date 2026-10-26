//! Reads a GnuCash book saved as SQLite.
//!
//! Tables used: `books` (the account tree's root, and the root of the scheduled-transaction
//! templates, which are left out), `commodities`, `accounts`, `transactions`, `splits`, `slots`,
//! `lots`, `customers`, `vendors`, `employees`, `jobs`, `invoices`, `entries` and `billterms`.
//! Columns are read by name and a missing one reads as empty, since GnuCash has added columns
//! over the years; the business tables may be missing altogether in a book that never used them.
//!
//! Slots hold the extra fields: a transaction's `trans-txn-type`, `trans-date-due` and `notes`; a
//! lot's owner and invoice; an invoice's `credit-note` flag. A frame slot (`gncOwner`) keeps its
//! children in rows whose `obj_guid` is the frame's own `guid_val` and whose names are full paths
//! (`gncOwner/owner-guid`).
//!
//! The file is opened read-only and immutable, so nothing is written next to it, even while
//! GnuCash has the book open.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqliteRow};
use sqlx::{ConnectOptions, Connection, Row, SqliteConnection};

use super::{commodity_key, parse_date, Account, Address, Book, Entry, Invoice, Job, Lot, Num, OwnerKind, Party, Split, Transaction};

fn text(row: &SqliteRow, column: &str) -> Option<String> {
    row.try_get_unchecked::<Option<String>, _>(column).ok().flatten()
}

fn int(row: &SqliteRow, column: &str) -> Option<i64> {
    row.try_get_unchecked::<Option<i64>, _>(column).ok().flatten()
}

fn num(row: &SqliteRow, prefix: &str) -> Num {
    match (int(row, &format!("{prefix}_num")), int(row, &format!("{prefix}_denom"))) {
        (Some(n), Some(d)) if d > 0 => Num::new(n, d),
        _ => Num::default(),
    }
}

async fn all(conn: &mut SqliteConnection, table: &str) -> Result<Vec<SqliteRow>, String> {
    sqlx::query(&format!("SELECT * FROM {table}")).fetch_all(conn).await.map_err(|e| format!("GNUCASH_PARSE_FAILED: table {table}: {e}"))
}

/// A business table, which a book that never used the feature may not have.
async fn optional(conn: &mut SqliteConnection, table: &str) -> Result<Vec<SqliteRow>, String> {
    let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)")
        .bind(table)
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| format!("GNUCASH_PARSE_FAILED: {e}"))?;
    if exists {
        all(conn, table).await
    } else {
        Ok(Vec::new())
    }
}

fn address(row: &SqliteRow) -> Address {
    Address {
        name: text(row, "addr_name"),
        lines: (1..=4).filter_map(|i| text(row, &format!("addr_addr{i}"))).collect(),
        phone: text(row, "addr_phone"),
        email: text(row, "addr_email"),
    }
}

fn party(row: &SqliteRow, name_column: &str) -> Option<Party> {
    Some(Party {
        guid: text(row, "guid")?,
        name: text(row, name_column)?,
        notes: text(row, "notes"),
        active: int(row, "active").is_none_or(|a| a != 0),
        address: address(row),
        terms: text(row, "terms"),
    })
}

fn owner(row: &SqliteRow, prefix: &str) -> Option<(OwnerKind, String)> {
    Some((OwnerKind::from_code(int(row, &format!("{prefix}_type"))?)?, text(row, &format!("{prefix}_guid"))?))
}

pub async fn read_sqlite(path: &Path) -> Result<Book, String> {
    let options = SqliteConnectOptions::new().filename(path).read_only(true).immutable(true).disable_statement_logging();
    let mut conn = SqliteConnection::connect_with(&options).await.map_err(|e| format!("GNUCASH_UNREADABLE: {}: {e}", path.display()))?;
    let book = read_connection(&mut conn).await;
    let _ = conn.close().await;
    book
}

pub(super) async fn read_connection(conn: &mut SqliteConnection) -> Result<Book, String> {
    let has_accounts: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'splits')")
        .fetch_one(&mut *conn)
        .await
        .map_err(|e| format!("GNUCASH_NOT_GNUCASH: {e}"))?;
    if !has_accounts {
        return Err("GNUCASH_NOT_GNUCASH: this SQLite file is not a GnuCash book (it has no splits table)".into());
    }

    let commodities: HashMap<String, String> = all(conn, "commodities")
        .await?
        .iter()
        .filter_map(|r| Some((text(r, "guid")?, commodity_key(&text(r, "namespace")?, &text(r, "mnemonic")?))))
        .collect();
    let commodity = |guid: Option<String>| guid.and_then(|g| commodities.get(&g).cloned()).unwrap_or_default();

    let template_roots: HashSet<String> = optional(conn, "books").await?.iter().filter_map(|r| text(r, "root_template_guid")).collect();
    let mut accounts: Vec<Account> = all(conn, "accounts")
        .await?
        .iter()
        .filter_map(|r| {
            Some(Account {
                guid: text(r, "guid")?,
                name: text(r, "name").unwrap_or_default(),
                kind: text(r, "account_type").unwrap_or_default(),
                commodity: commodity(text(r, "commodity_guid")),
                parent: text(r, "parent_guid"),
                hidden: int(r, "hidden").is_some_and(|h| h != 0),
            })
        })
        .collect();
    // Scheduled-transaction templates live in a tree of their own; leave it out.
    let parent: HashMap<String, Option<String>> = accounts.iter().map(|a| (a.guid.clone(), a.parent.clone())).collect();
    let in_templates = |guid: &str| {
        let mut cur = Some(guid.to_string());
        let mut steps = 0;
        while let Some(g) = cur {
            if template_roots.contains(&g) {
                return true;
            }
            steps += 1;
            if steps > 64 {
                return false;
            }
            cur = parent.get(&g).cloned().flatten();
        }
        false
    };
    let templates: HashSet<String> = accounts.iter().filter(|a| in_templates(&a.guid)).map(|a| a.guid.clone()).collect();
    accounts.retain(|a| !templates.contains(&a.guid));

    // Slots: frames resolve to the object that owns them.
    let slots = optional(conn, "slots").await?;
    let mut frame_owner: HashMap<String, String> = HashMap::new();
    for r in &slots {
        if let (Some(obj), Some(name), Some(guid)) = (text(r, "obj_guid"), text(r, "name"), text(r, "guid_val")) {
            if matches!(name.as_str(), "gncOwner" | "gncInvoice") {
                frame_owner.insert(guid, obj);
            }
        }
    }
    // (object, slot name) → slot row
    let mut slot: HashMap<(String, String), &SqliteRow> = HashMap::new();
    for r in &slots {
        let (Some(obj), Some(name)) = (text(r, "obj_guid"), text(r, "name")) else { continue };
        let obj = if name.contains('/') { frame_owner.get(&obj).cloned().unwrap_or(obj) } else { obj };
        slot.insert((obj, name), r);
    }
    let slot_of = |obj: &str, name: &str| slot.get(&(obj.to_string(), name.to_string())).copied();

    let mut splits: HashMap<String, Vec<Split>> = HashMap::new();
    for r in all(conn, "splits").await? {
        let (Some(tx), Some(account)) = (text(&r, "tx_guid"), text(&r, "account_guid")) else { continue };
        splits.entry(tx).or_default().push(Split { account, memo: text(&r, "memo"), value: num(&r, "value"), quantity: num(&r, "quantity"), lot: text(&r, "lot_guid") });
    }
    let mut transactions: Vec<Transaction> = Vec::new();
    for r in all(conn, "transactions").await? {
        let Some(guid) = text(&r, "guid") else { continue };
        let splits = splits.remove(&guid).unwrap_or_default();
        if splits.iter().any(|s| templates.contains(&s.account)) {
            continue;
        }
        let due = slot_of(&guid, "trans-date-due").and_then(|s| text(s, "timespec_val").or_else(|| text(s, "gdate_val"))).and_then(|d| parse_date(&d));
        transactions.push(Transaction {
            currency: commodity(text(&r, "currency_guid")),
            num: text(&r, "num"),
            date: text(&r, "post_date").and_then(|d| parse_date(&d)),
            description: text(&r, "description"),
            txn_type: slot_of(&guid, "trans-txn-type").and_then(|s| text(s, "string_val")),
            due,
            notes: slot_of(&guid, "notes").and_then(|s| text(s, "string_val")),
            splits,
            guid,
        });
    }

    let mut lots: HashMap<String, Lot> = HashMap::new();
    for r in optional(conn, "lots").await? {
        let Some(guid) = text(&r, "guid") else { continue };
        let kind = slot_of(&guid, "gncOwner/owner-type").and_then(|s| int(s, "int64_val")).and_then(OwnerKind::from_code);
        let owner_guid = slot_of(&guid, "gncOwner/owner-guid").and_then(|s| text(s, "guid_val"));
        let invoice = slot_of(&guid, "gncInvoice/invoice-guid").and_then(|s| text(s, "guid_val"));
        lots.insert(guid, Lot { owner: kind.zip(owner_guid), invoice });
    }

    let customers = optional(conn, "customers").await?.iter().filter_map(|r| party(r, "name")).collect();
    let vendors = optional(conn, "vendors").await?.iter().filter_map(|r| party(r, "name")).collect();
    let employees = optional(conn, "employees").await?.iter().filter_map(|r| party(r, "username")).collect();
    let jobs = optional(conn, "jobs")
        .await?
        .iter()
        .filter_map(|r| Some(Job { guid: text(r, "guid")?, name: text(r, "name").unwrap_or_default(), active: int(r, "active").is_none_or(|a| a != 0), owner: owner(r, "owner") }))
        .collect();
    let invoices = optional(conn, "invoices")
        .await?
        .iter()
        .filter_map(|r| {
            let guid = text(r, "guid")?;
            Some(Invoice {
                id: text(r, "id"),
                owner: owner(r, "owner"),
                terms: text(r, "terms"),
                post_txn: text(r, "post_txn"),
                post_lot: text(r, "post_lot"),
                credit_note: slot_of(&guid, "credit-note").and_then(|s| int(s, "int64_val")).is_some_and(|v| v != 0),
                guid,
            })
        })
        .collect();
    let entries = optional(conn, "entries")
        .await?
        .iter()
        .map(|r| Entry {
            invoice: text(r, "invoice"),
            bill: text(r, "bill"),
            description: text(r, "description"),
            quantity: num(r, "quantity"),
            i_account: text(r, "i_acct"),
            i_price: num(r, "i_price"),
            i_discount: num(r, "i_discount"),
            i_discount_type: text(r, "i_disc_type"),
            b_account: text(r, "b_acct"),
            b_price: num(r, "b_price"),
        })
        .collect();
    let terms = optional(conn, "billterms")
        .await?
        .iter()
        .filter_map(|r| {
            let days = text(r, "type").is_none_or(|t| t.contains("DAYS")).then(|| int(r, "duedays")).flatten()?;
            Some((text(r, "guid")?, days))
        })
        .collect();

    Ok(Book { accounts, transactions, customers, vendors, employees, jobs, invoices, entries, terms, lots })
}
