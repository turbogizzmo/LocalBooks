//! A deterministic fixture company: a year (2025) of typical small-business activity posted through
//! the same functions the app's commands call. Every amount and date is fixed, so the reports built
//! from it are byte-for-byte stable and any change to a number is a real change in behaviour.
//!
//! Harbor Lane Design Co. sells design services and some taxable products, invoices three regular
//! customers and one in euros, pays rent, software, supplies and a 1099 contractor, runs a monthly
//! payroll through journal entries, borrows from the bank, pays its sales tax quarterly, reconciles
//! its checking account for the first quarter and voids an expense entered in error.

use chrono::NaiveDate;
use sqlx::SqlitePool;

use super::account;
use crate::accounts::AccountInput;
use crate::banking::{DepositInput, DepositItemRef, TransferInput};
use crate::expenses::{BillInput, BillLineInput, BillPaymentInput, ExpenseInput, SplitLineInput};
use crate::invoices::{InvoiceInput, InvoiceLineInput, PaymentInput};
use crate::ledger::{ForeignCurrency, JournalEntry, JournalLine};
use crate::sales::{SalesLineInput, SalesReceiptInput};

pub(crate) fn d(year: i32, month: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(year, month, day).expect("valid fixture date")
}

/// The last day of a 2025 month.
fn month_end(month: u32) -> NaiveDate {
    if month == 12 { d(2025, 12, 31) } else { d(2025, month + 1, 1).pred_opt().unwrap() }
}

struct Chart {
    checking: i64,
    savings: i64,
    undeposited: i64,
    equipment: i64,
    card: i64,
    loan: i64,
    owner_investment: i64,
    owner_draw: i64,
    sales: i64,
    services: i64,
    advertising: i64,
    bank_fees: i64,
    office: i64,
    rent: i64,
    software: i64,
    contractors: i64,
    utilities: i64,
    interest_expense: i64,
    wages: i64,
    payroll_taxes: i64,
    payroll_liabilities: i64,
}

async fn new_account(pool: &SqlitePool, number: &str, name: &str, account_type: &str) -> i64 {
    crate::accounts::create_account(
        pool,
        AccountInput { name: name.into(), account_number: Some(number.into()), account_type: account_type.into(), parent_id: None, description: None },
    )
    .await
    .unwrap_or_else(|e| panic!("create account {name}: {e}"))
    .id
}

async fn journal(pool: &SqlitePool, date: NaiveDate, description: &str, lines: Vec<JournalLine>) -> i64 {
    crate::ledger::post_journal_entry(pool, JournalEntry { id: None, date, description: description.into(), lines, currency: None })
        .await
        .unwrap_or_else(|e| panic!("journal entry '{description}' on {date}: {e}"))
}

fn line(description: &str, quantity_e4: i64, rate_cents: i64, income: i64, is_taxable: bool) -> InvoiceLineInput {
    InvoiceLineInput { description: description.into(), quantity_e4, rate_cents, income_account_id: income, product_service_id: None, class_id: None, is_taxable }
}

async fn invoice(pool: &SqlitePool, customer: &str, issue: NaiveDate, terms: u64, lines: Vec<InvoiceLineInput>, tax_rate_id: Option<i64>, currency: Option<ForeignCurrency>) -> i64 {
    crate::invoices::create_and_post_invoice(
        pool,
        InvoiceInput { customer_name: customer.into(), issue_date: issue, due_date: issue + chrono::Days::new(terms), lines, currency, tax_rate_id, discount_cents: 0 },
    )
    .await
    .unwrap_or_else(|e| panic!("invoice for {customer} on {issue}: {e}"))
    .id
}

async fn receive(pool: &SqlitePool, invoice_id: i64, date: NaiveDate, amount_cents: i64, deposit_account_id: i64, exchange_rate_e6: Option<i64>) {
    crate::invoices::record_payment(
        pool,
        PaymentInput { invoice_id, deposit_account_id, payment_date: date, amount_cents, payment_method: "CHECK".into(), reference: None, exchange_rate_e6 },
    )
    .await
    .unwrap_or_else(|e| panic!("payment on invoice {invoice_id} on {date}: {e}"));
}

