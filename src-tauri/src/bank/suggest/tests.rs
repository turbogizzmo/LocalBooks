use std::collections::HashMap;
use std::time::Instant;

use regex::{Regex, RegexBuilder};

use super::*;
use crate::bank::{combine, learned_pattern, CategorySuggestion, RuleSet};
use crate::db::test_pool;

fn day(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn ex(description: &str, amount_cents: i64, account_id: i64, payee: Option<&str>, date: &str) -> Example {
    Example { bank_account_id: 1, date: day(date), description: description.into(), amount_cents, account_id, payee: payee.map(Into::into) }
}

fn query<'a>(description: &'a str, amount_cents: i64, date: &str) -> Query<'a> {
    Query { bank_account_id: 1, date: day(date), description, amount_cents }
}

#[test]
fn tokens_drop_numbers_codes_and_payment_noise() {
    assert_eq!(tokens("POS PURCHASE TARGET 00012345 AUSTIN TX"), ["TARGET", "AUSTIN", "TX"]);
    assert_eq!(tokens("AMZN Mktp US*2K4LM1"), ["AMZN", "MKTP", "US"]);
    assert_eq!(tokens("LOWE'S #1234*"), ["LOWE'S"]);
    assert_eq!(tokens("AT&T *BILL PAYMENT"), ["AT&T", "BILL", "PAYMENT"]);
    assert!(tokens("CHECK 1043").contains(&"CHECK".to_string()));
    assert!(tokens("#12 4 *").is_empty());
}

#[test]
fn suggests_from_similar_lines_with_a_reason_and_payee() {
    let model = Model::train(
        [
            ex("TARGET 00012345 AUSTIN TX", -4_210, 10, Some("Target"), "2026-01-03"),
            ex("TARGET 00012345 AUSTIN TX", -1_999, 10, Some("Target"), "2026-02-11"),
            ex("SHELL OIL 57444 AUSTIN TX", -5_200, 20, Some("Shell"), "2026-02-12"),
            ex("STRIPE TRANSFER", 120_000, 30, None, "2026-02-13"),
        ],
        None,
    );
    let p = model.predict(&query("TARGET 00099 DALLAS TX", -3_050, "2026-03-01")).expect("a suggestion");
    assert_eq!(p.account_id, 10);
    assert_eq!(p.payee.as_deref(), Some("Target"));
    assert!(p.similar_to.description.starts_with("TARGET"), "{:?}", p.similar_to);
    assert!(p.confidence >= 50, "{}", p.confidence);

    // A merchant's new descriptor shares one distinctive word with the past ones.
    let others = ["SHELL OIL AUSTIN TX", "STARBUCKS PORTLAND", "SPECTRUM CABLE", "GUSTO PAYROLL"];
    let amazon = Model::train(
        (1..=9)
            .map(|d| ex(&format!("AMZN MKTP US*{}", ["K2", "Q7", "Z1"][d % 3]), -2_000 - d as i64, 10, Some("Amazon"), &format!("2026-01-0{d}")))
            .chain((1..=20).map(|d| ex(others[d % 4], -5_000 - d as i64, 20 + (d % 4) as i64, None, &format!("2026-02-{:02}", d)))),
        None,
    );
    let p = amazon.predict(&query("AMZN DIGITAL*RT4KL2 SEATTLE WA", -1_299, "2026-03-01")).expect("a suggestion");
    assert_eq!((p.account_id, p.payee.as_deref()), (10, Some("Amazon")));

    // Money in never matches money out, and a line like nothing in the books gets nothing.
    assert_eq!(model.predict(&query("TARGET REFUND", 3_050, "2026-03-01")), None);
    assert_eq!(model.predict(&query("BLUE BOTTLE OAKLAND", -650, "2026-03-01")), None);
}

#[test]
fn exact_amounts_identify_recurring_charges_without_words() {
    let model = Model::train(
        (1..=6).map(|m| ex(&format!("CHECK {}", 1000 + m), -285_000, 40, Some("Riverside Properties"), &format!("2026-0{m}-01"))).chain(
            (1..=6).map(|m| ex(&format!("CHECK {}", 2000 + m), -(20_000 + m * 3_517), 50, Some("Jane Smith"), &format!("2026-0{m}-17"))),
        ),
        None,
    );
    let rent = model.predict(&query("CHECK 1107", -285_000, "2026-07-01")).expect("rent");
    assert_eq!((rent.account_id, rent.payee.as_deref()), (40, Some("Riverside Properties")));
    let contractor = model.predict(&query("CHECK 2207", -41_000, "2026-07-16")).expect("contractor");
    assert_eq!(contractor.account_id, 50);
}

