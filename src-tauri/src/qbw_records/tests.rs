use super::*;

/// Record-layer check on a real file's page cache: set LOCALBOOKS_QBW_PAGES to the file the
/// `decodes_pages_of_a_real_file` test writes and run with `--ignored --nocapture`.
#[test]
#[ignore]
fn walks_tables_of_a_real_file() {
    let pages = crate::qbw_pages::load_page_cache(&std::env::var("LOCALBOOKS_QBW_PAGES").expect("set LOCALBOOKS_QBW_PAGES"));
    let catalog = read_catalog(&pages);
    println!("tables named {}, tables with columns {}", catalog.names.len(), catalog.columns.len());
    let mut names: Vec<(&u32, &String)> = catalog.names.iter().filter(|(_, n)| n.starts_with("abmc_") && (n.ends_with("_lineitem") || n.ends_with("_header"))).collect();
    names.sort_by_key(|(_, n)| n.as_str());
    let mut txn: HashMap<i64, i64> = HashMap::new();
    let mut acct: HashMap<i64, i64> = HashMap::new();
    for (id, name) in names {
        let t = table_rows(&pages, &catalog, *id);
        println!("  {name:<40} walked {:>6} unchained {:>4} unwalkable {:>4}", t.rows.len(), t.unchained, t.unwalkable);
        if !name.ends_with("_lineitem") {
            continue;
        }
        for r in &t.rows {
            if r.get("is_no_post_bool").as_int() == Some(1) {
                continue;
            }
            let (Some(tid), Some(aid), Some(c)) = (r.get("transaction_id").as_int(), r.get("account_id").as_int(), r.get("amount_amt").as_cents()) else { continue };
            *txn.entry(tid).or_default() += c;
            *acct.entry(aid).or_default() += c;
        }
    }
    let unbalanced = txn.values().filter(|v| **v != 0).count();
    println!("transactions {} unbalanced {unbalanced}", txn.len());
    let stored = table_rows(&pages, &catalog, catalog.table_id("abmc_account_internal").unwrap());
    let mut matched = 0;
    for r in &stored.rows {
        let id = r.get("account_id").as_int().unwrap_or(-1);
        let st = r.get("ending_balance_amt").as_cents().unwrap_or(0);
        matched += (acct.get(&id).copied().unwrap_or(0) == st) as usize;
    }
    println!("accounts {} matching stored balance exactly {matched}", stored.rows.len());
}

fn col(name: &str, domain: u16, nullable: bool) -> Column {
    Column { id: 0, name: name.into(), domain, nullable, width: 40 }
}

#[test]
fn numerics_decode_as_base_100_with_sign_and_exponent() {
    let c = col("n", 3, false);
    // Examples from the format write-up: 208, 300 and 75.47.
    for (bytes, cents) in [(&[2u8, 0x40, 8, 2][..], 20_800), (&[1, 0x41, 3], 30_000), (&[2, 0x3F, 0x2F, 0x4B], 7_547)] {
        let (v, end) = read_value(bytes, 0, &c).unwrap();
        assert_eq!((v.as_cents(), end), (Some(cents), bytes.len()));
    }
    // From the test company: the same line amount with each sign.
    assert_eq!(read_value(&[3, 0x3F, 0x11, 0x07, 0x01], 0, &c).unwrap().0.as_cents(), Some(10_717));
    assert_eq!(read_value(&[2, 0xBF, 0x11, 0x07], 0, &c).unwrap().0.as_cents(), Some(-717));
    // Zero is stored with no digits and any exponent.
    assert_eq!(read_value(&[0, 0x81], 0, &c).unwrap().0.as_cents(), Some(0));
    // A digit byte above 99 means the walk is out of step.
    assert!(read_value(&[1, 0x40, 0xC8], 0, &c).is_none());
}

#[test]
fn dates_count_minutes_from_1600() {
    let d = col("d", 6, false);
    let minutes = 0x0d15_f920u32.to_le_bytes();
    assert_eq!(read_value(&minutes, 0, &d).unwrap().0, Value::Date(NaiveDate::from_ymd_opt(2017, 8, 2).unwrap()));
}

#[test]
fn rows_walk_with_null_bitmap_and_shorter_old_rows() {
    let cols = vec![col("a", 21, false), col("b", 9, true), col("c", 21, true), col("added_later", 21, true)];
    // len, flags, bitmap (b present, c null), a = 7, b = "hi"; written before added_later existed.
    let row = [11u8, 0, 0x00, 0b1000_0000, 7, 0, 0, 0, 2, b'h', b'i'];
    let values = walk_row(&row, &cols).unwrap();
    assert_eq!(values["a"], Value::Int(7));
    assert_eq!(values["b"].as_text().as_deref(), Some("hi"));
    assert_eq!(values["c"], Value::Null);
    assert_eq!(values.get("added_later").cloned().unwrap_or(Value::Null), Value::Null);
    // Nine nullable columns need a 2-byte bitmap; a row written when there were eight has a
    // 1-byte bitmap and only walks once the ninth column is dropped.
    let mut wide: Vec<Column> = (0..8).map(|i| col(&format!("n{i}"), 21, true)).collect();
    wide.push(col("ninth", 21, true));
    let old = [4u8, 0, 0, 0];
    let values = walk_row(&old, &wide).unwrap();
    assert!(!values.contains_key("ninth"));
}
