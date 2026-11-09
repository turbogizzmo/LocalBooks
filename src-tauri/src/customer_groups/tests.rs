use super::*;
use crate::db::test_pool;

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).unwrap()
}

fn facts(f: impl FnOnce(&mut CustomerFacts)) -> CustomerFacts {
    let mut c = CustomerFacts { id: 1, name: "Acme".into(), is_active: true, ..Default::default() };
    f(&mut c);
    c
}

#[test]
fn state_and_zip_come_from_the_last_line_of_the_address() {
    assert_eq!(state_and_zip(Some("Acme\n12 Elm St\nSpringfield, OR 97477-1234")), (Some("OR".into()), Some("97477-1234".into())));
    assert_eq!(state_and_zip(Some("Portland or 97201")), (Some("OR".into()), Some("97201".into())));
    assert_eq!(state_and_zip(Some("London")), (None, None));
    assert_eq!(state_and_zip(None), (None, None));
}

#[test]
fn each_rule_checks_one_detail() {
    let c = facts(|c| {
        c.balance_cents = 50_000;
        c.customer_type_id = Some(3);
        c.terms_id = Some(4);
        c.sales_rep_id = Some(5);
        c.billing_address = Some("1 Main St\nSalem, OR 97301".into());
        c.tax_rate_id = Some(6);
        c.credit_limit_cents = Some(40_000);
    });
    assert!(meets(&Rule::Balance { cents: 50_000, at_most: false }, &c));
    assert!(!meets(&Rule::Balance { cents: 49_999, at_most: true }, &c));
    assert!(meets(&Rule::CustomerType { id: 3 }, &c) && !meets(&Rule::CustomerType { id: 9 }, &c));
    assert!(meets(&Rule::Terms { id: 4 }, &c) && meets(&Rule::Rep { id: 5 }, &c));
    assert!(meets(&Rule::State { code: "or".into() }, &c) && !meets(&Rule::State { code: "WA".into() }, &c));
    assert!(meets(&Rule::Zip { prefix: "973".into() }, &c) && !meets(&Rule::Zip { prefix: "972".into() }, &c));
    assert!(meets(&Rule::TaxCode { id: Some(6) }, &c) && !meets(&Rule::TaxCode { id: None }, &c));
    assert!(meets(&Rule::CreditLimit { over: true }, &c) && meets(&Rule::CreditLimit { over: false }, &c));
    assert!(meets(&Rule::Status { active: true }, &c));
    assert!(meets_all(&[Rule::State { code: "OR".into() }, Rule::Rep { id: 5 }], &c));
    assert!(!meets_all(&[Rule::State { code: "OR".into() }, Rule::Rep { id: 7 }], &c));
    assert!(!meets_all(&[], &c), "a group needs a rule");
    let exempt = facts(|c| c.is_tax_exempt = true);
    assert!(meets(&Rule::TaxCode { id: None }, &exempt));
}