async fn bill(pool: &SqlitePool, vendor: &str, number: &str, date: NaiveDate, terms: u64, lines: Vec<(i64, i64, &str)>) -> (i64, i64) {
    let total = lines.iter().map(|l| l.1).sum();
    let summary = crate::expenses::post_vendor_bill(
        pool,
        BillInput {
            vendor_name: vendor.into(),
            bill_number: Some(number.into()),
            bill_date: date,
            due_date: date + chrono::Days::new(terms),
            memo: None,
            total_cents: total,
            lines: lines
                .into_iter()
                .map(|(account_id, amount_cents, memo)| BillLineInput {
                    account_id,
                    amount_cents,
                    memo: Some(memo.into()),
                    quantity_e4: None,
                    rate_cents: None,
                    product_service_id: None,
                    class_id: None,
                })
                .collect(),
        },
    )
    .await
    .unwrap_or_else(|e| panic!("bill {number} from {vendor}: {e}"));
    (summary.id, total)
}

async fn pay_bill(pool: &SqlitePool, bill_id: i64, date: NaiveDate, amount_cents: i64, from: i64) {
    crate::expenses::pay_vendor_bill(pool, BillPaymentInput { bill_id, payment_account_id: from, payment_date: date, amount_cents, reference: None })
        .await
        .unwrap_or_else(|e| panic!("payment of bill {bill_id} on {date}: {e}"));
}

async fn expense(pool: &SqlitePool, kind: &str, payee: &str, from: i64, date: NaiveDate, check_number: Option<&str>, lines: Vec<(i64, i64, &str)>) -> i64 {
    crate::expenses::post_expense(
        pool,
        ExpenseInput {
            kind: kind.into(),
            payee_name: Some(payee.into()),
            payment_account_id: from,
            expense_date: date,
            check_number: check_number.map(str::to_string),
            payment_method: None,
            memo: None,
            lines: lines
                .into_iter()
                .map(|(account_id, amount_cents, memo)| SplitLineInput { account_id, amount_cents, memo: Some(memo.into()), class_id: None })
                .collect(),
            currency: None,
        },
    )
    .await
    .unwrap_or_else(|e| panic!("{kind} to {payee} on {date}: {e}"))
    .journal_entry_id
    .expect("expense is posted")
}

