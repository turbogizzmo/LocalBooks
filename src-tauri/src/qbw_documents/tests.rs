use super::*;

#[test]
fn payment_methods_map_to_localbooks_methods() {
    for (name, code) in [
        ("Cash", "CASH"),
        ("Check", "CHECK"),
        ("E-Check", "CHECK"),
        ("VISA", "CREDIT_CARD"),
        ("Master Card", "CREDIT_CARD"),
        ("American Express", "CREDIT_CARD"),
        ("QuickBooks Payments-Credit Card", "CREDIT_CARD"),
        ("QuickBooks Payments-Bank", "ACH"),
        ("Direct Deposit", "ACH"),
        ("Wire", "WIRE"),
        ("Gift Certificate/Card", "OTHER"),
        ("Rewards Account", "OTHER"),
    ] {
        assert_eq!(payment_method(Some(name)), code, "{name}");
    }
    assert_eq!(payment_method(None), "OTHER");
}

/// Checks the documents of a real file against its ledger: form lines plus the receivable line
/// balance each invoice and credit memo, applications never exceed what they apply to, and the
/// open balances add up to each customer's stored A/R balance.
#[test]
#[ignore]
fn documents_of_a_real_file_agree_with_its_ledger() {
    use crate::qbw_company::{read_ledger, read_pages};
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
    let ledger = read_ledger(std::path::Path::new(&path)).unwrap();
    let (pages, catalog, _, _) = read_pages(std::path::Path::new(&path)).unwrap();
    let ar: std::collections::HashSet<i64> = ledger.accounts.iter().filter(|a| a.account_type == "ACCOUNTS_RECEIVABLE").map(|a| a.id).collect();
    let ar_of = |t: &crate::qbw_company::SourceTransaction| -t.lines.iter().filter(|l| ar.contains(&l.account_id)).map(|l| l.cents).sum::<i64>();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut open: HashMap<i64, i64> = HashMap::new();
    let mut customer_ar: HashMap<i64, i64> = HashMap::new();
    let (mut form_mismatch, mut over_applied, mut dangling) = (0, 0, 0);
    for t in &ledger.transactions {
        let Some(doc) = &t.document else { continue };
        let kind = match doc {
            SourceDocument::Invoice { .. } => "invoice",
            SourceDocument::CreditMemo { .. } => "credit memo",
            SourceDocument::SalesReceipt { .. } => "sales receipt",
            SourceDocument::Payment { .. } => "payment",
            SourceDocument::Deposit { .. } => "deposit",
            SourceDocument::Bill { .. } => "bill",
            SourceDocument::BillPayment { .. } => "bill payment",
            SourceDocument::Expense { .. } => "expense",
            SourceDocument::Transfer => "transfer",
            SourceDocument::Estimate { .. } => "estimate",
            SourceDocument::PurchaseOrder { .. } => "purchase order",
        };
        *counts.entry(kind).or_default() += 1;
        match doc {
            SourceDocument::Invoice { customer_id, lines, .. } | SourceDocument::CreditMemo { customer_id, lines, .. } => {
                let form: i64 = lines.iter().map(|l| l.cents).sum();
                if form != ar_of(t) {
                    form_mismatch += 1;
                }
                if matches!(doc, SourceDocument::Invoice { .. }) {
                    *open.entry(t.id).or_default() += ar_of(t);
                }
                *customer_ar.entry(*customer_id).or_default() += ar_of(t);
            }
            SourceDocument::Payment { customer_id, .. } => {
                *customer_ar.entry(*customer_id).or_default() += ar_of(t);
            }
            _ => {}
        }
    }
    for t in &ledger.transactions {
        let applied = match &t.document {
            Some(SourceDocument::Payment { applied_to, .. }) | Some(SourceDocument::CreditMemo { applied_to, .. }) => applied_to,
            _ => continue,
        };
        for a in applied {
            match open.get_mut(&a.transaction_id) {
                Some(o) => {
                    *o -= a.cents;
                    if *o < 0 {
                        over_applied += 1;
                    }
                }
                None => dangling += 1,
            }
        }
    }
    let stored: HashMap<i64, i64> = crate::qbw_company::rows_of(&pages, &catalog, "abmc_customer_internal")
        .iter()
        .filter_map(|r| Some((r.get("customer_id").as_int()?, r.get("end_balance_amt").as_cents()?)))
        .collect();
    let customers_matching = stored.iter().filter(|(c, b)| customer_ar.get(c).copied().unwrap_or(0) == **b).count();
    let open_invoices = open.values().filter(|o| **o > 0).count();
    println!("{counts:?}");
    println!("form lines not balancing A/R: {form_mismatch}; over-applied: {over_applied}; applied to unknown: {dangling}");
    println!("open invoices: {open_invoices}, open total {}", crate::ledger::format_cents(open.values().filter(|o| **o > 0).sum()));
    println!("customers with stored balance matching: {customers_matching}/{}", stored.len());
    let ap: std::collections::HashSet<i64> = ledger.accounts.iter().filter(|a| a.account_type == "ACCOUNTS_PAYABLE").map(|a| a.id).collect();
    let mut open_bills: HashMap<i64, i64> = HashMap::new();
    for t in &ledger.transactions {
        if let Some(SourceDocument::Bill { .. }) = &t.document {
            let owed: i64 = t.lines.iter().filter(|l| ap.contains(&l.account_id)).map(|l| l.cents).sum();
            if owed > 0 {
                open_bills.insert(t.id, owed);
            }
        }
    }
    for t in &ledger.transactions {
        if let Some(SourceDocument::BillPayment { applied_to, .. } | SourceDocument::Expense { applied_to, .. }) = &t.document {
            for a in applied_to {
                if let Some(o) = open_bills.get_mut(&a.transaction_id) {
                    *o -= a.cents;
                }
            }
        }
    }
    println!(
        "open bills: {}, open total {}",
        open_bills.values().filter(|o| **o > 0).count(),
        crate::ledger::format_cents(open_bills.values().filter(|o| **o > 0).sum())
    );
    for (c, b) in stored.iter().filter(|(c, b)| customer_ar.get(c).copied().unwrap_or(0) != **b).take(10) {
        println!("  customer {c}: stored {b} computed {:?}", customer_ar.get(c));
    }
}

