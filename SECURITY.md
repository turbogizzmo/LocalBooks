# Security policy

## Reporting a vulnerability

Please report security problems **privately**, not in a public issue:
[report a vulnerability](https://github.com/turbogizzmo/LocalBooks/security/advisories/new)
(GitHub's private vulnerability reporting). Include the LocalBooks version, your operating system,
and steps to reproduce. You should hear back within a week.

Never attach a real company file or backup to any report: they contain financial records.

## Supported versions

Only the latest release receives fixes. Installed copies offer updates automatically
(Settings › Updates).

## How LocalBooks is built to be safe

- Company files stay on the user's computer; LocalBooks has no server and collects no telemetry.
- Updates are installed only if signed with the project's update key (minisign); the public key
  is built into the app.
- The optional local AI access (MCP) and the second-launch hand-off listen on 127.0.0.1 only and
  require a random per-install token.
- SimpleFIN bank access is read-only and kept in the operating system's credential store, never in
  the company file.
- Emailed forms open in the user's own mail app by default. Sending over SMTP is optional, uses
  STARTTLS or TLS whenever a password is involved, and keeps the password in the operating system's
  credential store, never in the company file.
