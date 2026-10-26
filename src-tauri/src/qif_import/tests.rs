use super::*;

/// A Quicken export of two accounts and a category list (the layout Quicken writes with
/// "export all accounts"), including a transfer that appears in both accounts.
const EXPORT: &str = "!Option:AutoSwitch
!Account
NChecking
TBank
^
NSavings
TBank
^
NVisa
TCCard
^
NBrokerage
TInvst
^
!Clear:AutoSwitch
!Type:Cat
NConsulting
DFees from clients
I
^
NUtilities
E
^
NUtilities:Electric
E
^
!Account
NChecking
TBank
^
!Type:Bank
D1/ 1'24
T1,000.00
POpening Balance
L[Checking]
^
D1/ 5'24
T-120.50
N101
PCity Power
LUtilities:Electric
^
D1/ 8'24
T2,500.00
PClient A
MJanuary work
LConsulting/Business
^
D1/10'24
T-500.00
PTransfer to savings
L[Savings]
^
D1/12'24
T-300.00
PHardware store
SUtilities
ESplit one
$-100.00
SHome Repair
$-200.00
^
!Account
NSavings
TBank
^
!Type:Bank
D1/10'24
T500.00
L[Checking]
^
!Account
NVisa
TCCard
^
!Type:CCard
D1/15'24
T-42.10
PCoffee Shop
LMeals
^
D1/18'24
T12.00
PCoffee Shop
MRefund for a spilled order
LMeals
^
!Account
NBrokerage
TInvst
^
!Type:Invst
D1/3'24
NBuy
YACME
I10
Q5
T50.00
^
";

fn by_kind<'a>(ledger: &'a Ledger, kind: &str) -> Vec<&'a SourceTransaction> {
    ledger.transactions.iter().filter(|t| t.kind == kind).collect()
}

fn account<'a>(ledger: &'a Ledger, name: &str) -> &'a crate::qbw_company::SourceAccount {
    ledger.accounts.iter().find(|a| a.full_name == name).unwrap_or_else(|| panic!("no account {name}"))
}

#[test]
fn reads_accounts_and_categories() {
    let ledger = read(EXPORT.as_bytes(), "Ignored").unwrap();
    assert_eq!(account(&ledger, "Checking").account_type, "BANK");
    assert_eq!(account(&ledger, "Visa").account_type, "CREDIT_CARD");
    assert_eq!(account(&ledger, "Consulting").account_type, "INCOME");
    assert_eq!(account(&ledger, "Utilities:Electric").account_type, "EXPENSE");
    assert_eq!(account(&ledger, "Utilities:Electric").parent_id, Some(account(&ledger, "Utilities").id));
    assert_eq!(account(&ledger, "Meals").account_type, "EXPENSE", "an undeclared category takes the direction of its money");
    assert!(!ledger.accounts.iter().any(|a| a.name == "Brokerage"), "investment accounts are skipped");
    assert_eq!(ledger.quality.rejected_by_reason.get(SKIPPED_INVESTMENT), Some(&1));
}

#[test]
fn transactions_balance_and_are_classified() {
    let ledger = read(EXPORT.as_bytes(), "Ignored").unwrap();
    assert!(ledger.unbalanced.is_empty());
    assert!(ledger.unreadable.is_empty());

    let opening = by_kind(&ledger, "Opening balance");
    assert_eq!(opening.len(), 1);
    assert_eq!(opening[0].lines[1].account_id, account(&ledger, OPENING_BALANCE_EQUITY).id);
    assert_eq!(opening[0].lines[0].cents, -100_000, "money into the bank is a debit");

    let check = by_kind(&ledger, "Check")[0];
    assert_eq!((check.date, check.doc_num.as_deref()), (NaiveDate::from_ymd_opt(2024, 1, 5).unwrap(), Some("101")));
    assert!(matches!(check.document, Some(SourceDocument::Expense { .. })));

    let deposit = by_kind(&ledger, "Deposit")[0];
    assert_eq!(deposit.memo.as_deref(), Some("Client A - January work"));
    assert_eq!(deposit.lines[1].cents, 250_000, "income is a credit; the class is dropped");

    let split = by_kind(&ledger, "Withdrawal")[0];
    assert_eq!(split.lines.len(), 3);
    assert_eq!(split.lines[1].memo.as_deref(), Some("Split one"));

    let charge = by_kind(&ledger, "Credit card charge")[0];
    assert_eq!(charge.lines[0].cents, 4_210, "a charge is a credit to the card");
}

