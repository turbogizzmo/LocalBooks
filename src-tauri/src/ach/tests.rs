use super::*;
use crate::db::test_pool;
use crate::expenses::{post_vendor_bill, BillInput, BillLineInput};

async fn account_id(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

fn d(day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, day).unwrap()
}

fn at(day: u32, hour: u32) -> NaiveDateTime {
    d(day).and_hms_opt(hour, 30, 0).unwrap()
}

async fn bill(pool: &SqlitePool, vendor: &str, number: Option<&str>, cents: i64) -> i64 {
    let rent = account_id(pool, "6300").await;
    let input = BillInput {
        vendor_name: vendor.into(),
        bill_number: number.map(str::to_string),
        bill_date: d(1),
        due_date: d(31),
        memo: None,
        total_cents: cents,
        lines: vec![BillLineInput { account_id: rent, amount_cents: cents, memo: None, quantity_e4: None, rate_cents: None, product_service_id: None, class_id: None }],
    };
    post_vendor_bill(pool, input).await.unwrap().id
}

async fn vendor_id(pool: &SqlitePool, name: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM vendors WHERE name = ?1").bind(name).fetch_one(pool).await.unwrap()
}

fn bank(routing: &str, account: &str, sec: &str) -> Option<VendorBankInput> {
    Some(VendorBankInput { routing_number: routing.into(), account_number: account.into(), account_type: "CHECKING".into(), sec_code: sec.into() })
}

fn originator(account_id: i64) -> OriginatorDetails {
    OriginatorDetails {
        account_id,
        immediate_destination: "121042882".into(),
        immediate_destination_name: "Wells Fargo".into(),
        immediate_origin: "1234567890".into(),
        immediate_origin_name: "Acme Widgets LLC".into(),
        company_name: "Acme Widgets".into(),
        company_id: "1234567890".into(),
        entry_description: "Vendor pay".into(),
    }
}

fn request(id: &str, account: i64, bills: &[(i64, i64)]) -> AchPaymentInput {
    AchPaymentInput {
        request_id: id.into(),
        payment_account_id: account,
        payment_date: d(26),
        effective_date: d(27),
        bills: bills.iter().map(|&(bill_id, amount_cents)| AchBillInput { bill_id, amount_cents }).collect(),
    }
}

