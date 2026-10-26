//! Customers, products and sales receipts.

use super::prelude::*;

handlers![
    list_customers,
    create_customer,
    list_products,
    create_product,
    post_sales_receipt,
    list_all_sales,
];

#[tauri::command]
pub async fn list_customers(db: State<'_, Db>) -> Result<Vec<PartyView>, String> {
    sales::list_customers(&db.pool().await).await
}

#[tauri::command]
pub async fn create_customer(
    app: AppHandle,
    db: State<'_, Db>,
    name: String,
    email: Option<String>,
) -> Result<PartyView, String> {
    let customer = sales::create_customer(&db.pool().await, &name, email.as_deref()).await?;
    notify(&app);
    Ok(customer)
}

#[tauri::command]
pub async fn list_products(db: State<'_, Db>) -> Result<Vec<ProductView>, String> {
    sales::list_products(&db.pool().await).await
}

#[tauri::command]
pub async fn create_product(app: AppHandle, db: State<'_, Db>, input: ProductInput) -> Result<ProductView, String> {
    let product = sales::create_product(&db.pool().await, input).await?;
    notify(&app);
    Ok(product)
}

#[tauri::command]
pub async fn post_sales_receipt(
    app: AppHandle,
    db: State<'_, Db>,
    input: SalesReceiptInput,
) -> Result<SalesReceiptSummary, String> {
    let receipt = sales::post_sales_receipt(&db.pool().await, input).await?;
    notify(&app);
    Ok(receipt)
}

#[tauri::command]
pub async fn list_all_sales(db: State<'_, Db>) -> Result<Vec<SalesTransaction>, String> {
    sales::list_all_sales(&db.pool().await).await
}
