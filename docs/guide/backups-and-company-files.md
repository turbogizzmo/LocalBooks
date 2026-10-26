# Back up, restore and open company files

Everything is in one company file on your computer. These tools are in **Settings › File & Data
Controls**.

## Back up

- **Automatic:** a backup is taken when LocalBooks starts, at most once a day, and listed under
  **Snapshots on this computer**. Set how many to keep in **Settings › Preferences Console ›
  Automatic backups**.
- **Right now:** click **Back up now**.
- **To a file you choose** (a USB drive or cloud folder): click **Backup Company File…** and pick
  where to save it. It's saved as a `.localbooks` file and checked after writing.

## Restore a backup

1. Click **Restore from Backup…** and choose the backup, or click **Restore** beside a snapshot in
   the list.
2. LocalBooks checks the file first: that it's a LocalBooks file, undamaged, balanced, and not from
   a newer version.
3. Read the summary, then click **Replace current books**. Your current books are saved as a
   "Before restore" snapshot first.

> **Important:** a restore replaces the whole company file with the backup. Anything entered after
> the backup was taken isn't kept. To undo just one desktop migration import, use **Reverse posted
> entries** on the import screen instead.

## Check and repair the file (File Dentist)

**File Dentist** checks the company file for damage and repairs what can be repaired safely.

1. Click **Run Check-up**. It only reads, so it's safe any time. It checks:
   - the file structure
   - the schema version
   - the database safeguards that enforce double-entry
   - record links
   - that debits equal credits
   - the posting history
   - stored totals, such as amounts paid and stock on hand
2. Each check shows **OK**, **Attention** or **Problem**, and says what to do next. Click
   **Details** to see the specific records.
3. If a check says **Rebuild fixes this**, click **Rebuild…**, then **Back up and rebuild**. A
   "Before rebuild" snapshot is saved first. Rebuild restores missing or changed safeguards and
   indexes, then compacts the file. The check-up runs again when it's done.

Rebuild never changes your transactions. When a problem is in the data itself (broken links,
unbalanced or changed entries, totals that disagree), the file was edited outside LocalBooks. Fix it
with a reviewed correction on the Audit Log (see [Verify and correct the ledger](ledger-integrity.md)),
or restore a backup taken before it happened. If the file structure is damaged, Rebuild is turned
off: restore the most recent backup that passes the check-up.

## Open another company file

- Double-click a `.localbooks` file, or click **Open Company File…** and choose one.
- It opens in place for this session. The window title and the top bar show which file is open, and
  your usual company file is unchanged.
- To open a company **without restoring windows**, hold **Option** (Mac) or **Alt** (Windows and
  Linux) while you click **Open**, or choose **File › Open Company Without Restoring Windows…**
  (Ctrl+Alt+O, ⌘⌥O on a Mac). Other windows close and the company opens on a plain Home page instead
  of the page you last had open, which helps when a page won't load.

## Open a QuickBooks company file or backup

**Open Company File…**, **Restore from Backup…** and **File › Open or Restore Company…** also accept
QuickBooks `.qbw` company files, `.qbb` backups and `.qbm` portable files.

1. Choose the QuickBooks file.
2. Pick where to save the new LocalBooks company file. It's named after the QuickBooks file.
3. The new company opens on the **Desktop Migration Wizard**, which starts reading the QuickBooks
   file. Review and post what it finds as described in
   [Desktop Migration Wizard](desktop-migration.md).

The QuickBooks file is only read, never changed, and the books you had open are left as they were.

To bring a QuickBooks, Quicken or GnuCash file into the books you have open instead, click
**Desktop Migration Wizard…** (also under **Accounting › Desktop Migration Wizard**).

## Keep the books on a network or synced drive

1. Click **Change Company File Location…** and choose the **Folder**, for example on a network drive
   or in Dropbox.
2. Choose **Move books here**, **Open this company file** (if one is already there) or **Create
   company file here** for a new, empty one.

Only one person can have the books open at a time. LocalBooks tells another computer that they're
in use.
