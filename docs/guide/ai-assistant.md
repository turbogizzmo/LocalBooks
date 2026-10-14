# Connect an AI assistant (MCP)

LocalBooks can let an AI assistant on your computer (Claude Code, Claude Desktop, Cursor or any
other MCP client) read three summaries: the trial balance, a profit and loss summary, and overdue
receivables. The assistant can't post, void or change anything.

## Turn it on

1. Go to **Settings › AI Access (MCP)** and click **Turn on**.
2. The status shows **● Running at http://127.0.0.1:8321/mcp**.

## Connect Claude Code

1. Under **Connect an assistant**, click **Copy command**.
2. Paste it into a terminal and press Return. It looks like this, with your token:

   `claude mcp add --transport http localbooks http://127.0.0.1:8321/mcp --header "Authorization: Bearer <token>"`

## Connect Claude Desktop, Cursor or another client

1. Click **Copy JSON**.
2. Paste it into the client's MCP configuration, as described in that client's documentation.

## Keep it private

- The server only answers on this computer and refuses requests from web pages.
- Every request needs the access token. **Show** reveals it; **Regenerate** issues a new one, and
  assistants using the old token stop working until you update them.
- LocalBooks sends nothing anywhere itself, but the assistant may send what it reads to its AI
  provider.
- Click **Turn off** to stop it. It stays off until you turn it on again.
