use super::*;

fn line(account_id: i64, cents: i64) -> SourceLine {
    SourceLine { account_id, cents, memo: None }
}

fn txn(id: i64, lines: Vec<SourceLine>) -> SourceTransaction {
    SourceTransaction { id, kind: "Check", date: NaiveDate::from_ymd_opt(2024, 7, 30).unwrap(), doc_num: None, name: None, memo: None, lines, document: None }
}

#[test]
fn damaged_lines_are_used_only_when_they_balance_their_transaction() {
    let mut txns: BTreeMap<i64, SourceTransaction> = [
        // Missing a -150000 line; one candidate matches exactly, another does not.
        (1, txn(1, vec![line(7, 150_000)])),
        // Missing -2700 in two lines that only balance together.
        (2, txn(2, vec![line(10, 2_700)])),
        // The candidate's amount is wrong: it must not be used.
        (3, txn(3, vec![line(7, 1_998)])),
        // Already balanced: a stray candidate must not unbalance it.
        (4, txn(4, vec![line(7, 500), line(16, -500)])),
    ]
    .into_iter()
    .collect();
    let salvage: HashMap<i64, Vec<SourceLine>> = [
        (1, vec![line(33, -1_000), line(16, -150_000)]),
        (2, vec![line(23, -1_700), line(24, -1_000)]),
        (3, vec![line(33, -1_997)]),
        (4, vec![line(33, -500)]),
        (9, vec![line(33, -10)]),
    ]
    .into_iter()
    .collect();
    assert_eq!(recover_damaged_lines(&mut txns, salvage).len(), 3);
    assert_eq!(txns[&1].lines, vec![line(7, 150_000), line(16, -150_000)]);
    assert_eq!(txns[&2].imbalance_cents(), 0);
    assert_eq!(txns[&3].lines.len(), 1);
    assert_eq!(txns[&3].imbalance_cents(), 1_998);
    assert_eq!(txns[&4].lines.len(), 2);
}

#[test]
#[ignore]
fn lists_unreadable_line_rows() {
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
    let (pages, catalog, _, _) = read_pages(Path::new(&path)).unwrap();
    for page in &pages {
        let Some(name) = catalog.names.get(&page.owner) else { continue };
        if !name.ends_with("_lineitem") || CHUNK_KINDS.contains(&page.kind) {
            continue;
        }
        let cols = &catalog.columns[&page.owner];
        for r in &page.rows {
            let walks = r.chained && walk_row(page.row(r), cols).is_some();
            if !walks {
                let zone = if r.start as usize >= S7_START { "s7" } else if (r.start + r.len) as usize > S7_START { "crosses" } else { "s0-6" };
                println!("{name:<36} page {:>5} kind {} row {:>4}+{:<4} chained {:<5} {zone:<7} s7 {:?} q {} r7 {} rows on page {}", page.number, page.kind, r.start, r.len, r.chained, page.sector7, page.q, page.r7, page.rows.len());
            }
        }
    }
}

/// Traces the transactions that do not balance: every line row of the involved transactions,
/// readable or not (unreadable rows are partially decoded), with page and sector-7 status.
#[test]
#[ignore]
fn traces_unbalanced_transactions() {
    use crate::qbw_records::walk_prefix;
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
    let (pages, catalog, _, _) = read_pages(Path::new(&path)).unwrap();
    let ledger = read_ledger(Path::new(&path)).unwrap();
    let wanted: HashSet<i64> = ledger.unbalanced.iter().map(|t| t.id).collect();
    let names: HashMap<i64, &str> = ledger.accounts.iter().map(|a| (a.id, a.full_name.as_str())).collect();
    for t in &ledger.unbalanced {
        println!("txn {} {} #{:?} {} off {}", t.id, t.kind, t.doc_num, t.date, crate::ledger::format_cents(t.imbalance_cents()));
    }
    for page in &pages {
        let Some(name) = catalog.names.get(&page.owner) else { continue };
        if !name.ends_with("_lineitem") || CHUNK_KINDS.contains(&page.kind) {
            continue;
        }
        let cols = &catalog.columns[&page.owner];
        for r in &page.rows {
            let bytes = page.row(r);
            let full = if r.chained { walk_row(bytes, cols) } else { None };
            let (prefix, stop) = walk_prefix(bytes, cols);
            let get = |k: &str| -> Value {
                match &full {
                    Some(v) => v.get(k).cloned().unwrap_or(Value::Null),
                    None => prefix.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone()).unwrap_or(Value::Null),
                }
            };
            let tid = get("transaction_id").as_int().unwrap_or(-1);
            let interesting = wanted.contains(&tid) || full.is_none();
            if !interesting {
                continue;
            }
            let zone = if r.start as usize >= S7_START { "s7" } else if (r.start + r.len) as usize > S7_START { "crosses" } else { "s0-6" };
            let acct = get("account_id").as_int();
            println!(
                "{} {name:<34} page {:>5} k{} row {:>4}+{:<4} chained {:<5} {zone:<7} {:?} | txn {tid} target {:?} acct {:?} ({}) amt {:?} date {:?} | stop {stop}/{} after {:?}",
                if full.is_some() { "OK  " } else { "FAIL" },
                page.number, page.kind, r.start, r.len, r.chained, page.sector7,
                get("target_id").as_int(), acct, acct.and_then(|a| names.get(&a)).unwrap_or(&"?"),
                get("amount_amt").as_cents().map(crate::ledger::format_cents), get("transaction_date").as_date(),
                bytes.len(), prefix.last().map(|(n, _)| n.as_str())
            );
        }
    }
}