#[test]
fn inactive_categories_and_the_bank_itself_are_never_suggested() {
    let history = [ex("ACME SUPPLY", -1_000, 10, None, "2026-01-01"), ex("ACME SUPPLY", -1_100, 1, None, "2026-01-02")];
    let model = Model::train(history.clone(), Some([1].into_iter().collect()));
    assert_eq!(model.predict(&query("ACME SUPPLY", -1_050, "2026-02-01")), None);
    let model = Model::train(history, None);
    assert_eq!(model.predict(&query("ACME SUPPLY", -1_050, "2026-02-01")).map(|p| p.account_id), Some(10));
}

#[test]
fn a_rule_restricts_the_model_to_its_category_for_the_payee() {
    let model = Model::train(
        [
            ex("COSTCO WHSE #0123 AUSTIN", -20_000, 10, Some("Costco"), "2026-01-01"),
            ex("COSTCO GAS #0123 AUSTIN", -5_000, 20, Some("Costco Gas"), "2026-01-02"),
        ],
        None,
    );
    let q = [query("COSTCO GAS #0456 DALLAS", -5_100, "2026-02-01")];
    let p = model.predict_batch(&q, &[Some(10)]).pop().flatten().expect("restricted prediction");
    assert_eq!((p.account_id, p.payee.as_deref()), (10, Some("Costco")));
}

#[test]
fn common_words_still_count_toward_similarity() {
    // "ZZZ" is seen first, so its id is lower than "AAA"'s although it sorts later as a key: the
    // stored ids must still be in id order for the common-word lookup.
    let mut history = vec![ex("ZZZ STORE", -5_000, 10, None, "2026-01-01")];
    history.extend((0..20).map(|d| ex("AAA COMMON ZZZ", -5_000, 10, None, &format!("2026-02-{:02}", d + 1))));
    history.extend((0..20).map(|d| ex(&format!("OTHER{}", ["X", "Y", "Q", "W"][d % 4]), -5_000, 20, None, &format!("2026-03-{:02}", d + 1))));
    let model = Model::train(history, None);
    assert!(model.rows.iter().all(|r| r.features.windows(2).all(|w| w[0] < w[1])), "feature ids are sorted");
}

#[test]
fn confidence_levels() {
    assert_eq!((confidence_level(95), confidence_level(80), confidence_level(79), confidence_level(50), confidence_level(49)), ("HIGH", "HIGH", "MEDIUM", "MEDIUM", "LOW"));
}

// ------------------------------------------------------------------------------------------------
// The cache and the database: approvals teach the model, exclusions do not
// ------------------------------------------------------------------------------------------------

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.unwrap()
}

