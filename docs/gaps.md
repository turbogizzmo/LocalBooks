# Gaps

The open work, one row per gap. Agents: pick a row, claim it in your PR description, and update the
row in the same PR (see `AGENTS.md`). Keep the table format: one row per gap, `|`-separated, the
columns below in this order, so tools can read it.

- **id:** stable; never reuse one. `R*` rows come from `docs/requirements-gap-review.md` (its
  summary number), `A*` rows from the 2026-10-26 architecture audit, `T*` rows from testers'
  feedback on how the screens work.
- **status:** `open`, `partial`, `in-progress` (claimed by an open PR), `done` (link the PR), or
  `wontfix` (say why in the notes).
- **size:** S ≈ a day or two, M ≈ a week, L ≈ several weeks, XL ≈ a project of its own.
- **accept:** what has to be true to call it done, as a test or a check someone can run.

`R*` statuses are carried over from the review (written at v0.4.0) and updated where known since;
re-check one against the code before you start it.

## Product and compliance

| id | status | size | gap | accept |
|---|---|---|---|---|
| R1 | partial | L | Encryption at rest covers vendor tax IDs only; the rest of the file, backups and attachments are plain | Opening a company file, backup or attachment with plain SQLite or a hex dump shows no readable business data; the key is password-wrapped |
| R2 | open | S | 1099-NEC report uses $600 for every year; payments from 2026 use $2,000 | Test: the 1099 report uses the threshold for the payment year, for years before and from 2026 |
| R3 | partial | M | Import accuracy not signed off (one sample bank account off by −$3,995 after import) | Every sample in the test kit imports with account balances matching the source's own balance sheet; differences are listed, not hidden |
| R4 | open | S–M | Plain-text copies outside the file: CSV/PDF exports, mail staging, staged imports | Temp and staging copies are deleted after use or encrypted; exports warn that they are plain |
| R5 | partial | M | Users, roles and idle lock: merged in #60; the remaining phase (help topic, `SECURITY.md`) to check | `SECURITY.md` and Help describe sign-in, roles and idle lock as built |
| R6 | partial | S | `SECURITY.md` and README claims vs reality | Each claim in both files points to the code or test that backs it |
| R7 | open | M | FTC Safeguards Rule fit for tax preparers and bookkeepers | A guide maps each Safeguards duty to a LocalBooks feature or a stated limit (after R1, R5) |
| R8 | partial | S–M | Backups stay on the same disk; nothing prompts an offsite copy or a test restore | A reminder appears when no backup was copied off this disk or test-restored in N days |
| R9 | partial | M | Retention: attachments can be hard-deleted; no retention guidance | Deleting an attachment on posted records is void-style (kept, hidden); guide states retention |
| R10 | open | L | 1099-NEC forms, IRIS e-file export, W-9/TIN collection | Generates forms and an IRIS-format file that validates against the published schema |
| R11 | partial | M–L | Sales tax: one rate and agency per document; no per-jurisdiction return; no cash basis | Combined rates split by agency; a sales tax return report per agency and period, accrual and cash |
| R12 | open | S–M | No crash reporting or diagnostics bundle (no telemetry) | Help › Save diagnostics writes a local bundle (logs, versions, no business data) the user can send |
| R13 | partial | M | Updates: no rollback; macOS not notarized; Windows not signed | Previous version can be reinstalled from the app; installers are signed and notarized |
| R14 | partial | L | Accountant workflows: no Accountant's Copy or review mode | An accountant can work on a copy and the changes merge back with an audit record |
| R15 | wontfix | XL | Multi-user access (one user at a time by design) | Revisit only if testers ask; would change the storage model |
| R16 | open | M | Payroll: nothing posts a payroll service's results | Imports a payroll service's journal export into balanced entries |
| R17 | open | M | Tax software export (TXF / tax-line mapping) | Accounts map to tax lines; TXF export imports into a tax program |
| R18 | open | L | No condensing or archiving of old years | Old years condense to opening balances with an archive copy; reports from the archive still tie out |
| R19 | partial | M | Accessibility untested | Every screen is reachable and operable by keyboard; an automated a11y check runs in the e2e suite |
| R20 | partial | S | US-only, and not stated anywhere | README and the tester page say it is US-only and what that means |
| R21 | done | S | PCI DSS: out of scope while card numbers are never stored | Keep a check that no field accepts a card number |
| T1 | in-progress | S | Windows tester round 3: no Add new vendor from forms, Enter doesn't move between fields, no vendor transaction list, registers and Reconcile don't sort, register rows don't open the transaction, recent companies show the modified date (branch `feedback/windows-tester-round3`) | Each item works by hand on Windows; `storage::tests::described_recent_list_is_most_recently_opened_first` and the Find `source_id` test pass |
| T2 | open | S | The customer box on invoices, sales receipts, estimates and credit memos has no Add new customer, unlike the vendor box (`VendorField.jsx`) | Typing an unknown customer offers Add as a new customer and Add with details |
| T3 | open | L | Preferences cover a few of QuickBooks Desktop's categories. The per-category inventory and five suggested batches are in `docs/plans/preferences.md` | Each option in that plan is built, or marked left out with the reason; Preferences shows the familiar category list with My and Company tabs |
| T4 | open | M | Forms lack the desktop keyboard and save flow: date-box keys (t, +, -, w/k, m/h, y/r), Save & Close / Save & New / Clear, Alt/Ctrl shortcuts, Edit-menu actions on the open form (`docs/plans/desktop-parity.md`) | Every date box takes the date keys; every transaction form has Save & Close, Save & New and Clear with Alt+S / Alt+N; Ctrl+D, Ctrl+M, Ctrl+Insert, Ctrl+Delete act on the open form |
| T5 | open | M | No Terms, Payment Method, Customer / Vendor / Job Type or Customer Message lists; no Add new in the item column (`docs/plans/desktop-parity.md`; with T2) | The lists are under Lists › Customer & Vendor Profile Lists, are used on forms and records, and item, customer, terms and payment-method boxes offer Add new |
| T6 | open | M | Sales forms have no Bill To / Ship To blocks, P.O. No., FOB, template picker or Customer Message; customer records lack a second address, contacts, credit limit, account no. and type (`docs/plans/desktop-parity.md`) | Each field shows on invoices, sales receipts and estimates, prints, and comes from the customer record |
| T7 | open | M–L | Registers are read-only; no QuickReport, Transaction History or Journal; a name's transactions can't be filtered by type or date (`docs/plans/desktop-parity.md`) | A check or deposit can be entered and recorded in its register; Ctrl+Q opens a QuickReport from a register, account, name or item; the transactions pane filters by type and date |
| T8 | open | M | No print-later queue for checks and invoices, Record Bounced Check, credit-card pay-down or Item Receipt (`docs/plans/desktop-parity.md`) | File › Print Forms prints queued checks and invoices; a bounced check posts its reversal and fee; items can be received from a PO without a bill |
| T9 | open | M | Missing list and detail reports (Transaction Detail by Account, Transaction List by Customer / Vendor, Phone and Contact Lists, Voided/Deleted, P&L by Month and others) and columns by period on summary reports (`docs/plans/desktop-parity.md`) | Each report in the plan's Reports section runs, exports and ties out to the ledger |
| T10 | done (#147) | M | Transaction forms had no toolbar, and the payment method was a drop-down, unlike the desktop forms people are moving from | Each transaction form shows a toolbar of only the actions it supports (Find, New, Save, Void, Print, Email, Attach, Memorize), reachable by Tab and arrow keys; Receive Payment and sales receipts pick the method with `aria-pressed` tiles |
| T11 | open | M | The form toolbar's Previous/Next, Print and Email on a saved document work on invoices only; other forms open saved transactions from their lists, and bills, checks, credit memos and deposits have no printable form of their own | Previous/Next step through saved bills, checks, sales receipts and journal entries; each of those prints from the toolbar |

## Engineering (from the 2026-10-26 audit)

| id | status | size | gap | accept |
|---|---|---|---|---|
| A1 | done | S | No agent protocol in the repo | `AGENTS.md` at the root; `CLAUDE.md` imports it |
| A2 | done | S | No machine-readable backlog | This file |
| A3 | done | S | No lint gate on pull requests | `.github/workflows/lint.yml` runs Clippy (correctness, suspicious, perf, float arithmetic) on every PR |
| A4 | open | S | The Lint check is not yet a required check | Ruleset "Protect main" lists `Clippy` among required checks (repo admin setting) |
| A5 | open | M | Code is not rustfmt-formatted (over 3,000 hunks), so there is no format gate | One formatting-only PR with a `rustfmt.toml`, landed when no feature PRs are open; then `cargo fmt --check` in `lint.yml` |
| A6 | open | M | 111 Clippy style/complexity warnings, so the gate can't use `-D warnings` | `cargo clippy --all-targets -- -D warnings` passes; `lint.yml` uses it |
| A7 | partial | L | `Cents` newtype covers journal lines, entry totals and FX conversion only; documents and commands still pass bare `i64` `*_cents` fields | Invoice, bill, expense, payment and report structs use `Cents`; the JSON stays a bare integer |
| A8 | done | S | Floats allowed anywhere | `#![deny(clippy::float_arithmetic)]` in `main.rs`; each allowed module or item gives a reason |
| A9 | done | S | Imported quantities passed through `f64` | `qbw_records::Value::as_quantity_e4` scales decimal values exactly in integers |
| A10 | open | S | Import drops amounts stored as doubles that are not whole cents: `qbw_company.rs` stored balances fall back to 0 (`read_accounts`, `end_balance_amt`), and `qbw_documents.rs` skips such form lines and payment links without saying so | The import summary counts and lists each value that was not whole cents |
| A11 | done | S | Nothing stopped other modules writing journal rows | Test `ledger::tests::only_the_ledger_writes_journal_rows` |
| A12 | open | S | IIF import tests embed rows from the vendor's sample IIF kit (`iif_import/tests.rs`) | Fixtures are synthetic data written for this project; the opt-in kit test stays opt-in and reads files from outside the repo |
| A13 | open | S | Internal names echo the vendor's product names (`file_dentist`, migration `0008_classic_parity`) | Module renamed (e.g. `file_repair`); the migration file keeps its name (it is pinned) but its comments are neutral |
| A14 | open | S | The windows inside LocalBooks (#111) are covered by unit tests and a hand test on macOS only: no e2e spec drives them, and Ctrl+F6 has not been pressed on Windows | An e2e spec opens two windows, switches with Ctrl+F6, tiles them, closes one with unsaved entries (the prompt shows) and reopens the company with its desktop restored |
