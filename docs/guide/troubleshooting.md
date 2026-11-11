# Troubleshooting

## macOS says Tillhouse can't be opened

Tillhouse isn't notarized by Apple yet, so macOS stops it the first time you open it. Open
**System Settings › Privacy & Security**, scroll down to the message about Tillhouse, and click
**Open Anyway**. You only need to do this once.

If macOS says the app is damaged, it has quarantined the download. Run this once in Terminal, then
open it normally:

`xattr -cr /Applications/Tillhouse.app`

## "Opening <company>…"

While Tillhouse opens a company file, the window says which one and what it's doing. This happens
when it starts, and when you use File › Open, the recent list, Restore, New Company or Change Company
File Location. Usually it's gone in a moment. It stays longer when:

- **Saving a copy before updating the company file.** A new version of Tillhouse updates the
  file's layout the first time it opens it, and keeps a copy of the file as it was before (in the
  backups folder) in case anything goes wrong.
- **Updating the company file for this version.** The update itself.
- The file is in a network or synced folder, or an antivirus program is checking it.

Let it finish. Then Tillhouse shows the company, the sign-in screen, or, if the file couldn't be
opened, the screen below. Help › Save Diagnostics records how long each opening took.

## "Tillhouse couldn't open your company file"

Tillhouse shows this instead of starting when the company file can't be opened. Nothing in the
file was changed.

The screen says what usually causes the problem and what to do:

1. **Try again** restarts Tillhouse and opens the file again. Use it when another program had the
   file open: a backup, sync (OneDrive, Dropbox, iCloud) or antivirus program, or another copy of
   Tillhouse. Wait a minute or close that program first.
2. **Check for updates** looks for a newer Tillhouse and installs it without opening the books
   first. A file saved by a newer version needs the newer app.
3. **Copy details** copies the message for support. **Technical details** shows it, and it is also
   saved in `startup-error.log` next to the company file.
4. To go back to a backup, look in the `backups` folder next to the company file.

### "has a log file next to it that doesn't belong to it"

SQLite keeps recent changes in a log file named after the company file with `-wal` on the end
(`Books.localbooks-wal`). If a log next to a file was written for a different file, Tillhouse
won't open the pair, because reading them together could show leftover memory instead of your
data. This only happens with a leftover or tampered log, for example when files were copied
between folders one at a time. Move the `-wal` file out of that folder (keep it until you're sure),
then open the company file again. The same check covers backups you restore and GnuCash or
QuickBooks for Mac files you import.

## The books are in use on another computer

When the company file is on a network or synced drive, only one computer can open it at a time.
Close Tillhouse on the other computer, or wait a few minutes if it crashed.

## An entry can't be posted or voided

- **"Target period is legally closed":** the date is on or before the closing date. See
  [Close a period](closing-the-books.md).
- **Doesn't balance:** a journal entry's debits must equal its credits.
- **Reconciled:** reconciled transactions are locked. See [Reconcile an account](reconciling.md).

## Emailing a form doesn't attach the PDF

- **On a Mac, nothing happens or you see a permission error:** macOS asks once whether Tillhouse
  may control Mail or Outlook. If you chose **Don't Allow**, open **System Settings › Privacy &
  Security › Automation**, find **Tillhouse** and turn on your mail app.
- **The message opens without the attachment:** your mail app doesn't accept attachments from
  other apps (the new Outlook for Mac is one). Tillhouse shows the PDF in Finder or File Explorer:
  drag it into the message. Or set up **Settings › Email** to send from Tillhouse instead.
- **"The server refused the username or password":** many providers (Gmail, iCloud, Outlook.com,
  Yahoo) need an *app password* for SMTP instead of your usual password. Create one in your
  account's security settings, enter it in **Settings › Email**, then click **Test connection**.
- **"Cannot reach the email server":** check the server name, the security setting and the port
  (STARTTLS on 587 or SSL/TLS on 465 are the usual pairs).

## Get updates

**Settings › Updates** (also **Help › Check for Updates…**) shows the version you are using and
where to read the release notes.

- **Check for updates when Tillhouse starts** is on at first. Shortly after Tillhouse opens, it
  asks Tillhouse' GitHub releases page whether there is a newer version and, if there is, shows a
  bar at the top of the window. Nothing about your company is sent. The choice is kept for this
  computer.
- **Check for updates** asks now. When a newer version is available, **Install and restart**
  downloads it, checks its signature against the key built into Tillhouse (an unsigned or changed
  download is refused), installs it and restarts Tillhouse.
- The first time the new version opens a company file that it needs to upgrade, it saves a copy of
  the file in your backups first (kind *Before upgrade*).

## Go back to the previous version

If a new version of Tillhouse has a problem, choose **Help › Reinstall Previous Version…** (or
**Settings › Updates**) and click **Find the previous version**.

- Tillhouse looks up the release before this one on Tillhouse' GitHub releases page and checks
  that it can open your company file. A new version sometimes changes the company file in a way an
  older one can't read. If so, Tillhouse says it can't go back, and why. To go back anyway, restore
  the copy saved before this version changed the file (**Settings › File & Data Controls**, kind *Before
  upgrade*); anything entered since then is not in that copy.
- **Back up and reinstall…** saves a backup of your company file first. It then downloads the older
  version and installs it only if its signature matches the key built into Tillhouse, as with any
  update. Click **Restart now** to use it.
- To come back to the newest version later, check for updates.

Only an Admin can reinstall an older version.

## Save diagnostics for whoever is helping you

Choose **Help › Save Diagnostics…** (or **Settings › Diagnostics**) and click **Save
diagnostics…**. Tillhouse saves a zip file where you choose. It holds:

- Tillhouse's version, your operating system and processor, and SQLite's version;
- the company file's name and size, which schema updates it has, its integrity check, and how many
  rows each table holds (counts only);
- Tillhouse's own log: when it started and quit, how long starting took (and which steps were
  slow, such as opening the company file), which company file it opened, and any crash with where
  in the program it happened.

It holds no customers, vendors, amounts or other business data. **Show what's in it** lets you read
everything before you save. Tillhouse never sends it anywhere; send the file yourself to whoever
asked for it. The log itself is in Tillhouse's log folder (on a Mac, `~/Library/Logs/com.localbooks.desktop`)
and keeps about half a megabyte.

## Where are my files?

On a Mac:

- Company file: `~/Library/Application Support/com.localbooks.desktop/localbooks.db`
- Backups: the `backups` folder next to it

On Windows they're in `%APPDATA%\com.localbooks.desktop`, and on Linux in
`~/.local/share/com.localbooks.desktop`. The company file may be elsewhere if you moved it;
**Settings › File & Data Controls** shows where.
