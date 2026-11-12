use super::*;

/// Record-layer check on a real file's page cache: set LOCALBOOKS_QBW_PAGES to the file the
/// `decodes_pages_of_a_real_file` test writes and run with `--ignored --nocapture`.
#[test]
#[ignore]
fn walks_tables_of_a_real_file() {
    let pages = crate::qbw_pages::load_page_cache(&std::env::var("LOCALBOOKS_QBW_PAGES").expect("set LOCALBOOKS_QBW_PAGES"));
    let catalog = read_catalog(&pages);
    println!("tables named {}, tables with columns {}", catalog.names.len(), catalog.columns.len());
    let mut names: Vec<(&u32, &String)> =
        catalog.names.iter().filter(|(_, n)| n.starts_with("abmc_") && (n.ends_with("_lineitem") || n.ends_with("_header"))).collect();
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
            let (Some(tid), Some(aid), Some(c)) = (r.get("transaction_id").as_int(), r.get("account_id").as_int(), r.get("amount_amt").as_cents()) else {
                continue;
            };
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

#[test]
fn quantities_scale_to_ten_thousandths_rounding_half_away_from_zero() {
    let num = |coefficient, exponent, negative| Value::Numeric(Numeric { coefficient, exponent, negative });
    // 1.5 (150 × 100⁻¹) is exact.
    assert_eq!(num(150, -1, false).as_quantity_e4(), Some(15_000));
    // 0.123456 rounds to 0.1235; the exact half 0.12345 rounds away from zero on both signs.
    assert_eq!(num(123_456, -3, false).as_quantity_e4(), Some(1_235));
    assert_eq!(num(123_450, -3, false).as_quantity_e4(), Some(1_235));
    assert_eq!(num(123_450, -3, true).as_quantity_e4(), Some(-1_235));
    assert_eq!(num(123_449, -3, false).as_quantity_e4(), Some(1_234));
    // Amounts still need whole cents: the same rounding never applies to `as_cents`.
    assert_eq!(num(123_456, -3, false).as_cents(), None);
    assert_eq!(num(1_234, -1, false).as_cents(), Some(1_234));
    assert_eq!(Value::Int(3).as_quantity_e4(), Some(30_000));
    assert_eq!(Value::Int(i64::MAX).as_quantity_e4(), None);
    assert_eq!(Value::Float(2.5).as_quantity_e4(), Some(25_000));
    assert_eq!(Value::Float(-0.33333).as_quantity_e4(), Some(-3_333));
    assert_eq!(Value::Float(f64::NAN).as_quantity_e4(), None);
    assert_eq!(Value::Float(1e300).as_quantity_e4(), None);
    assert_eq!(Value::Null.as_quantity_e4(), None);
}

#[test]
fn a_double_that_is_not_whole_cents_is_named_not_read() {
    assert_eq!(Value::Float(12.34).as_cents(), Some(1_234));
    assert_eq!(Value::Float(12.34).fractional_amount(), None);
    assert_eq!(Value::Float(10.005).as_cents(), None);
    assert_eq!(Value::Float(10.005).fractional_amount().as_deref(), Some("10.005"));
    assert_eq!(Value::Float(f64::NAN).fractional_amount(), None);
    assert_eq!(Value::Int(7).fractional_amount(), None);
}

/// Item percentages are stored as the percent itself (10 for 10%), base-100 NUMERIC like other
/// amounts; a sales tax item stores its rate negative.
#[test]
fn percentages_read_as_parts_per_million() {
    let n = |coefficient: u128, exponent: i32, negative: bool| Value::Numeric(Numeric { coefficient, exponent, negative });
    assert_eq!(n(10, 0, false).as_percent_ppm(), Some(100_000), "10%");
    assert_eq!(n(805, -1, true).as_percent_ppm(), Some(80_500), "8.05%, sign ignored");
    assert_eq!(n(1_250, -2, false).as_percent_ppm(), Some(1_250), "0.125%");
    assert_eq!(n(1, 1, false).as_percent_ppm(), Some(1_000_000), "100%");
    assert_eq!(n(101, 0, false).as_percent_ppm(), None, "over 100%");
    assert_eq!(n(5, -3, false).as_percent_ppm(), None, "finer than a millionth");
    assert_eq!(n(0, 0, false).as_percent_ppm(), None, "zero is no percent");
    assert_eq!(Value::Float(10.0).as_percent_ppm(), None);
    assert_eq!(Value::Null.as_percent_ppm(), None);
}

fn column(id: u32, name: &str, domain: u16, nullable: bool, width: u32) -> Column {
    Column { id, name: name.into(), domain, nullable, width }
}

/// A row in synthetic form: 3 header bytes, the null bitmap, then the stored values.
fn synthetic_row(bitmap: u8, values: &[&[u8]]) -> Vec<u8> {
    let mut row = vec![0, 0, 0, bitmap];
    for v in values {
        row.extend_from_slice(v);
    }
    row
}

/// A catalog that lost a column's definition gets a placeholder in its place, and rows that store
/// the column still read: the walk tries the column as not stored, then as each likely type.
#[test]
fn rows_read_when_the_catalog_lost_a_column() {
    let mut cols = vec![column(4, "amount_amt", 3, true, 20), column(1, "target_id", 2, false, 4), column(3, "account_id", 2, true, 4)];
    fill_missing_columns(&mut cols);
    assert_eq!(cols.iter().map(|c| (c.id, c.domain)).collect::<Vec<_>>(), [(1, 2), (2, MISSING_DOMAIN), (3, 2), (4, 3)]);
    assert_eq!(cols[1].name, "column_2");
    // A damaged definition with a wild id is not a run of lost columns: nothing is filled in.
    let mut wild = vec![column(1, "target_id", 2, false, 4), column(4_000_000_000, "garbled", 2, true, 4)];
    fill_missing_columns(&mut wild);
    assert_eq!(wild.len(), 2);

    // $720.00: two base-100 digits (20, 7), exponent 0, positive.
    let amount: &[u8] = &[2, 0x40, 20, 7];
    // The row stores column 2 (a nullable id, 41998): three nullable columns, all present.
    let stored = synthetic_row(0b1110_0000, &[&41_997u32.to_le_bytes(), &41_998u32.to_le_bytes(), &30u32.to_le_bytes(), amount]);
    let row = Row { values: walk_row(&stored, &cols).expect("reads with the missing column guessed") };
    assert_eq!(row.get("target_id"), &Value::Int(41_997));
    assert_eq!(row.get("column_2"), &Value::Int(41_998));
    assert_eq!(row.get("account_id"), &Value::Int(30));
    assert_eq!(row.get("amount_amt").as_cents(), Some(72_000));

    // A row that does not store column 2 (it was dropped) reads as it always did.
    let without = synthetic_row(0b1100_0000, &[&41_997u32.to_le_bytes(), &30u32.to_le_bytes(), amount]);
    let row = Row { values: walk_row(&without, &cols).expect("reads without the column") };
    assert_eq!((row.get("account_id"), row.get("column_2")), (&Value::Int(30), &Value::Null));
    assert_eq!(row.get("amount_amt").as_cents(), Some(72_000));

    // The leading columns of a damaged row read as if the lost column were not stored, as they
    // always did, so a row that never had it still gives its account.
    let (prefix, _) = walk_prefix(&without[..without.len() - 1], &cols);
    assert_eq!(prefix.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["target_id", "account_id"]);

    // A garbled id 0 is not one of the ids from 1 up: the gap at 2 is still filled.
    let mut zero = vec![column(0, "garbled", 2, true, 4), column(1, "target_id", 2, false, 4), column(3, "account_id", 2, true, 4)];
    fill_missing_columns(&mut zero);
    assert!(zero.iter().any(|c| c.id == 2 && c.domain == MISSING_DOMAIN));
}

/// A table that lost more column definitions than each row can afford to guess learns its layout
/// once: the search over every guess finds the reading that fits, and the other rows of the table
/// read with that layout directly.
#[test]
fn a_table_that_lost_many_columns_learns_its_layout_once() {
    // Columns 2 to 5 lost: a nullable id, a 2-byte code, a flag and a never-null id, in that order.
    let mut cols = vec![column(1, "target_id", 2, false, 4), column(6, "account_id", 2, true, 4), column(7, "amount_amt", 3, true, 20)];
    fill_missing_columns(&mut cols);
    assert_eq!(cols.iter().filter(|c| c.domain == MISSING_DOMAIN).count(), 4);
    let amount: &[u8] = &[2, 0x40, 20, 7];
    let row = |target: u32, account: u32| {
        // Nullable columns in order: column 2, column 3, account, amount; all present.
        synthetic_row(
            0b1111_0000,
            &[&target.to_le_bytes(), &9u32.to_le_bytes(), &3u16.to_le_bytes(), &[1], &5u32.to_le_bytes(), &account.to_le_bytes(), amount],
        )
    };
    let first = row(41_997, 30);
    assert!(walk_row(&first, &cols).is_none(), "four lost columns are more than each row guesses");
    let (values, layout) = guess_layout(&first, &cols, 4).expect("the search finds the layout");
    let first = Row { values };
    assert_eq!((first.get("account_id"), first.get("amount_amt").as_cents()), (&Value::Int(30), Some(72_000)));
    let second = Row { values: walk_dropping(&row(41_998, 31), &layout).expect("the next row reads with the learned layout") };
    assert_eq!((second.get("target_id"), second.get("account_id")), (&Value::Int(41_998), &Value::Int(31)));
    // Seven gaps are more than a catalog can lose and still be guessed: no placeholders.
    let mut damaged = vec![column(1, "target_id", 2, false, 4), column(9, "account_id", 2, true, 4)];
    fill_missing_columns(&mut damaged);
    assert_eq!(damaged.len(), 2);
}
