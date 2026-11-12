use super::*;
use crate::bank::suggest::Suggester;
use crate::db::test_pool;

fn rule(kind: &str, pattern: &str, name: &str) -> PayeeRule {
    PayeeRule { id: 0, match_kind: kind.into(), pattern: pattern.into(), payee_name: name.into() }
}

#[test]
fn rules_match_the_start_middle_or_end_and_the_longest_wins() {
    let rules = vec![
        PayeeRule { id: 1, ..rule("BEGINS", "amzn  mktp", "Amazon") },
        PayeeRule { id: 2, ..rule("CONTAINS", "AMZN MKTP US", "Amazon US") },
        PayeeRule { id: 3, ..rule("ENDS", "oakland", "Blue Bottle") },
    ];
    assert_eq!(rename(&rules, "AMZN MKTP DE*7Q1").as_deref(), Some("Amazon"));
    assert_eq!(rename(&rules, "AMZN  MKTP US*2K4").as_deref(), Some("Amazon US"), "the longer text wins");
    assert_eq!(rename(&rules, "SQ *BLUE BOTTLE OAKLAND").as_deref(), Some("Blue Bottle"));
    assert_eq!(rename(&rules, "OAKLAND PARKING SQ"), None, "ENDS looks at the end only");
    assert_eq!(suggested_pattern("AMZN MKTP US*2K4 SEATTLE"), "AMZN MKTP");
    assert_eq!(suggested_pattern("1234 5678"), "1234");
}

#[tokio::test]
async fn a_renaming_rule_names_later_downloads_and_the_list_edits_and_removes_it() {
    let pool = test_pool().await;
    let checking: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '1010'").fetch_one(&pool).await.unwrap();
    let saved = save(&pool, rule("BEGINS", " AMZN   MKTP ", " Amazon ")).await.unwrap();
    assert_eq!((saved.pattern.as_str(), saved.payee_name.as_str()), ("AMZN MKTP", "Amazon"));
    assert!(save(&pool, rule("BEGINS", "amzn mktp", "Other")).await.unwrap_err().starts_with("BANK_RENAME_DUPLICATE"));
    assert!(save(&pool, rule("SOUNDS_LIKE", "x", "y")).await.unwrap_err().starts_with("BANK_RENAME_BAD_KIND"));

    let mut tx = pool.begin().await.unwrap();
    crate::bank::insert_line(&mut tx, checking, "2026-03-04".parse().unwrap(), "AMZN MKTP US*2K4", -2_599, "a").await.unwrap().unwrap();
    crate::bank::insert_line(&mut tx, checking, "2026-03-04".parse().unwrap(), "AMZN MKTP REFUND", 1_000, "b").await.unwrap().unwrap();
    tx.commit().await.unwrap();
    let lines = crate::bank::list_pending(&pool, &Suggester::default(), "test").await.unwrap();
    let out = lines.iter().find(|l| l.amount_cents < 0).unwrap();
    assert_eq!((out.payee.as_deref(), out.payee_origin), (Some("Amazon"), Some("RULE")));
    assert_eq!(lines.iter().find(|l| l.amount_cents > 0).unwrap().payee, None, "money in has no payee");

    save(&pool, PayeeRule { id: saved.id, ..rule("CONTAINS", "MKTP", "Amazon Marketplace") }).await.unwrap();
    let lines = crate::bank::list_pending(&pool, &Suggester::default(), "test").await.unwrap();
    assert_eq!(lines.iter().find(|l| l.amount_cents < 0).unwrap().payee.as_deref(), Some("Amazon Marketplace"));
    remove(&pool, saved.id).await.unwrap();
    assert!(list(&pool).await.unwrap().is_empty());
    assert!(remove(&pool, saved.id).await.unwrap_err().starts_with("BANK_RENAME_NOT_FOUND"));
}

#[tokio::test]
async fn category_rules_are_listed_as_words_and_can_be_repointed_or_removed() {
    let pool = test_pool().await;
    let (rent, checking): (i64, i64) = (
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '6300'").fetch_one(&pool).await.unwrap(),
        sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '1010'").fetch_one(&pool).await.unwrap(),
    );
    let pattern = crate::bank::learned_pattern("SQ *BLUE BOTTLE #123").unwrap();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO bank_rules (pattern, direction, account_id, origin, priority) VALUES (?1, 'OUTFLOW', ?2, 'LEARNED', 1000) RETURNING id",
    )
    .bind(&pattern)
    .bind(rent)
    .fetch_one(&pool)
    .await
    .unwrap();
    let listed = list_category_rules(&pool).await.unwrap();
    assert_eq!(listed.iter().find(|r| r.id == id).unwrap().words, "BLUE BOTTLE");
    assert!(set_category_rule_account(&pool, id, checking).await.unwrap_err().starts_with("BANK_RULE_BAD_ACCOUNT"));
    let supplies: i64 = sqlx::query_scalar("SELECT id FROM chart_of_accounts WHERE account_number = '6100'").fetch_one(&pool).await.unwrap();
    set_category_rule_account(&pool, id, supplies).await.unwrap();
    remove_category_rule(&pool, id).await.unwrap();
    assert!(list_category_rules(&pool).await.unwrap().iter().all(|r| r.id != id));
}
