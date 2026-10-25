//! Chart of accounts maintenance: creating, editing and inactivating accounts.
//!
//! The database enforces the structural rules (subaccounts share their parent's type and nest at
//! most five levels, system accounts cannot be retyped, nested or inactivated, an account with
//! posted history keeps its type) and records every identity change in the ledger audit trail.
//! These functions add the checks that need a friendlier message.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::ledger::db_err;
use crate::reports::AccountView;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct AccountTypeView {
    pub code: String,
    pub display_name: String,
    pub classification: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInput {
    pub name: String,
    #[serde(default)]
    pub account_number: Option<String>,
    pub account_type: String,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub description: Option<String>,
}

pub async fn list_account_types(pool: &SqlitePool) -> Result<Vec<AccountTypeView>, String> {
    sqlx::query_as("SELECT code, display_name, classification FROM account_types ORDER BY sort_order")
        .fetch_all(pool)
        .await
        .map_err(db_err)
}

fn clean(text: Option<&str>) -> Option<&str> {
    text.map(str::trim).filter(|t| !t.is_empty())
}

fn validate(input: &AccountInput) -> Result<&str, String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err("COA_NAME_REQUIRED: enter an account name".into());
    }
    if name.contains(':') {
        return Err("COA_NAME_COLON: an account name cannot contain ':' (it separates an account from its subaccount)".into());
    }
    Ok(name)
}

/// Turns unique-index violations (number, or name among siblings) into readable messages.
fn unique_err(err: sqlx::Error, input: &AccountInput) -> String {
    let msg = db_err(err);
    if msg.contains("UNIQUE") && msg.contains("account_number") {
        format!("COA_DUPLICATE_NUMBER: account number {} is already in use", clean(input.account_number.as_deref()).unwrap_or_default())
    } else if msg.contains("UNIQUE") {
        format!("COA_DUPLICATE_NAME: there is already an account named '{}' here", input.name.trim())
    } else {
        msg
    }
}

async fn get_account(pool: &SqlitePool, id: i64) -> Result<AccountView, String> {
    crate::reports::list_accounts(pool).await?.into_iter().find(|a| a.id == id).ok_or_else(|| format!("COA_NOT_FOUND: account {id} does not exist"))
}

pub async fn create_account(pool: &SqlitePool, input: AccountInput) -> Result<AccountView, String> {
    let name = validate(&input)?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO chart_of_accounts (account_number, name, account_type, parent_id, description)
         VALUES (?1, ?2, ?3, ?4, ?5) RETURNING id",
    )
    .bind(clean(input.account_number.as_deref()))
    .bind(name)
    .bind(&input.account_type)
    .bind(input.parent_id)
    .bind(clean(input.description.as_deref()))
    .fetch_one(pool)
    .await
    .map_err(|e| unique_err(e, &input))?;
    get_account(pool, id).await
}

pub async fn update_account(pool: &SqlitePool, id: i64, input: AccountInput) -> Result<AccountView, String> {
    let name = validate(&input)?;
    let updated = sqlx::query(
        "UPDATE chart_of_accounts
         SET account_number = ?1, name = ?2, account_type = ?3, parent_id = ?4, description = ?5
         WHERE id = ?6",
    )
    .bind(clean(input.account_number.as_deref()))
    .bind(name)
    .bind(&input.account_type)
    .bind(input.parent_id)
    .bind(clean(input.description.as_deref()))
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| unique_err(e, &input))?;
    if updated.rows_affected() == 0 {
        return Err(format!("COA_NOT_FOUND: account {id} does not exist"));
    }
    get_account(pool, id).await
}

/// Makes an account inactive (hidden from pickers; nothing new can post to it) or active again.
/// An account with active subaccounts stays active until they are inactivated, and a subaccount
/// cannot be reactivated beneath an inactive parent.
pub async fn set_account_active(pool: &SqlitePool, id: i64, active: bool) -> Result<AccountView, String> {
    let account = get_account(pool, id).await?;
    if active {
        let parent_inactive: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM chart_of_accounts a JOIN chart_of_accounts p ON p.id = a.parent_id WHERE a.id = ?1 AND p.is_active = 0)",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .map_err(db_err)?;
        if parent_inactive {
            return Err(format!("COA_PARENT_INACTIVE: make the parent of {} active first", account.full_name));
        }
    } else {
        let active_children: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM chart_of_accounts WHERE parent_id = ?1 AND is_active = 1)")
            .bind(id)
            .fetch_one(pool)
            .await
            .map_err(db_err)?;
        if active_children {
            return Err(format!("COA_HAS_ACTIVE_SUBACCOUNTS: make the subaccounts of {} inactive first", account.full_name));
        }
    }
    sqlx::query("UPDATE chart_of_accounts SET is_active = ?1 WHERE id = ?2").bind(active).bind(id).execute(pool).await.map_err(db_err)?;
    get_account(pool, id).await
}

#[cfg(test)]
mod tests;