async fn add_line(pool: &SqlitePool, bank: i64, date: &str, description: &str, amount: i64) {
    let mut tx = pool.begin().await.unwrap();
    crate::bank::insert_line(&mut tx, bank, day(date), description, amount, &format!("{date}|{description}|{amount}")).await.unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn approvals_retrain_incrementally_and_rules_still_win() {
    use crate::bank::{approve, exclude, list_pending, Approval};
    let pool = test_pool().await;
    let checking = account(&pool, "1010").await;
    let supplies = account(&pool, "6250").await;
    let suggester = Suggester::default();

    add_line(&pool, checking, "2026-01-05", "TARGET 00012345 AUSTIN TX", -4_210).await;
    add_line(&pool, checking, "2026-01-06", "PIZZA PLANET 12", -2_000).await;
    let pending = list_pending(&pool, &suggester, "f").await.unwrap();
    assert_eq!(suggester.cached_len().await, Some(0));
    assert!(pending.iter().all(|t| t.suggestion.as_ref().unwrap().origin == "DEFAULT"));

    let target = pending.iter().find(|t| t.description.starts_with("TARGET")).unwrap();
    let approval = Approval { category_account_id: supplies, payee_name: Some("Target".into()), ..Approval::default() };
    approve(&pool, &suggester, "f", target.id, approval).await.unwrap();
    assert_eq!(suggester.cached_len().await, Some(1), "the approval was added without a rebuild");

    let pizza = pending.iter().find(|t| t.description.starts_with("PIZZA")).unwrap();
    exclude(&pool, pizza.id).await.unwrap();

    // A new store number and city: the learned rule (TARGET AUSTIN) misses, the model fills in.
    add_line(&pool, checking, "2026-02-07", "TARGET 00099 DALLAS TX", -3_000).await;
    // Same store as before: the learned rule matches and wins.
    add_line(&pool, checking, "2026-02-08", "TARGET 00012345 AUSTIN TX", -1_000).await;
    let pending = list_pending(&pool, &suggester, "f").await.unwrap();
    assert_eq!(suggester.cached_len().await, Some(1), "excluding taught nothing and forced no rebuild");
    let dallas = pending.iter().find(|t| t.description.contains("DALLAS")).unwrap();
    let s = dallas.suggestion.as_ref().unwrap();
    assert_eq!((s.account_id, s.origin.as_str()), (supplies, "MODEL"));
    assert!(s.confidence.is_some() && s.similar_to.as_ref().unwrap().description.contains("AUSTIN"));
    assert_eq!(dallas.payee.as_deref(), Some("Target"));
    let austin = pending.iter().find(|t| t.description.contains("AUSTIN")).unwrap();
    assert_eq!(austin.suggestion.as_ref().unwrap().origin, "LEARNED");
    assert_eq!(austin.payee.as_deref(), Some("Target"), "the model still offers a payee");

    // The approval became an expense with the chosen vendor.
    let vendor: String = sqlx::query_scalar("SELECT v.name FROM expenses x JOIN vendors v ON v.id = x.vendor_id").fetch_one(&pool).await.unwrap();
    assert_eq!(vendor, "Target");

    // Voiding the approval changes the history, so the model is rebuilt without it.
    let entry: i64 = sqlx::query_scalar("SELECT journal_entry_id FROM expenses").fetch_one(&pool).await.unwrap();
    crate::ledger::void_transaction(&pool, entry).await.unwrap();
    list_pending(&pool, &suggester, "f").await.unwrap();
    assert_eq!(suggester.cached_len().await, Some(0));

    // Another file never reuses this one's model.
    list_pending(&pool, &suggester, "other").await.unwrap();
    assert_eq!(suggester.cached_len().await, Some(0));
}

#[tokio::test]
async fn expenses_entered_directly_are_history_too() {
    let pool = test_pool().await;
    let checking = account(&pool, "1010").await;
    let software = account(&pool, "6350").await;
    let expense = crate::expenses::ExpenseInput {
        kind: "EXPENSE".into(),
        payee_name: Some("Canva".into()),
        payment_account_id: checking,
        expense_date: day("2026-01-10"),
        check_number: None,
        payment_method: None,
        memo: Some("Canva Pro subscription".into()),
        lines: vec![crate::expenses::SplitLineInput { account_id: software, amount_cents: 1_499, memo: None, class_id: None }],
        currency: None,
    };
    crate::expenses::post_expense(&pool, expense).await.unwrap();
    let examples = load_examples(&pool).await.unwrap();
    assert_eq!(examples.len(), 1);
    assert_eq!((examples[0].description.as_str(), examples[0].amount_cents, examples[0].account_id), ("Canva Canva Pro subscription", -1_499, software));
    let model = Model::train(examples, None);
    let p = model.predict(&Query { bank_account_id: checking, date: day("2026-02-10"), description: "CANVA* 0423-1234 SYDNEY", amount_cents: -1_499 }).unwrap();
    assert_eq!((p.account_id, p.payee.as_deref()), (software, Some("Canva")));
}

// ------------------------------------------------------------------------------------------------
// Offline evaluation on a synthetic history
// ------------------------------------------------------------------------------------------------

/// A small deterministic generator (SplitMix64), so the evaluation gives the same numbers every run.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len() as u64) as usize]
    }
}

/// A payee as it shows up on statements. `forms` use `{n}` (store or check number), `{c}` (city) and
/// `{r}` (reference code). Amounts are in cents; `fixed` charges are always the same.
struct Vendor {
    forms: &'static [&'static str],
    category: &'static str,
    payee: &'static str,
    min: i64,
    max: i64,
    fixed: bool,
    /// Usual day of the month, if the charge is monthly.
    day: Option<u32>,
    weight: u64,
    inflow: bool,
    card: bool,
}

const fn v(forms: &'static [&'static str], category: &'static str, payee: &'static str, min: i64, max: i64, weight: u64) -> Vendor {
    Vendor { forms, category, payee, min, max, fixed: min == max, day: None, weight, inflow: false, card: false }
}

const CITIES: &[&str] = &["AUSTIN TX", "ROUND ROCK TX", "DALLAS TX", "SAN ANTONIO TX", "HOUSTON TX", "GEORGETOWN TX"];