/// Builds Harbor Lane Design Co. on a fresh in-memory company file.
pub(crate) async fn harbor_lane() -> SqlitePool {
    let pool = crate::db::test_pool().await;
    let p = &pool;
    crate::preferences::set(p, "company_name", "Harbor Lane Design Co.").await.expect("company name");

    let c = Chart {
        checking: account(p, "1010").await,
        savings: account(p, "1020").await,
        undeposited: account(p, "1300").await,
        equipment: account(p, "1500").await,
        card: account(p, "2100").await,
        loan: account(p, "2500").await,
        owner_investment: account(p, "3100").await,
        owner_draw: account(p, "3200").await,
        sales: account(p, "4000").await,
        services: account(p, "4100").await,
        advertising: account(p, "6000").await,
        bank_fees: account(p, "6050").await,
        office: account(p, "6250").await,
        rent: account(p, "6300").await,
        software: account(p, "6350").await,
        contractors: account(p, "6400").await,
        utilities: account(p, "6500").await,
        interest_expense: account(p, "8000").await,
        wages: new_account(p, "6700", "Wages", "EXPENSE").await,
        payroll_taxes: new_account(p, "6710", "Payroll Taxes", "EXPENSE").await,
        payroll_liabilities: new_account(p, "2300", "Payroll Liabilities", "OTHER_CURRENT_LIABILITY").await,
    };
    let tax = crate::sales_tax::create_tax_rate(
        p,
        crate::sales_tax::TaxRateInput { name: "State Sales Tax".into(), rate_ppm: 72_500, agency_name: Some("State Department of Revenue".into()), liability_account_id: None },
    )
    .await
    .expect("tax rate")
    .id;

    // Opening: the owner's capital and a bank loan; part of it parked in savings.
    journal(p, d(2025, 1, 2), "Owner investment", vec![JournalLine::debit(c.checking, 5_000_000), JournalLine::credit(c.owner_investment, 5_000_000)]).await;
    journal(p, d(2025, 1, 3), "Equipment loan proceeds", vec![JournalLine::debit(c.checking, 2_400_000), JournalLine::credit(c.loan, 2_400_000)]).await;
    crate::banking::post_transfer(p, TransferInput { from_account_id: c.checking, to_account_id: c.savings, transfer_date: d(2025, 1, 6), amount_cents: 1_500_000, memo: Some("Reserve".into()) })
        .await
        .expect("transfer to savings");
    expense(p, "CHECK", "Pacific Office Outfitters", c.checking, d(2025, 1, 8), Some("1001"), vec![(c.equipment, 1_875_000, "Workstations and plotter")]).await;

    let customers = ["Alder & Finch LLP", "Brightwater Cafe", "Cobalt Outfitters"];
    let mut open_items: Vec<(i64, NaiveDate, i64)> = Vec::new(); // (invoice, issue date, total) to settle later
    let mut late: Option<(i64, i64)> = None; // (invoice, amount still due) of a partly paid invoice
    let mut check_number = 1002;
    for month in 1..=12u32 {
        let m = i64::from(month);
        let first = d(2025, month, 1);

        // Rent by check on the 1st, utilities on the card, software as a bill paid the next month.
        expense(p, "CHECK", "Riverbend Properties", c.checking, first, Some(&check_number.to_string()), vec![(c.rent, 240_000, "Studio rent")]).await;
        check_number += 1;
        expense(p, "EXPENSE", "Harbor Power & Light", c.card, d(2025, month, 12), None, vec![(c.utilities, 18_000 + 1_150 * (m % 4), "Electricity")]).await;
        let (software_bill, software_total) =
            bill(p, "Northwind Software", &format!("NW-{month:02}"), d(2025, month, 3), 30, vec![(c.software, 12_900, "Design suite seats")]).await;
        if month < 12 {
            pay_bill(p, software_bill, d(2025, month, 28), software_total, c.checking).await;
        }

        // Sales: a services invoice to each customer in turn; products are taxable.
        let customer = customers[(month as usize - 1) % customers.len()];
        let lines = vec![
            line("Brand design", 10_000 * (80 + 5 * (m % 3)), 12_500, c.services, false),
            line("Printed collateral", 25_000 * (4 + m % 4), 4_250, c.sales, true),
        ];
        let inv = invoice(p, customer, d(2025, month, 10), 30, lines, Some(tax), None).await;
        let total = crate::invoices::list_invoices(p).await.unwrap().into_iter().find(|i| i.id == inv).unwrap().total_cents;
        open_items.push((inv, d(2025, month, 10), total));

        // A walk-in product sale each quarter, into Undeposited Funds.
        if month % 3 == 2 {
            crate::sales::post_sales_receipt(
                p,
                SalesReceiptInput {
                    customer_name: None,
                    receipt_date: d(2025, month, 20),
                    deposit_account_id: c.undeposited,
                    payment_method: "CASH".into(),
                    reference: None,
                    memo: None,
                    lines: vec![SalesLineInput {
                        product_service_id: None,
                        description: "Poster prints".into(),
                        quantity_e4: 60_000,
                        rate_cents: 3_500,
                        income_account_id: c.sales,
                        class_id: None,
                        is_taxable: true,
                    }],
                    tax_rate_id: Some(tax),
                    discount_cents: 0,
                },
            )
            .await
            .expect("sales receipt");
        }

        // Customers pay last month's invoice: in full, except one partial payment in the summer.
        // Payments wait in Undeposited Funds until a deposit at month end.
        if month >= 2 {
            let (prev, prev_date, prev_total) = open_items.remove(0);
            let amount = if month == 7 { prev_total / 2 } else { prev_total };
            receive(p, prev, prev_date + chrono::Days::new(27), amount, c.undeposited, None).await;
            if month == 7 {
                late = Some((prev, prev_total - amount));
            }
        }
        if month == 9 {
            // The rest of the partial payment arrives late.
            let (prev, rest) = late.take().expect("a partly paid invoice");
            receive(p, prev, d(2025, 9, 25), rest, c.undeposited, None).await;
        }
        let waiting: Vec<DepositItemRef> =
            crate::banking::list_undeposited(p).await.unwrap().into_iter().map(|i| DepositItemRef { kind: i.kind, id: i.id }).collect();
        if !waiting.is_empty() && month < 12 {
            crate::banking::post_bank_deposit(p, DepositInput { bank_account_id: c.checking, deposit_date: month_end(month), memo: None, items: waiting, other_lines: Vec::new() })
                .await
                .expect("deposit");
        }

        // Payroll: gross wages and employer taxes, net pay from checking, withholding owed.
        let gross = 600_000 + 25_000 * (m / 4);
        let employer_tax = gross * 765 / 10_000;
        let withheld = gross * 2_200 / 10_000;
        journal(
            p,
            month_end(month),
            &format!("Payroll {month:02}/2025"),
            vec![
                JournalLine::debit(c.wages, gross),
                JournalLine::debit(c.payroll_taxes, employer_tax),
                JournalLine::credit(c.checking, gross - withheld),
                JournalLine::credit(c.payroll_liabilities, withheld + employer_tax),
            ],
        )
        .await;
        if month < 12 {
            journal(
                p,
                d(2025, month + 1, 15),
                "Payroll tax deposit",
                vec![JournalLine::debit(c.payroll_liabilities, withheld + employer_tax), JournalLine::credit(c.checking, withheld + employer_tax)],
            )
            .await;
        }

        // Loan payment: interest and principal.
        let principal = 200_000;
        let interest = (2_400_000 - principal * (m - 1)) * 6 / 100 / 12;
        journal(
            p,
            d(2025, month, 25),
            "Loan payment",
            vec![JournalLine::debit(c.loan, principal), JournalLine::debit(c.interest_expense, interest), JournalLine::credit(c.checking, principal + interest)],
        )
        .await;
        expense(p, "EXPENSE", "First Harbor Bank", c.checking, month_end(month), None, vec![(c.bank_fees, 1_500, "Monthly service fee")]).await;

        // Pay the card off from checking each month.
        let card_balance: i64 = sqlx::query_scalar("SELECT balance_cents FROM v_account_balances WHERE account_id = ?1").bind(c.card).fetch_one(p).await.unwrap();
        if card_balance > 0 {
            crate::banking::post_transfer(p, TransferInput { from_account_id: c.checking, to_account_id: c.card, transfer_date: d(2025, month, 26), amount_cents: card_balance, memo: Some("Card payment".into()) })
                .await
                .expect("card payment");
        }

        // The 1099 contractor, paid by check every other month.
        if month % 2 == 0 {
            expense(p, "CHECK", "Jordan Reyes", c.checking, d(2025, month, 18), Some(&check_number.to_string()), vec![(c.contractors, 95_000, "Illustration")]).await;
            check_number += 1;
        }

        // Quarterly sales tax payment for the quarter just ended.
        if month % 3 == 1 && month > 1 {
            let owed: i64 = sqlx::query_scalar(
                "SELECT COALESCE(SUM(credit_cents - debit_cents), 0) FROM v_posted_lines
                 WHERE entry_date < ?1 AND account_id = (SELECT liability_account_id FROM tax_rates WHERE id = ?2)",
            )
            .bind(first)
            .bind(tax)
            .fetch_one(p)
            .await
            .unwrap();
            crate::sales_tax::pay_sales_tax(
                p,
                crate::sales_tax::SalesTaxPaymentInput { tax_rate_id: Some(tax), liability_account_id: None, account_id: c.checking, payment_date: d(2025, month, 20), amount_cents: owed, reference: None },
            )
            .await
            .expect("sales tax payment");
        }
    }

    // Office supplies bills: one paid in part, two still open at year end at different ages.
    let (supplies, _) = bill(p, "Pinecrest Office Supply", "PC-4410", d(2025, 9, 5), 30, vec![(c.office, 64_250, "Paper and toner"), (c.advertising, 30_000, "Flyers")]).await;
    pay_bill(p, supplies, d(2025, 10, 1), 40_000, c.checking).await;
    bill(p, "Pinecrest Office Supply", "PC-4521", d(2025, 11, 20), 30, vec![(c.office, 18_740, "Desk supplies")]).await;
    let (ads, ads_total) = bill(p, "Lumen Media", "LM-88", d(2025, 12, 1), 15, vec![(c.advertising, 150_000, "Spring campaign retainer")]).await;
    pay_bill(p, ads, d(2025, 12, 31), ads_total / 4, c.card).await;

    // A euro invoice, paid when the euro had strengthened: a realized exchange gain.
    let euro = invoice(
        p,
        "Bonn Studio GmbH",
        d(2025, 9, 15),
        30,
        vec![line("Packaging design", 10_000, 480_000, c.services, false)],
        None,
        Some(ForeignCurrency { code: "EUR".into(), exchange_rate_e6: 1_085_000 }),
    )
    .await;
    receive(p, euro, d(2025, 10, 20), 300_000, c.checking, Some(1_102_500)).await;
    // The rest is still open at year end.

    // A duplicate expense entered by mistake, then voided.
    let duplicate = expense(p, "EXPENSE", "Harbor Power & Light", c.card, d(2025, 6, 13), None, vec![(c.utilities, 19_150, "Duplicate entry")]).await;
    crate::ledger::void_transaction(p, duplicate).await.expect("void duplicate");

    // The owner takes a draw at year end.
    journal(p, d(2025, 12, 30), "Owner draw", vec![JournalLine::debit(c.owner_draw, 1_000_000), JournalLine::credit(c.checking, 1_000_000)]).await;

    // Reconcile checking for the first quarter against the bank statement.
    let lines: Vec<_> = crate::reconcile::unreconciled_lines(p, c.checking)
        .await
        .unwrap()
        .into_iter()
        .filter(|l| l.entry_date.as_str() <= "2025-03-31")
        .collect();
    let ending: i64 = lines.iter().map(|l| l.amount_cents).sum();
    let rec = crate::reconcile::start(p, c.checking, d(2025, 3, 31), ending).await.expect("start reconciliation");
    let ids: Vec<i64> = lines.iter().map(|l| l.line_id).collect();
    let progress = crate::reconcile::set_lines_cleared(p, rec.id, &ids, true).await.expect("tick lines");
    assert_eq!(progress.difference_cents, 0, "the statement balances");
    crate::reconcile::lock(p, rec.id).await.expect("finish reconciliation");

    // The contractor is a 1099 vendor.
    let reyes: i64 = sqlx::query_scalar("SELECT id FROM vendors WHERE name = 'Jordan Reyes'").fetch_one(p).await.unwrap();
    crate::parties::update_party(
        p,
        "vendor",
        reyes,
        crate::parties::PartyDetailsInput {
            name: "Jordan Reyes".into(),
            company_name: None,
            email: None,
            phone: None,
            address: Some("18 Gull Point Rd, Portland, ME 04101".into()),
            terms_days: None,
            notes: None,
            tax_id: Some("123-45-6789".into()),
            keep_tax_id: false,
            is_1099: true,
            tax_rate_id: None,
            is_tax_exempt: false,
            tax_exemption: None,
            form_style_id: None,
        },
        crate::tax_ids::Access::OPEN,
    )
    .await
    .expect("mark 1099 vendor");

    pool
}