#[test]
fn a_transfer_written_in_both_accounts_imports_once() {
    let ledger = read(EXPORT.as_bytes(), "Ignored").unwrap();
    let transfers = by_kind(&ledger, "Transfer");
    assert_eq!(transfers.len(), 1);
    assert!(matches!(transfers[0].document, Some(SourceDocument::Transfer)));
    assert_eq!(ledger.transactions.len(), 7);
}

#[test]
fn a_bank_download_without_an_account_record_uses_the_given_name() {
    let file = "!Type:Bank\nD03/04/2024\nT-12.00\nPGROCERY\n^\nD25/04/2024\nT40\nPREFUND\n^\n";
    let ledger = read(file.as_bytes(), "Bank Download").unwrap();
    assert_eq!(account(&ledger, "Bank Download").account_type, "BANK");
    let first = &ledger.transactions[0];
    // 25/04 can only be day-first, so 03/04 is 3 April.
    assert_eq!(first.date, NaiveDate::from_ymd_opt(2024, 4, 3).unwrap());
    assert_eq!(first.lines[1].account_id, account(&ledger, "Uncategorized Expense").id);
}

#[test]
fn quicken_year_forms() {
    assert_eq!(normalize_date("1/ 5'04"), "1/5/2004");
    assert_eq!(normalize_date("12/21' 7"), "12/21/2007");
    assert_eq!(normalize_date("2/10'2020"), "2/10/2020");
    assert_eq!(normalize_date("6/ 1/94"), "6/1/94");
}

#[test]
fn rejects_files_that_are_not_qif() {
    assert!(read(b"Date,Amount\n1/1/2024,5\n", "x").unwrap_err().starts_with("QIF_NOT_QIF"));
    assert!(read(b"!Type:Bank\n", "x").unwrap_err().starts_with("QIF_EMPTY"));
}

#[tokio::test]
async fn a_full_import_of_a_qif_file_creates_checks_card_credits_deposits_and_transfers() {
    use crate::qbw_ledger_import::{auto_map_accounts, post_batch, stage, ImportMode, PostLedgerInput};
    let pool = crate::db::test_pool().await;
    let ledger = read(EXPORT.as_bytes(), "Ignored").unwrap();
    let batch = stage(&pool, std::path::Path::new("/exports/Quicken.qif"), EXPORT.len() as i64, &ledger).await.unwrap();
    auto_map_accounts(&pool, batch).await.unwrap();
    let full = post_batch(&pool, &PostLedgerInput { batch_id: batch, mode: ImportMode::Full }).await.unwrap().full.unwrap();
    assert_eq!((full.checks, full.expenses, full.card_credits, full.deposits, full.transfers, full.fell_back), (1, 2, 1, 1, 1, 0), "{full:?}");
    assert_eq!(full.journal_entries, 1, "the opening balance");

    let checking: i64 = sqlx::query_scalar(
        "SELECT SUM(l.debit_cents) - SUM(l.credit_cents) FROM v_posted_lines l JOIN chart_of_accounts a ON a.id = l.account_id WHERE a.name = 'Checking'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(checking, 100_000 - 12_050 + 250_000 - 50_000 - 30_000);
    let visa: i64 = sqlx::query_scalar(
        "SELECT SUM(l.debit_cents) - SUM(l.credit_cents) FROM v_posted_lines l JOIN chart_of_accounts a ON a.id = l.account_id WHERE a.name = 'Visa'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(visa, -4_210 + 1_200, "the refund brings the card balance down");
    let (debits, credits): (i64, i64) = sqlx::query_as("SELECT SUM(debit_cents), SUM(credit_cents) FROM journal_lines").fetch_one(&pool).await.unwrap();
    assert_eq!(debits, credits, "the books balance");
}