fn vendors() -> Vec<Vendor> {
    let monthly = |mut x: Vendor, d: u32| {
        x.day = Some(d);
        x
    };
    let card = |mut x: Vendor| {
        x.card = true;
        x
    };
    let inflow = |mut x: Vendor| {
        x.inflow = true;
        x
    };
    vec![
        card(v(&["TARGET {n} {c}", "TARGET.COM *{r}", "TARGET T-{n} {c}"], "6250", "Target", 1_500, 25_000, 6)),
        card(v(&["AMZN MKTP US*{r}", "AMAZON.COM*{r} AMZN.COM/BILL WA", "AMZN Mktp US {r}", "AMAZON MARKETPLACE {r}"], "6250", "Amazon", 900, 30_000, 10)),
        card(v(&["COSTCO WHSE #{n} {c}", "COSTCO WHOLESALE {n}"], "6250", "Costco", 5_000, 40_000, 3)),
        card(v(&["COSTCO GAS #{n} {c}"], "6100", "Costco", 3_000, 7_500, 3)),
        card(v(&["THE HOME DEPOT #{n} {c}", "HOMEDEPOT.COM {r}"], "5000", "Home Depot", 2_000, 60_000, 5)),
        card(v(&["LOWE'S #{n} {c}", "LOWES #{n}*"], "5000", "Lowe's", 2_000, 45_000, 3)),
        card(v(&["JAVA JUNCTION {c}", "SQ *JAVA JUNCTION"], "6200", "Java Junction", 400, 1_800, 6)),
        card(v(&["TST* TAQUERIA ARANDAS {n}", "TAQUERIA ARANDAS #{n} {c}"], "6200", "Taqueria Arandas", 1_200, 6_000, 4)),
        card(v(&["H-E-B #{n} {c}", "HEB ONLINE {r}"], "6200", "H-E-B", 1_500, 9_000, 4)),
        card(v(&["CHUY'S {n} {c}", "CHUYS {c}"], "6200", "Chuy's", 2_500, 12_000, 2)),
        monthly(v(&["ONLINE PMT RIVERSIDE PROPERTIES", "RIVERSIDE PROP MGMT WEB PMT {r}"], "6300", "Riverside Properties", 285_000, 285_000, 1), 1),
        monthly(v(&["SPECTRUM {r}", "CHARTER SPECTRUM ONLINE"], "6550", "Spectrum", 8_999, 8_999, 1), 9),
        monthly(v(&["AUSTIN ENERGY WEB PMT {r}", "CITY OF AUSTIN UTIL {n}"], "6500", "Austin Energy", 9_000, 24_000, 1), 15),
        monthly(card(v(&["INTUIT *QBOOKS ONLINE", "INTUIT *QUICKBOOKS {r}"], "6350", "Intuit", 3_000, 3_000, 1)), 3),
        monthly(card(v(&["CANVA* {n}-{n} SYDNEY", "CANVA PTY LTD {r}"], "6350", "Canva", 1_499, 1_499, 1)), 21),
        monthly(card(v(&["GOOGLE *GSUITE_{r}", "GOOGLE *WORKSPACE {r}"], "6350", "Google", 1_400, 1_400, 1)), 1),
        v(&["PAYPAL *JSMITHDESIGN", "PAYPAL INST XFER JSMITHDESIGN"], "6400", "Jane Smith Design", 20_000, 150_000, 2),
        v(&["ZELLE PAYMENT TO MARIA LOPEZ CONF# {r}", "ZELLE TO LOPEZ MARIA {r}"], "6400", "Maria Lopez", 12_000, 12_000, 3),
        v(&["ZELLE PAYMENT TO TOM BAKER CONF# {r}", "ZELLE TO BAKER TOM {r}"], "OWNER", "Tom Baker", 50_000, 200_000, 1),
        monthly(v(&["GUSTO PAYROLL {n}", "GUSTO DES:NET PAY ID:{n}"], "WAGES", "Gusto", 400_000, 650_000, 2), 15),
        monthly(v(&["GUSTO TAX {n}", "GUSTO DES:TAX ID:{n}"], "PAYROLL_TAX", "Gusto", 60_000, 120_000, 2), 16),
        card(v(&["SHELL OIL {n} {c}", "SHELL SERVICE STATION {c}"], "6100", "Shell", 2_500, 7_000, 4)),
        card(v(&["BUC-EE'S #{n} {c}", "BUCEES {n} {c}"], "6100", "Buc-ee's", 2_500, 8_000, 3)),
        monthly(v(&["HISCOX INC {r}"], "6150", "Hiscox", 4_200, 4_200, 1), 5),
        card(v(&["USPS PO {n} {c}", "USPS.COM CLICKNSHIP {r}"], "6600", "USPS", 500, 9_000, 3)),
        monthly(v(&["MONTHLY SERVICE FEE"], "6050", "Bank", 1_500, 1_500, 1), 28),
        card(v(&["FACEBK *{r}", "META PLATFORMS ADS {r}", "FACEBK ADS {r}"], "6000", "Meta", 2_000, 50_000, 3)),
        card(v(&["SOUTHWES {n}", "SOUTHWEST AIRLINES {r}"], "6450", "Southwest Airlines", 9_000, 60_000, 1)),
        card(v(&["MARRIOTT {c}", "COURTYARD BY MARRIOTT {c}"], "6450", "Marriott", 12_000, 50_000, 1)),
        inflow(v(&["STRIPE TRANSFER ST-{r}"], "4000", "", 20_000, 900_000, 6)),
        inflow(v(&["SHOPIFY PAYOUT {r}", "SHOPIFY* {n}"], "4000", "", 10_000, 400_000, 4)),
        inflow(v(&["MOBILE DEPOSIT {n}", "REMOTE DEPOSIT CAPTURE {n}"], "4100", "", 50_000, 500_000, 3)),
        monthly(inflow(v(&["INTEREST PAYMENT", "INTEREST CREDIT"], "7000", "", 10, 900, 1)), 28),
        inflow(v(&["AMAZON.COM REFUND {r}", "AMZN MKTP US REFUND {r}"], "6250", "", 900, 15_000, 1)),
        inflow(v(&["ONLINE TRANSFER FROM SAV {n}", "TRANSFER FROM SAVINGS {n}"], "1020", "", 100_000, 500_000, 1)),
    ]
}

