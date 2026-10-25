use super::*;
use crate::db::test_pool;

async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1")
        .bind(number)
        .fetch_one(pool)
        .await
        .expect("seeded account exists")
}

#[test]
fn parses_amounts_to_exact_cents() {
    let cases = [
        ("12.34", 1_234),
        ("-12.34", -1_234),
        ("$1,234.56", 123_456),
        ("(45.00)", -4_500),
        ("45.00-", -4_500),
        ("+3", 300),
        ("0.1", 10),
        (".05", 5),
        ("7.500", 750),
        ("-$0.99", -99),
    ];
    for (raw, cents) in cases {
        assert_eq!(parse_amount_cents(raw), Ok(cents), "{raw}");
    }
    for bad in ["", "abc", "1.234", "1.2.3", "12e3", "--5", "."] {
        assert!(parse_amount_cents(bad).is_err(), "{bad} should fail");
    }
}

#[test]
fn parses_common_date_formats() {
    let expected = NaiveDate::from_ymd_opt(2026, 3, 7).unwrap();
    for raw in [
        "2026-03-07", "03/07/2026", "3/7/2026", "3/7/26", "Mar 07, 2026", "2026-03-07 00:00:00", "2026-03-07T09:30:00Z",
        "2026/03/07", "03-07-2026", "7 Mar 2026", "07-Mar-2026", "March 7, 2026", "20260307", "20260307120000.000[-5:EST]",
    ] {
        assert_eq!(parse_date(raw), Ok(expected), "{raw}");
    }
    for raw in ["07/03/2026", "7.3.2026", "07-03-26"] {
        assert_eq!(parse_date_ordered(raw, DateOrder::Dmy), Ok(expected), "{raw}");
    }
    for bad in ["", "2026", "2026-02-30", "13/45/2026", "Smarch 7, 2026", "2026-03", "1/2/3/4", "12345", "03/07/123456", "3/7/1850"] {
        assert!(parse_date(bad).is_err(), "{bad} should be rejected");
    }
}

/// The date parser the single-pass one replaced: chrono formats tried in turn.
fn legacy_parse_date(raw: &str) -> Result<NaiveDate, String> {
    const FORMATS: &[&str] = &["%Y-%m-%d", "%m/%d/%y", "%m/%d/%Y", "%m-%d-%y", "%m-%d-%Y", "%Y/%m/%d", "%b %d, %Y", "%d %b %Y"];
    let s = raw.trim();
    let s = s.split_once(' ').filter(|(d, _)| d.contains(['-', '/'])).map_or(s, |(d, _)| d);
    FORMATS
        .iter()
        .find_map(|fmt| NaiveDate::parse_from_str(s, fmt).ok())
        .filter(|d| (1900..=2200).contains(&d.year()))
        .ok_or_else(|| format!("unrecognized date '{raw}'"))
}

fn date_samples() -> Vec<String> {
    let mut samples = Vec::new();
    let months = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    for (y, yy) in [(1999, 99), (2000, 0), (2024, 24), (2026, 26), (2069, 69), (1970, 70)] {
        for m in 1..=12u32 {
            for d in [1u32, 9, 12, 13, 28, 29, 30, 31] {
                samples.push(format!("{y}-{m:02}-{d:02}"));
                samples.push(format!("{m:02}/{d:02}/{y}"));
                samples.push(format!("{m}/{d}/{yy:02}"));
                samples.push(format!("{m:02}-{d:02}-{y}"));
                samples.push(format!("{y}/{m:02}/{d:02}"));
                samples.push(format!("{} {d:02}, {y}", months[m as usize - 1]));
                samples.push(format!("{d} {} {y}", months[m as usize - 1]));
                samples.push(format!("{m:02}/{d:02}/{y} 10:15:00"));
            }
        }
    }
    samples
}

#[test]
fn single_pass_date_parser_agrees_with_the_chrono_formats_it_replaced() {
    let samples = date_samples();
    for raw in &samples {
        assert_eq!(parse_date(raw), legacy_parse_date(raw), "{raw}");
    }
    assert!(samples.len() > 2_000);
}

