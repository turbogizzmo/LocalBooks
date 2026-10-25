use super::*;

/// Intuit's sample files (IIF Import Kit), trimmed to the columns they use, joined into one file.
const SAMPLE: &str = "!ACCNT\tNAME\tACCNTTYPE\tDESC\tACCNUM\tEXTRA\r
ACCNT\tAccounts Receivable\tAR\t\t1200\t\r
ACCNT\tUndeposited Funds\tOCASSET\t\t1400\tUNDEPOSIT\r
ACCNT\tChecking\tBANK\t\t\t\r
ACCNT\tAccounts Payable\tAP\t\t2000\t\r
ACCNT\tConstruction:Labor\tINC\t\t4100\t\r
ACCNT\tConstruction:Materials\tINC\t\t4200\t\r
ACCNT\tRepairs\tEXP\t\t6100\t\r
ACCNT\tEstimates\tNONPOSTING\t\t\t\r
!INVITEM\tNAME\tINVITEMTYPE\tDESC\tACCNT\tPRICE\tTAXABLE\r
INVITEM\tFraming\tSERV\tFraming labor\tConstruction:Labor\t55\tN\r
INVITEM\tWood Door:Exterior\tINVENTORY\tExterior wood door\tConstruction:Materials\t120\tY\r
!CUST\tNAME\tBADDR1\tBADDR2\tBADDR3\tPHONE1\tTERMS\tCOMPANYNAME\tFIRSTNAME\tLASTNAME\r
CUST\tCustomer\tJoe Customer\t444 Road Rd\t\"Anywhere, AZ 85740\"\t5554443333\tNet 30\t\tJoe\tCustomer\r
!VEND\tNAME\tADDR1\tTAXID\t1099\r
VEND\tVendor\tJon Vendor\t12-3456789\tY\r
!TRNS\tTRNSID\tTRNSTYPE\tDATE\tACCNT\tNAME\tCLASS\tAMOUNT\tDOCNUM\tMEMO\tDUEDATE\r
!SPL\tSPLID\tTRNSTYPE\tDATE\tACCNT\tNAME\tCLASS\tAMOUNT\tDOCNUM\tMEMO\tQNTY\tPRICE\tINVITEM\r
!ENDTRNS\r
TRNS\t\tINVOICE\t7/16/98\tAccounts Receivable\tCustomer\t\t175\t1\t\t8/15/98\r
SPL\t\tINVOICE\t7/16/98\tConstruction:Labor\t\t\t-55\t\tFraming labor\t\t55\tFraming\r
SPL\t\tINVOICE\t7/16/98\tConstruction:Materials\t\t\t-120\t\tExterior wood door\t-1\t120\tWood Door:Exterior\r
ENDTRNS\r
TRNS\t\tPAYMENT\t7/20/98\tUndeposited Funds\tCustomer\t\t100\t\t\t\r
SPL\t\tPAYMENT\t7/20/98\tAccounts Receivable\tCustomer\t\t-100\t\t\t\r
ENDTRNS\r
TRNS\t\tDEPOSIT\t7/21/98\tChecking\t\t\t100\t\t\t\r
SPL\t\tDEPOSIT\t7/21/98\tUndeposited Funds\tCustomer\t\t-100\t\t\t\r
ENDTRNS\r
TRNS\t\tBILL\t7/10/98\tAccounts Payable\tVendor\t\t-35\t88\t\t\r
SPL\t\tBILL\t7/10/98\tRepairs\t\t\t35\t\tFix door\t\r
ENDTRNS\r
TRNS\t\tBILLPMT\t7/16/1998\tChecking\tVendor\t\t-35\t\tTest Memo\t\r
SPL\t\tBILLPMT\t7/16/1998\tAccounts Payable\tVendor\t\t35\t\t\t\r
ENDTRNS\r
TRNS\t\tGENERAL JOURNAL\t7/1/98\tChecking\t\t\t650\t\t\t\r
SPL\t\tGENERAL JOURNAL\t7/1/98\tConstruction:Labor\t\t\t-650\t\t\t\r
ENDTRNS\r
TRNS\t\tESTIMATES\t7/2/98\tEstimates\tCustomer\t\t55\t7\t\t\r
SPL\t\tESTIMATES\t7/2/98\tEstimates\t\t\t-55\t\tFraming labor\t1\t55\tFraming\r
ENDTRNS\r
";

fn by_kind<'a>(ledger: &'a Ledger, kind: &str) -> &'a SourceTransaction {
    ledger.transactions.iter().find(|t| t.kind == kind).unwrap_or_else(|| panic!("no {kind}"))
}