struct Txn {
    bank: i64,
    date: NaiveDate,
    description: String,
    amount: i64,
    category: i64,
    payee: Option<String>,
}

/// About `count` transactions over two years, oldest first. 3% are categorized inconsistently, as
/// people do.
fn synthetic_history(count: usize, seed: u64, categories: &HashMap<&str, i64>, banks: (i64, i64)) -> Vec<Txn> {
    let vendors = vendors();
    let mut all: Vec<i64> = categories.values().copied().collect();
    all.sort_unstable();
    all.dedup();
    let total_weight: u64 = vendors.iter().map(|v| v.weight).sum();
    let mut rng = Rng(seed);
    let start = day("2024-11-01");
    let mut txns = Vec::with_capacity(count);
    let fill = |rng: &mut Rng, form: &str| {
        let mut out = String::new();
        let mut rest = form;
        while let Some(i) = rest.find('{') {
            out.push_str(&rest[..i]);
            match &rest[i..i + 3] {
                "{n}" => out.push_str(&format!("{:0width$}", rng.below(99_999), width = 3 + rng.below(3) as usize)),
                "{c}" => out.push_str(rng.pick(CITIES)),
                _ => out.extend((0..6 + rng.below(4)).map(|_| char::from(b"ABCDEFGHJKLMNPQRSTUVWXYZ0123456789"[rng.below(34) as usize]))),
            }
            rest = &rest[i + 3..];
        }
        out.push_str(rest);
        out
    };
    for _ in 0..count {
        let mut pick = rng.below(total_weight);
        let mut vendor = &vendors[0];
        for v in &vendors {
            vendor = v;
            if pick < v.weight {
                break;
            }
            pick -= v.weight;
        }
        let date = match vendor.day {
            Some(d) => {
                let month = start + chrono::Months::new(rng.below(24) as u32);
                month.with_day(d).unwrap() + chrono::Days::new(rng.below(3))
            }
            None => start + chrono::Days::new(rng.below(730)),
        };
        let amount = if vendor.fixed { vendor.min } else { vendor.min + rng.below((vendor.max - vendor.min) as u64) as i64 };
        let mut category = categories[vendor.category];
        if rng.below(100) < 3 {
            category = all[rng.below(all.len() as u64) as usize];
        }
        txns.push(Txn {
            bank: if vendor.card { banks.1 } else { banks.0 },
            date,
            description: {
                let form = rng.pick(vendor.forms);
                fill(&mut rng, form)
            },
            amount: if vendor.inflow { amount } else { -amount },
            category,
            payee: Some(vendor.payee.to_string()).filter(|p| !p.is_empty()),
        });
    }
    txns.sort_by_key(|t| t.date);
    txns
}

/// The app's rules, simulated in memory: built-in rules from the company file, plus a learned rule
/// upserted on every approval exactly as `write_approval` does, matched in the order `RuleSet::load` uses.
struct SimRules {
    rules: Vec<(i64, Regex, String, i64, String, i64, i64)>, // id, regex, direction, account, origin, priority, hits
    uncategorized: (i64, i64),
    next_id: i64,
}