#[test]
fn detects_day_first_dates_and_says_when_it_is_guessing() {
    assert_eq!(detect_date_order(["03/07/2026", "15/07/2026"]), (DateOrder::Dmy, true));
    assert_eq!(detect_date_order(["03/07/2026", "07/15/2026"]), (DateOrder::Mdy, true));
    assert_eq!(detect_date_order(["03/07/2026", "05/06/2026"]), (DateOrder::Mdy, false), "every date reads both ways");
    assert_eq!(detect_date_order(["2026-03-07", "2026-03-08"]), (DateOrder::Mdy, true), "ISO dates are unambiguous");
    assert_eq!(detect_date_order(["01/01/2026"]), (DateOrder::Mdy, true), "same date either way");
}

#[test]
#[ignore]
fn benchmark_date_parsing() {
    let samples = date_samples();
    let rounds = 50;
    let time = |f: &dyn Fn(&str) -> Result<NaiveDate, String>| {
        let start = std::time::Instant::now();
        let mut ok = 0;
        for _ in 0..rounds {
            for s in &samples {
                ok += usize::from(f(std::hint::black_box(s)).is_ok());
            }
        }
        (start.elapsed(), ok)
    };
    let (old, old_ok) = time(&legacy_parse_date);
    let (new, new_ok) = time(&parse_date);
    let n = (samples.len() * rounds) as f64;
    println!(
        "{} dates: chrono formats {:.0} ns/date, single pass {:.0} ns/date ({:.1}x faster)",
        samples.len() * rounds,
        old.as_nanos() as f64 / n,
        new.as_nanos() as f64 / n,
        old.as_secs_f64() / new.as_secs_f64()
    );
    assert_eq!(old_ok, new_ok);
}

#[test]
fn recognizes_column_layouts() {
    let h = |cols: &[&str]| cols.iter().map(|c| c.to_string()).collect::<Vec<_>>();
    let signed = detect_mapping(&h(&["Posted Date", "Reference Number", "Payee", "Address", "Amount"])).unwrap();
    assert_eq!((signed.date.as_str(), signed.description.as_str(), signed.amount.as_deref()), ("Posted Date", "Payee", Some("Amount")));

    // "Debit Amount"/"Credit Amount" contain "amount" but are a split layout, not a signed amount.
    let split = detect_mapping(&h(&["Date", "Description", "Debit Amount", "Credit Amount"])).unwrap();
    assert_eq!((split.amount, split.money_in.as_deref(), split.money_out.as_deref()), (None, Some("Credit Amount"), Some("Debit Amount")));

    let uk = detect_mapping(&h(&["Date", "Details", "Paid out", "Paid in", "Balance"])).unwrap();
    assert_eq!((uk.money_in.as_deref(), uk.money_out.as_deref()), (Some("Paid in"), Some("Paid out")));
    let plural = detect_mapping(&h(&["Transaction Date", "Description", "Withdrawals", "Deposits"])).unwrap();
    assert_eq!((plural.money_in.as_deref(), plural.money_out.as_deref()), (Some("Deposits"), Some("Withdrawals")));

    assert!(detect_mapping(&h(&["Fecha", "Concepto", "Importe"])).is_none(), "unrecognized: the user maps it");
}

#[test]
fn split_columns_net_money_in_minus_money_out_whatever_their_signs() {
    let csv = "Date,Description,Debit Amount,Credit Amount\n10/01/2026,RENT,1200.00,\n10/02/2026,CLIENT,,-450.00\n10/03/2026,FEE,-2.50,\n";
    let rows = parse_bank_csv_reader(csv.as_bytes()).unwrap();
    assert_eq!(rows.iter().map(|r| r.amount_cents).collect::<Vec<_>>(), [-120_000, 45_000, -250]);
}

#[test]
fn reads_day_first_csv_and_windows_1252_text() {
    let mut bytes = b"Date,Description,Amount\n03/07/2026,Caf\xe9 \x93Bleu\x94,-4.50\n15/07/2026,Refund,4.50\n".to_vec();
    bytes.extend_from_slice(b"");
    let rows = parse_bank_csv_reader(bytes.as_slice()).unwrap();
    assert_eq!(rows[0].date, NaiveDate::from_ymd_opt(2026, 7, 3).unwrap(), "15/07 shows the file is day-first");
    assert_eq!(rows[0].description, "Café “Bleu”");
}

