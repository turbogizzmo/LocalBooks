//! Renaming rules for downloaded payees (migration 0089), and the list of learned category rules.
//!
//! A renaming rule turns a bank's description into a payee name: when the description begins
//! with, contains or ends with the rule's text (case and repeated spaces aside), the line's payee is
//! the rule's name. The longest matching text wins, so a specific rule beats a general one. The
//! review screen offers a rule when the user changes a payee, and lists the rules to search, edit
//! and remove. Category rules (`bank_rules`) get a list too: their account can be changed, and a
//! rule removed.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::db_err;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct PayeeRule {
    #[serde(default)]
    pub id: i64,
    /// `BEGINS`, `CONTAINS` or `ENDS`.
    pub match_kind: String,
    pub pattern: String,
    pub payee_name: String,
}

fn squeeze(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").to_uppercase()
}

impl PayeeRule {
    /// Whether the rule applies to a bank description.
    pub fn applies_to(&self, description: &str) -> bool {
        let (text, pattern) = (squeeze(description), squeeze(&self.pattern));
        !pattern.is_empty()
            && match self.match_kind.as_str() {
                "BEGINS" => text.starts_with(&pattern),
                "ENDS" => text.ends_with(&pattern),
                _ => text.contains(&pattern),
            }
    }
}