impl SimRules {
    async fn load(pool: &SqlitePool) -> Self {
        let rows: Vec<(i64, String, String, i64, String, i64, i64)> =
            sqlx::query_as("SELECT id, pattern, direction, account_id, origin, priority, hit_count FROM bank_rules").fetch_all(pool).await.unwrap();
        let rules = rows.into_iter().map(|(id, p, d, a, o, pr, h)| (id, RegexBuilder::new(&p).case_insensitive(true).build().unwrap(), d, a, o, pr, h)).collect();
        SimRules { rules, uncategorized: (account(pool, "6900").await, account(pool, "4900").await), next_id: 1_000_000 }
    }

    fn matching(&mut self, description: &str, amount: i64) -> Option<CategorySuggestion> {
        let direction = if amount < 0 { "OUTFLOW" } else { "INFLOW" };
        self.rules.sort_by(|a, b| b.5.cmp(&a.5).then(b.6.cmp(&a.6)).then(a.0.cmp(&b.0)));
        self.rules
            .iter()
            .find(|r| (r.2 == "ANY" || r.2 == direction) && r.1.is_match(description))
            .map(|r| CategorySuggestion::plain(r.3, Some(r.0), &r.4))
    }

    fn learn(&mut self, description: &str, amount: i64, account_id: i64) {
        let Some(pattern) = learned_pattern(description) else { return };
        let direction = if amount < 0 { "OUTFLOW" } else { "INFLOW" };
        match self.rules.iter_mut().find(|r| r.1.as_str() == pattern && r.2 == direction) {
            Some(r) => {
                r.3 = account_id;
                r.4 = "LEARNED".into();
                r.5 = r.5.max(1000);
                r.6 += 1;
            }
            None => {
                self.next_id += 1;
                let regex = RegexBuilder::new(&pattern).case_insensitive(true).build().unwrap();
                self.rules.push((self.next_id, regex, direction.into(), account_id, "LEARNED".into(), 1000, 1));
            }
        }
    }

    fn fallback(&self, amount: i64) -> i64 {
        if amount < 0 { self.uncategorized.0 } else { self.uncategorized.1 }
    }
}

#[derive(Default, Debug)]
struct Score {
    total: usize,
    rules_only: usize,
    rules_then_model: usize,
    model_only: usize,
    no_rule: usize,
    no_rule_model_right: usize,
    model_suggested: usize,
    payee_right: usize,
    payee_total: usize,
    by_level: HashMap<&'static str, (usize, usize)>,
    alone_by_level: HashMap<&'static str, (usize, usize)>,
}