/// Lists transaction headers that decode but have no lines in the ledger (whole transactions
/// missing), with the header's own total where the table has one.
#[test]
#[ignore]
fn lists_headers_without_lines() {
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
    let (pages, catalog, _, _) = read_pages(Path::new(&path)).unwrap();
    let ledger = read_ledger(Path::new(&path)).unwrap();
    let have: HashSet<i64> = ledger.transactions.iter().chain(&ledger.unbalanced).map(|t| t.id).collect();
    for (stem, kind) in TRANSACTION_TABLES {
        let mut missing = Vec::new();
        for h in rows_of(&pages, &catalog, &format!("abmc_{stem}_header")) {
            let Some(tid) = h.get("transaction_id").as_int() else { continue };
            if h.get("is_no_post_bool").as_int() == Some(1) || have.contains(&tid) {
                continue;
            }
            missing.push(format!("{tid} {:?} #{:?} {:?}", h.get("transaction_date").as_date(), text(&h, "doc_num"), text(&h, "memo")));
        }
        if !missing.is_empty() {
            println!("{kind}: {} headers without lines", missing.len());
            for m in missing.iter().take(12) {
                println!("   {m}");
            }
        }
    }
}

/// Ledger read of a real company file (read-only): set LOCALBOOKS_QBW_CHECK and run
/// `cargo test --release -- --ignored reads_the_ledger_of_a_real --nocapture`.
#[test]
#[ignore]
fn reads_the_ledger_of_a_real_file() {
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
    let started = std::time::Instant::now();
    let ledger = read_ledger(Path::new(&path)).unwrap();
    let q = &ledger.quality;
    println!("read in {:.1}s; refined pages {}; recovered lines {}", started.elapsed().as_secs_f64(), q.refined_pages, q.recovered_lines);
    println!(
        "accounts {} (stored balances matched {}/{}), transactions {} balanced, {} unbalanced; unreadable lines {}, rejected {}",
        ledger.accounts.len(),
        q.accounts_matching_stored,
        q.accounts_with_stored_balance,
        ledger.transactions.len(),
        ledger.unbalanced.len(),
        q.unreadable_lines,
        q.rejected_lines
    );
    let mut by_kind: BTreeMap<&str, usize> = BTreeMap::new();
    for t in &ledger.transactions {
        *by_kind.entry(t.kind).or_default() += 1;
    }
    println!("by kind: {by_kind:?}");
    println!("rejected: {:?}", q.rejected_by_reason);
    for u in &ledger.unreadable {
        println!("  unreadable line: {:?}", u);
    }
    println!(
        "lists: {} customers ({} active), {} vendors, {} items",
        ledger.customers.len(),
        ledger.customers.iter().filter(|c| c.active).count(),
        ledger.vendors.len(),
        ledger.items.len()
    );
    for c in ledger.customers.iter().filter(|c| c.balance_cents.is_some_and(|b| b != 0)).take(5) {
        println!("  customer with balance: {:?}", c);
    }
    for i in ledger.items.iter().take(4) {
        println!("  item: {:?}", i);
    }
    if let (Some(first), Some(last)) = (ledger.transactions.first(), ledger.transactions.last()) {
        println!("dates {} .. {}", first.date, last.date);
    }
    for t in &ledger.unbalanced {
        println!("  unbalanced {} #{:?} {} {:?} off by {}", t.kind, t.doc_num, t.date, t.name, crate::ledger::format_cents(t.imbalance_cents()));
    }
    let names: HashMap<i64, &str> = ledger.accounts.iter().map(|a| (a.id, a.full_name.as_str())).collect();
    let mut totals: HashMap<i64, i64> = HashMap::new();
    for t in &ledger.transactions {
        for l in &t.lines {
            *totals.entry(l.account_id).or_default() += l.cents;
        }
    }
    for a in ledger.accounts.iter().filter(|a| a.stored_balance_cents.is_some_and(|s| s != totals.get(&a.id).copied().unwrap_or(0))) {
        println!(
            "  differs: {:<40} stored {:>14} read {:>14}",
            names[&a.id],
            crate::ledger::format_cents(a.stored_balance_cents.unwrap()),
            crate::ledger::format_cents(totals.get(&a.id).copied().unwrap_or(0))
        );
    }
}