const OFX_SGML: &str = "OFXHEADER:100\r\nDATA:OFXSGML\r\nVERSION:102\r\nCHARSET:1252\r\n\r\n<OFX>\r\n<BANKMSGSRSV1><STMTTRNRS><STMTRS>\r\n<BANKTRANLIST>\r\n<DTSTART>20261001\r\n\
<STMTTRN>\r\n<TRNTYPE>DEBIT\r\n<DTPOSTED>20261001120000.000[-5:EST]\r\n<TRNAMT>-18.75\r\n<FITID>2026100101\r\n<NAME>JOES DINER &amp; GRILL\r\n<MEMO>POS PURCHASE\r\n\
<STMTTRN>\r\n<TRNTYPE>CREDIT\r\n<DTPOSTED>20261002\r\n<TRNAMT>+1500.00\r\n<FITID>2026100202\r\n<NAME>STRIPE TRANSFER\r\n\
<STMTTRN>\r\n<TRNTYPE>DEBIT\r\n<DTPOSTED>20261003\r\n<TRNAMT>-4.5\r\n<FITID>2026100303\r\n<MEMO>Coffee\r\n\
</BANKTRANLIST>\r\n</STMTRS></STMTTRNRS></BANKMSGSRSV1>\r\n</OFX>\r\n";

#[test]
fn parses_sgml_and_xml_ofx() {
    assert!(is_ofx(OFX_SGML));
    let rows = parse_ofx(OFX_SGML).unwrap();
    let got: Vec<(String, i64, &str, Option<&str>)> =
        rows.iter().map(|r| (r.date.to_string(), r.amount_cents, r.description.as_str(), r.fitid.as_deref())).collect();
    assert_eq!(
        got,
        [
            ("2026-10-01".into(), -1_875, "JOES DINER & GRILL POS PURCHASE", Some("2026100101")),
            ("2026-10-02".into(), 150_000, "STRIPE TRANSFER", Some("2026100202")),
            ("2026-10-03".into(), -450, "Coffee", Some("2026100303")),
        ]
    );

    let xml = r#"<?xml version="1.0"?><?OFX OFXHEADER="200" VERSION="220"?><OFX><BANKMSGSRSV1><STMTTRNRS><STMTRS><BANKTRANLIST>
            <STMTTRN><TRNTYPE>DEBIT</TRNTYPE><DTPOSTED>20261005</DTPOSTED><TRNAMT>-99.00</TRNAMT><FITID>X1</FITID><NAME>Hardware Store</NAME><MEMO>Hardware Store</MEMO></STMTTRN>
            </BANKTRANLIST></STMTRS></STMTTRNRS></BANKMSGSRSV1></OFX>"#;
    assert!(is_ofx(xml));
    let rows = parse_ofx(xml).unwrap();
    assert_eq!((rows[0].amount_cents, rows[0].description.as_str(), rows[0].fitid.as_deref()), (-9_900, "Hardware Store", Some("X1")));

    assert!(parse_ofx("OFXHEADER:100\n<OFX></OFX>").unwrap_err().starts_with("BANK_OFX_EMPTY"));
    let broken = OFX_SGML.replace("<TRNAMT>+1500.00", "<TRNAMT>lots");
    assert!(parse_ofx(&broken).unwrap_err().starts_with("BANK_OFX_ROW: transaction at line 17"));
    assert!(!is_ofx("Date,Description,Amount\n"));
}

