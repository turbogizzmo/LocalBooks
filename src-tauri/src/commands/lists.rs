//! Registers and list records (customers, vendors, products).

use super::prelude::*;

handlers![
    get_account_register,
    get_party,
    update_party,
    set_party_active,
    update_product,
    set_product_active,
];

#[tauri::command]
pub async fn get_account_register(
    db: State<'_, Db>,
    account_id: i64,
    start_date: Option<NaiveDate>,
    end_date: Option<NaiveDate>,
    include_subaccounts: Option<bool>,
) -> Result<Register, String> {
    registers::account_register(&db.pool().await, account_id, start_date, end_date, include_subaccounts.unwrap_or(false)).await
}

#[tauri::command]
pub async fn get_party(db: State<'_, Db>, kind: String, id: i64) -> Result<PartyDetails, String> {
    let (pool, session) = signed_in(&db).await?;
    parties::get_party(&pool, &kind, id, sensitive_access(&pool, &session).await?).await
}

#[tauri::command]
pub async fn update_party(app: AppHandle, db: State<'_, Db>, kind: String, id: i64, input: PartyDetailsInput) -> Result<PartyDetails, String> {
    let (pool, session) = signed_in(&db).await?;
    let party = parties::update_party(&pool, &kind, id, input, sensitive_access(&pool, &session).await?).await?;
    notify(&app);
    Ok(party)
}

#[tauri::command]
pub async fn set_party_active(app: AppHandle, db: State<'_, Db>, kind: String, id: i64, active: bool) -> Result<PartyDetails, String> {
    let (pool, session) = signed_in(&db).await?;
    let party = parties::set_party_active(&pool, &kind, id, active, sensitive_access(&pool, &session).await?).await?;
    notify(&app);
    Ok(party)
}

#[tauri::command]
pub async fn update_product(app: AppHandle, db: State<'_, Db>, id: i64, input: ProductInput) -> Result<ProductView, String> {
    let product = parties::update_product(&db.pool().await, id, input).await?;
    notify(&app);
    Ok(product)
}

#[tauri::command]
pub async fn set_product_active(app: AppHandle, db: State<'_, Db>, id: i64, active: bool) -> Result<ProductView, String> {
    let product = parties::set_product_active(&db.pool().await, id, active).await?;
    notify(&app);
    Ok(product)
}
