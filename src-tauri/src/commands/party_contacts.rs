//! Contacts on a customer or vendor: any number, each with a role, and the default contact for
//! invoices and statements (`party_contacts.rs`).

use super::prelude::*;
use crate::party_contacts::{Contact, ContactInput};

handlers![list_party_contacts, save_party_contacts];

#[tauri::command]
pub async fn list_party_contacts(db: State<'_, Db>, kind: String, party_id: i64) -> Result<Vec<Contact>, String> {
    crate::party_contacts::list_contacts(&db.pool().await, &kind, party_id).await
}

#[tauri::command]
pub async fn save_party_contacts(app: AppHandle, db: State<'_, Db>, kind: String, party_id: i64, contacts: Vec<ContactInput>) -> Result<Vec<Contact>, String> {
    let saved = crate::party_contacts::save_contacts(&db.pool().await, &kind, party_id, &contacts).await?;
    notify(&app);
    Ok(saved)
}
