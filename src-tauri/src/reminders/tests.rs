use super::*;
use crate::db::test_pool;
use crate::invoices::{create_and_post_invoice, InvoiceInput, InvoiceLineInput};

fn d(m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, m, day).expect("valid date")
}

async fn account(pool: &SqlitePool, number: &str) -> i64 {
    sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = ?1").bind(number).fetch_one(pool).await.expect("seeded account")
}

async fn invoice(pool: &SqlitePool, customer: &str, issue: NaiveDate, due: NaiveDate, cents: i64) -> i64 {
    let services = account(pool, "4100").await;
    create_and_post_invoice(
        pool,
        InvoiceInput {
            customer_name: customer.into(),
            issue_date: issue,
            due_date: due,
            lines: vec![InvoiceLineInput { description: "Work".into(), quantity_e4: 10_000, rate_cents: cents, income_account_id: services, product_service_id: None, class_id: None, is_taxable: false }],
            currency: None,
            tax_rate_id: None,
            discount_cents: 0,
            details: Default::default(),
        },
    )
    .await
    .expect("invoice posts")
    .id
}

async fn bill(pool: &SqlitePool, vendor: &str, number: &str, due: NaiveDate, cents: i64) -> i64 {
    let office = account(pool, "6250").await;
    let input = serde_json::from_value(serde_json::json!({
        "vendorName": vendor, "billNumber": number, "billDate": "2026-01-01", "dueDate": due.to_string(), "memo": null,
        "totalCents": cents, "lines": [{ "accountId": office, "amountCents": cents, "memo": null }],
    }))
    .expect("bill input");
    crate::expenses::post_vendor_bill(pool, input).await.expect("bill posts").id
}

/// A user straight in the table (the password is never checked here).
async fn user(pool: &SqlitePool, username: &str, role: &str) -> i64 {
    sqlx::query_scalar("INSERT INTO users (username, display_name, role, can_see_sensitive, password_verifier) VALUES (?1, ?1, ?2, ?3, 'argon2id$test') RETURNING id")
        .bind(username)
        .bind(role)
        .bind(role == "ADMIN")
        .fetch_one(pool)
        .await
        .expect("user")
}

fn todo(title: &str, due: Option<NaiveDate>) -> TodoInput {
    TodoInput { title: title.into(), note: String::new(), due_date: due, customer_id: None, vendor_id: None, employee_id: None, link_kind: None, link_id: None, assigned_user_id: None }
}

const NOBODY: Viewer = Viewer { user_id: None, role: None };

#[tokio::test]
async fn overdue_invoices_are_open_and_past_due_by_more_than_the_days() {
    let pool = test_pool().await;
    let late = invoice(&pool, "Acme", d(1, 1), d(1, 31), 50_000).await;
    invoice(&pool, "Initech", d(2, 1), d(3, 3), 20_000).await; // due later
    let paid = invoice(&pool, "Globex", d(1, 1), d(1, 15), 10_000).await;
    let checking = account(&pool, "1010").await;
    crate::invoices::record_payment(
        &pool,
        serde_json::from_value(serde_json::json!({
            "invoiceId": paid, "depositAccountId": checking, "paymentDate": "2026-01-20", "amountCents": 10_000, "paymentMethod": "CHECK", "reference": null,
        }))
        .unwrap(),
    )
    .await
    .unwrap();

    let items = overdue_invoices(&pool, d(3, 2), 0).await.unwrap();
    assert_eq!(items.len(), 1, "paid and not-yet-due invoices stay out");
    assert_eq!((items[0].id, items[0].amount_cents, items[0].detail.as_str()), (Some(late), Some(50_000), "30 days overdue"));
    assert!(items[0].label.contains("Acme"));
    assert!(items[0].entry_id.is_some());
    assert!(overdue_invoices(&pool, d(3, 2), 30).await.unwrap().is_empty(), "a 30-day grace hides it");
    assert_eq!(overdue_invoices(&pool, d(3, 2), 29).await.unwrap().len(), 1);
}

