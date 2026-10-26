use super::*;

fn vector(name: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/nacha/testdata").join(name)).unwrap()
}

/// moov's vectors sometimes trim trailing spaces; banks (and we) want every record padded to 94.
fn padded(text: &str) -> String {
    text.lines().map(|l| format!("{l:<94}\n")).collect()
}

fn originator() -> Originator {
    Originator {
        immediate_destination: "121042882".into(),
        immediate_destination_name: "Wells Fargo".into(),
        immediate_origin: "1234567890".into(),
        immediate_origin_name: "Acme Widgets LLC".into(),
        company_name: "Acme Widgets".into(),
        company_id: "1234567890".into(),
        entry_description: "Vendor pay".into(),
    }
}

fn entry(routing: &str, account: &str, cents: i64, name: &str, seq: i64, addenda: Option<&str>) -> Entry {
    Entry {
        routing: routing.into(),
        account: account.into(),
        account_type: AccountType::Checking,
        amount_cents: cents,
        individual_id: format!("V{seq}"),
        name: name.into(),
        trace_number: format!("12104288{seq:07}"),
        addenda: addenda.map(str::to_string),
    }
}

fn sample() -> File {
    File {
        originator: originator(),
        created: NaiveDate::from_ymd_opt(2026, 10, 26).unwrap().and_hms_opt(9, 5, 0).unwrap(),
        file_id_modifier: 'A',
        effective_date: NaiveDate::from_ymd_opt(2026, 10, 27).unwrap(),
        batches: vec![
            Batch {
                sec_code: SecCode::Ccd,
                entries: vec![
                    entry("021000021", "123456789", 125_000, "Staples Business Advantage", 1, Some("Bills 1001, 1002")),
                    entry("011000015", "98-7654", 4_999, "Joe's Plumbing", 2, None),
                ],
            },
            Batch { sec_code: SecCode::Ppd, entries: vec![entry("231380104", "55554444", 30_000, "Pat Smith", 3, None)] },
        ],
    }
}

#[test]
fn routing_check_digits() {
    for good in ["121042882", "021000021", "011000015", "231380104", "026009593"] {
        assert!(routing_number_ok(good), "{good}");
    }
    for bad in ["121042883", "021000022", "12104288", "1210428820", "12104288a", ""] {
        assert!(!routing_number_ok(bad), "{bad}");
    }
    assert_eq!(check_digit("12104288"), Some(2));
    assert_eq!(check_digit("1210428"), None);
}

#[test]
fn writes_94_character_records_blocked_to_ten() {
    let text = write(&sample()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(text.ends_with('\n'));
    assert!(lines.iter().all(|l| l.len() == 94), "every record is 94 characters");
    // Header, 2 × (batch header + control), 3 entries, 1 addenda, file control = 10: no filler.
    assert_eq!(lines.len(), 10);
    let mut one_more = sample();
    one_more.batches[1].entries.push(entry("231380104", "1111", 100, "Lee", 4, None));
    let text = write(&one_more).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 20, "11 records block up to 20");
    assert_eq!(lines.iter().filter(|l| l.bytes().all(|b| b == b'9')).count(), 9);
    assert!(lines[10].starts_with("900000200000200000005"), "the file control counts 2 blocks: {}", lines[10]);
}

#[test]
fn writes_each_field_in_its_place() {
    let text = write(&sample()).unwrap();
    let l: Vec<&str> = text.lines().collect();
    let blank = |n: usize| " ".repeat(n);
    // File header: priority 01, destination, origin, date and time, modifier, 094 10 1, names, reference.
    assert_eq!(l[0], format!("101 1210428821234567890261026" ) + "0905A094101" + &format!("{:<23}{:<23}", "WELLS FARGO", "ACME WIDGETS LLC") + &blank(8));
    // Batch header: credits only (220), company name, discretionary data, company ID, SEC code,
    // description, descriptive date, effective date, settlement date (the bank's), status 1, ODFI, batch.
    let batch = |sec: &str, n: u32| format!("5220{:<16}{}1234567890{sec}VENDOR PAY{}261027{}1121042880{n:06}", "ACME WIDGETS", blank(20), blank(6), blank(3));
    assert_eq!(l[1], batch("CCD", 1));
    // Entry: credit to checking (22), RDFI routing with check digit, account, amount, ID, name,
    // discretionary data, addenda indicator, trace number.
    assert_eq!(l[2], format!("622021000021{:<17}0000125000{:<15}{:<22}  1121042880000001", "123456789", "V1", "STAPLES BUSINESS ADVAN"));
    assert_eq!(l[3], format!("705{:<80}00010000001", "BILLS 1001, 1002"));
    assert_eq!(l[4], format!("622011000015{:<17}0000004999{:<15}{:<22}  0121042880000002", "98-7654", "V2", "JOE'S PLUMBING"));
    // Batch control: count (entries and addenda), entry hash 02100002 + 01100001, debits, credits,
    // company ID, message authentication code and reserved (blank), ODFI, batch.
    assert_eq!(l[5], format!("8220000003000320000300000000000000000012999912345678900{}", blank(0)).replace("12345678900", "1234567890") + &blank(25) + "121042880000001");
    assert_eq!(l[6], batch("PPD", 2));
    assert_eq!(l[7], format!("622231380104{:<17}0000030000{:<15}{:<22}  0121042880000003", "55554444", "V3", "PAT SMITH"));
    assert_eq!(l[8], "822000000100231380100000000000000000000300001234567890".to_string() + &blank(25) + "121042880000002");
    // File control: batches, blocks, entries and addenda, hash 3200003 + 23138010, debits, credits.
    assert_eq!(l[9], "9000002000001000000040026338013000000000000000000159999".to_string() + &blank(39));
}