/// Prints every link row touching the payments that apply to nothing, and what each linked
/// transaction is.
#[test]
#[ignore]
fn traces_unapplied_payments() {
    use crate::qbw_company::{read_ledger, read_pages, rows_of};
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
    let ledger = read_ledger(std::path::Path::new(&path)).unwrap();
    let (pages, catalog, _, _) = read_pages(std::path::Path::new(&path)).unwrap();
    let kinds: HashMap<i64, (&str, Option<String>)> = ledger.transactions.iter().map(|t| (t.id, (t.kind, t.doc_num.clone()))).collect();
    let lonely: Vec<i64> = ledger
        .transactions
        .iter()
        .filter(|t| matches!(&t.document, Some(SourceDocument::Payment { applied_to, .. }) if applied_to.is_empty()))
        .map(|t| t.id)
        .collect();
    println!("{} payments apply to nothing", lonely.len());
    let links = rows_of(&pages, &catalog, "abmc_transaction_link");
    for id in lonely.iter().take(12) {
        println!("payment {id} {:?}", kinds.get(id));
        for r in links.iter().filter(|r| r.get("transaction_id").as_int() == Some(*id) || r.get("transaction_2_id").as_int() == Some(*id)) {
            let other = if r.get("transaction_id").as_int() == Some(*id) { r.get("transaction_2_id").as_int() } else { r.get("transaction_id").as_int() };
            println!(
                "   type {:?} t1 {:?} t2 {:?} src {:?} amt {:?} other {:?}",
                r.get("link_type").as_int(),
                r.get("transaction_id").as_int(),
                r.get("transaction_2_id").as_int(),
                r.get("is_t1_source_bool").as_int(),
                r.get("amount_amt").as_cents(),
                other.and_then(|o| kinds.get(&o))
            );
        }
    }
    // Applications the reader found but that point at transactions the ledger lacks or that are not invoices.
    let mut targets: BTreeMap<String, usize> = BTreeMap::new();
    for t in &ledger.transactions {
        if let Some(SourceDocument::Payment { applied_to, .. }) = &t.document {
            for a in applied_to {
                let k = kinds.get(&a.transaction_id).map_or("missing".to_string(), |k| k.0.to_string());
                *targets.entry(k).or_default() += 1;
            }
        }
    }
    println!("payment applications by target kind: {targets:?}");
}