#[tokio::test]
async fn bills_due_look_ahead_and_include_overdue_ones() {
    let pool = test_pool().await;
    let overdue = bill(&pool, "Staples", "S-1", d(2, 20), 3_000).await;
    let soon = bill(&pool, "Landlord", "", d(3, 10), 120_000).await;
    bill(&pool, "Power Co", "P-9", d(3, 20), 9_000).await;

    let items = bills_due(&pool, d(3, 1), 10).await.unwrap();
    assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![Some(overdue), Some(soon)]);
    assert_eq!(items[0].detail, "9 days overdue");
    assert_eq!((items[1].label.as_str(), items[1].detail.as_str()), ("Bill · Landlord", "Due in 9 days"));
    assert_eq!(bills_due(&pool, d(3, 1), 30).await.unwrap().len(), 3);
    assert_eq!(bills_due(&pool, d(3, 10), 0).await.unwrap()[1].detail, "Due today");
}

#[tokio::test]
async fn todos_due_skip_done_undated_and_other_users_ones() {
    let pool = test_pool().await;
    let (ann, bob) = (user(&pool, "ann", "STANDARD").await, user(&pool, "bob", "VIEW_ONLY").await);
    let mine = save_todo(&pool, None, &TodoInput { assigned_user_id: Some(ann), ..todo("Call Acme", Some(d(3, 1))) }, NOBODY).await.unwrap();
    let open = save_todo(&pool, None, &todo("File receipts", Some(d(3, 4))), NOBODY).await.unwrap();
    save_todo(&pool, None, &TodoInput { assigned_user_id: Some(bob), ..todo("Bob's", Some(d(3, 1))) }, NOBODY).await.unwrap();
    save_todo(&pool, None, &todo("Someday", None), NOBODY).await.unwrap();
    let done = save_todo(&pool, None, &todo("Done already", Some(d(2, 1))), NOBODY).await.unwrap();
    set_done(&pool, done.id, true, NOBODY).await.unwrap();

    let ann_view = Viewer { user_id: Some(ann), role: Some(Role::Standard) };
    let ids = |items: Vec<ReminderItem>| items.into_iter().map(|i| i.id.unwrap()).collect::<Vec<_>>();
    assert_eq!(ids(todos_due(&pool, d(3, 2), 0, ann_view).await.unwrap()), vec![mine.id]);
    assert_eq!(ids(todos_due(&pool, d(3, 2), 7, ann_view).await.unwrap()), vec![mine.id, open.id], "unassigned ones show to everyone");
    assert_eq!(todos_due(&pool, d(3, 2), 7, NOBODY).await.unwrap().len(), 3, "without users, every open to-do");
    assert_eq!(todos_due(&pool, d(3, 2), 0, ann_view).await.unwrap()[0].detail, "1 day overdue");
}

#[tokio::test]
async fn recurring_due_follows_next_date_activity_and_end_date() {
    let pool = test_pool().await;
    let (checking, rent) = (account(&pool, "1010").await, account(&pool, "6300").await);
    let check = serde_json::json!({ "kind": "CHECK", "payeeName": "Landlord", "paymentAccountId": checking, "expenseDate": "2026-01-01",
        "checkNumber": null, "paymentMethod": null, "memo": null, "lines": [{ "accountId": rent, "amountCents": 120_000, "memo": null }] });
    let template = |name: &str, next: NaiveDate, end: Option<NaiveDate>| crate::workflows::RecurringInput {
        name: name.into(), kind: "EXPENSE".into(), payload: check.clone(), frequency: "MONTHLY".into(), every: 1, next_date: next, end_date: end,
    };
    let rent_id = crate::workflows::save_recurring(&pool, None, template("Rent", d(3, 1), None)).await.unwrap();
    crate::workflows::save_recurring(&pool, None, template("Later", d(3, 15), None)).await.unwrap();
    let paused = crate::workflows::save_recurring(&pool, None, template("Paused", d(2, 1), None)).await.unwrap();
    crate::workflows::set_recurring_active(&pool, paused, false).await.unwrap();

    let items = recurring_due(&pool, d(3, 2), 0).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!((items[0].id, items[0].amount_cents, items[0].detail.as_str()), (Some(rent_id), Some(120_000), "Expense · Landlord"));
    assert_eq!(recurring_due(&pool, d(3, 2), 14).await.unwrap().len(), 2);
}

