//! Test company files for trying LocalBooks by hand (see `docs/testing/edge-cases.md` and the
//! `run-localbooks` project skill). Built through the same posting functions the app uses, so every
//! rule and trigger applies; never part of the app.
//!
//! `LOCALBOOKS_SEED_DIR=<folder> cargo test --release -- --ignored seed_test_companies --nocapture`
//! writes, replacing earlier copies of these names only:
//! - `Edge Cases.localbooks`: users on (Admin, Standard, View only), encrypted tax IDs, sales tax,
//!   a foreign-currency invoice, partial payments, credits, a voided entry, a job, unusual names,
//!   leap-day and year-end dates, a closed period;
//! - `No Users.localbooks`: a small company without users whose vendor has a plain tax ID, for
//!   turning users on;
//! - `Five Years.localbooks`: about 2,800 documents over 2021–2025, for speed and long reports
//!   (skipped when `LOCALBOOKS_SEED_SKIP_LARGE` is set, as the end-to-end tests do).
//!
//! Sign-ins and the closing passcode are listed in `docs/testing/edge-cases.md`. Built under
//! `cfg(test)`, the password hashing uses Argon2's minimum cost: fine for test files, never for real
//! books.

use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use sqlx::SqlitePool;

use crate::closing::{self, ClosingInput};
use crate::credits::{self, ApplyCreditInput, CreditMemoInput};
use crate::expenses::{self, BillInput, BillLineInput, BillPaymentInput, ExpenseInput, SplitLineInput};
use crate::invoices::{self, InvoiceInput, InvoiceLineInput, PaymentInput};
use crate::ledger::{self, ForeignCurrency, JournalEntry, JournalLine};
use crate::parties::{self, PartyDetailsInput};
use crate::sales::{self, SalesLineInput};
use crate::sales_tax::{self, TaxRateInput};
use crate::tax_ids::Access;
use crate::users::{self, NewUserInput, Role};

/// The Admin's password in every seeded company with users (test value).
pub const ADMIN_PASSWORD: &str = "owner password";
/// The closing date passcode in `Edge Cases` (test value).
pub const CLOSING_PASSCODE: &str = "close-2025";

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).expect("a real date")
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.expect(number)
}

/// A fresh company file at `path`, replacing one this seeder made before.
async fn fresh(path: &Path, name: &str) -> SqlitePool {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    let pool = crate::db::open(path).await.expect("create the company file");
    crate::preferences::set(&pool, "company_name", name).await.expect("name the company");
    pool
}

fn line(description: &str, quantity_e4: i64, rate_cents: i64, income: i64, taxable: bool) -> InvoiceLineInput {
    InvoiceLineInput { description: description.into(), quantity_e4, rate_cents, income_account_id: income, product_service_id: None, class_id: None, is_taxable: taxable }
}

fn invoice(customer: &str, date: NaiveDate, lines: Vec<InvoiceLineInput>) -> InvoiceInput {
    InvoiceInput { customer_name: customer.into(), issue_date: date, due_date: date + chrono::Days::new(30), lines, currency: None, tax_rate_id: None, discount_cents: 0 }
}

async fn pay(pool: &SqlitePool, invoice_id: i64, bank: i64, date: NaiveDate, cents: i64, method: &str) {
    invoices::record_payment(
        pool,
        PaymentInput { invoice_id, deposit_account_id: bank, payment_date: date, amount_cents: cents, payment_method: method.into(), reference: None, exchange_rate_e6: None },
    )
    .await
    .expect("record a payment");
}

async fn bill(pool: &SqlitePool, vendor: &str, number: &str, date: NaiveDate, account_id: i64, cents: i64) -> i64 {
    expenses::post_vendor_bill(
        pool,
        BillInput {
            vendor_name: vendor.into(),
            bill_number: Some(number.into()),
            bill_date: date,
            due_date: date + chrono::Days::new(30),
            memo: None,
            total_cents: cents,
            lines: vec![BillLineInput { account_id, amount_cents: cents, memo: None, quantity_e4: None, rate_cents: None, product_service_id: None, class_id: None }],
        },
    )
    .await
    .expect("post a bill")
    .id
}

async fn pay_bill(pool: &SqlitePool, bill_id: i64, bank: i64, date: NaiveDate, cents: i64) {
    expenses::pay_vendor_bill(pool, BillPaymentInput { bill_id, payment_account_id: bank, payment_date: date, amount_cents: cents, reference: None }).await.expect("pay a bill");
}