fn temp_file(name: &str, contents: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("localbooks-bank-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

#[tokio::test]
async fn overlapping_ofx_statements_import_each_transaction_once() {
    let pool = test_pool().await;
    let checking = account_id(&pool, "1010").await;
    let first = temp_file("sept.ofx", OFX_SGML.as_bytes());
    assert_eq!(import_file(&pool, &first, checking, None).await.unwrap(), ImportSummary { imported: 3, duplicates: 0, skipped_zero: 0 });

    // The next download overlaps: two transactions again, plus one new one.
    let next = OFX_SGML
        .replacen("<STMTTRN>\r\n<TRNTYPE>DEBIT\r\n<DTPOSTED>20261001120000.000[-5:EST]\r\n<TRNAMT>-18.75\r\n<FITID>2026100101\r\n<NAME>JOES DINER &amp; GRILL\r\n<MEMO>POS PURCHASE\r\n", "", 1)
        .replace("</BANKTRANLIST>", "<STMTTRN>\r\n<DTPOSTED>20261004\r\n<TRNAMT>-60.00\r\n<FITID>2026100404\r\n<NAME>FUEL\r\n</BANKTRANLIST>");
    let second = temp_file("oct.ofx", next.as_bytes());
    assert_eq!(import_file(&pool, &second, checking, None).await.unwrap(), ImportSummary { imported: 1, duplicates: 2, skipped_zero: 0 });
    assert_eq!(list_pending(&pool, &Suggester::default(), "test").await.unwrap().len(), 4);
    std::fs::remove_dir_all(first.parent().unwrap()).unwrap();
    std::fs::remove_dir_all(second.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn unrecognized_csv_is_mapped_once_and_remembered() {
    let pool = test_pool().await;
    let checking = account_id(&pool, "1010").await;
    let csv = "Fecha,Concepto,Importe\n03/07/2026,Supermercado,-45.10\n05/07/2026,Nomina,1800.00\n";
    let path = temp_file("santander.csv", csv.as_bytes());

    let seen = inspect_file(&pool, &path, None, None).await.unwrap();
    assert_eq!((seen.format, seen.mapping.is_none(), seen.headers.len()), (BankFileFormat::Csv, true, 3));
    assert!(seen.problem.is_some());
    assert!(import_file(&pool, &path, checking, None).await.unwrap_err().starts_with("BANK_CSV_COLUMNS"));

    let mapping = ColumnMapping {
        date: "Fecha".into(),
        description: "Concepto".into(),
        amount: Some("Importe".into()),
        money_in: None,
        money_out: None,
        date_order: DateOrder::Dmy,
    };
    let preview = inspect_file(&pool, &path, Some(mapping.clone()), None).await.unwrap();
    assert_eq!((preview.mapping_source, preview.row_count, preview.problem.clone()), (Some("GIVEN"), 2, None));
    assert_eq!(preview.preview[0].date, NaiveDate::from_ymd_opt(2026, 7, 3).unwrap());

    let summary = import_file(&pool, &path, checking, Some(mapping.clone())).await.unwrap();
    assert_eq!(summary.imported, 2);

    // The same bank's next file imports with the saved layout.
    let later = temp_file("santander-aug.csv", "Fecha,Concepto,Importe\n02/08/2026,Gasolina,-60.00\n".as_bytes());
    let seen = inspect_file(&pool, &later, None, None).await.unwrap();
    assert_eq!((seen.mapping_source, seen.mapping.as_ref()), (Some("SAVED"), Some(&mapping)));
    assert_eq!(import_file(&pool, &later, checking, None).await.unwrap().imported, 1);
    let dates: Vec<String> = list_pending(&pool, &Suggester::default(), "test").await.unwrap().into_iter().map(|t| t.date).collect();
    assert_eq!(dates, ["2026-08-02", "2026-07-05", "2026-07-03"]);

    let bad = ColumnMapping { money_in: Some("Importe".into()), ..mapping };
    assert!(inspect_file(&pool, &path, Some(bad), None).await.unwrap().problem.unwrap().starts_with("BANK_CSV_MAPPING"));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    std::fs::remove_dir_all(later.parent().unwrap()).unwrap();
}

#[test]
fn parses_signed_amount_csv() {
    let csv = "\u{feff}Date,Description,Amount\n10/01/2026,AWS EMEA  aws.amazon.com,-120.50\n10/02/2026,STRIPE TRANSFER,1500.00\n,,\n";
    let rows = parse_bank_csv_reader(csv.as_bytes()).expect("parses");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].description, "AWS EMEA aws.amazon.com");
    assert_eq!(rows[0].amount_cents, -12_050);
    assert_eq!(rows[1].amount_cents, 150_000);
    assert_eq!(rows[1].row, 3);
}

#[test]
fn parses_split_debit_credit_csv() {
    let csv = "Posted Date,Payee,Withdrawal,Deposit\n2026-10-01,EXXONMOBIL 4421,52.10,\n2026-10-02,CLIENT ACH,,900\n";
    let rows = parse_bank_csv_reader(csv.as_bytes()).expect("parses");
    assert_eq!(rows.iter().map(|r| r.amount_cents).collect::<Vec<_>>(), vec![-5_210, 90_000]);
}

#[test]
fn reports_bad_row_with_line_number() {
    let csv = "Date,Description,Amount\n2026-10-01,OK,1.00\n2026-10-02,BAD,twelve\n";
    let err = parse_bank_csv_reader(csv.as_bytes()).expect_err("bad amount");
    assert!(err.starts_with("BANK_CSV_ROW: line 3"), "{err}");
}

#[test]
fn learns_patterns_that_ignore_noise() {
    let pattern = learned_pattern("SQ *BLUE BOTTLE #123 OAKLAND").expect("pattern");
    let regex = RegexBuilder::new(&pattern).case_insensitive(true).build().unwrap();
    assert!(regex.is_match("SQ *BLUE BOTTLE #998 SF"));
    assert!(regex.is_match("Blue-Bottle Coffee"));
    assert!(!regex.is_match("BLUE APRON"));
    assert_eq!(learned_pattern("POS DEBIT 0042 #"), None);
}

#[tokio::test]
async fn built_in_rules_suggest_vendor_categories() {
    let pool = test_pool().await;
    let rules = RuleSet::load(&pool).await.expect("rules compile");
    let expect = |desc: &str, amount: i64| rules.suggest(desc, amount).map(|s| s.account_id);

    assert_eq!(expect("AWS EMEA aws.amazon.com", -12_050), Some(account_id(&pool, "6350").await));
    assert_eq!(expect("EXXONMOBIL 4421 HOUSTON", -5_210), Some(account_id(&pool, "6100").await));
    assert_eq!(expect("UBER EATS 8005928996", -2_399), Some(account_id(&pool, "6200").await));
    assert_eq!(expect("UBER *TRIP", -1_850), Some(account_id(&pool, "6450").await));
    assert_eq!(expect("STRIPE TRANSFER", 150_000), Some(account_id(&pool, "4000").await));
    assert_eq!(expect("RANDOM VENDOR", -100), Some(account_id(&pool, "6900").await));
    assert_eq!(expect("RANDOM PAYER", 100), Some(account_id(&pool, "4900").await));
}

#[tokio::test]
async fn import_dedupes_and_approval_posts_and_learns() {
    let pool = test_pool().await;
    let checking = account_id(&pool, "1010").await;
    let meals = account_id(&pool, "6200").await;

    let dir = std::env::temp_dir().join(format!("localbooks-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("feed.csv");
    std::fs::write(
        &path,
        "Date,Description,Amount\n2026-10-01,JOES DINER 0042,-18.75\n2026-10-01,JOES DINER 0042,-18.75\n2026-10-02,TRANSFER,0.00\n",
    )
    .unwrap();

    let first = import_file(&pool, &path, checking, None).await.expect("imports");
    assert_eq!(first, ImportSummary { imported: 2, duplicates: 0, skipped_zero: 1 });
    let again = import_file(&pool, &path, checking, None).await.expect("re-imports");
    assert_eq!(again, ImportSummary { imported: 0, duplicates: 2, skipped_zero: 1 });
    std::fs::remove_dir_all(&dir).unwrap();

    let pending = list_pending(&pool, &Suggester::default(), "test").await.expect("pending");
    assert_eq!(pending.len(), 2);
    assert_eq!(pending[0].suggestion.as_ref().map(|s| s.origin.as_str()), Some("DEFAULT"));

    approve(&pool, &Suggester::default(), "test", pending[0].id, Approval { category_account_id: meals, ..Approval::default() }).await.expect("approves");
    let (checking_balance, meals_balance): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT balance_cents FROM v_account_balances WHERE account_id = ?1),
                    (SELECT balance_cents FROM v_account_balances WHERE account_id = ?2)",
    )
    .bind(checking)
    .bind(meals)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((checking_balance, meals_balance), (-1_875, 1_875));

    let remaining = list_pending(&pool, &Suggester::default(), "test").await.expect("pending");
    assert_eq!(remaining.len(), 1);
    let learned = remaining[0].suggestion.as_ref().expect("suggestion");
    assert_eq!((learned.account_id, learned.origin.as_str()), (meals, "LEARNED"));

    let err = approve(&pool, &Suggester::default(), "test", pending[0].id, Approval { category_account_id: meals, ..Approval::default() }).await.expect_err("already approved");
    assert!(err.starts_with("BANK_TXN_NOT_PENDING"), "{err}");

    exclude(&pool, remaining[0].id).await.expect("excludes");
    assert!(list_pending(&pool, &Suggester::default(), "test").await.unwrap().is_empty());

    // The approval is a real Expense document; voiding it sends the line back for review.
    let entry_id: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM expenses WHERE kind = 'EXPENSE'")
        .fetch_one(&pool)
        .await
        .expect("approval created an expense");
    crate::ledger::void_transaction(&pool, entry_id).await.expect("voids");
    let back = list_pending(&pool, &Suggester::default(), "test").await.unwrap();
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].id, pending[0].id);
    let checking_after: i64 = sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1")
        .bind(checking)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(checking_after, 0);
}