/// Replays the history in date order: the first 70% are approved up front, then each later line is
/// suggested for (rules only; rules then the model; the model alone), scored against what the user
/// approved, and approved, so both the rules and the model learn from it as they would in the app.
#[tokio::test]
async fn offline_evaluation_on_synthetic_history() {
    let pool = test_pool().await;
    let mut categories: HashMap<&str, i64> = HashMap::new();
    for n in ["4000", "4100", "5000", "6000", "6050", "6100", "6150", "6200", "6250", "6300", "6350", "6400", "6450", "6500", "6550", "6600", "7000", "1020"] {
        categories.insert(n, account(&pool, n).await);
    }
    categories.extend([("OWNER", 900_001), ("WAGES", 900_002), ("PAYROLL_TAX", 900_003)]);
    let banks = (account(&pool, "1010").await, account(&pool, "2100").await);
    let rules_for_combine = RuleSet::load(&pool).await.unwrap();

    let mut overall = Score::default();
    for seed in [7, 11, 23] {
        let history = synthetic_history(5_000, seed, &categories, banks);
        let split = history.len() * 7 / 10;
        let mut rules = SimRules::load(&pool).await;
        let mut model = Model::default();
        for t in &history[..split] {
            rules.learn(&t.description, t.amount, t.category);
            model.add(Example { bank_account_id: t.bank, date: t.date, description: t.description.clone(), amount_cents: t.amount, account_id: t.category, payee: t.payee.clone() });
        }
        for t in &history[split..] {
            let q = Query { bank_account_id: t.bank, date: t.date, description: &t.description, amount_cents: t.amount };
            let ruled = rules.matching(&t.description, t.amount);
            let rules_only = ruled.as_ref().map_or(rules.fallback(t.amount), |s| s.account_id);
            let alone = model.predict(&q);
            let only = ruled.as_ref().map(|s| s.account_id);
            let predicted = model.predict_batch(std::slice::from_ref(&q), &[only]).pop().flatten();
            let had_rule = ruled.is_some();
            let (combined, payee) = combine(&rules_for_combine, t.amount, ruled, predicted);
            let combined = combined.unwrap();
            let combined_account = if combined.origin == "DEFAULT" { rules.fallback(t.amount) } else { combined.account_id };

            overall.total += 1;
            overall.rules_only += usize::from(rules_only == t.category);
            overall.rules_then_model += usize::from(combined_account == t.category);
            overall.model_only += usize::from(alone.as_ref().map_or(rules.fallback(t.amount), |p| p.account_id) == t.category);
            if let Some(p) = &alone {
                let e = overall.alone_by_level.entry(confidence_level(p.confidence)).or_default();
                e.0 += 1;
                e.1 += usize::from(p.account_id == t.category);
            }
            if !had_rule {
                overall.no_rule += 1;
                overall.no_rule_model_right += usize::from(combined_account == t.category);
            }
            if let Some(level) = combined.confidence_level {
                overall.model_suggested += 1;
                let e = overall.by_level.entry(level).or_default();
                e.0 += 1;
                e.1 += usize::from(combined_account == t.category);
            }
            if t.amount < 0 && t.payee.is_some() {
                overall.payee_total += 1;
                overall.payee_right += usize::from(payee == t.payee);
            }

            rules.learn(&t.description, t.amount, t.category);
            model.add(Example { bank_account_id: t.bank, date: t.date, description: t.description.clone(), amount_cents: t.amount, account_id: t.category, payee: t.payee.clone() });
        }
    }

    let pct = |n: usize, d: usize| 100.0 * n as f64 / d.max(1) as f64;
    println!("offline evaluation: {} test lines over 3 synthetic histories", overall.total);
    println!("  top-1 rules only (today):        {:.1}%", pct(overall.rules_only, overall.total));
    println!("  top-1 rules, then model:         {:.1}%", pct(overall.rules_then_model, overall.total));
    println!("  top-1 model alone (no rules):    {:.1}%", pct(overall.model_only, overall.total));
    println!("  lines no rule matched:           {} ({:.1}%), model right on {:.1}% of them", overall.no_rule, pct(overall.no_rule, overall.total), pct(overall.no_rule_model_right, overall.no_rule));
    for level in ["HIGH", "MEDIUM", "LOW"] {
        let (n, right) = overall.by_level.get(level).copied().unwrap_or_default();
        let (alone_n, alone_right) = overall.alone_by_level.get(level).copied().unwrap_or_default();
        println!("  {level:<6} confidence: {n} lines no rule matched, {:.1}% right; model alone {alone_n} lines, {:.1}% right", pct(right, n), pct(alone_right, alone_n));
    }
    println!("  payee right (money out):         {:.1}%", pct(overall.payee_right, overall.payee_total));

    assert!(overall.rules_then_model > overall.rules_only, "{overall:?}");
    assert!(pct(overall.rules_then_model, overall.total) - pct(overall.rules_only, overall.total) >= 5.0, "{overall:?}");
    let (high, high_right) = overall.alone_by_level["HIGH"];
    assert!(pct(high_right, high) >= 90.0, "HIGH confidence should be right at least 90% of the time: {overall:?}");
}

/// About 50,000 rows of history and a batch of 500 suggestions. Prints timings; fails if a batch
/// takes over 100 ms in an optimized build.
#[test]
fn fast_on_fifty_thousand_rows() {
    let mut categories: HashMap<&str, i64> = HashMap::new();
    for (i, v) in vendors().iter().enumerate() {
        categories.entry(v.category).or_insert(100 + i as i64);
    }
    let history = synthetic_history(50_500, 99, &categories, (1, 2));
    let (train, test) = history.split_at(50_000);
    let started = Instant::now();
    let model = Model::train(
        train.iter().map(|t| Example { bank_account_id: t.bank, date: t.date, description: t.description.clone(), amount_cents: t.amount, account_id: t.category, payee: t.payee.clone() }),
        None,
    );
    let trained = started.elapsed();
    let queries: Vec<Query> = test.iter().map(|t| Query { bank_account_id: t.bank, date: t.date, description: &t.description, amount_cents: t.amount }).collect();
    // The best of three runs, so other tests running at the same time don't decide the result.
    let mut batch = std::time::Duration::MAX;
    let mut predictions = Vec::new();
    for _ in 0..3 {
        let started = Instant::now();
        predictions = model.predict_batch(&queries, &vec![None; queries.len()]);
        batch = batch.min(started.elapsed());
    }
    let started = Instant::now();
    let mut grown = model;
    grown.add(Example { bank_account_id: 1, date: day("2026-10-01"), description: "NEW VENDOR".into(), amount_cents: -100, account_id: 100, payee: None });
    let added = started.elapsed();
    println!("50,000 rows: trained in {trained:?}; 500 suggestions in {batch:?}; one approval learned in {added:?}");
    assert_eq!(predictions.len(), 500);
    // Unoptimized test builds are many times slower; the 100 ms target is for the app's build.
    let limit = if cfg!(debug_assertions) { 10_000 } else { 100 };
    assert!(batch.as_millis() < limit, "500 suggestions took {batch:?}");
}