#[tokio::test]
async fn email_queue_lists_forms_marked_email_later() {
    let pool = test_pool().await;
    let a = invoice(&pool, "Acme", d(1, 1), d(1, 31), 5_000).await;
    invoice(&pool, "Initech", d(1, 1), d(1, 31), 7_000).await;
    assert!(email_queue(&pool).await.unwrap().is_empty());
    crate::email_log::set_queued(&pool, crate::forms::FormKind::Invoice, a, true).await.unwrap();
    let items = email_queue(&pool).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!((items[0].id, items[0].amount_cents, items[0].detail.as_str()), (Some(a), Some(5_000), "No email address on file"));
    assert!(items[0].label.starts_with("Invoice ") && items[0].label.ends_with("Acme"));
}

#[tokio::test]
async fn tax_dates_fall_inside_the_look_ahead() {
    let pool = test_pool().await;
    // Schedule C, calendar year: the 2025 return and Q1 estimate are both due Wednesday April 15, 2026.
    let items = tax_dates(&pool, d(4, 1), 30).await.unwrap();
    assert!(!items.is_empty());
    assert!(items.iter().all(|i| i.date.as_deref() == Some("2026-04-15")), "{items:?}");
    assert!(items[0].detail.ends_with("due in 14 days"));
    assert!(tax_dates(&pool, d(4, 1), 10).await.unwrap().is_empty());
    assert!(tax_dates(&pool, d(4, 16), 0).await.unwrap().is_empty(), "past dates drop off");
}

#[test]
fn a_backup_is_due_when_the_newest_is_too_old_or_missing() {
    assert!(backup_due(Some(d(3, 1)), d(3, 8), 7).is_empty());
    let due = backup_due(Some(d(3, 1)), d(3, 9), 7);
    assert_eq!((due.len(), due[0].detail.as_str()), (1, "The last backup is 8 days old"));
    assert_eq!(backup_due(None, d(3, 9), 7)[0].detail, "No backup of this company yet");
}

#[tokio::test]
async fn settings_are_per_user_validated_and_choose_the_categories() {
    let pool = test_pool().await;
    let ann = user(&pool, "ann", "ADMIN").await;
    let ann_view = Viewer { user_id: Some(ann), role: Some(Role::Admin) };
    assert_eq!(settings(&pool, ann_view).await.unwrap(), ReminderSettings::default());
    assert!(!ReminderSettings::default().show_at_startup, "off unless chosen");

    invoice(&pool, "Acme", d(1, 1), d(1, 31), 5_000).await;
    bill(&pool, "Staples", "S-1", d(3, 5), 3_000).await;
    let keys = |groups: Vec<ReminderGroup>| groups.into_iter().map(|g| g.key).collect::<Vec<_>>();
    assert_eq!(keys(reminders(&pool, d(3, 1), ann_view, None).await.unwrap()), vec!["OVERDUE_INVOICES", "BILLS_DUE", "BACKUP"], "April 15 is past the 30-day look-ahead");

    let mine = ReminderSettings { overdue_invoices: Category { show: false, days: 0 }, bills_due: Category { show: true, days: 2 }, tax_dates: Category { show: true, days: 60 }, backup: Category { show: true, days: 30 }, ..Default::default() };
    save_settings(&pool, ann_view, &mine).await.unwrap();
    assert_eq!(settings(&pool, ann_view).await.unwrap(), mine);
    assert_eq!(settings(&pool, NOBODY).await.unwrap(), ReminderSettings::default(), "settings are the user's own");
    let groups = reminders(&pool, d(3, 1), ann_view, Some(d(2, 20))).await.unwrap();
    assert_eq!(keys(groups.clone()), vec!["TAX_DATES"], "bills more than 2 days out and a recent backup stay out");

    let bad = ReminderSettings { bills_due: Category { show: true, days: 400 }, ..Default::default() };
    assert!(save_settings(&pool, ann_view, &bad).await.unwrap_err().starts_with("REMINDER_BAD_DAYS"));
    // Older or unknown fields fall back to the defaults rather than failing.
    let partial: ReminderSettings = serde_json::from_value(serde_json::json!({ "showAtStartup": true })).unwrap();
    assert_eq!(partial, ReminderSettings { show_at_startup: true, ..Default::default() });
}