#[test]
fn round_trips_through_the_validator() {
    let file = sample();
    let summary = validate(&write(&file).unwrap()).unwrap();
    assert_eq!((summary.batch_count, summary.block_count, summary.entry_addenda_count), (2, 1, 4));
    assert_eq!((summary.total_debit_cents, summary.total_credit_cents), (0, 159_999));
    assert_eq!(summary.entry_hash, 2_100_002 + 1_100_001 + 23_138_010);
    let traces: Vec<&str> = summary.entries.iter().map(|e| e.trace_number.as_str()).collect();
    assert_eq!(traces, ["121042880000001", "121042880000002", "121042880000003"]);
    assert_eq!(summary.entries[0].addenda.as_deref(), Some("BILLS 1001, 1002"));
    assert_eq!(summary.entries[2].sec_code, "PPD");
    // Windows line endings read the same.
    assert_eq!(validate(&write(&file).unwrap().replace('\n', "\r\n")).unwrap(), summary);
}

#[test]
fn validator_catches_wrong_controls() {
    let text = write(&sample()).unwrap();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let good = lines.clone();
    let join = |lines: &[String]| lines.join("\n") + "\n";

    // A changed amount no longer matches the batch and file totals.
    lines[2].replace_range(29..39, "0000125001");
    let err = validate(&join(&lines)).unwrap_err();
    assert!(err.contains("batch control credit total") && err.contains("file control total credit"), "{err}");

    // A wrong entry hash.
    lines = good.clone();
    lines[5].replace_range(10..20, "0003200004");
    assert!(validate(&join(&lines)).unwrap_err().contains("entry hash"));

    // A missing filler line.
    let mut more = sample();
    more.batches[1].entries.push(entry("231380104", "1111", 100, "Lee", 4, None));
    let mut short = write(&more).unwrap().lines().map(str::to_string).collect::<Vec<_>>();
    short.pop();
    assert!(validate(&join(&short)).unwrap_err().contains("not a multiple of 10"));

    // A record that is not 94 characters.
    lines = good.clone();
    lines[4].push(' ');
    assert!(validate(&join(&lines)).unwrap_err().contains("line 5: is 95 characters"));

    // Trace numbers out of order.
    lines = good.clone();
    lines[4].replace_range(79..94, "121042880000001");
    assert!(validate(&join(&lines)).unwrap_err().contains("not above the one before it"));

    // An addenda indicator with no addenda.
    lines = good;
    lines[4].replace_range(78..79, "1");
    assert!(validate(&join(&lines)).unwrap_err().contains("no addenda follows"));
}

#[test]
fn writer_refuses_bad_details() {
    let mut f = sample();
    f.batches[0].entries[0].routing = "021000022".into();
    assert!(write(&f).unwrap_err().starts_with("ACH_BAD_ROUTING"));
    let mut f = sample();
    f.batches[0].entries[1].account = "12".into();
    assert!(write(&f).unwrap_err().starts_with("ACH_BAD_ACCOUNT"));
    let mut f = sample();
    f.batches[0].entries[1].amount_cents = 0;
    assert!(write(&f).unwrap_err().starts_with("ACH_BAD_AMOUNT"));
    let mut f = sample();
    f.batches[0].entries[1].amount_cents = MAX_ENTRY_CENTS + 1;
    assert!(write(&f).unwrap_err().starts_with("ACH_BAD_AMOUNT"));
    let mut f = sample();
    f.batches[0].entries[1].trace_number = "121042880000001".into();
    assert!(write(&f).unwrap_err().starts_with("ACH_BAD_TRACE"));
    let mut f = sample();
    f.originator.immediate_destination = "121042883".into();
    assert!(write(&f).unwrap_err().starts_with("ACH_BAD_DESTINATION"));
    let mut f = sample();
    f.batches.iter_mut().for_each(|b| b.entries.clear());
    assert!(write(&f).unwrap_err().starts_with("ACH_EMPTY"));
}

#[test]
fn a_nine_digit_origin_gets_a_leading_space() {
    let mut f = sample();
    f.originator.immediate_origin = "231380104".into();
    let text = write(&f).unwrap();
    assert_eq!(&text[13..23], " 231380104");
    validate(&text).unwrap();
}

#[test]
fn agrees_with_moov_test_vectors() {
    let debit = validate(&padded(&vector("ppd-debit.ach"))).unwrap();
    assert_eq!((debit.total_debit_cents, debit.total_credit_cents, debit.entry_hash), (100_000_000, 0, 23_138_010));

    let mixed = validate(&vector("ppd-mixedDebitCredit.ach")).unwrap();
    assert_eq!((mixed.entries.len(), mixed.total_debit_cents, mixed.total_credit_cents), (3, 200_000_000, 200_000_000));
    assert_eq!(mixed.entry_hash, 69_414_030);

    let bad = validate(&padded(&vector("ppd-debit-invalid-entryDetail-checkDigit.ach"))).unwrap_err();
    assert!(bad.contains("routing number 231380105 fails its check digit"), "{bad}");

    let short = validate(&vector("short-line.ach")).unwrap_err();
    assert!(short.contains("line 5: is 74 characters, not 94"), "{short}");
}

#[test]
fn text_fields_are_upper_case_ascii() {
    assert_eq!(alpha("Café Müller & Co.", 16), "CAF  M LLER & CO");
    assert_eq!(alpha("x", 3), "X  ");
    assert_eq!(alpha("  far too long a name  ", 8), "FAR TOO ");
}
