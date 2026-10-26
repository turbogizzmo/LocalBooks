// Local MCP server (read-only AI access).
import { invoke } from '@tauri-apps/api/core';

export default {
  getMcpStatus: () => invoke('get_mcp_status'),
  setMcpEnabled: (enabled) => invoke('set_mcp_enabled', { enabled }),
  regenerateMcpToken: () => invoke('regenerate_mcp_token'),
};
