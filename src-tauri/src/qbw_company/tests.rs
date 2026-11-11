use super::*;

fn line(account_id: i64, cents: i64) -> SourceLine {
    SourceLine { account_id, cents, memo: None, class: None }
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
    println!(
        "read in {:.1}s; refined pages {}; recovered lines {}; joined transactions {}",
        started.elapsed().as_secs_f64(),
        q.refined_pages,
        q.recovered_lines,
        q.joined_transactions
    );
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
    for g in &ledger.sales_lists.item_groups {
        println!("  group {} ({} items, print items {}): {:?}", g.name, g.members.len(), g.print_items, g.members);
    }
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
    // LOCALBOOKS_QBW_ACCOUNT=<full name>: every transaction with a line on that account.
    if let Ok(name) = std::env::var("LOCALBOOKS_QBW_ACCOUNT") {
        let id = ledger.accounts.iter().find(|a| a.full_name == name).map(|a| a.id);
        for t in ledger.transactions.iter().filter(|t| t.lines.iter().any(|l| Some(l.account_id) == id)) {
            let lines: Vec<String> = t.lines.iter().map(|l| format!("{} {}", l.account_id, crate::ledger::format_cents(l.cents))).collect();
            println!("  on {name}: {} {} {} [{}]", t.kind, t.id, t.date, lines.join(", "));
        }
    }
    for t in &ledger.unbalanced {
        println!("  unbalanced {} #{:?} {} {:?} off by {}", t.kind, t.doc_num, t.date, t.name, crate::ledger::format_cents(t.imbalance_cents()));
        if std::env::var("LOCALBOOKS_QBW_LINES").is_ok() {
            for l in &t.lines {
                println!("      transaction {} account {} {}", t.id, l.account_id, crate::ledger::format_cents(l.cents));
            }
        }
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

#[test]
fn custom_field_values_take_the_label_defined_for_their_list() {
    // Position 1 is a name field on customers and vendors, 2 is customers only; items have their own 1.
    let definitions = vec![
        (1, "Region".to_string(), [true, true, false]),
        (2, "Birthday".to_string(), [true, false, false]),
        (1, "Color".to_string(), [false, false, true]),
    ];
    let values = vec![
        ("C1".to_string(), 2, "May 4".to_string()),
        ("C1".to_string(), 1, " West ".to_string()),
        ("V1".to_string(), 2, "ignored: not a vendor field".to_string()),
        ("V1".to_string(), 1, "East".to_string()),
        ("I1".to_string(), 1, "Red".to_string()),
        ("C2".to_string(), 1, "  ".to_string()),
    ];
    let customers = custom_values(&definitions, &values, FieldOwner::Customer);
    assert_eq!(customers["C1"], [("Region".to_string(), "West".to_string()), ("Birthday".into(), "May 4".into())]);
    assert!(!customers.contains_key("C2"), "blank values are skipped");
    let vendors = custom_values(&definitions, &values, FieldOwner::Vendor);
    assert_eq!(vendors["V1"], [("Region".to_string(), "East".to_string())]);
    // Items read their own definitions: position 1 is Color for them.
    assert_eq!(custom_values(&definitions, &values, FieldOwner::Item)["I1"], [("Color".to_string(), "Red".to_string())]);
}

#[test]
fn sales_lists_keep_one_rep_per_set_of_initials() {
    let mut lists = SourceSalesLists::default();
    lists.add_rep("JS", true);
    lists.add_rep("js:Jane Smith", true);
    lists.add_rep("JS:Someone Else", false);
    lists.add_ship_method("UPS", true);
    lists.add_ship_method(" ups ", false);
    assert_eq!(lists.reps, [("js:Jane Smith".to_string(), true)]);
    assert_eq!(lists.ship_methods, [("UPS".to_string(), true)]);
}

#[test]
fn price_levels_read_as_the_file_stores_them() {
    use crate::price_levels::{ItemBasis, LevelItem, LevelKind};
    // Fixed %: value_pct −10.5 (hundredths −1050) → −10,500 thousandths.
    let fixed = price_level_of("Wholesale".into(), false, Some(1), Some(-1_050), &[]);
    assert_eq!((fixed.kind, fixed.percent_milli, fixed.active, fixed.items.len()), (LevelKind::FixedPercent, -10_500, true, 0));
    // Per item: the computed prices (stored negative like item prices) become fixed prices; an
    // item with no amount is left off.
    let per = price_level_of("Contract".into(), true, Some(2), None, &[(7, Some(-1_250)), (8, None)]);
    assert_eq!((per.kind, per.active), (LevelKind::PerItem, false));
    assert_eq!(per.items, [LevelItem { product_id: 7, basis: ItemBasis::Fixed, price_cents: Some(1_250), percent_milli: None }]);
    // An unknown type that lists items is per item.
    assert_eq!(price_level_of("X".into(), false, Some(0), None, &[(7, Some(100))]).kind, LevelKind::PerItem);
}

/// A cost row as the file keeps it: the form line it belongs to, an account and an amount
/// (negative = debit). Made-up values.
fn cost_row(target: i64, account: i64, cents: i64) -> Row {
    let amount = crate::qbw_records::Numeric { coefficient: cents.unsigned_abs() as u128, exponent: -1, negative: cents < 0 };
    Row {
        values: [
            ("target_id".to_string(), Value::Int(target)),
            ("account_id".to_string(), Value::Int(account)),
            ("amount_amt".to_string(), Value::Numeric(amount)),
        ]
        .into_iter()
        .collect(),
    }
}

#[test]
fn inventory_cost_joins_the_form_it_was_sold_on() {
    // Invoice 1: receivable 120.00 debit, income 120.00 credit. Accounts: 4 receivable, 26
    // income, 30 cost of goods sold, 6 inventory asset.
    let mut txns: BTreeMap<i64, SourceTransaction> = [(1, txn(1, vec![line(4, -12_000), line(26, 12_000)]))].into_iter().collect();
    let form_lines: HashMap<i64, (i64, Option<String>)> = [(501, (1, Some("Remodel".to_string()))), (502, (1, None)), (503, (9, None))].into_iter().collect();
    let known: HashSet<i64> = [4, 26, 30, 6].into_iter().collect();
    let rows = [
        // Line 501 sold stock that cost 73.50: debit cost of goods sold, credit inventory.
        cost_row(501, 30, -7_350),
        cost_row(501, 6, 7_350),
        // Line 502's pair does not net to zero (one row lost): neither row is used.
        cost_row(502, 30, -1_000),
        cost_row(502, 6, 900),
        // Line 503 belongs to a transaction that was not read.
        cost_row(503, 30, -500),
        cost_row(503, 6, 500),
        // A zero-cost line adds nothing.
        cost_row(504, 30, 0),
    ];
    let mut quality = LedgerQuality::default();
    add_inventory_cost(&mut txns, &form_lines, &rows, &known, &mut quality);

    let invoice = &txns[&1];
    assert_eq!(invoice.imbalance_cents(), 0);
    assert_eq!(&invoice.lines[2..], [
        SourceLine { account_id: 30, cents: -7_350, memo: None, class: Some("Remodel".into()) },
        SourceLine { account_id: 6, cents: 7_350, memo: None, class: Some("Remodel".into()) },
    ]);
    assert_eq!(quality.rejected_lines, 4);
    assert_eq!(quality.rejected_by_reason["inventory cost is not pairs that net to zero"], 2);
    assert_eq!(quality.rejected_by_reason["inventory cost of a form line that was not read"], 2);
}

/// Synthetic pages for `repair_pages`: a table list, a column list whose second column's row
/// sits in sector 7 with one wrong byte, and a page of rows of that table.
mod catalog_repair {
    use super::*;
    use crate::qbw_pages::{RowSpan, Sector7, ROW_AREA_END};

    fn page(number: u64, owner: u32, rows: &[(usize, Vec<u8>)]) -> LogicalPage {
        let mut bytes = vec![0u8; ROW_AREA_END];
        let mut spans = Vec::new();
        for (start, row) in rows {
            bytes[*start..*start + row.len()].copy_from_slice(row);
            spans.push(RowSpan { start: *start as u16, len: row.len() as u16, chained: true });
        }
        LogicalPage { number, kind: 0, owner, bytes, rows: spans, sector7: Sector7::BestEffort, q: 0, r7: 0, c_exact: None }
    }

    fn with_len(mut row: Vec<u8>) -> Vec<u8> {
        let len = row.len() as u16;
        row[..2].copy_from_slice(&len.to_le_bytes());
        row
    }

    fn table_row(table: u32, name: &str) -> Vec<u8> {
        let mut r = vec![0u8; 55];
        r[4..8].copy_from_slice(&table.to_le_bytes());
        r[54] = name.len() as u8;
        r.extend_from_slice(name.as_bytes());
        with_len(r)
    }

    fn column_row(table: u32, id: u32, name: &str, nullable: u8) -> Vec<u8> {
        let mut r = vec![0u8; 39];
        r[4..8].copy_from_slice(&table.to_le_bytes());
        r[8..12].copy_from_slice(&id.to_le_bytes());
        r[12..14].copy_from_slice(&2u16.to_le_bytes()); // a 4-byte integer
        r[14] = 1;
        r[15] = nullable;
        r[16..20].copy_from_slice(&4u32.to_le_bytes());
        r[38] = name.len() as u8;
        r.extend_from_slice(name.as_bytes());
        with_len(r)
    }

    #[test]
    fn a_damaged_catalog_page_is_re_solved_before_its_table_is_read() {
        let widget = 700;
        let good = column_row(widget, 2, "size_id", b'N');
        let mut damaged = good.clone();
        damaged[15] = 0x9B; // the nullable flag, garbled by a wrong sector-7 decode
        let data = with_len([vec![0u8; 3], 41i32.to_le_bytes().to_vec(), 7i32.to_le_bytes().to_vec()].concat());
        let mut pages = vec![
            page(10, 1, &[(200, table_row(widget, "abmc_widget"))]),
            page(11, 2, &[(200, column_row(widget, 1, "widget_id", b'N')), (S7_START + 40, damaged)]),
            page(12, widget, &[(300, data.clone())]),
        ];
        let fixed = page(11, 2, &[(200, column_row(widget, 1, "widget_id", b'N')), (S7_START + 40, good)]);
        assert_eq!(read_catalog(&pages).columns[&widget].len(), 1, "the damaged definition is not read");

        // Stands in for the decoder: page 11 has a better decode; nothing else does.
        let refine = |p: &LogicalPage, score: &dyn Fn(&LogicalPage) -> usize| (p.number == 11 && score(&fixed) > score(p)).then(|| fixed.clone());
        let (catalog, refined) = repair_pages(&mut pages, &refine);
        assert_eq!(refined, 1);
        let cols = &catalog.columns[&widget];
        assert_eq!(cols.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["widget_id", "size_id"]);
        let row = walk_row(&data, cols).expect("the table's row now reads");
        assert_eq!((row["widget_id"].as_int(), row["size_id"].as_int()), (Some(41), Some(7)));
    }
}

/// A cost row with its record id and kind, as a real file stores them.
fn cost_record(record: i64, target_type: i64, target: i64, account: i64, cents: i64) -> Row {
    let mut row = cost_row(target, account, cents);
    row.values.insert("inventory_record_id".to_string(), Value::Int(record));
    row.values.insert("target_type".to_string(), Value::Int(target_type));
    row
}

#[test]
fn inventory_cost_rows_are_counted_once_and_summed_with_checks() {
    let mut txns: BTreeMap<i64, SourceTransaction> = [(1, txn(1, vec![line(4, -12_000), line(26, 12_000)]))].into_iter().collect();
    let form_lines: HashMap<i64, (i64, Option<String>)> = [601, 602, 603, 604, 605].into_iter().map(|t| (t, (1, None))).collect();
    let known: HashSet<i64> = [4, 26, 30, 6].into_iter().collect();
    let rows = [
        // Line 601's pair, then the same two records again (read twice from a re-solved page).
        cost_record(11, 2, 601, 30, -2_000),
        cost_record(11, 1, 601, 6, 2_000),
        cost_record(11, 2, 601, 30, -2_000),
        cost_record(11, 1, 601, 6, 2_000),
        // Line 602 drew on two purchases: two pairs, both kept.
        cost_record(12, 2, 602, 30, -300),
        cost_record(12, 1, 602, 6, 300),
        cost_record(13, 2, 602, 30, -450),
        cost_record(13, 1, 602, 6, 450),
        // Line 603's amounts would overflow when added: rejected, not wrapped to zero.
        cost_record(14, 2, 603, 30, i64::MAX),
        cost_record(15, 2, 603, 30, i64::MAX),
        cost_record(14, 1, 603, 6, -1),
        cost_record(15, 1, 603, 6, -1),
        // Line 604 has one debit and two credits: not pairs, rejected though it nets to zero.
        cost_record(16, 2, 604, 30, -100),
        cost_record(16, 1, 604, 6, 50),
        cost_record(17, 1, 604, 6, 50),
        // Line 605 drew the same cost twice; its rows carry no record id, so nothing says they are
        // one row read twice and all four are kept.
        cost_row(605, 30, -500),
        cost_row(605, 6, 500),
        cost_row(605, 30, -500),
        cost_row(605, 6, 500),
    ];
    let mut quality = LedgerQuality::default();
    add_inventory_cost(&mut txns, &form_lines, &rows, &known, &mut quality);

    let cents: Vec<i64> = txns[&1].lines[2..].iter().map(|l| l.cents).collect();
    assert_eq!(cents, [-2_000, 2_000, -300, 300, -450, 450, -500, 500, -500, 500]);
    assert_eq!(txns[&1].imbalance_cents(), 0);
    assert_eq!((quality.rejected_lines, quality.rejected_by_reason["inventory cost is not pairs that net to zero"]), (7, 7));
}

#[test]
fn an_imbalance_that_overflows_is_never_zero() {
    assert_eq!(txn(1, vec![line(4, i64::MAX), line(26, 1)]).imbalance_cents(), i64::MAX);
    assert_eq!(txn(2, vec![line(4, i64::MAX), line(26, -i64::MAX)]).imbalance_cents(), 0);
    assert_eq!(checked_total([i64::MIN, -1]), None);
}

#[test]
fn item_type_codes_map_to_kinds_and_the_rest_are_left_off() {
    let kinds: Vec<Option<&str>> = (0..=12).map(|c| item_kind(Some(c))).collect();
    assert_eq!(
        kinds,
        [
            Some("Service"),
            Some("Inventory part"),
            Some("Inventory part"), // an assembly
            Some("Non-inventory part"),
            Some("Fixed asset"),
            Some("Other charge"),
            None, // subtotal: a form line, not an item
            None, // group
            Some("Discount"),
            None, // payment
            None, // sales tax item
            None, // sales tax group
            Some("Item"), // a code this reader does not know
        ]
    );
    assert_eq!(item_kind(None), Some("Item"));
}

/// Each table of inventory cost rows is read with the line table of its own form, bills included
/// (a bill that changes the cost of stock already sold keeps its cost rows there).
#[test]
fn inventory_cost_tables_pair_with_their_forms() {
    for (cost, lines) in INVENTORY_COST_TABLES {
        let form = cost.strip_suffix("_inventory_lineitem").expect("a cost table");
        assert!(lines.starts_with(&form.replace("sale_receipt", "sales_receipt")), "{cost} pairs with {lines}");
    }
    assert!(INVENTORY_COST_TABLES.contains(&("abmc_bill_inventory_lineitem", "abmc_bill_lineitem")));
}

/// Item groups are the type-7 items; their items come from the grouped-items rows in the order
/// column 2 gives (found by id, since some files lost its name), with quantities made positive and
/// an empty quantity taken as 1.
/// The parts of an assembly are in the same table and are not a group.
#[test]
fn item_groups_read_their_items_in_order() {
    use crate::qbw_records::Numeric;
    let int = |v: i64| Value::Int(v);
    let quantity = |q: u128| Value::Numeric(Numeric { coefficient: q, exponent: 0, negative: true });
    let row = |values: Vec<(&str, Value)>| Row { values: values.into_iter().map(|(k, v)| (k.to_string(), v)).collect() };
    let users = vec![
        row(vec![("item_id", int(60)), ("item_type", int(7)), ("name", Value::Bytes(b"Door kit".to_vec())), ("print_grouped_bool", int(1)), ("is_hidden_bool", int(0))]),
        row(vec![("item_id", int(61)), ("item_type", int(2)), ("name", Value::Bytes(b"Framed door".to_vec()))]),
        row(vec![("item_id", int(20)), ("item_type", int(1)), ("name", Value::Bytes(b"Frame".to_vec()))]),
        row(vec![("item_id", int(21)), ("item_type", int(1)), ("name", Value::Bytes(b"Hinge".to_vec()))]),
    ];
    let grouped = vec![
        row(vec![("item_id", int(60)), ("column_2", int(1)), ("sub_item_id", int(21)), ("quantity_qnty", quantity(3))]),
        row(vec![("item_id", int(60)), ("column_2", int(0)), ("sub_item_id", int(20)), ("quantity_qnty", quantity(1))]),
        row(vec![("item_id", int(61)), ("column_2", int(0)), ("sub_item_id", int(20)), ("quantity_qnty", quantity(1))]),
        // A blank line in the group, then an item with no quantity (typed on the form): it comes in at 1.
        row(vec![("item_id", int(60)), ("column_2", int(3)), ("sub_item_id", int(0))]),
        row(vec![("item_id", int(60)), ("column_2", int(2)), ("sub_item_id", int(22))]),
    ];
    let groups = item_groups_from(&users, &grouped, Some("column_2"));
    assert_eq!(groups.len(), 1, "the assembly is not a group");
    let kit = &groups[0];
    assert_eq!((kit.name.as_str(), kit.print_items, kit.active), ("Door kit", true, true));
    assert_eq!(kit.members, [(20, 10_000), (21, 30_000), (22, 10_000)]);
}

/// A memorized transaction is a template kept for entering the same transaction again: marked on
/// its lines or on its header, it posts nothing and is not read as a transaction.
#[test]
fn memorized_transactions_are_templates() {
    let row = |memorized: Option<i64>| Row {
        values: memorized.map(|m| ("is_memorized_transaction_bool".to_string(), Value::Int(m))).into_iter().collect(),
    };
    assert!(!memorized(&row(Some(0)), Some(&row(Some(0)))));
    assert!(!memorized(&row(None), None));
    assert!(memorized(&row(Some(1)), None), "marked on the line");
    assert!(memorized(&row(Some(0)), Some(&row(Some(1)))), "marked on the header");
}

/// One movement kept as two halves on the same day (a payment with only its receivables line, a
/// deposit with only its bank line) is read as one transaction under the named half. Halves that
/// could pair with more than one other transaction are left alone.
#[test]
fn split_halves_of_one_movement_are_joined() {
    let half = |id: i64, date: &str, name: Option<&str>, lines: Vec<SourceLine>| SourceTransaction {
        name: name.map(String::from),
        date: date.parse().unwrap(),
        ..txn(id, lines)
    };
    let mut txns: BTreeMap<i64, SourceTransaction> = [
        half(62, "2025-03-01", Some("Home office"), vec![line(4, 750_000)]),
        half(77, "2025-03-01", None, vec![line(1, -750_000)]),
        // Three halves of 100.00 on one day: no unique pair, so none is joined.
        half(80, "2025-04-01", None, vec![line(1, -10_000)]),
        half(81, "2025-04-01", None, vec![line(4, 10_000)]),
        half(82, "2025-04-01", None, vec![line(4, 10_000)]),
        // The same amounts on different days are not one movement.
        half(90, "2025-05-01", None, vec![line(1, -5_000)]),
        half(91, "2025-05-02", None, vec![line(4, 5_000)]),
    ]
    .into_iter()
    .map(|t| (t.id, t))
    .collect();
    assert_eq!(join_split_halves(&mut txns), 1);
    assert!(!txns.contains_key(&77), "the unnamed half goes into the named one");
    assert_eq!((txns[&62].lines.len(), txns[&62].imbalance_cents()), (2, 0));
    assert_eq!(txns.values().filter(|t| t.imbalance_cents() != 0).count(), 5);
}
