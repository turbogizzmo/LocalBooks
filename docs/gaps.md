# Gaps

The open work, one row per gap. Agents: pick a row, claim it in your PR description, and update the
row in the same PR (see `AGENTS.md`). Keep the table format: one row per gap, `|`-separated, the
columns below in this order, so tools can read it.

- **id:** stable; never reuse one. `R*` rows come from `docs/requirements-gap-review.md` (its
  summary number), `A*` rows from the 2026-10-26 architecture audit.
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