async fn count(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

/// Two vendors with bank details (a business with two bills, a person with one) and a third without.
async fn books(pool: &SqlitePool, access: Access<'_>) -> (i64, [i64; 4]) {
    let checking = account_id(pool, "1010").await;
    set_ach_originator(pool, originator(checking)).await.unwrap();
    let bills = [
        bill(pool, "Staples", Some("1001"), 125_000).await,
        bill(pool, "Staples", Some("1002"), 4_999).await,
        bill(pool, "Pat Smith", None, 30_000).await,
        bill(pool, "No Bank Co", Some("9"), 500).await,
    ];
    set_vendor_bank(pool, vendor_id(pool, "Staples").await, bank("021000021", "123456789", "CCD"), access).await.unwrap();
    set_vendor_bank(pool, vendor_id(pool, "Pat Smith").await, bank("231380104", "55554444", "PPD"), access).await.unwrap();
    (checking, bills)
}

#[tokio::test]
async fn bank_details_are_checked_and_masked() {
    let pool = test_pool().await;
    let staples = crate::expenses::create_vendor(&pool, "Staples", None).await.unwrap().id;
    let open = Access::OPEN;
    let err = set_vendor_bank(&pool, staples, bank("021000022", "123456789", "CCD"), open).await.unwrap_err();
    assert!(err.starts_with("BANK_BAD_ROUTING") && err.contains("check digit"), "{err}");
    assert!(set_vendor_bank(&pool, staples, bank("02100002", "123456789", "CCD"), open).await.unwrap_err().starts_with("BANK_BAD_ROUTING"));
    assert!(set_vendor_bank(&pool, staples, bank("021000021", "12", "CCD"), open).await.unwrap_err().starts_with("BANK_BAD_ACCOUNT"));
    assert!(set_vendor_bank(&pool, staples, bank("021000021", "1234 5678", "XYZ"), open).await.unwrap_err().starts_with("BANK_BAD_SEC"));

    let saved = set_vendor_bank(&pool, staples, bank("0210-00021", " 1234 5678 9 ", "CCD"), open).await.unwrap().unwrap();
    assert_eq!((saved.routing_number.as_deref(), saved.account_number.as_deref()), (Some("021000021"), Some("123456789")));
    assert_eq!((saved.routing_masked.as_str(), saved.account_masked.as_str(), saved.locked), ("*****0021", "*****6789", false));
    // A file without users stores them as typed.
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM vendor_bank_accounts WHERE account_number = '123456789'").await, 1);

    assert_eq!(set_vendor_bank(&pool, staples, None, open).await.unwrap(), None);
    assert_eq!(get_vendor_bank(&pool, staples, open).await.unwrap(), None);
}

#[tokio::test]
async fn protected_bank_details_need_the_key() {
    let pool = test_pool().await;
    let staples = crate::expenses::create_vendor(&pool, "Staples", None).await.unwrap().id;
    let pat = crate::expenses::create_vendor(&pool, "Pat Smith", None).await.unwrap().id;
    // Saved before users are on: plain, then encrypted when users are turned on.
    set_vendor_bank(&pool, pat, bank("231380104", "55554444", "PPD"), Access::OPEN).await.unwrap();

    let (admin, _) = crate::users::turn_on(&pool, "Owner", None, "owner password").await.unwrap();
    let mut conn = pool.acquire().await.unwrap();
    let with_key = Access::of(&mut conn, admin.key.as_ref()).await.unwrap();
    let without = Access::of(&mut conn, None).await.unwrap();
    drop(conn);
    let plain = count(&pool, "SELECT COUNT(*) FROM vendor_bank_accounts WHERE routing_number NOT LIKE 'lb1:%' OR account_number NOT LIKE 'lb1:%'").await;
    assert_eq!(plain, 0, "turning users on encrypted the details saved before");
    assert_eq!(get_vendor_bank(&pool, pat, with_key).await.unwrap().unwrap().account_number.as_deref(), Some("55554444"));

    set_vendor_bank(&pool, staples, bank("021000021", "123456789", "CCD"), with_key).await.unwrap();
    let (routing, account): (String, String) =
        sqlx::query_as("SELECT routing_number, account_number FROM vendor_bank_accounts WHERE vendor_id = ?1").bind(staples).fetch_one(&pool).await.unwrap();
    assert!(crypto::is_encrypted(&routing) && crypto::is_encrypted(&account) && !account.contains("6789"));

    let seen = get_vendor_bank(&pool, staples, without).await.unwrap().unwrap();
    assert_eq!((seen.routing_number, seen.account_number, seen.account_masked.as_str(), seen.locked), (None, None, "*****6789", true));
    assert!(set_vendor_bank(&pool, staples, bank("021000021", "999999999", "CCD"), without).await.unwrap_err().starts_with("BANK_DETAILS_LOCKED"));
    assert!(set_vendor_bank(&pool, staples, None, without).await.unwrap_err().starts_with("BANK_DETAILS_LOCKED"), "cannot remove them either");

    // Paying by ACH needs the key too: the file holds the numbers.
    let checking = account_id(&pool, "1010").await;
    set_ach_originator(&pool, originator(checking)).await.unwrap();
    let b = bill(&pool, "Staples", Some("1"), 10_000).await;
    let err = pay_bills_by_ach(&pool, request("locked-request", checking, &[(b, 10_000)]), without, at(26, 9)).await.unwrap_err();
    assert!(err.starts_with("BANK_DETAILS_LOCKED"), "{err}");
    let file = pay_bills_by_ach(&pool, request("keyed-request", checking, &[(b, 10_000)]), with_key, at(26, 9)).await.unwrap();
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM ach_entries WHERE account_number NOT LIKE 'lb1:%' OR routing_number NOT LIKE 'lb1:%'").await, 0, "the file's copy is encrypted");
    let mut conn = pool.acquire().await.unwrap();
    assert!(render_ach_file(&mut conn, file.id, without).await.unwrap_err().starts_with("BANK_DETAILS_LOCKED"));
    let (text, _) = render_ach_file(&mut conn, file.id, with_key).await.unwrap();
    assert!(text.contains("622021000021123456789"));
    drop(conn);

    // Removing every user erases what can no longer be decrypted.
    crate::users::remove_all_users(&pool, crate::users::REMOVE_ALL_CONFIRMATION).await.unwrap();
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM vendor_bank_accounts").await, 0);
    let mut conn = pool.acquire().await.unwrap();
    assert!(render_ach_file(&mut conn, file.id, Access::OPEN).await.unwrap_err().starts_with("ACH_FILE_ERASED"));
}

#[tokio::test]
async fn paying_by_ach_records_one_entry_per_vendor_and_writes_a_valid_file() {
    let pool = test_pool().await;
    let (checking, [s1, s2, pat, nobank]) = books(&pool, Access::OPEN).await;

    // A vendor without bank details stops the whole file; nothing is recorded.
    let err = pay_bills_by_ach(&pool, request("request-0001", checking, &[(s1, 125_000), (nobank, 500)]), Access::OPEN, at(26, 9)).await.unwrap_err();
    assert!(err.starts_with("ACH_NO_BANK_DETAILS") && err.contains("No Bank Co"), "{err}");
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM bill_payments").await + count(&pool, "SELECT COUNT(*) FROM ach_files").await, 0);

    let file = pay_bills_by_ach(&pool, request("request-0002", checking, &[(s1, 125_000), (pat, 30_000), (s2, 4_999)]), Access::OPEN, at(26, 9)).await.unwrap();
    assert_eq!((file.total_cents, file.file_id_modifier.as_str(), file.file_name.as_str(), file.times_saved), (159_999, "A", "ACH-20261026-A.ach", 0));
    let entries: Vec<(&str, &str, i64, &str, i64)> =
        file.entries.iter().map(|e| (e.vendor_name.as_str(), e.sec_code.as_str(), e.amount_cents, e.trace_number.as_str(), e.bill_count)).collect();
    assert_eq!(entries, [("Staples", "CCD", 129_999, "121042880000001", 2), ("Pat Smith", "PPD", 30_000, "121042880000002", 1)]);
    assert_eq!(file.entries[0].account_masked, "*****6789");

    // The bill payments are ordinary ones, marked ACH, referencing the trace number.
    let payments: Vec<(i64, String, String)> = sqlx::query_as("SELECT bill_id, kind, reference FROM bill_payments ORDER BY bill_id").fetch_all(&pool).await.unwrap();
    assert_eq!(
        payments,
        [(s1, "ACH".into(), "ACH 121042880000001".into()), (s2, "ACH".into(), "ACH 121042880000001".into()), (pat, "ACH".into(), "ACH 121042880000002".into())]
    );
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM bills WHERE status = 'PAID'").await, 3);
    let checking_balance: i64 = sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1").bind(checking).fetch_one(&pool).await.unwrap();
    assert_eq!(checking_balance, -159_999);

    // The file validates, and holds what was recorded.
    let path = std::env::temp_dir().join(format!("localbooks-ach-{}.ach", std::process::id()));
    let saved = save_ach_file(&pool, file.id, &path, Access::OPEN).await.unwrap();
    assert_eq!(saved.times_saved, 1);
    let text = std::fs::read_to_string(&path).unwrap();
    let summary = nacha::validate(&text).unwrap();
    assert_eq!((summary.batch_count, summary.entry_addenda_count, summary.total_credit_cents), (2, 3, 159_999));
    assert_eq!(summary.entries[0].addenda.as_deref(), Some("BILLS 1001, 1002"));
    assert!(text.lines().nth(2).unwrap().starts_with("622021000021123456789        0000129999V"));
    assert!(text.lines().all(|l| l.len() == 94) && text.lines().count() % 10 == 0);

    // The audit trail records the file without any account number.
    let payload: String = sqlx::query_scalar("SELECT payload_snapshot FROM ledger_audit_log WHERE action_type = 'ACH_FILE'").fetch_one(&pool).await.unwrap();
    let event: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!((event["event"].as_str(), event["totalCreditCents"].as_i64()), (Some("generated"), Some(159_999)));
    assert_eq!(event["entries"][1]["traceNumber"], "121042880000002");
    for secret in ["123456789", "55554444", "6789", "4444"] {
        assert!(!payload.contains(secret), "the audit payload holds {secret}: {payload}");
    }
    let chain = crate::audit_chain::verify(&pool).await.unwrap();
    assert!(chain.problems.is_empty() && chain.records_pending == 0, "the event is sealed into the chain: {:?}", chain.problems);

    // Saving again writes the identical file and pays nothing more.
    let again = save_ach_file(&pool, file.id, &path, Access::OPEN).await.unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    assert_eq!(again.times_saved, 2);
    let events: Vec<String> = sqlx::query_scalar("SELECT json_extract(payload_snapshot, '$.event') FROM ledger_audit_log WHERE action_type = 'ACH_FILE' ORDER BY id").fetch_all(&pool).await.unwrap();
    assert_eq!(events, ["generated", "regenerated"]);
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM bill_payments").await, 3);

    // So does sending the same Pay again (a double click, a retry after an error).
    let repeat = pay_bills_by_ach(&pool, request("request-0002", checking, &[(s1, 125_000), (pat, 30_000), (s2, 4_999)]), Access::OPEN, at(26, 10)).await.unwrap();
    assert_eq!(repeat.id, file.id);
    assert_eq!((count(&pool, "SELECT COUNT(*) FROM bill_payments").await, count(&pool, "SELECT COUNT(*) FROM ach_files").await), (3, 1));
    // And a new request for bills already paid is refused.
    let err = pay_bills_by_ach(&pool, request("request-0003", checking, &[(s1, 125_000)]), Access::OPEN, at(26, 10)).await.unwrap_err();
    assert!(err.starts_with("BILL_NOT_OPEN"), "{err}");

    // Recorded files cannot be edited.
    assert!(sqlx::query("UPDATE ach_entries SET amount_cents = 1").execute(&pool).await.unwrap_err().to_string().contains("ACH_FILE_IMMUTABLE"));
    assert!(sqlx::query("DELETE FROM ach_entry_payments").execute(&pool).await.unwrap_err().to_string().contains("ACH_FILE_IMMUTABLE"));
    std::fs::remove_file(&path).ok();
}

#[tokio::test]
async fn a_second_file_the_same_day_gets_the_next_modifier_and_traces() {
    let pool = test_pool().await;
    let (checking, [s1, s2, pat, _]) = books(&pool, Access::OPEN).await;
    let first = pay_bills_by_ach(&pool, request("request-a", checking, &[(s1, 125_000), (pat, 10_000)]), Access::OPEN, at(26, 9)).await.unwrap();
    let second = pay_bills_by_ach(&pool, request("request-b", checking, &[(s2, 4_999), (pat, 15_000)]), Access::OPEN, at(26, 11)).await.unwrap();
    assert_eq!((first.file_id_modifier.as_str(), second.file_id_modifier.as_str()), ("A", "B"));
    let traces: Vec<&str> = second.entries.iter().map(|e| e.trace_number.as_str()).collect();
    assert_eq!(traces, ["121042880000003", "121042880000004"]);
    let next_day = pay_bills_by_ach(&pool, request("request-c", checking, &[(pat, 1)]), Access::OPEN, at(27, 9)).await.unwrap();
    assert_eq!(next_day.file_id_modifier, "A");
    let files = list_ach_files(&pool).await.unwrap();
    assert_eq!(files.iter().map(|f| f.id).collect::<Vec<_>>(), [next_day.id, second.id, first.id]);
}

#[tokio::test]
async fn a_voided_payment_stops_the_file_being_saved_again() {
    let pool = test_pool().await;
    let (checking, [s1, _, pat, _]) = books(&pool, Access::OPEN).await;
    let file = pay_bills_by_ach(&pool, request("request-void", checking, &[(s1, 125_000), (pat, 30_000)]), Access::OPEN, at(26, 9)).await.unwrap();
    let entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM bill_payments WHERE bill_id = ?1").bind(pat).fetch_one(&pool).await.unwrap();
    crate::ledger::void_transaction(&pool, entry).await.unwrap();
    let view = list_ach_files(&pool).await.unwrap().remove(0);
    assert_eq!(view.voided_payments, 1);
    let path = std::env::temp_dir().join(format!("localbooks-ach-void-{}.ach", std::process::id()));
    let err = save_ach_file(&pool, file.id, &path, Access::OPEN).await.unwrap_err();
    assert!(err.starts_with("ACH_FILE_HAS_VOIDS"), "{err}");
    assert!(!path.exists());
    // The voided bill can be paid again, in a new file.
    let again = pay_bills_by_ach(&pool, request("request-void-2", checking, &[(pat, 30_000)]), Access::OPEN, at(26, 10)).await.unwrap();
    assert_eq!(again.entries[0].trace_number, "121042880000003");
}

#[tokio::test]
async fn originator_details_are_checked() {
    let pool = test_pool().await;
    let checking = account_id(&pool, "1010").await;
    let card = account_id(&pool, "2100").await;
    assert!(set_ach_originator(&pool, originator(card)).await.unwrap_err().starts_with("ACH_NOT_BANK"));
    let mut bad = originator(checking);
    bad.immediate_destination = "121042883".into();
    assert!(set_ach_originator(&pool, bad).await.unwrap_err().starts_with("ACH_BAD_DESTINATION"));
    let mut long = originator(checking);
    long.company_name = "Acme Widgets Incorporated".into();
    assert!(set_ach_originator(&pool, long).await.unwrap_err().starts_with("ACH_TOO_LONG"));
    let mut spaced = originator(checking);
    spaced.immediate_origin = " 231380104 ".into();
    let saved = set_ach_originator(&pool, spaced).await.unwrap();
    assert_eq!((saved.immediate_origin.as_str(), saved.company_name.as_str()), ("231380104", "ACME WIDGETS"));
    assert_eq!(get_ach_originator(&pool, checking).await.unwrap(), Some(saved));

    // Without originator details there is no file.
    let savings_less = sqlx::query_scalar::<_, i64>("SELECT id FROM chart_of_accounts WHERE account_type = 'BANK' AND id <> ?1").bind(checking).fetch_optional(&pool).await.unwrap();
    if let Some(other) = savings_less {
        let b = bill(&pool, "Staples", None, 100).await;
        let err = pay_bills_by_ach(&pool, request("request-noorig", other, &[(b, 100)]), Access::OPEN, at(26, 9)).await.unwrap_err();
        assert!(err.starts_with("ACH_NO_ORIGINATOR"), "{err}");
    }
}
