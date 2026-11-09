//! Bank feed rules: renaming rules for downloaded payees, and the learned category rules' list.

use super::prelude::*;
use crate::bank::payee_rules::{self, CategoryRule, PayeeRule};

handlers![
    list_bank_payee_rules,
    save_bank_payee_rule,
    remove_bank_payee_rule,
    list_bank_category_rules,
    set_bank_category_rule_account,
    remove_bank_category_rule,
];

#[tauri::command]
pub async fn list_bank_payee_rules(db: State<'_, Db>) -> Result<Vec<PayeeRule>, String> {
    payee_rules::list(&db.pool().await).await
}

/// Saves a new renaming rule (id 0) or changes one.
#[tauri::command]
pub async fn save_bank_payee_rule(app: AppHandle, db: State<'_, Db>, rule: PayeeRule) -> Result<PayeeRule, String> {
    let saved = payee_rules::save(&db.pool().await, rule).await?;
    notify(&app);
    Ok(saved)
}

#[tauri::command]
pub async fn remove_bank_payee_rule(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    payee_rules::remove(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn list_bank_category_rules(db: State<'_, Db>) -> Result<Vec<CategoryRule>, String> {
    payee_rules::list_category_rules(&db.pool().await).await
}

#[tauri::command]
pub async fn set_bank_category_rule_account(app: AppHandle, db: State<'_, Db>, id: i64, account_id: i64) -> Result<(), String> {
    payee_rules::set_category_rule_account(&db.pool().await, id, account_id).await?;
    notify(&app);
    Ok(())
}

#[tauri::command]
pub async fn remove_bank_category_rule(app: AppHandle, db: State<'_, Db>, id: i64) -> Result<(), String> {
    payee_rules::remove_category_rule(&db.pool().await, id).await?;
    notify(&app);
    Ok(())
}
