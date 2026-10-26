//! Third-party notices.

use super::prelude::*;

handlers![
    get_third_party_notices,
];

/// THIRD-PARTY-NOTICES.txt, which the installers carry as a resource (Help › About LocalBooks).
#[tauri::command]
pub fn get_third_party_notices(app: AppHandle) -> Result<String, String> {
    let path = app.path().resolve("THIRD-PARTY-NOTICES.txt", tauri::path::BaseDirectory::Resource).map_err(|e| e.to_string())?;
    std::fs::read_to_string(&path).map_err(|e| format!("could not read {}: {e}", path.display()))
}
