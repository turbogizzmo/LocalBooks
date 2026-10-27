# Requirements gap review

What a QuickBooks Desktop–style app needs beyond its features, and where LocalBooks stands on each.
Written 2026-10-24 against `main` at v0.4.0 (`c1d4e09`), plus the unmerged
`feat/company-file-encryption` branch (phases 1–4: `c327d4f`…`0c4d10d`). Every status cites the
code or docs it rests on. Nothing here was changed in code.

Status: **Done** (meets the requirement), **Partial** (some of it, or only on an unmerged branch),
**Missing**. Size: **S** ≈ a day or two, **M** ≈ a week, **L** ≈ several weeks, **XL** ≈ a project
of its own.

Legal and tax points are summarized for planning, not advice. Thresholds and due dates change;
confirm each one against the IRS or state source before building it.

## Summary, ranked by risk

| # | Requirement | Status | Risk if left | Size |
|---|---|---|---|---|
| 1 | Encryption of the company file, backups and attachments at rest | Partial (tax IDs only, unmerged) | A lost laptop or synced folder leaks every customer, vendor, bank and payroll-adjacent record; no breach-law safe harbor | L |
| 2 | 1099-NEC threshold and year rules | Partial | Report uses $600 for every year; payments made in 2026 onward use a $2,000 threshold, so users over-issue (or later under-issue) forms | S |
| 3 | Import accuracy sign-off (First Interstate −$3,995) | Partial | Users migrate books that don't match QuickBooks' bank balance and file taxes on them | M |
| 4 | Plain-text copies outside the file: CSV/PDF exports, mail staging, staged imports | Missing | Sensitive data leaks around the encryption once it ships | S–M |
| 5 | Users, roles and idle lock merged and documented | Partial (unmerged; phase 5 left) | Anyone at the computer can read and post; `SECURITY.md` silent about it | M |
| 6 | `SECURITY.md` and README match reality | Partial | Users trust claims that aren't true (README says "No sales tax"); missing ones (file not encrypted) | S |
| 7 | FTC Safeguards Rule fit, if tax preparers or bookkeepers use it | Missing | Those users can't meet MFA/encryption/WISP duties with LocalBooks as is | M (after 1, 5) |
| 8 | Backups offsite and restore testing | Partial | Local backups die with the disk; nothing reminds users to copy them away | S–M |
| 9 | Record retention (≥7 years) vs deleting data | Partial | Attachments can be hard-deleted; no retention guidance; no way to honor a deletion request without breaking the ledger | M |
| 10 | 1099-NEC forms, e-filing (IRIS) and W-9 / TIN collection | Missing | At 10+ information returns e-filing is mandatory; users must retype everything elsewhere | L |
| 11 | Sales tax returns and multi-jurisdiction rates | Partial | One rate and one agency per document; no per-jurisdiction return, no cash basis | M–L |
| 12 | Crash reporting and diagnostics (no telemetry) | Missing | Bugs on testers' machines are invisible unless they report them by hand | S–M |
| 13 | Update safety: rollback, code signing, notarization | Partial | A bad release can't be undone on installed copies; Gatekeeper/SmartScreen warnings train users to click through | M |
| 14 | Accountant workflows (Accountant's Copy, review, adjusting entries) | Partial | Accountants can't work on the books in parallel or mark adjusting entries | L |
| 15 | Multi-user access | Missing (by design: one at a time) | Offices with two bookkeepers can't switch from QuickBooks | XL |
| 16 | Payroll | Missing | Users still need a payroll service; nothing posts its results | XL (build) / M (import) |
| 17 | Tax software export (TXF / tax-line mapping) | Missing | Tax time means re-keying the P&L | M |
| 18 | Condensing or archiving old years | Missing | Files grow without bound (attachments are stored inside) | L |
| 19 | Accessibility | Partial | Keyboard and screen-reader users may hit dead ends; untested | M |
| 20 | Localization (US-only) | Partial (stated nowhere) | Non-US users find US-only tax features and formats | S (state it) / XL (support it) |
| 21 | PCI DSS scope | Done (out of scope) | Low, as long as card numbers never get stored | S (guard) |

## Suggested order

1. **Quick fixes that are wrong today (S each):** the 1099 threshold by year (#2); bring `SECURITY.md`
   and the README up to date (#6); a backup reminder and an "open this backup to test it" check (#8).
2. **Finish and merge the encryption branch (#5):** phase 5 (idle lock, help topic, `SECURITY.md`), and
   close its known gap (staged-but-unimported batches keep plain tax IDs).
3. **Encrypt the whole file, not fields (#1, #4):** SQLCipher (or an equivalent page-level cipher)
   keyed by the same password-wrapped data key, so backups, attachments and every table are covered
   at once; then deal with copies outside the file (exports, mail staging, temp PDFs).
4. **Settle import accuracy (#3)** before more testers migrate real books.
5. **Compliance basics (#9, #7):** retention rules, attachment delete → void-style removal, a
   Safeguards Rule checklist in the docs once 1 and 5 exist.
6. **Crash reports and update rollback (#12, #13).**
7. **1099 forms and IRIS export (#10)**, then **sales tax returns (#11)**, then **TXF (#17)**.
8. **Accountant's Copy (#14)**; payroll as an import of a payroll service's journal (#16); condense (#18).
9. **Multi-user (#15)** only if testers ask for it; it's the largest piece and changes the storage model.

Accessibility (#19) should be a short audit now and a rule for every new screen afterwards.

---

## 1. Security and privacy

### 1.1 Encryption at rest — **Partial**, size L

- **What's encrypted:** on `feat/company-file-encryption` only, vendor tax IDs (`vendors.tax_id`),
  AES-256-GCM per field, bound to key ID, column and row, under a data key wrapped per user with
  Argon2id plus a recovery key (`src-tauri/src/crypto.rs`, `tax_ids.rs`, migrations 0042–0044 on
  that branch). Not on `main`.
- **What isn't:** everything else in the SQLite file — customers, addresses, emails, invoices, bank
  feed lines (`bank.rs`, migration 0004), SimpleFIN transaction data (migration 0023), employees
  (0008), the forensic snapshots in `ledger_audit_log` (0013), import staging tables (0012, 0029,
  0035 — `ledger_import_parties.tax_id` is plain), and **attachments**, stored as BLOBs in the
  file (`attachments.content`, migration 0028; `workflows.rs:693`).
- **Backups:** `VACUUM INTO` copies (`backup.rs:89`) are the same plain file. Automatic ones live in
  `backups/` beside the default file even when the books are on a network drive (README, "Where your
  data lives"). A `.localbooks` file copied to a USB stick or cloud folder is fully readable.
- **Risk:** the main threat for a desktop app is a lost or stolen laptop, a shared family computer, or
  a synced folder. Most US state breach laws exempt *encrypted* data; plain SSNs/EINs plus names are
  reportable. Field encryption covers the most legally sensitive field but leaves bank account and
  customer data.
- **Recommendation:** page-level encryption of the whole database (SQLCipher via sqlx's
  `sqlite` with a SQLCipher build, or `rusqlite` + `bundled-sqlcipher`) unlocked by the same wrapped
  data key. Backups made with `VACUUM INTO` then stay encrypted for free. Keep field encryption for
  "can-see-sensitive" separation between users.

### 1.2 Copies of data outside the company file — **Missing**, size S–M

- CSV exports (`export_engine.rs`) and the 1099 CSV include full tax IDs on the branch (memory note
  for phase 3: "1099 masked on screen + full in CSV").
- Emailed PDFs are written to `app_cache_dir()/outgoing/<stamp>/` and deleted only after a day
  (`mail_app.rs:296`, `commands.rs:1146`).
- Printed / "Save as PDF" output is wherever the user puts it.
- Imports: staged batches keep the source data until **Start over** (`qbw_ledger_import.rs:1352`);
  pre-import and pre-upgrade snapshots (`backup.rs:282`, `:302`) are more plain copies.
- **Recommendation:** warn before exporting sensitive columns; mask tax IDs in CSV unless the user
  chooses otherwise; delete the outgoing PDF folder when the mail app has taken it (or at exit);
  encrypt staging along with the file (1.1).

### 1.3 Users, roles, sign-in and idle lock — **Partial**, size M

- Branch has users, Admin/Standard/View roles, a can-see-sensitive flag, a check before every
  command with unlisted commands treated as writes (`permissions.rs` on the branch), and the
  signed-in user in the audit trail.
- Not done: **idle lock** (phase 5), Keychain "remember me", the help topic, `SECURITY.md`.
- `permissions.rs` itself says roles are LocalBooks' own rule: "the company file is still a SQLite
  file that another program can read and change". That stays true until 1.1.
- `restore_backup_as`, `open_company_file` and `create_company_file` are in the `OPEN` list (run
  without sign-in). Opening another file is reasonable; confirm a signed-out user can't use
  "restore as" to overwrite the current books.
- Closing-date passcode is separate from users: PBKDF2-SHA256, 600,000 iterations, minimum **4**
  characters (`closing.rs:17–20`). With users on, decide whether the closing date should be an
  Admin right instead of a second shared secret (QuickBooks does both: Admin sets it, optional
  password).

### 1.4 Secure delete — **Missing**, size S

- No `secure_delete` or `auto_vacuum` pragma anywhere (`db.rs:429–455`). Deleted attachments
  (`workflows.rs:737`), discarded import batches and replaced tax IDs stay in free pages until a
  `VACUUM`. Backups via `VACUUM INTO` drop free pages, so the backup is cleaner than the live file.
- **Recommendation:** `PRAGMA secure_delete = ON` (cheap) on the live connection; moot for content
  once 1.1 lands, but still useful against forensic recovery of plaintext from before encryption
  was turned on (run a `VACUUM` when encryption is turned on).

### 1.5 `SECURITY.md` vs reality — **Partial**, size S

| `SECURITY.md` says | Reality |
|---|---|
| No server, no telemetry | True (no network crate besides updater, SimpleFIN, SMTP; MCP binds 127.0.0.1) |
| Signed updates | True (`tauri.conf.json` `plugins.updater.pubkey`, `main.rs:88`) |
| MCP and hand-off loopback + token | True (README, `mcp_server.rs`, `instance.rs`) |
| SimpleFIN and SMTP secrets in the OS credential store | True (`secrets.rs`) |
| *(not mentioned)* | The company file, backups and attachments are **not encrypted**; anyone with the file can read it |
| *(not mentioned)* | Installers are not code-signed or notarized (`release.yml:17`) |
| *(not mentioned)* | No sign-in on `main`; users only on the unmerged branch |

README drift in the same spirit: "**No sales tax** on invoices or receipts" (Known limitations) is
wrong since migration 0027 / `sales_tax.rs`; "0001–0010 SQL migrations" (Project layout; there are
41); "107 tests" (there are about 234 `#[test]`/`#[tokio::test]` functions). The user guide has no
topics for sales tax, 1099s, or (on the branch) users.

**Recommendation:** add a "What LocalBooks does *not* protect" section to `SECURITY.md`, and a
release checklist item to re-read it and the README's Known limitations.

---

## 2. Compliance

### 2.1 Record retention — **Partial**, size M

- IRS guidance: keep records generally 3 years from filing, 6 if income is underreported by more than
  25%, **7** for bad-debt or worthless-securities deductions, 4 for employment tax records, and
  indefinitely if no return was filed. Planners use "7 years" as the safe default.
- **Good:** posted entries can't be edited or deleted (triggers, README "Double-entry enforced"),
  voids are reversals, `audit_log` and `ledger_audit_log` are append-only, closing locks are
  append-only.
- **Gaps:** attachments (receipts — the evidence the IRS asks for) are hard-deleted with no trail
  and no age check (`workflows.rs:737`); auto-backups keep only 7 by default (`backup.rs:351`), so
  there's no yearly archive; nothing tells the user how long to keep files.
- **Recommendation:** attachments on posted entries in closed periods can't be deleted (or deletion
  is logged and the file kept until a retention date); a year-end "save an archive copy" prompt;
  a short retention section in the backups guide.

### 2.2 1099-NEC — **Partial** (report) / **Missing** (forms, e-file), size S + L

- **Done:** vendors have `is_1099` and `tax_id` (migration 0026); `vendors_1099` sums bank-paid
  expenses and bill payments per calendar year (`workflows.rs:166`); card-paid amounts are left out,
  which matches IRS rules (the card processor reports those on 1099-K). The tax calendar lists the
  January 31 due date (`preferences.rs:348`).
- **Wrong:** `NEC_THRESHOLD_CENTS = 60_000` ($600) for every year (`workflows.rs:161`). Legislation in
  July 2025 raised the 1099-NEC/MISC threshold to **$2,000 for payments made after 2025**, indexed for
  inflation from 2027. Make the threshold a function of the year. *(Verify against the current
  1099-NEC instructions before shipping.)*
- **Missing:**
  - Box mapping: only NEC. No 1099-MISC (rent, box 1; attorney gross proceeds, box 10), no
    account-to-box mapping like QuickBooks' 1099 wizard, no exclusion of payments to corporations
    beyond the user's own flag.
  - Form printing: Copy A for paper filing must be the official red-ink scannable form; plain-paper
    printing works only for Copies B/C and 1096. Most apps print onto preprinted forms or skip
    paper Copy A entirely.
  - E-filing: since tax year 2023 a filer with **10 or more information returns in total** must
    e-file. The IRS's free route is **IRIS** (web upload of a CSV in IRIS's template, or its A2A
    API, which needs a Transmitter Control Code). An IRIS-template CSV export is the realistic
    first step; A2A transmission is a large project with IRS onboarding.
  - Recipient copies (mail or electronic with consent).
- **W-9 / TIN collection — Missing.** No W-9 status, request date, or backup-withholding flag on
  vendors. If a vendor fails to give a TIN, the payer must withhold 24% (backup withholding) and
  report it on 1099 and Form 945. Add: W-9 received (date, attachment), TIN type (SSN/EIN), name as
  on W-9, backup-withholding flag. Optional later: IRS TIN Matching is only available to e-Services
  users, so leave it to the user.

### 2.3 Sales tax — **Partial**, size M–L

- **Done:** rates with an agency and liability account, taxable items, exempt customers, tax on
  invoices/receipts/estimates/credit memos, immutable once posted, payments to the agency, liability
  report (`sales_tax.rs`, migration 0027, `SalesTax.jsx`).
- **Missing:**
  - One rate per document, one agency per rate. Most US locations owe state + county + city
    (sometimes district) to different agencies; QuickBooks handles this with tax groups. A blended
    rate can't be split across agencies on the return.
  - No return by jurisdiction or filing period, no cash-basis option (many states let small filers
    report on cash basis), no record of a return as filed.
  - Liability report skips foreign-currency documents (`sales_tax.rs:392`, `currency_code IS NULL`).
  - No user guide topic.
- Economic nexus (post-*Wayfair*) and automated rate lookup are out of scope for a local app; say so
  in the docs.

### 2.4 PCI DSS — **Done** (out of scope), size S to guard

- LocalBooks stores no card numbers: payment method is a label (`sales.rs`), SimpleFIN is read-only,
  and no payment processor is integrated. That keeps it out of PCI scope.
- **Guard:** memo and reference fields are free text, and bank-feed descriptions can carry partial
  account numbers. A small check that refuses or masks 13–19 digit Luhn-valid numbers in memos
  would keep it that way. Write the rule down: "LocalBooks never stores card numbers."

### 2.5 State breach-notification laws — **Missing** (not addressed), size covered by 1.1

- All 50 states have breach laws. Personal information typically means name plus SSN, driver's
  license, or a financial account number with its access code; most exempt data that was encrypted
  and whose key wasn't taken. The users (small businesses) are the data owners; LocalBooks' job is to
  make the safe harbor available: full-file encryption (1.1), encrypted backups, and a doc page that
  says what is and isn't encrypted.

### 2.6 FTC Safeguards Rule — **Missing**, size M (after 1.1 and 1.3)

- Applies if accountants, bookkeepers or **tax preparers** use LocalBooks for client data (the FTC
  treats tax preparers as financial institutions; the IRS ties it to the PTIN and Publication 4557 /
  the written information security plan, "WISP").
- Requirements that touch the software: encrypt customer information at rest and in transit,
  **multi-factor authentication** for anyone accessing it, access controls, logging of user activity,
  secure disposal (no more than 2 years after last use unless needed), and, since May 2024, telling
  the FTC within 30 days of a breach affecting 500+ consumers.
- LocalBooks today: no encryption, no MFA, users only on the branch, audit trail of postings but not
  of reads/exports. **Recommendation:** don't market to tax preparers until 1.1 and 1.3 land; then
  write a "Using LocalBooks under the Safeguards Rule" page that maps each requirement to a feature
  or to what the firm must do itself. MFA for a local app can be an OS-level step (Touch ID /
  Windows Hello via the Keychain "remember me" planned in phase 5).

---

## 3. Multi-user — **Missing** (by design), size XL

- One user at a time per file, including on shared folders: exclusive SQLite locking plus a heartbeat
  lock file on network drives (`storage.rs:7`, `:38`, `db.rs:453`; README "Single user"; guide
  "The books are in use on another computer").
- QuickBooks Desktop's multi-user mode runs a database server (the Database Server Manager) on the
  host, and several users work at once with record locking. SQLite over SMB can't do that safely.
- **Options, cheapest first:** (a) keep single-user and say it clearly on the tester page; (b) a
  "switch user" flow so two people can take turns quickly, with the lock showing who has it
  (`storage::host_name` already exists); (c) a small LocalBooks host service on one machine that owns
  the SQLite file and serves other copies over the LAN (the existing command surface in
  `commands.rs` is the API). (c) is a project of its own: authentication, conflict rules, and the
  `ledger-changed` event across machines.

## 4. Payroll — **Missing**, size XL to build, M to import

- `employees` has name, email, active (migration 0008:142); time activities and weekly timesheets
  are non-posting (`timesheet.rs`, `TimeTracking.jsx:419`). IIF import maps `PAYCHECK` and
  `LIABILITY CHECK` to a "Payroll" account only (`iif_import.rs:74`). README states "No payroll".
- A real payroll needs: employee SSN, W-4 and state forms, pay rates, federal/state/local
  withholding tables updated every year, FICA and FUTA/SUTA, paychecks and direct deposit, deposit
  schedules, Forms 941 (quarterly), 940 (annual), W-2/W-3 (January 31 to SSA), state returns, new
  hire reporting. Errors carry penalties for the user. Tables change yearly, which conflicts with a
  free, volunteer-maintained app.
- **Recommendation:** don't build payroll. Instead (a) import a payroll service's journal or
  register (Gusto, ADP, Paychex export CSVs) as balanced entries with liability accounts; (b) add
  a "payroll liabilities" report; (c) if SSNs are ever stored for employees, they must be encrypted
  like tax IDs.

## 5. Accountant workflows — **Partial**, size L

- **Done:** "For my accountant" reports — trial balance, general ledger, cash flow (`Reports.jsx:308`);
  journal entries; reclassify (`Reclassify.jsx`); closing date with passcode (`closing.rs`); audit log
  and integrity check (`docs/guide/ledger-integrity.md`).
- **Missing:**
  - **Accountant's Copy:** QuickBooks lets the client keep working after a dividing date while the
    accountant edits the period before it, then imports the accountant's changes. LocalBooks has only
    "send a backup and stop working", which loses one side's work. A middle step: an export of
    journal entries the accountant made (IIF or LocalBooks' own format) that the client imports,
    with the closing date as the dividing line.
  - **Adjusting entries:** no "adjusting" flag on journal entries, so no "Adjusting Journal Entries"
    report or "before/after adjustments" trial balance.
  - **Accountant user:** the branch's roles are Admin/Standard/View. QuickBooks has an External
    Accountant user that can see everything but not sensitive data and can work in closed periods.
  - **Closing-date exceptions report:** what changed in closed periods (QuickBooks has one). The
    `closing_locks` and audit trail have the data.
  - **Review / reconcile discrepancy report** for reconciliations changed after locking (reconciled
    lines are frozen here, so this is mostly covered).

## 6. Data lifecycle

### 6.1 Condense or archive old years — **Done** (Condense Data)

- Done: see [Condense data](guide/backups-and-company-files.md#condense-data-archive-closed-years). The notes below are the original gap.

- Nothing condenses. Attachments make the file grow fastest. QuickBooks' Condense replaces old
  detail with summary journal entries after a backup. With posted history immutable and the forensic
  snapshots per entry, condensing here means: save an archive copy, then start a new file from
  closing balances (open invoices/bills carried forward). That's the simpler and safer design.

### 6.2 Deleting personal data on request — **Missing**, size M

- No customer/vendor/employee delete at all (no `DELETE FROM customers|vendors|employees` in
  `src-tauri/src`); only deactivate. That's right for ledger integrity and retention, but there's
  no way to scrub personal fields (address, email, phone, notes) once the retention period is over.
  US privacy laws (CCPA and state equivalents) mostly don't reach small businesses below their
  thresholds, but the business's own customers may still ask.
- **Recommendation:** "Remove personal details" on an inactive contact with no activity in the last
  N years: blank contact fields, keep the name (or replace with an ID) so the ledger still reads;
  log it in the audit trail. Note that the `ledger_audit_log` snapshots hold only account lines and
  memos, so check memos too.

### 6.3 Export for tax software (TXF) — **Missing**, size M

- No tax-line mapping on accounts and no TXF or similar export. QuickBooks assigns each account a
  tax line (Schedule C line, 1120-S line) and exports TXF for TurboTax/Lacerte. The tax calendar
  already knows the entity type (`preferences.rs`), so a tax-line field per account plus a "tax
  summary" report and TXF export would follow from it.

### 6.4 Backups offsite and restore testing — **Partial**, size S–M

- **Done:** daily automatic backups at launch, verified snapshots, pre-restore/pre-upgrade/pre-import
  copies, restore that checks integrity, migrations, foreign keys and balances before swapping
  (`backup.rs`, README "Online snapshots"); "Backup Company File…" to a chosen place.
- **Missing:** automatic backups only go to the local `backups/` folder, so a dead disk takes the
  books and the backups. No reminder to copy offsite, no scheduled backup to a second location, no
  "last offsite backup" date, no restore test ("open this backup read-only and show its trial
  balance"). Backups are taken only at launch, so an app left open for a week has no new backup.
- **Recommendation:** a second backup destination preference (USB/cloud folder) used daily; a
  reminder if the newest copy outside `backups/` is older than 7 days; timed backups while open;
  a "Test this backup" button reusing `backup::inspect`.

## 7. Reliability and operations

### 7.1 Crash reporting — **Missing**, size S–M

- No telemetry, by design (`SECURITY.md`). No panic hook either: `panic` appears only as code
  paths, no `std::panic::set_hook` and no log crate in `Cargo.toml`. `StartupFailure.jsx` covers
  only start-up failures (`main.rs:116`).
- **Recommendation (privacy-preserving):** a panic hook and a rolling local log file in the app
  data folder; a "Copy diagnostic report" button (version, OS, last errors, no company data) that
  the user pastes into an issue. Opt-in, never automatic.

### 7.2 Update safety — **Partial**, size M

- **Done:** minisign-verified updates; a verified copy before any schema upgrade, newest five kept
  (`backup.rs:296–309`); migrations pinned (`db.rs` `released_migrations_are_never_edited`); newer
  files refused by older builds (restore checks).
- **Missing:** no rollback path: once a new build migrates the file, the old build refuses it, and
  the only way back is the pre-upgrade copy plus a manual reinstall. No staged rollout or "skip this
  version". No code signing or notarization (`release.yml:17`), so testers learn to bypass OS
  warnings. The update key is a single point of failure (memory: losing it breaks all updates);
  keep an offline backup.

### 7.3 Import accuracy — **Partial**, size M

- `.qbw`/`.qbb` full-ledger reader: on the user's test file, 106 of 112 account balances match the
  file's stored ending balances; First Interstate (bank) is **−$3,995** off and drifted as other fixes
  landed. Waiting on the user's QuickBooks trial balance and invoice list (project memory,
  `qbw-record-layer-refs`). Full import re-check figures: 77/77 balances, 5 open invoices / $3,349.00.
- **Requirement missing:** a written acceptance rule for imports: e.g. "every account balance matches
  QuickBooks' trial balance as of the last date, or the difference is listed on screen with the
  transactions behind it". The import screen should show a per-account comparison and refuse to call
  an import "done" while differences exist, instead of relying on users to compare reports.

### 7.4 Database durability — **Done**

- WAL with `synchronous = FULL` locally, rollback journal with exclusive locking on network drives
  (`db.rs:429–455`); File Dentist checks and repairs (`file_dentist.rs`); integrity checks on every
  backup and restore. No gap found.

## 8. Accessibility — **Partial**, size M

- About 141 `aria-label`s and dialogs in `src/components`; keyboard shortcuts for `+ New` (README).
  No automated accessibility tests, no record of a screen-reader pass, no stated support for
  high-contrast or reduced motion, no dark mode. Color is used for aging buckets
  (`ArAgingWidget.jsx:40`) — check those have text labels too.
- **Recommendation:** one VoiceOver/NVDA pass over the main flows (invoice, receive payment, bill,
  reconcile); add `axe-core` checks to the frontend tests; write the target (WCAG 2.1 AA) in the
  README.

## 9. Localization — **Partial** (US-only, unstated), size S to state / XL to support

- Dates and money are formatted with `en-US` throughout (`src/lib/money.js:21`, `Reports.jsx:14`,
  `PrintDocuments.jsx:19`); print layouts are US Letter (`styles/print.css`); tax calendar, 1099s and
  sales tax are US federal concepts. Multi-currency exists for invoices and expenses, two-decimal
  only.
- **Recommendation:** say "US businesses only" on the tester page and README now. Supporting VAT/GST,
  A4, other date orders and translations is a separate product decision.

---

## Not covered here

Feature parity gaps (card credits from imports, FIFO inventory, foreign-currency bills) are tracked
in the import plan and README Known limitations. This review is about requirements around the
features.