async fn expense(pool: &SqlitePool, kind: &str, payee: &str, paid_from: i64, date: NaiveDate, account_id: i64, cents: i64) {
    expenses::post_expense(
        pool,
        ExpenseInput {
            kind: kind.into(),
            payee_name: Some(payee.into()),
            payment_account_id: paid_from,
            expense_date: date,
            check_number: (kind == "CHECK").then(|| "1001".into()),
            payment_method: None,
            memo: None,
            lines: vec![SplitLineInput { account_id, amount_cents: cents, memo: None, class_id: None }],
            currency: None,
        },
    )
    .await
    .expect("post an expense");
}

/// Sets a vendor's tax ID (encrypted when the file has users) and marks it for a 1099.
async fn vendor_tax_id(pool: &SqlitePool, name: &str, tax_id: &str, access: Access<'_>) {
    let id = expenses::create_vendor(pool, name, None).await.expect("create a vendor").id;
    let input = PartyDetailsInput {
        name: name.into(),
        company_name: None,
        email: None,
        phone: None,
        address: Some("1 Main St\nSpringfield, IL 62701".into()),
        terms_days: Some(30),
        notes: None,
        tax_id: Some(tax_id.into()),
        keep_tax_id: false,
        is_1099: true,
        tax_rate_id: None,
        is_tax_exempt: false,
        tax_exemption: None,
        form_style_id: None,
    };
    parties::update_party(pool, "vendor", id, input, access).await.expect("set the tax ID");
}