/// Prints the purchase tables' columns and how transaction links pair up by kind, to find
/// where vendor credits, card credits and bill payments live.
#[test]
#[ignore]
fn explores_purchase_tables() {
    use crate::qbw_company::{read_ledger, read_pages, rows_of};
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
    let (pages, catalog, _, _) = read_pages(std::path::Path::new(&path)).unwrap();
    for stem in ["bill", "bill_check", "bill_ccard", "check", "credit_cardcharge"] {
        for part in ["header", "lineitem"] {
            let name = format!("abmc_{stem}_{part}");
            let Some(id) = catalog.table_id(&name) else { continue };
            let cols: Vec<&str> = catalog.columns[&id].iter().map(|c| c.name.as_str()).collect();
            println!("{name}: {}", cols.join(", "));
        }
    }
    let mut names: Vec<&String> = catalog.names.values().filter(|n| n.contains("credit") || n.contains("vendor") || n.contains("bill")).collect();
    names.sort();
    println!("tables: {names:?}");
    let ledger = read_ledger(std::path::Path::new(&path)).unwrap();
    let kinds: HashMap<i64, &str> = ledger.transactions.iter().chain(&ledger.unbalanced).map(|t| (t.id, t.kind)).collect();
    let mut pairs: BTreeMap<(i64, &str, &str), usize> = BTreeMap::new();
    for r in rows_of(&pages, &catalog, "abmc_transaction_link") {
        if r.get("is_t1_source_bool").as_int() != Some(1) {
            continue;
        }
        let k = |c: &str| r.get(c).as_int().and_then(|t| kinds.get(&t).copied()).unwrap_or("?");
        *pairs.entry((r.get("link_type").as_int().unwrap_or(-1), k("transaction_id"), k("transaction_2_id"))).or_default() += 1;
    }
    for ((t, a, b), n) in pairs {
        println!("link {t:>3}: {a} -> {b}: {n}");
    }
    // Sign of bill and card-charge totals: credits show up as the other sign on A/P or the card.
    let mut signs: BTreeMap<(&str, bool), usize> = BTreeMap::new();
    let types: HashMap<i64, &str> = ledger.accounts.iter().map(|a| (a.id, a.account_type)).collect();
    for t in &ledger.transactions {
        let side: i64 = t.lines.iter().filter(|l| matches!(types.get(&l.account_id), Some(&"ACCOUNTS_PAYABLE") | Some(&"CREDIT_CARD") | Some(&"BANK"))).map(|l| l.cents).sum();
        *signs.entry((t.kind, side > 0)).or_default() += 1;
    }
    println!("kind, pays out (credit to bank/card/AP): {signs:?}");
}

/// Prints the transfer, estimate and purchase-order tables: columns and row counts.
#[test]
#[ignore]
fn explores_transfers_estimates_and_orders() {
    use crate::qbw_company::{read_pages, rows_of};
    let path = std::env::var("LOCALBOOKS_QBW_CHECK").expect("set LOCALBOOKS_QBW_CHECK");
    let (pages, catalog, _, _) = read_pages(std::path::Path::new(&path)).unwrap();
    let mut names: Vec<&String> =
        catalog.names.values().filter(|n| n.starts_with("abmc_") && (n.contains("transfer") || n.contains("estimate") || n.contains("purchase_order") || n.contains("sales_order"))).collect();
    names.sort();
    for name in names {
        let id = catalog.table_id(name).unwrap();
        let cols: Vec<&str> = catalog.columns[&id].iter().map(|c| c.name.as_str()).collect();
        let rows = rows_of(&pages, &catalog, name);
        println!("{name} ({} rows): {}", rows.len(), cols.join(", "));
    }
}