/// The name the rules give a description: the matching rule with the longest text; between rules
/// of the same length, the oldest (lowest id).
pub fn rename(rules: &[PayeeRule], description: &str) -> Option<String> {
    rules
        .iter()
        .filter(|r| r.applies_to(description))
        .map(|r| (squeeze(&r.pattern).len(), r))
        .max_by(|(a_len, a), (b_len, b)| a_len.cmp(b_len).then_with(|| b.id.cmp(&a.id)))
        .map(|(_, r)| r.payee_name.clone())
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<PayeeRule>, String> {
    sqlx::query_as::<_, PayeeRule>(
        "SELECT id, match_kind, pattern, payee_name FROM bank_payee_rules ORDER BY payee_name COLLATE NOCASE, pattern COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)
}

fn checked(rule: &PayeeRule) -> Result<(String, String), String> {
    if !matches!(rule.match_kind.as_str(), "BEGINS" | "CONTAINS" | "ENDS") {
        return Err("BANK_RENAME_BAD_KIND: a renaming rule begins with, contains or ends with its text".into());
    }
    let pattern = rule.pattern.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = rule.payee_name.trim().to_string();
    if pattern.is_empty() || pattern.chars().count() > 100 {
        return Err("BANK_RENAME_BAD_TEXT: give the text the bank's description has, 1 to 100 characters".into());
    }
    if name.is_empty() || name.chars().count() > 100 {
        return Err("BANK_RENAME_BAD_NAME: give the name to use, 1 to 100 characters".into());
    }
    Ok((pattern, name))
}

fn duplicate(err: sqlx::Error) -> String {
    let msg = db_err(err);
    if msg.contains("UNIQUE") {
        "BANK_RENAME_DUPLICATE: there is already a rule for that text".into()
    } else {
        msg
    }
}

/// Saves a new rule (`id` 0) or changes one; returns it.
pub async fn save(pool: &SqlitePool, rule: PayeeRule) -> Result<PayeeRule, String> {
    let (pattern, name) = checked(&rule)?;
    let id: i64 = if rule.id == 0 {
        sqlx::query_scalar("INSERT INTO bank_payee_rules (match_kind, pattern, payee_name) VALUES (?1, ?2, ?3) RETURNING id")
            .bind(&rule.match_kind)
            .bind(&pattern)
            .bind(&name)
            .fetch_one(pool)
            .await
            .map_err(duplicate)?
    } else {
        sqlx::query_scalar(
            "UPDATE bank_payee_rules SET match_kind = ?1, pattern = ?2, payee_name = ?3, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?4 RETURNING id",
        )
        .bind(&rule.match_kind)
        .bind(&pattern)
        .bind(&name)
        .bind(rule.id)
        .fetch_optional(pool)
        .await
        .map_err(duplicate)?
        .ok_or("BANK_RENAME_NOT_FOUND: that rule no longer exists")?
    };
    Ok(PayeeRule { id, match_kind: rule.match_kind, pattern, payee_name: name })
}

pub async fn remove(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let done = sqlx::query("DELETE FROM bank_payee_rules WHERE id = ?1").bind(id).execute(pool).await.map_err(db_err)?;
    if done.rows_affected() != 1 {
        return Err("BANK_RENAME_NOT_FOUND: that rule no longer exists".into());
    }
    Ok(())
}

/// The text a renaming rule would start from for a description: its first distinctive words, as
/// the bank writes them ("AMZN MKTP US*2K4" → "AMZN MKTP").
#[cfg(test)]
pub fn suggested_pattern(description: &str) -> String {
    let words: Vec<&str> = description.split_whitespace().take_while(|w| w.chars().any(|c| c.is_ascii_alphabetic())).take(2).collect();
    match words.as_slice() {
        [] => description.split_whitespace().next().unwrap_or("").to_string(),
        _ => words.join(" "),
    }
}

/// One category rule, for the rules list.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct CategoryRule {
    pub id: i64,
    /// The words the rule looks for, as text ("BLUE BOTTLE"), from its stored pattern.
    #[sqlx(skip)]
    pub words: String,
    #[serde(skip)]
    pub pattern: String,
    /// `ANY`, `OUTFLOW` or `INFLOW`.
    pub direction: String,
    pub account_id: i64,
    /// `LEARNED` or `BUILT_IN`.
    pub origin: String,
    pub hit_count: i64,
}

/// A stored pattern as plain words: `\bBLUE[^A-Za-z&']+BOTTLE\b` → "BLUE BOTTLE".
fn words_of(pattern: &str) -> String {
    let text = pattern.replace(r"\b", "").replace("[^A-Za-z&']+", " ");
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.extend(chars.next()),
            '^' | '$' | '(' | ')' | '?' | '*' | '+' | '[' | ']' | '{' | '}' | '|' => out.push(' '),
            c => out.push(c),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub async fn list_category_rules(pool: &SqlitePool) -> Result<Vec<CategoryRule>, String> {
    let mut rules = sqlx::query_as::<_, CategoryRule>(
        "SELECT id, pattern, direction, account_id, origin, hit_count FROM bank_rules ORDER BY origin DESC, hit_count DESC, id",
    )
    .fetch_all(pool)
    .await
    .map_err(db_err)?;
    for r in &mut rules {
        r.words = words_of(&r.pattern);
    }
    Ok(rules)
}

/// Points a category rule at another account.
pub async fn set_category_rule_account(pool: &SqlitePool, id: i64, account_id: i64) -> Result<(), String> {
    let usable: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM chart_of_accounts WHERE id = ?1 AND is_active = 1 AND account_type NOT IN ('BANK', 'CREDIT_CARD'))")
            .bind(account_id)
            .fetch_one(pool)
            .await
            .map_err(db_err)?;
    if !usable {
        return Err("BANK_RULE_BAD_ACCOUNT: choose an active category account (not a bank or card)".into());
    }
    let done = sqlx::query("UPDATE bank_rules SET account_id = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?2")
        .bind(account_id)
        .bind(id)
        .execute(pool)
        .await
        .map_err(db_err)?;
    if done.rows_affected() != 1 {
        return Err("BANK_RULE_NOT_FOUND: that rule no longer exists".into());
    }
    Ok(())
}

pub async fn remove_category_rule(pool: &SqlitePool, id: i64) -> Result<(), String> {
    let done = sqlx::query("DELETE FROM bank_rules WHERE id = ?1").bind(id).execute(pool).await.map_err(db_err)?;
    if done.rows_affected() != 1 {
        return Err("BANK_RULE_NOT_FOUND: that rule no longer exists".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
