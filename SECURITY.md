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

## Users and sensitive data

A company file can have users (Settings › Users & Passwords). Without them, anyone who can open the
file can see and change everything, as in earlier versions.

- **Sign-in:** each user's password is kept only as an Argon2id verifier (64 MiB, 3 passes). Wrong
  passwords and unknown user names get the same answer in the same time.
- **Roles:** Admin, Standard and View only, checked by LocalBooks before every command runs. A
  command not yet classified is treated as a change, so View-only users cannot run it.
- **Sensitive data:** vendor tax IDs are encrypted with AES-256-GCM using a random key for each
  company file, and each value is bound to its vendor and to that file. The key is never stored as
  is: each user allowed to see sensitive data, and the file's recovery key, has a copy locked with a
  key derived from their password (Argon2id). Users without that permission have no copy and cannot
  decrypt tax IDs; they see a masked form (`***-**-6789`).
- **Recovery:** the recovery key is shown once when users are turned on, and LocalBooks keeps no
  copy. There is no reset service. With neither an Admin password nor the recovery key, the only way
  back is removing all users, which erases every encrypted value and keeps the books.
- **Idle sign-out:** after 15 minutes without activity in any window (configurable by an Admin),
  including time asleep.
- **Remember me on this Mac** (optional, off by default) keeps the user's password in the operating
  system's credential store and is used once per opening of the file, never after a sign-out or idle
  sign-out.
- **Audit trail:** ledger entries record the signed-in user together with the computer's user.
  Turning users on, adding or changing a user, password resets, using or replacing the recovery key
  and removing all users are recorded in the same hash-chained trail (never with a password or key).
- **No plain copies left in the file:** turning users on encrypts existing tax IDs and then compacts
  the file and empties its write-ahead log; deleted content is overwritten from then on.
- The local AI access (MCP) server sees no books while a file with users has nobody signed in.

What this does not cover:

- Only tax IDs are encrypted. The rest of the company file, its backups, attachments and anything
  exported (CSV, PDF) are readable by anyone who has the file. Use full-disk encryption (FileVault,
  BitLocker) on every computer that holds them.
- Backups made *before* users were turned on still hold tax IDs in plain text. Delete them, or keep
  them somewhere as safe as the books themselves.
- Anyone at the sign-in screen can remove all users (the way back when no Admin password or recovery
  key is left). It erases every encrypted tax ID and is recorded in the audit trail, but it does
  give that person the rest of the books.
- Roles are enforced by LocalBooks, not by the file: another program can read or change the
  database directly. The ledger audit trail is hash-chained and anchored outside the file, so such
  changes are detected (Accounting › Audit Log › Verify ledger integrity), not prevented.
- A tax ID read from a file that is staged for import but not yet imported stays unencrypted until
  the import is carried out or discarded.
- Newer QuickBooks files encrypt tax IDs with QuickBooks' own key; LocalBooks cannot read those and
  imports the vendor without one. Files imported before LocalBooks 0.5.1 may hold that unreadable
  value; it is cleared when the file is opened (or, if already encrypted, at the next unlock).
