//! Reads a GnuCash book saved as XML (`gnc-v2`), gzip-compressed or not, as a stream.
//!
//! The file is a flat run of objects under `gnc:book`: `gnc:commodity`, `gnc:account` (whose
//! `act:lots` hold its lots), `gnc:transaction` (with its `trn:split`s), and the business objects
//! `gnc:GncCustomer`, `gnc:GncVendor`, `gnc:GncEmployee`, `gnc:GncJob`, `gnc:GncInvoice`,
//! `gnc:GncEntry` and `gnc:GncBillTerm`. Each object is collected on its own as the parser passes
//! it (its leaf values by path, its slots by full key) and converted when it closes; nothing else
//! of the document is kept. Scheduled-transaction templates (`gnc:template-transactions`) and
//! schedules are skipped.
//!
//! Slots are `<slot><slot:key>k</slot:key><slot:value type="...">v</slot:value></slot>`; a frame's
//! value holds more slots, which are named by their full path (`gncOwner/owner-guid`), as the SQL
//! backend names them.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

use flate2::read::MultiGzDecoder;
use quick_xml::events::Event;
use quick_xml::Reader;

use super::{commodity_key, parse_date, Account, Address, Book, Entry, Format, Invoice, Job, Lot, Num, OwnerKind, Party, Split, Transaction};

/// Objects collected whole.
const OBJECTS: &[&str] =
    &["gnc:account", "gnc:transaction", "gnc:GncCustomer", "gnc:GncVendor", "gnc:GncEmployee", "gnc:GncJob", "gnc:GncInvoice", "gnc:GncEntry", "gnc:GncBillTerm"];
/// Parts of an object collected as sub-records: a transaction's splits, an account's lots.
const PARTS: &[&str] = &["trn:split", "gnc:lot"];
/// Subtrees with nothing to import.
const SKIPPED: &[&str] = &["gnc:template-transactions", "gnc:schedxaction", "gnc:pricedb", "gnc:budget"];

/// An object, or a part of one, as read: leaf values by path (relative to the object) and slot
/// values by full key.
#[derive(Default)]
struct Record {
    name: String,
    leaves: Vec<(String, String)>,
    slots: HashMap<String, String>,
    parts: Vec<Record>,
}

impl Record {
    fn get(&self, path: &str) -> Option<String> {
        self.leaves.iter().find(|(p, _)| p == path).map(|(_, v)| v.clone())
    }

    fn trimmed(&self, path: &str) -> Option<String> {
        self.get(path).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
    }

    fn num(&self, path: &str) -> Num {
        self.get(path).and_then(|v| Num::parse(&v)).unwrap_or_default()
    }

    fn flag(&self, path: &str) -> Option<bool> {
        self.trimmed(path).map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    }

    fn slot(&self, key: &str) -> Option<String> {
        self.slots.get(key).map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
    }

    fn commodity(&self, prefix: &str) -> String {
        match (self.get(&format!("{prefix}/cmdty:space")), self.get(&format!("{prefix}/cmdty:id"))) {
            (Some(space), Some(id)) => commodity_key(&space, &id),
            _ => String::new(),
        }
    }

    fn owner(&self, prefix: &str) -> Option<(OwnerKind, String)> {
        Some((OwnerKind::from_name(&self.get(&format!("{prefix}/owner:type"))?)?, self.trimmed(&format!("{prefix}/owner:id"))?))
    }

    fn address(&self, prefix: &str) -> Address {
        Address {
            name: self.get(&format!("{prefix}/addr:name")),
            lines: (1..=4).filter_map(|i| self.get(&format!("{prefix}/addr:addr{i}"))).collect(),
            phone: self.get(&format!("{prefix}/addr:phone")),
            email: self.get(&format!("{prefix}/addr:email")),
        }
    }

    fn party(&self, ns: &str, name: &str) -> Option<Party> {
        Some(Party {
            guid: self.trimmed(&format!("{ns}:guid"))?,
            name: self.get(&format!("{ns}:{name}"))?,
            notes: self.get(&format!("{ns}:notes")),
            active: self.flag(&format!("{ns}:active")).unwrap_or(true),
            address: self.address(&format!("{ns}:addr")),
            terms: self.trimmed(&format!("{ns}:terms")),
        })
    }
}

/// Opens a book file and reads it.
pub(super) fn open(path: &Path, format: Format) -> Result<Book, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("GNUCASH_UNREADABLE: {}: {e}", path.display()))?;
    match format {
        Format::GzipXml => read_xml(BufReader::new(MultiGzDecoder::new(BufReader::new(file)))),
        _ => read_xml(BufReader::new(file)),
    }
}

