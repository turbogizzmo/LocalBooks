# Troubleshooting

## macOS says LocalBooks can't be opened

Downloaded builds aren't signed yet. Right-click **LocalBooks** in Applications and choose **Open**.
If macOS says the app is damaged, run this once in Terminal, then open it normally:

`xattr -cr /Applications/LocalBooks.app`

## "LocalBooks couldn't open your company file"

LocalBooks shows this instead of starting when the company file can't be opened. Nothing in the
file was changed.

The screen says what usually causes the problem and what to do:

1. **Try again** restarts LocalBooks and opens the file again. Use it when another program had the
   file open: a backup, sync (OneDrive, Dropbox, iCloud) or antivirus program, or another copy of
   LocalBooks. Wait a minute or close that program first.
2. **Check for updates** looks for a newer LocalBooks and installs it without opening the books
   first. A file saved by a newer version needs the newer app.
3. **Copy details** copies the message for support. **Technical details** shows it, and it is also
   saved in `startup-error.log` next to the company file.
4. To go back to a backup, look in the `backups` folder next to the company file.

### "has a log file next to it that doesn't belong to it"

SQLite keeps recent changes in a log file named after the company file with `-wal` on the end
(`Books.localbooks-wal`). If a log next to a file was written for a different file, LocalBooks
won't open the pair, because reading them together could show leftover memory instead of your
data. This only happens with a leftover or tampered log, for example when files were copied
between folders one at a time. Move the `-wal` file out of that folder (keep it until you're sure),
then open the company file again. The same check covers backups you restore and GnuCash or
QuickBooks for Mac files you import.

## The books are in use on another computer

When the company file is on a network or synced drive, only one computer can open it at a time.
Close LocalBooks on the other computer, or wait a few minutes if it crashed.

## An entry can't be posted or voided

- **"Target period is legally closed":** the date is on or before the closing date. See
  [Close a period](closing-the-books.md).
- **Doesn't balance:** a journal entry's debits must equal its credits.
- **Reconciled:** reconciled transactions are locked. See [Reconcile an account](reconciling.md).

## Emailing a form doesn't attach the PDF

- **On a Mac, nothing happens or you see a permission error:** macOS asks once whether LocalBooks
  may control Mail or Outlook. If you chose **Don't Allow**, open **System Settings › Privacy &
  Security › Automation**, find **LocalBooks** and turn on your mail app.
- **The message opens without the attachment:** your mail app doesn't accept attachments from
  other apps (the new Outlook for Mac is one). LocalBooks shows the PDF in Finder or File Explorer:
  drag it into the message. Or set up **Settings › Email** to send from LocalBooks instead.
- **"The server refused the username or password":** many providers (Gmail, iCloud, Outlook.com,
  Yahoo) need an *app password* for SMTP instead of your usual password. Create one in your
  account's security settings, enter it in **Settings › Email**, then click **Test connection**.
- **"Cannot reach the email server":** check the server name, the security setting and the port
  (STARTTLS on 587 or SSL/TLS on 465 are the usual pairs).

## Save diagnostics for whoever is helping you

Choose **Help › Save Diagnostics…** (or **Settings › Diagnostics**) and click **Save
diagnostics…**. LocalBooks saves a zip file where you choose. It holds:

- LocalBooks' version, your operating system and processor, and SQLite's version;
- the company file's name and size, which schema updates it has, its integrity check, and how many
  rows each table holds (counts only);
- LocalBooks' own log: when it started and quit, which company file it opened, and any crash with
  where in the program it happened.

It holds no customers, vendors, amounts or other business data. **Show what's in it** lets you read
everything before you save. LocalBooks never sends it anywhere; send the file yourself to whoever
asked for it. The log itself is in LocalBooks' log folder (on a Mac, `~/Library/Logs/com.localbooks.desktop`)
and keeps about half a megabyte.

## Where are my files?

On a Mac:

- Company file: `~/Library/Application Support/com.localbooks.desktop/localbooks.db`
- Backups: the `backups` folder next to it

On Windows they're in `%APPDATA%\com.localbooks.desktop`, and on Linux in
`~/.local/share/com.localbooks.desktop`. The company file may be elsewhere if you moved it;
**Settings › File & Data Controls** shows where.
