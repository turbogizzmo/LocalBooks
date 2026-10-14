# Troubleshooting

## macOS says LocalBooks can't be opened

Downloaded builds aren't signed yet. Right-click **LocalBooks** in Applications and choose **Open**.
If macOS says the app is damaged, run this once in Terminal, then open it normally:

`xattr -cr /Applications/LocalBooks.app`

## "LocalBooks couldn't open your company file"

LocalBooks shows this instead of starting when the company file can't be opened. Nothing in the
file was changed.

1. Read the reason shown. A file from a newer version of LocalBooks needs the newer app.
2. **Copy details** copies the message for support. It is also saved in `startup-error.log` next to
   the company file.
3. To go back to a backup, look in the `backups` folder next to the company file.

## The books are in use on another computer

When the company file is on a network or synced drive, only one computer can open it at a time.
Close LocalBooks on the other computer, or wait a few minutes if it crashed.

## An entry can't be posted or voided

- **"Target period is legally closed":** the date is on or before the closing date. See
  [Close a period](closing-the-books.md).
- **Doesn't balance:** a journal entry's debits must equal its credits.
- **Reconciled:** reconciled transactions are locked. See [Reconcile an account](reconciling.md).

## Where are my files?

On a Mac:

- Company file: `~/Library/Application Support/com.localbooks.desktop/localbooks.db`
- Backups: the `backups` folder next to it

On Windows they're in `%APPDATA%\com.localbooks.desktop`, and on Linux in
`~/.local/share/com.localbooks.desktop`. The company file may be elsewhere if you moved it;
**Settings › File & Data Controls** shows where.