async fn edge_cases(path: &Path) -> String {
    let pool = fresh(path, "Edge Cases Test Co").await;
    let (checking, savings, card) = (account(&pool, "1010").await, account(&pool, "1020").await, account(&pool, "2100").await);
    let (sales_acct, services) = (account(&pool, "4000").await, account(&pool, "4100").await);
    let (office, rent, contractors, equity) = (account(&pool, "6250").await, account(&pool, "6300").await, account(&pool, "6400").await, account(&pool, "3100").await);

    // Users first, so tax IDs are stored encrypted from the start.
    let (admin, recovery) = users::turn_on(&pool, "owner", Some("Olive Owner"), ADMIN_PASSWORD).await.expect("turn users on");
    for (username, display, role, sensitive) in [
        ("clerk", "Casey Clerk", Role::Standard, false),
        ("bookkeeper", "Billie Books", Role::Standard, true),
        ("viewer", "Vic Viewer", Role::ViewOnly, false),
    ] {
        users::add(&pool, &admin, NewUserInput { username: username.into(), display_name: Some(display.into()), role, can_see_sensitive: sensitive, password: format!("{username} temp pw") })
            .await
            .expect("add a user");
    }
    let access = Access { protected: true, key: admin.key.as_ref() };

    // Opening balance and a funded savings account.
    ledger::post_journal_entry(&pool, JournalEntry { id: None, date: d(2024, 1, 2), description: "Owner's investment".into(), lines: vec![JournalLine::debit(checking, 5_000_000), JournalLine::credit(equity, 5_000_000)], currency: None })
        .await
        .expect("opening balance");
    crate::banking::post_transfer(&pool, crate::banking::TransferInput { from_account_id: checking, to_account_id: savings, transfer_date: d(2024, 1, 3), amount_cents: 1_000_000, memo: Some("Reserve".into()) })
        .await
        .expect("transfer");

    // Customers with awkward names, and a job under a customer.
    for name in ["Acme Corp", "Café Olé & Søn's \"Best\" Bakery", "A Customer With A Very Long Name That Goes On And On For Testing Layout Ltd", "Zed's 50% Off <Outlet>"] {
        sales::create_customer(&pool, name, None).await.expect("create a customer");
    }
    sales::create_customer(&pool, "Acme Corp:Kitchen Remodel", None).await.expect("create a job");

    // Sales tax, and invoices covering the awkward amounts.
    let rate = sales_tax::create_tax_rate(&pool, TaxRateInput { name: "City 8.875%".into(), rate_ppm: 88_750, agency_name: Some("City Tax Dept".into()), liability_account_id: None })
        .await
        .expect("create a tax rate");
    let paid = invoices::create_and_post_invoice(&pool, invoice("Acme Corp", d(2024, 2, 29), vec![line("Leap-day consulting", 10_000, 150_000, services, false)])).await.expect("leap-day invoice");
    pay(&pool, paid.id, checking, d(2024, 3, 15), 150_000, "CHECK").await;
    let partial = invoices::create_and_post_invoice(&pool, invoice("Acme Corp:Kitchen Remodel", d(2024, 6, 10), vec![line("Cabinets", 30_000, 99_999, sales_acct, false)]))
        .await
        .expect("job invoice");
    pay(&pool, partial.id, checking, d(2024, 7, 1), 100_000, "ACH").await;
    let mut taxed = invoice("Café Olé & Søn's \"Best\" Bakery", d(2024, 12, 31), vec![line("Fractional quantity", 33_333, 333, sales_acct, true), line("Service, not taxed", 10_000, 12_345, services, false)]);
    taxed.tax_rate_id = Some(rate.id);
    taxed.discount_cents = 101;
    invoices::create_and_post_invoice(&pool, taxed).await.expect("taxed year-end invoice");
    let mut euro = invoice("A Customer With A Very Long Name That Goes On And On For Testing Layout Ltd", d(2025, 3, 1), vec![line("Export order", 10_000, 250_000, sales_acct, false)]);
    euro.currency = Some(ForeignCurrency { code: "EUR".into(), exchange_rate_e6: 1_080_000 });
    let euro = invoices::create_and_post_invoice(&pool, euro).await.expect("EUR invoice");
    invoices::record_payment(
        &pool,
        PaymentInput { invoice_id: euro.id, deposit_account_id: checking, payment_date: d(2025, 4, 1), amount_cents: 100_000, payment_method: "WIRE".into(), reference: Some("SWIFT 123".into()), exchange_rate_e6: Some(1_100_000) },
    )
    .await
    .expect("partial EUR payment at a new rate");
    let credited = invoices::create_and_post_invoice(&pool, invoice("Zed's 50% Off <Outlet>", d(2025, 5, 5), vec![line("Clearance", 20_000, 5_000, sales_acct, false)])).await.expect("invoice to credit");
    let credit = credits::create_credit_memo(
        &pool,
        CreditMemoInput {
            customer_name: "Zed's 50% Off <Outlet>".into(),
            credit_date: d(2025, 5, 20),
            memo: Some("Damaged goods".into()),
            refund_account_id: None,
            lines: vec![SalesLineInput { product_service_id: None, description: "Return".into(), quantity_e4: 10_000, rate_cents: 2_500, income_account_id: sales_acct, class_id: None, is_taxable: false }],
            tax_rate_id: None,
        },
    )
    .await
    .expect("credit memo");
    credits::apply_customer_credit(&pool, ApplyCreditInput { credit_id: credit.id, document_id: credited.id, amount_cents: 2_500, applied_date: d(2025, 5, 21) }).await.expect("apply the credit");

    // Vendors: 1099 contractors with encrypted tax IDs, bills paid, part paid and open.
    vendor_tax_id(&pool, "Pat Contractor", "123-45-6789", access).await;
    vendor_tax_id(&pool, "O'Brien & Sons Plumbing", "12-3456789", access).await;
    let contractor = bill(&pool, "Pat Contractor", "PC-1", d(2025, 1, 15), contractors, 250_000).await;
    pay_bill(&pool, contractor, checking, d(2025, 2, 1), 250_000).await;
    let plumber = bill(&pool, "O'Brien & Sons Plumbing", "OB-77", d(2025, 6, 1), contractors, 80_000).await;
    pay_bill(&pool, plumber, checking, d(2025, 6, 20), 30_000).await;
    bill(&pool, "Landlord LLC", "RENT-2025-12", d(2025, 12, 1), rent, 200_000).await;
    expense(&pool, "CHECK", "Office Depot", checking, d(2025, 8, 8), office, 4_599).await;
    expense(&pool, "EXPENSE", "Paper Co", card, d(2025, 9, 9), office, 1).await;

    // A mistake, voided.
    let mistake = ledger::post_journal_entry(&pool, JournalEntry { id: None, date: d(2025, 10, 1), description: "Posted by mistake".into(), lines: vec![JournalLine::debit(office, 99_900), JournalLine::credit(checking, 99_900)], currency: None })
        .await
        .expect("entry to void");
    ledger::void_transaction(&pool, mistake).await.expect("void it");

    // 2026 activity after the closed year.
    let open = invoices::create_and_post_invoice(&pool, invoice("Acme Corp", d(2026, 1, 15), vec![line("January retainer", 10_000, 300_000, services, false)])).await.expect("2026 invoice");
    pay(&pool, open.id, checking, d(2026, 2, 1), 1, "CASH").await;
    closing::set_closing_date(&pool, &ClosingInput { lock_date: Some(d(2025, 12, 31)), current_passcode: None, new_passcode: Some(CLOSING_PASSCODE.into()) }).await.expect("close 2025");

    crate::db::compact(&pool).await.expect("compact");
    pool.close().await;
    recovery.as_str().to_string()
}