/// Reads an uncompressed `gnc-v2` document.
pub fn read_xml<R: BufRead>(input: R) -> Result<Book, String> {
    let mut reader = Reader::from_reader(input);
    let mut buf = Vec::new();
    // Element names from the document root down.
    let mut stack: Vec<String> = Vec::new();
    // Open records with the stack depth of their element.
    let mut open: Vec<(usize, Record)> = Vec::new();
    // Inside a skipped subtree: the stack depth where it starts.
    let mut skipping: Option<usize> = None;
    // Slot keys by slot nesting level, for the record being read.
    let mut slot_keys: Vec<String> = Vec::new();
    let mut text = String::new();
    // The last event opened an element that has not had a child yet.
    let mut leaf = false;
    let mut saw_root = false;
    let mut book = Book::default();

    loop {
        let event = reader.read_event_into(&mut buf).map_err(|e| format!("GNUCASH_PARSE_FAILED: the XML could not be read at byte {}: {e}", reader.buffer_position()))?;
        match event {
            Event::Start(e) => {
                let name = e.name().as_ref().to_string();
                if stack.is_empty() {
                    if name != "gnc-v2" {
                        return Err("GNUCASH_NOT_GNUCASH: this XML file is not a GnuCash book".into());
                    }
                    saw_root = true;
                }
                stack.push(name);
                let name = stack.last().expect("just pushed");
                if skipping.is_none() && SKIPPED.contains(&name.as_str()) {
                    skipping = Some(stack.len());
                }
                let collect = OBJECTS.contains(&name.as_str()) && open.is_empty() || PARTS.contains(&name.as_str()) && !open.is_empty();
                if skipping.is_none() && collect {
                    open.push((stack.len(), Record { name: name.clone(), ..Default::default() }));
                    if open.len() == 1 {
                        slot_keys.clear();
                    }
                }
                text.clear();
                leaf = true;
            }
            Event::Empty(e) => {
                // `<split:memo/>` and the like: an empty value.
                if skipping.is_none() {
                    if let Some((depth, record)) = open.last_mut() {
                        let name = e.name().as_ref().to_string();
                        let mut path = stack[*depth..].join("/");
                        if !path.is_empty() {
                            path.push('/');
                        }
                        path.push_str(&name);
                        record.leaves.push((path, String::new()));
                    }
                }
                leaf = false;
            }
            Event::Text(t) => {
                if leaf && skipping.is_none() && !open.is_empty() {
                    text.push_str(&t.xml10_content());
                }
            }
            Event::CData(t) => {
                if leaf && skipping.is_none() && !open.is_empty() {
                    text.push_str(&t);
                }
            }
            Event::GeneralRef(r) => {
                if leaf && skipping.is_none() && !open.is_empty() {
                    let entity = format!("&{};", r.xml10_content());
                    match quick_xml::escape::unescape(&entity) {
                        Ok(s) => text.push_str(&s),
                        Err(_) => text.push_str(&entity),
                    }
                }
            }
            Event::End(_) => {
                let depth = stack.len();
                if skipping.is_none() && leaf {
                    if let Some((start, record)) = open.last_mut() {
                        let path = stack[*start..].join("/");
                        let element = stack.last().map(String::as_str).unwrap_or_default();
                        // Slot nesting: how many `slot` elements enclose this one.
                        let level = stack[*start..].iter().filter(|n| *n == "slot").count();
                        if element == "slot:key" {
                            slot_keys.truncate(level.saturating_sub(1));
                            slot_keys.push(text.trim().to_string());
                        } else if level > 0 && stack[*start..].iter().any(|n| n == "slot:value") {
                            slot_keys.truncate(level);
                            record.slots.insert(slot_keys.join("/"), std::mem::take(&mut text));
                        } else {
                            record.leaves.push((path, std::mem::take(&mut text)));
                        }
                    }
                }
                leaf = false;
                if open.last().is_some_and(|(d, _)| *d == depth) {
                    let (_, record) = open.pop().expect("checked");
                    match open.last_mut() {
                        Some((_, parent)) => parent.parts.push(record),
                        None => add(&mut book, record),
                    }
                }
                if skipping == Some(depth) {
                    skipping = None;
                }
                stack.pop();
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if !saw_root {
        return Err("GNUCASH_NOT_GNUCASH: this file is not a GnuCash XML book".into());
    }
    if !stack.is_empty() {
        return Err("GNUCASH_PARSE_FAILED: the file ends part-way through the book (it may be damaged or still being saved)".into());
    }
    Ok(book)
}

/// Converts a finished object into the book.
fn add(book: &mut Book, r: Record) {
    match r.name.as_str() {
        "gnc:account" => {
            let Some(guid) = r.trimmed("act:id") else { return };
            for lot in r.parts.iter().filter(|p| p.name == "gnc:lot") {
                let Some(id) = lot.trimmed("lot:id") else { continue };
                let kind = lot.slot("gncOwner/owner-type").and_then(|t| t.parse().ok()).and_then(OwnerKind::from_code);
                book.lots.insert(id, Lot { owner: kind.zip(lot.slot("gncOwner/owner-guid")), invoice: lot.slot("gncInvoice/invoice-guid") });
            }
            book.accounts.push(Account {
                guid,
                name: r.get("act:name").unwrap_or_default(),
                kind: r.trimmed("act:type").unwrap_or_default(),
                commodity: r.commodity("act:commodity"),
                parent: r.trimmed("act:parent"),
                hidden: r.slot("hidden").is_some_and(|v| v.eq_ignore_ascii_case("true")),
            });
        }
        "gnc:transaction" => {
            let Some(guid) = r.trimmed("trn:id") else { return };
            let splits = r
                .parts
                .iter()
                .filter(|p| p.name == "trn:split")
                .filter_map(|s| {
                    Some(Split {
                        account: s.trimmed("split:account")?,
                        memo: s.get("split:memo"),
                        value: s.num("split:value"),
                        quantity: s.num("split:quantity"),
                        lot: s.trimmed("split:lot"),
                    })
                })
                .collect();
            book.transactions.push(Transaction {
                guid,
                currency: r.commodity("trn:currency"),
                num: r.get("trn:num"),
                date: r.get("trn:date-posted/ts:date").and_then(|d| parse_date(&d)),
                description: r.get("trn:description"),
                txn_type: r.slot("trans-txn-type"),
                due: r.slot("trans-date-due").and_then(|d| parse_date(&d)),
                notes: r.slot("notes"),
                splits,
            });
        }
        "gnc:GncCustomer" => book.customers.extend(r.party("cust", "name")),
        "gnc:GncVendor" => book.vendors.extend(r.party("vendor", "name")),
        "gnc:GncEmployee" => book.employees.extend(r.party("employee", "username")),
        "gnc:GncJob" => {
            if let Some(guid) = r.trimmed("job:guid") {
                book.jobs.push(Job { guid, name: r.get("job:name").unwrap_or_default(), active: r.flag("job:active").unwrap_or(true), owner: r.owner("job:owner") });
            }
        }
        "gnc:GncInvoice" => {
            if let Some(guid) = r.trimmed("invoice:guid") {
                book.invoices.push(Invoice {
                    guid,
                    id: r.trimmed("invoice:id"),
                    owner: r.owner("invoice:owner"),
                    terms: r.trimmed("invoice:terms"),
                    post_txn: r.trimmed("invoice:posttxn"),
                    post_lot: r.trimmed("invoice:postlot"),
                    credit_note: r.slot("credit-note").is_some_and(|v| v != "0"),
                });
            }
        }
        "gnc:GncEntry" => book.entries.push(Entry {
            invoice: r.trimmed("entry:invoice"),
            bill: r.trimmed("entry:bill"),
            description: r.get("entry:description"),
            quantity: r.num("entry:qty"),
            // Books from before GnuCash 1.8 name the invoice side without the `i-`.
            i_account: r.trimmed("entry:i-acct").or_else(|| r.trimmed("entry:acct")),
            i_price: if r.get("entry:i-price").is_some() { r.num("entry:i-price") } else { r.num("entry:price") },
            i_discount: if r.get("entry:i-discount").is_some() { r.num("entry:i-discount") } else { r.num("entry:discount") },
            i_discount_type: r.trimmed("entry:i-disc-type").or_else(|| r.trimmed("entry:disc-type")),
            b_account: r.trimmed("entry:b-acct"),
            b_price: r.num("entry:b-price"),
        }),
        "gnc:GncBillTerm" => {
            if let (Some(guid), Some(days)) = (r.trimmed("billterm:guid"), r.trimmed("billterm:days/bt-days:due-days").and_then(|d| d.parse().ok())) {
                book.terms.insert(guid, days);
            }
        }
        _ => {}
    }
}
