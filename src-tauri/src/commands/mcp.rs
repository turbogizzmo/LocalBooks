//! Local MCP server (read-only AI access).

use super::prelude::*;

handlers![
    get_mcp_status,
    set_mcp_enabled,
    regenerate_mcp_token,
];

#[tauri::command]
pub async fn get_mcp_status(mcp: State<'_, McpService>) -> Result<McpStatus, String> {
    Ok(mcp.status().await)
}

#[tauri::command]
pub async fn set_mcp_enabled(mcp: State<'_, McpService>, enabled: bool) -> Result<McpStatus, String> {
    mcp.set_enabled(enabled).await
}

#[tauri::command]
pub async fn regenerate_mcp_token(mcp: State<'_, McpService>) -> Result<McpStatus, String> {
    mcp.regenerate_token().await
}