/// A company for trying suggestions by hand: two years of approved bank lines (through the real
/// approval path), then new lines to review that no learned rule matches.
/// `LOCALBOOKS_SEED_DIR=<folder> cargo test -- --ignored seed_smart_suggestions_demo --nocapture`
#[tokio::test]
#[ignore = "writes a company file for a hands-on check"]
async fn seed_smart_suggestions_demo() {
    use crate::bank::{approve, list_pending, Approval};
    let dir = std::path::PathBuf::from(std::env::var("LOCALBOOKS_SEED_DIR").expect("set LOCALBOOKS_SEED_DIR"));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Smart Suggestions.localbooks");
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    let pool = crate::db::open(&path).await.expect("create the company file");
    crate::preferences::set(&pool, "company_name", "Smart Suggestions Test Co").await.unwrap();

    let mut categories: HashMap<&str, i64> = HashMap::new();
    for n in ["4000", "4100", "5000", "6000", "6050", "6100", "6150", "6200", "6250", "6300", "6350", "6400", "6450", "6500", "6550", "6600", "7000", "1020"] {
        categories.insert(n, account(&pool, n).await);
    }
    let contractors = categories["6400"];
    categories.extend([("OWNER", contractors), ("WAGES", contractors), ("PAYROLL_TAX", contractors)]);
    let banks = (account(&pool, "1010").await, account(&pool, "2100").await);
    let suggester = Suggester::default();
    let history = synthetic_history(400, 5, &categories, banks);
    for (i, t) in history.iter().enumerate() {
        let mut tx = pool.begin().await.unwrap();
        crate::bank::insert_line(&mut tx, t.bank, t.date, &t.description, t.amount, &format!("demo|{i}")).await.unwrap();
        tx.commit().await.unwrap();
        let id: i64 = sqlx::query_scalar("SELECT MAX(id) FROM bank_transactions").fetch_one(&pool).await.unwrap();
        let payee_name = t.payee.clone().filter(|_| t.amount < 0);
        approve(&pool, &suggester, "demo", id, Approval { category_account_id: t.category, payee_name, ..Approval::default() }).await.unwrap_or_else(|e| panic!("line {i}: {e}"));
    }
    for (date, description, amount, card) in [
        ("2026-10-02", "TARGET T-4471 PFLUGERVILLE TX", -6_412, true),
        ("2026-10-03", "AMZN DIGITAL*RT4KL2 SEATTLE WA", -1_299, true),
        ("2026-10-04", "SQ *JAVA JUNCTION KYLE", -725, true),
        ("2026-10-05", "HOMEDEPOT.COM 9X2LLQ", -18_840, true),
        ("2026-10-06", "CANVA PTY LTD SYDNEY", -1_499, true),
        ("2026-10-01", "RIVERSIDE PROPERTIES ACH", -285_000, false),
        ("2026-10-07", "ZELLE TO LOPEZ MARIA QQ81", -12_000, false),
        ("2026-10-08", "SHOPIFY PAYMENTS PAYOUT", 84_210, false),
        ("2026-10-09", "BLUE BOTTLE OAKLAND", -650, true),
        ("2026-10-10", "SHELL OIL 57444 AUSTIN TX", -4_810, true),
    ] {
        add_line(&pool, if card { banks.1 } else { banks.0 }, date, description, amount).await;
    }
    let pending = list_pending(&pool, &suggester, "demo").await.unwrap();
    // What the Bank Feed screen receives, for checking the screen against real suggestions.
    std::fs::write(dir.join("Smart Suggestions.pending.json"), serde_json::to_string_pretty(&pending).unwrap()).unwrap();
    for t in pending {
        let s = t.suggestion.unwrap();
        println!("{:<34} {:>9} {:<9} {:>4} payee {:?} similar {:?}", t.description, t.amount_cents, s.origin, s.confidence.map_or(String::new(), |c| c.to_string()), t.payee, s.similar_to.map(|x| x.description));
    }
    crate::db::compact(&pool).await.unwrap();
    pool.close().await;
}