#[tokio::test]
async fn pdf_statement_is_checked_against_its_balances_and_imported_once() {
    use crate::bank_pdf::tests::checking_statement_pdf;
    let pool = test_pool().await;
    let checking = account_id(&pool, "1010").await;
    let path = temp_file("maple-jan.pdf", &checking_statement_pdf());

    let seen = inspect_file(&pool, &path, None, Some(checking)).await.unwrap();
    assert_eq!((seen.format, seen.mapping_source, seen.row_count, seen.problem.clone()), (BankFileFormat::Pdf, Some("DETECTED"), 7, None));
    let mapping = seen.mapping.clone().unwrap();
    assert_eq!((mapping.money_out.as_deref(), mapping.money_in.as_deref()), (Some("Withdrawals"), Some("Deposits")));
    assert_eq!(seen.preview[0].amount_cents, -5_420);
    assert_eq!(seen.preview[0].date, NaiveDate::from_ymd_opt(2026, 1, 3).unwrap());
    let check = seen.balance_check.unwrap();
    assert_eq!(
        (check.status, check.opening_cents, check.change_cents, check.closing_cents, check.signs_reversed, check.owed),
        ("MATCHES", Some(100_000), 101_940, Some(201_940), false, false)
    );

    // Choosing the columns remembers them for this bank's next statement.
    let summary = import_file(&pool, &path, checking, Some(mapping.clone())).await.unwrap();
    assert_eq!(summary, ImportSummary { imported: 7, duplicates: 0, skipped_zero: 0 });
    let again = inspect_file(&pool, &path, None, Some(checking)).await.unwrap();
    assert_eq!(again.mapping_source, Some("SAVED"));
    assert_eq!(import_file(&pool, &path, checking, None).await.unwrap(), ImportSummary { imported: 0, duplicates: 7, skipped_zero: 0 });

    let pending = list_pending(&pool).await.unwrap();
    assert_eq!(pending.iter().map(|t| t.amount_cents).sum::<i64>(), 101_940);
    assert!(pending.iter().any(|t| t.description == "POS PURCHASE GREENLEAF GROCERY SPRINGFIELD CARD 4321"));

    // Read with the columns swapped, the statement no longer adds up as a bank account... until the
    // signs are reversed, which the balances show to be right.
    let swapped = ColumnMapping { money_in: mapping.money_out.clone(), money_out: mapping.money_in.clone(), ..mapping.clone() };
    let check = inspect_file(&pool, &path, Some(swapped), Some(checking)).await.unwrap().balance_check.unwrap();
    assert_eq!((check.status, check.signs_reversed), ("MATCHES", true));

    // Reading the running balance as an amount breaks both totals; the check says so.
    let wrong = ColumnMapping { money_in: Some("Balance".into()), ..mapping };
    let check = inspect_file(&pool, &path, Some(wrong), Some(checking)).await.unwrap().balance_check.unwrap();
    assert_eq!((check.status, check.signs_reversed), ("DIFFERS", false));
    assert_ne!(check.difference_cents, 0);
    assert_eq!(check.first_mismatch_row, Some(1));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn card_statement_charges_import_as_money_out() {
    use crate::bank_pdf::tests::card_statement_pdf;
    let pool = test_pool().await;
    let card = account_id(&pool, "2100").await;
    let path = temp_file("visa-jan.pdf", &card_statement_pdf());

    let seen = inspect_file(&pool, &path, None, Some(card)).await.unwrap();
    let check = seen.balance_check.clone().unwrap();
    assert_eq!((check.status, check.owed, check.signs_reversed), ("MATCHES", true, true));
    assert_eq!((check.opening_cents, check.change_cents, check.closing_cents), (Some(50_000), 13_215, Some(63_215)));
    assert_eq!(seen.preview.iter().map(|r| r.amount_cents).collect::<Vec<_>>(), [-1_250, 50_000, -21_965, -40_000]);

    import_file(&pool, &path, card, seen.mapping.clone()).await.unwrap();
    let pending = list_pending(&pool).await.unwrap();
    let cafe = pending.iter().find(|t| t.description == "CORNER CAFE").unwrap();
    assert_eq!((cafe.amount_cents, cafe.date.as_str()), (-1_250, "2025-12-18"));
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn scanned_pdf_cannot_be_inspected() {
    use crate::bank_pdf::tests::scanned_statement_pdf;
    let pool = test_pool().await;
    let path = temp_file("scan.pdf", &scanned_statement_pdf());
    let err = inspect_file(&pool, &path, None, None).await.unwrap_err();
    assert!(err.starts_with("BANK_PDF_NO_TEXT"), "{err}");
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