#[test]
fn rules_read_and_write_as_tagged_json() {
    let rules: Vec<Rule> = serde_json::from_str(r#"[{"field":"balance","cents":10000},{"field":"state","code":"OR"},{"field":"creditLimit","over":true}]"#).unwrap();
    assert_eq!(rules, vec![Rule::Balance { cents: 10_000, at_most: false }, Rule::State { code: "OR".into() }, Rule::CreditLimit { over: true }]);
    assert!(serde_json::to_string(&Rule::Balance { cents: 1, at_most: true }).unwrap().contains(r#""atMost":true"#));
}

#[test]
fn a_statement_reminder_is_due_on_its_schedule() {
    let never = StatementReminder { every_days: Some(30), ..Default::default() };
    assert_eq!(never.next_due(d(5, 1)), Some(d(5, 1)));
    let sent = StatementReminder { every_days: Some(30), last_sent: Some(d(5, 1)), ..Default::default() };
    assert_eq!(sent.next_due(d(5, 10)), Some(d(5, 31)));
    assert_eq!(StatementReminder::default().next_due(d(5, 1)), None);
}

async fn customer(pool: &SqlitePool, name: &str, address: &str, rep: Option<i64>) -> i64 {
    sqlx::query_scalar("INSERT INTO customers (name, billing_address, sales_rep_id) VALUES (?1, ?2, ?3) RETURNING id").bind(name).bind(address).bind(rep).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn automatic_groups_follow_the_customers_and_manual_groups_keep_their_list() {
    let pool = test_pool().await;
    let a = customer(&pool, "Alder Cafe", "9 Pine St\nSalem, OR 97301", None).await;
    let b = customer(&pool, "Birch Books", "4 Oak Ave\nSeattle, WA 98101", None).await;
    let c = customer(&pool, "Cedar Clinic", "7 Fir Rd\nPortland, OR 97201", None).await;

    let oregon = save(&pool, None, &CustomerGroupInput { name: " Oregon ".into(), rules: vec![Rule::State { code: "OR".into() }], member_ids: vec![b], statement: StatementReminder::default() }).await.unwrap();
    assert!(oregon.automatic);
    assert_eq!(oregon.name, "Oregon");
    assert_eq!(oregon.member_ids, vec![a, c], "the rule decides; a member list is ignored");

    // A customer moves: the automatic group follows.
    sqlx::query("UPDATE customers SET billing_address = '4 Oak Ave\nEugene, OR 97401' WHERE id = ?1").bind(b).execute(&pool).await.unwrap();
    assert_eq!(get(&pool, oregon.id).await.unwrap().member_ids, vec![a, b, c]);

    // An open balance rule follows the books too.
    let services: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '4100'").fetch_one(&pool).await.unwrap();
    let invoice: crate::invoices::InvoiceInput = serde_json::from_value(serde_json::json!({ "customerName": "Cedar Clinic", "issueDate": "2026-05-01", "dueDate": "2026-05-31",
        "lines": [{ "description": "Work", "quantityE4": 10_000, "rateCents": 75_000, "incomeAccountId": services }] })).unwrap();
    crate::invoices::create_and_post_invoice(&pool, invoice).await.unwrap();
    let owing = save(&pool, None, &CustomerGroupInput { name: "Owe 500+".into(), rules: vec![Rule::Balance { cents: 50_000, at_most: false }], member_ids: vec![], statement: StatementReminder::default() }).await.unwrap();
    assert_eq!(owing.member_ids, vec![c]);

    // A manual group keeps exactly who was picked (repeats and unknown ids dropped).
    let picked = save(&pool, None, &CustomerGroupInput { name: "Picked".into(), rules: vec![], member_ids: vec![b, a, b, 9_999], statement: StatementReminder::default() }).await.unwrap();
    assert!(!picked.automatic);
    assert_eq!(picked.member_ids, vec![a, b]);
    let edited = save(&pool, Some(picked.id), &CustomerGroupInput { name: "Picked".into(), rules: vec![], member_ids: vec![c], statement: StatementReminder::default() }).await.unwrap();
    assert_eq!(edited.member_ids, vec![c]);

    assert!(save(&pool, None, &CustomerGroupInput { name: "oregon".into(), rules: vec![], member_ids: vec![], statement: StatementReminder::default() }).await.unwrap_err().starts_with("GROUP_DUPLICATE"));
    for bad in [Rule::State { code: "Oregon".into() }, Rule::State { code: "O1".into() }, Rule::Zip { prefix: "".into() }, Rule::Zip { prefix: "A1".into() }] {
        assert!(save(&pool, None, &CustomerGroupInput { name: "Bad".into(), rules: vec![bad], member_ids: vec![], statement: StatementReminder::default() }).await.unwrap_err().starts_with("GROUP_BAD_RULE"));
    }
    assert!(save(&pool, None, &CustomerGroupInput { name: "Zip ok".into(), rules: vec![Rule::Zip { prefix: "97477-".into() }], member_ids: vec![], statement: StatementReminder::default() }).await.is_ok());
    assert_eq!(list(&pool).await.unwrap().iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), ["Oregon", "Owe 500+", "Picked", "Zip ok"]);
    delete(&pool, picked.id).await.unwrap();
    assert!(get(&pool, picked.id).await.unwrap_err().starts_with("GROUP_NOT_FOUND"));
}

#[tokio::test]
async fn statement_reminders_come_due_and_wait_after_they_are_sent() {
    let pool = test_pool().await;
    let a = customer(&pool, "Alder Cafe", "Salem, OR 97301", None).await;
    let monthly = StatementReminder { every_days: Some(30), open_only: true, message: Some("Your statement is attached.".into()), ..Default::default() };
    let g = save(&pool, None, &CustomerGroupInput { name: "Monthly".into(), rules: vec![], member_ids: vec![a], statement: monthly }).await.unwrap();
    let _empty = save(&pool, None, &CustomerGroupInput { name: "Nobody".into(), rules: vec![], member_ids: vec![], statement: StatementReminder { every_days: Some(7), ..Default::default() } }).await.unwrap();
    let due = statements_due(&pool, d(5, 1)).await.unwrap();
    assert_eq!(due.iter().map(|(g, day)| (g.name.as_str(), *day)).collect::<Vec<_>>(), [("Monthly", d(5, 1))], "a group with no members never comes up");
    let sent = mark_statements_sent(&pool, g.id, d(5, 1)).await.unwrap();
    assert_eq!(sent.statement.last_sent, Some(d(5, 1)));
    assert!(sent.statement.open_only);
    assert!(statements_due(&pool, d(5, 30)).await.unwrap().is_empty());
    assert_eq!(statements_due(&pool, d(5, 31)).await.unwrap().len(), 1);
}