#[tokio::test]
async fn todos_save_link_and_validate() {
    let pool = test_pool().await;
    let inv = invoice(&pool, "Acme", d(1, 1), d(1, 31), 5_000).await;
    let acme: i64 = sqlx::query_scalar("SELECT id FROM customers WHERE name = 'Acme'").fetch_one(&pool).await.unwrap();
    let saved = save_todo(&pool, None, &TodoInput { customer_id: Some(acme), link_kind: Some("INVOICE".into()), link_id: Some(inv), note: " Ask about PO ".into(), ..todo(" Chase payment ", Some(d(2, 1))) }, NOBODY)
        .await
        .unwrap();
    assert_eq!((saved.title.as_str(), saved.note.as_str(), saved.customer_name.as_deref()), ("Chase payment", "Ask about PO", Some("Acme")));
    assert!(saved.link_label.as_deref().unwrap().starts_with("Invoice "));
    assert!(saved.link_entry_id.is_some());
    assert_eq!(link_options(&pool, Some(acme), None).await.unwrap()[0].id, inv);

    let edited = save_todo(&pool, Some(saved.id), &todo("Chase payment again", None), NOBODY).await.unwrap();
    assert_eq!((edited.due_date, edited.customer_id, edited.link_kind), (None, None, None));

    let err = |r: Result<TodoView, String>| r.unwrap_err().split(':').next().unwrap().to_string();
    assert_eq!(err(save_todo(&pool, None, &todo("  ", None), NOBODY).await), "TODO_TITLE_REQUIRED");
    assert_eq!(err(save_todo(&pool, None, &TodoInput { customer_id: Some(acme), vendor_id: Some(acme), ..todo("x", None) }, NOBODY).await), "TODO_ONE_NAME");
    assert_eq!(err(save_todo(&pool, None, &TodoInput { link_kind: Some("BILL".into()), link_id: Some(999), ..todo("x", None) }, NOBODY).await), "TODO_BAD_LINK");
    assert_eq!(err(save_todo(&pool, None, &TodoInput { assigned_user_id: Some(42), ..todo("x", None) }, NOBODY).await), "TODO_BAD_ASSIGNEE");
    assert_eq!(err(save_todo(&pool, Some(999), &todo("x", None), NOBODY).await), "TODO_NOT_FOUND");

    delete_todo(&pool, saved.id).await.unwrap();
    assert!(list_todos(&pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn view_only_users_complete_only_their_own_todos() {
    let pool = test_pool().await;
    let (viewer, other) = (user(&pool, "vic", "VIEW_ONLY").await, user(&pool, "sam", "STANDARD").await);
    let vic = Viewer { user_id: Some(viewer), role: Some(Role::ViewOnly) };
    let sam = Viewer { user_id: Some(other), role: Some(Role::Standard) };
    let theirs = save_todo(&pool, None, &TodoInput { assigned_user_id: Some(other), ..todo("Sam's", None) }, sam).await.unwrap();
    let unassigned = save_todo(&pool, None, &todo("Anyone's", None), sam).await.unwrap();
    let own = save_todo(&pool, None, &TodoInput { assigned_user_id: Some(viewer), ..todo("Vic's", None) }, sam).await.unwrap();

    assert!(set_done(&pool, theirs.id, true, vic).await.unwrap_err().starts_with("ROLE_VIEW_ONLY"));
    assert!(set_done(&pool, unassigned.id, true, vic).await.unwrap_err().starts_with("ROLE_VIEW_ONLY"));
    let done = set_done(&pool, own.id, true, vic).await.unwrap();
    assert!(done.is_done && done.done_at.is_some());
    let reopened = set_done(&pool, own.id, false, vic).await.unwrap();
    assert!(!reopened.is_done && reopened.done_at.is_none());
    assert!(set_done(&pool, theirs.id, true, Viewer { user_id: Some(viewer), role: Some(Role::Standard) }).await.unwrap().is_done, "a Standard user completes anyone's");
}