#[test]
fn reads_intuits_sample_lists() {
    let ledger = read(SAMPLE.as_bytes()).unwrap();
    let labor = ledger.accounts.iter().find(|a| a.full_name == "Construction:Labor").unwrap();
    assert_eq!(labor.account_type, "INCOME");
    let parent = ledger.accounts.iter().find(|a| a.full_name == "Construction").unwrap();
    assert_eq!(labor.parent_id, Some(parent.id));
    assert!(!ledger.accounts.iter().any(|a| a.name == "Estimates"), "non-posting accounts are not imported");
    assert_eq!(ledger.accounts.iter().find(|a| a.name == "Undeposited Funds").unwrap().account_type, "OTHER_CURRENT_ASSET");

    let customer = &ledger.customers[0];
    assert_eq!(customer.name, "Customer");
    assert_eq!(customer.address.as_deref(), Some("Joe Customer\n444 Road Rd\nAnywhere, AZ 85740"));
    assert_eq!(customer.company_name.as_deref(), Some("Joe Customer"));
    assert_eq!(customer.terms_days, Some(30));
    let vendor = &ledger.vendors[0];
    assert_eq!((vendor.tax_id.as_deref(), vendor.is_1099), (Some("12-3456789"), true));

    let door = ledger.items.iter().find(|i| i.name == "Wood Door:Exterior").unwrap();
    assert_eq!((door.kind, door.rate_cents, door.taxable), ("Inventory part", 12_000, true));
}

#[test]
fn transactions_balance_with_ledger_signs() {
    let ledger = read(SAMPLE.as_bytes()).unwrap();
    assert!(ledger.unbalanced.is_empty());
    assert_eq!(ledger.transactions.len(), 7);
    let journal = by_kind(&ledger, "Journal entry");
    assert_eq!(journal.date, NaiveDate::from_ymd_opt(1998, 7, 1).unwrap());
    // 650 debited to Checking is -650 in the ledger's sign.
    assert_eq!(journal.lines[0].cents, -65_000);
    assert_eq!(by_kind(&ledger, "Bill payment").date, NaiveDate::from_ymd_opt(1998, 7, 16).unwrap());
}

#[test]
fn invoices_keep_their_form_lines() {
    let ledger = read(SAMPLE.as_bytes()).unwrap();
    let invoice = by_kind(&ledger, "Invoice");
    let Some(SourceDocument::Invoice { number, due_date, lines, .. }) = &invoice.document else { panic!("{:?}", invoice.document) };
    assert_eq!(number.as_deref(), Some("1"));
    assert_eq!(*due_date, NaiveDate::from_ymd_opt(1998, 8, 15));
    assert_eq!(lines.len(), 2);
    let door = &lines[1];
    assert_eq!((door.quantity_e4, door.rate_cents, door.cents), (10_000, 12_000, 12_000));
    assert!(door.item_id.is_some());
    assert_eq!(door.description.as_deref(), Some("Exterior wood door"));
}

#[test]
fn payments_apply_to_the_oldest_open_invoice_and_deposits_find_them() {
    let ledger = read(SAMPLE.as_bytes()).unwrap();
    let invoice = by_kind(&ledger, "Invoice").id;
    let payment = by_kind(&ledger, "Payment");
    let Some(SourceDocument::Payment { applied_to, .. }) = &payment.document else { panic!() };
    assert_eq!(applied_to, &[Applied { transaction_id: invoice, cents: 10_000 }]);
    let Some(SourceDocument::Deposit { payments }) = &by_kind(&ledger, "Deposit").document else { panic!() };
    assert_eq!(payments, &[Applied { transaction_id: payment.id, cents: 10_000 }]);

    let bill = by_kind(&ledger, "Bill").id;
    let Some(SourceDocument::BillPayment { applied_to, .. }) = &by_kind(&ledger, "Bill payment").document else { panic!() };
    assert_eq!(applied_to, &[Applied { transaction_id: bill, cents: 3_500 }]);
}

#[test]
fn estimates_post_nothing_but_keep_their_lines() {
    let ledger = read(SAMPLE.as_bytes()).unwrap();
    let estimate = by_kind(&ledger, "Estimate");
    assert!(estimate.lines.is_empty());
    let Some(SourceDocument::Estimate { lines, .. }) = &estimate.document else { panic!() };
    assert_eq!(lines[0].cents, 5_500);
}