async fn no_users(path: &Path) {
    let pool = fresh(path, "No Users Test Co").await;
    let (checking, services, contractors) = (account(&pool, "1010").await, account(&pool, "4100").await, account(&pool, "6400").await);
    vendor_tax_id(&pool, "Plain Text Contractor", "987-65-4321", Access::OPEN).await;
    let id = bill(&pool, "Plain Text Contractor", "PT-1", d(2025, 4, 1), contractors, 75_000).await;
    pay_bill(&pool, id, checking, d(2025, 4, 15), 75_000).await;
    let sale = invoices::create_and_post_invoice(&pool, invoice("First Customer", d(2025, 4, 2), vec![line("Setup", 10_000, 120_000, services, false)])).await.expect("invoice");
    pay(&pool, sale.id, checking, d(2025, 4, 20), 120_000, "CHECK").await;
    crate::db::compact(&pool).await.expect("compact");
    pool.close().await;
}

async fn five_years(path: &Path) {
    let pool = fresh(path, "Five Years Test Co").await;
    let (checking, card) = (account(&pool, "1010").await, account(&pool, "2100").await);
    let (sales_acct, services) = (account(&pool, "4000").await, account(&pool, "4100").await);
    let expense_accounts = [account(&pool, "6000").await, account(&pool, "6250").await, account(&pool, "6350").await, account(&pool, "6500").await, account(&pool, "6550").await];
    let rent = account(&pool, "6300").await;
    let customers: Vec<String> = (1..=25).map(|n| format!("Customer {n:02}")).collect();
    let mut n = 0i64;
    for year in 2021..=2025 {
        for month in 1..=12u32 {
            for k in 0..40i64 {
                n += 1;
                let date = d(year, month, (k % 28 + 1) as u32);
                let income = if k % 3 == 0 { services } else { sales_acct };
                let cents = 5_000 + (n * 7_919) % 250_000;
                let inv = invoices::create_and_post_invoice(&pool, invoice(&customers[(n % 25) as usize], date, vec![line("Monthly work", 10_000, cents, income, false)]))
                    .await
                    .expect("invoice");
                // Most are paid; the last months stay open for aging reports.
                if !(year == 2025 && month >= 11) && n % 9 != 0 {
                    pay(&pool, inv.id, checking, date + chrono::Days::new(20), cents, "ACH").await;
                }
            }
            let rent_bill = bill(&pool, "Landlord LLC", &format!("R-{year}-{month:02}"), d(year, month, 1), rent, 180_000).await;
            pay_bill(&pool, rent_bill, checking, d(year, month, 5), 180_000).await;
            for (k, &acct) in expense_accounts.iter().enumerate() {
                let paid_from = if k % 2 == 0 { checking } else { card };
                expense(&pool, "EXPENSE", &format!("Supplier {k}"), paid_from, d(year, month, 10 + k as u32), acct, 1_000 + (n % 50_000)).await;
            }
        }
    }
    crate::db::compact(&pool).await.expect("compact");
    pool.close().await;
}

/// Writes the test companies into `LOCALBOOKS_SEED_DIR`.
#[tokio::test]
#[ignore]
async fn seed_test_companies() {
    let dir = PathBuf::from(std::env::var("LOCALBOOKS_SEED_DIR").expect("set LOCALBOOKS_SEED_DIR to the folder for the test companies"));
    std::fs::create_dir_all(&dir).expect("create the folder");
    let started = std::time::Instant::now();
    let recovery = edge_cases(&dir.join("Edge Cases.localbooks")).await;
    no_users(&dir.join("No Users.localbooks")).await;
    let large = std::env::var_os("LOCALBOOKS_SEED_SKIP_LARGE").is_none();
    if large {
        five_years(&dir.join("Five Years.localbooks")).await;
    }
    let note = format!(
        "Test companies made by `seed_test_companies` (src-tauri/src/test_companies.rs). Test values only.\n\n\
         Edge Cases.localbooks\n  owner / {ADMIN_PASSWORD} (Admin)\n  clerk / clerk temp pw (Standard; must choose a new password)\n  \
         bookkeeper / bookkeeper temp pw (Standard, can see sensitive data; must choose a new password)\n  \
         viewer / viewer temp pw (View only; must choose a new password)\n  recovery key: {recovery}\n  closing passcode: {CLOSING_PASSCODE}\n\n\
         No Users.localbooks: no sign-in.\n{}",
        if large { "Five Years.localbooks: no sign-in.\n" } else { "" }
    );
    std::fs::write(dir.join("README.txt"), note).expect("write the README");
    println!("seeded {} in {:?}", dir.display(), started.elapsed());
}
