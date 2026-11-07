//! Recalling a vendor's last transaction (My Preferences › General).

use super::prelude::*;
use crate::recall::{self, Recalled};

handlers![
    recall_vendor_transaction,
];

#[tauri::command]
pub async fn recall_vendor_transaction(db: State<'_, Db>, vendor_name: String) -> Result<Option<Recalled>, String> {
    recall::last_for_vendor(&db.pool().await, &vendor_name).await
}