#[test]
fn undeclared_accounts_and_names_are_created_and_bad_rows_reported() {
    let file = "!TRNS\tTRNSTYPE\tDATE\tACCNT\tNAME\tAMOUNT\n!SPL\tTRNSTYPE\tDATE\tACCNT\tNAME\tAMOUNT\n!ENDTRNS\n\
TRNS\tCHECK\t1/5/2024\tBusiness Checking\tAcme Supply\t-80\n\
SPL\tCHECK\t1/5/2024\tOffice Supplies\t\t80\n\
ENDTRNS\n\
TRNS\tGENERAL JOURNAL\t1/6/2024\tBusiness Checking\t\t10\n\
SPL\tGENERAL JOURNAL\t1/6/2024\tOffice Supplies\t\tten\n\
ENDTRNS\n";
    let ledger = read(file.as_bytes()).unwrap();
    let kind_of = |name: &str| ledger.accounts.iter().find(|a| a.name == name).unwrap().account_type;
    assert_eq!(kind_of("Business Checking"), "BANK");
    assert_eq!(kind_of("Office Supplies"), "EXPENSE");
    assert_eq!(ledger.transactions.len(), 1);
    assert_eq!(ledger.unbalanced.len(), 1, "the line that could not be read leaves its entry unbalanced");
    assert_eq!(ledger.unreadable.len(), 1);
    assert_eq!(ledger.quality.unreadable_lines, 1);
    // A payee on an ordinary check is not made a vendor.
    assert!(ledger.vendors.is_empty());
}

#[test]
fn rejects_a_file_that_is_not_iif() {
    assert!(read(b"Date,Amount\n1/1/2024,5\n").unwrap_err().starts_with("IIF_EMPTY"));
}

/// The sample goes all the way through staging and a full import: native invoice, payment,
/// deposit, bill, bill payment and estimate, with nothing falling back to a journal entry.
#[tokio::test]
async fn a_full_import_of_an_iif_file_creates_native_documents() {
    use crate::qbw_ledger_import::{auto_map_accounts, batch_summary, post_batch, stage, ImportMode, PostLedgerInput};
    let pool = crate::db::test_pool().await;
    let ledger = read(SAMPLE.as_bytes()).unwrap();
    let batch = stage(&pool, std::path::Path::new("/exports/Sample.iif"), SAMPLE.len() as i64, &ledger).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let posted = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap();
    let full = posted.full.unwrap();
    assert_eq!(
        (full.invoices, full.payments, full.deposits, full.bills, full.bill_payments, full.estimates, full.fell_back),
        (1, 1, 1, 1, 1, 1, 0),
        "{full:?}"
    );
    assert_eq!(full.journal_entries, 1, "only the general journal entry is a journal entry");
    let summary = batch_summary(&pool, batch).await.unwrap();
    assert!(summary.fallbacks.is_empty(), "{:?}", summary.fallbacks.iter().map(|f| &f.reason).collect::<Vec<_>>());

    let (status, total, paid): (String, i64, i64) =
        sqlx::query_as("SELECT status, total_cents, amount_paid_cents FROM invoices").fetch_one(&pool).await.unwrap();
    assert_eq!((status.as_str(), total, paid), ("PARTIAL", 17_500, 10_000));
    let bill_status: String = sqlx::query_scalar("SELECT status FROM bills").fetch_one(&pool).await.unwrap();
    assert_eq!(bill_status, "PAID");
    assert!(crate::banking::list_undeposited(&pool).await.unwrap().is_empty(), "the deposit swept the payment");
}

/// Reads every IIF sample in Intuit's import kit: set LOCALBOOKS_IIF_DIR to the unpacked kit.
/// Each must read with every transaction balanced, no line unreadable, and every transaction
/// of a known kind, or hold nothing LocalBooks imports.
#[test]
#[ignore]
fn reads_the_import_kit_samples() {
    let dir = std::env::var("LOCALBOOKS_IIF_DIR").expect("set LOCALBOOKS_IIF_DIR");
    let mut stack = vec![std::path::PathBuf::from(dir)];
    let mut files = 0;
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            stack.extend(std::fs::read_dir(&path).unwrap().filter_map(Result::ok).map(|e| e.path()));
        } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("iif")) {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let ledger = match read(&std::fs::read(&path).unwrap()) {
                Ok(ledger) => ledger,
                // The kit also has files of records LocalBooks has no use for (timer activities).
                Err(e) if e.starts_with("IIF_EMPTY") => continue,
                Err(e) => panic!("{name}: {e}"),
            };
            assert!(ledger.unbalanced.is_empty(), "{name}: unbalanced transactions");
            assert!(ledger.unreadable.is_empty(), "{name}: unreadable lines");
            assert!(ledger.transactions.iter().all(|t| t.kind != "Other"), "{name}: unknown transaction type");
            files += 1;
        }
    }
    assert!(files > 0, "no .iif files under LOCALBOOKS_IIF_DIR");
}
