# AGENTS.md

How coding agents (and people) work on LocalBooks. Read this before changing code. Where this file
and another doc disagree, the code and its tests win; fix the doc in the same PR.

## The project in one paragraph

LocalBooks is a local-first, double-entry accounting desktop app. The accounting engine is Rust +
SQLite (`src-tauri/src`, `src-tauri/migrations`); the screens are React + Tailwind in a Tauri v2
window (`src/`). A company is one SQLite file on the user's disk. Every rule that decides what is
posted lives in Rust and in the schema; the screens only display and collect input.

## Commands

| Task | Command (from the repo root unless noted) |
|---|---|
| Install | `npm install` |
| Run the app with hot reload | `npm run tauri dev` |
| Rust tests (the gate) | `cd src-tauri && cargo test --locked` |
| Lint gate | `cd src-tauri && cargo clippy --all-targets --locked -- -D clippy::correctness -D clippy::suspicious -D clippy::perf` |
| End-to-end tests | see `docs/testing/end-to-end.md` |
| Run the app on test companies | the `run-localbooks` skill in `.claude/skills/` |
| Release build (macOS) | `npm run build:mac` |

CI (`.github/workflows/`) runs the Rust tests on macOS, Windows and Linux, end-to-end tests on
Windows and Linux, and Clippy, on every pull request. A PR merges only when all of them are green.

## Non-negotiable rules

### Money

1. **Money is integer cents.** Never `f32`/`f64`, never `parseFloat` for an amount. In Rust, journal
   lines carry [`Cents`](src-tauri/src/money.rs), which has checked arithmetic only; elsewhere
   amounts are `i64` fields named `*_cents`. In JS, use `src/lib/money.js`.
2. **Quantities are `i64` × 10,000** (`ledger::QUANTITY_SCALE`), **exchange rates `i64` × 1,000,000**
   (`ledger::RATE_SCALE`), **tax rates in parts per million.** Multiply and divide with
   `ledger::mul_div_round` (128-bit, rounds half away from zero).
3. **Sums are checked** (`checked_add`, `Cents::checked_sum`). Overflow is an error, never a wrap.
4. **Floats are a lint error** (`#![deny(clippy::float_arithmetic)]` in `main.rs`). A module that
   needs them for layout, scoring or reading a foreign file format allows the lint on the smallest
   item that needs it, with a `reason = "..."` that says why no amount is affected.

### The ledger

5. **Only `ledger.rs` writes `journal_entries` and `journal_lines`.** Post with
   `ledger::post_entry_in_tx` (or `post_journal_entry`) and void with `ledger::void_transaction`.
   The test `ledger::tests::only_the_ledger_writes_journal_rows` enforces this; reconciliation may
   only mark lines cleared.
6. **Debits equal credits, enforced twice:** `ledger::validate_entry` in Rust and the
   `trg_je_post_must_balance` trigger in SQLite. Never weaken either.
7. **Posted entries are never edited or deleted.** Corrections are reversing entries or the
   reviewed forensic corrections (`forensic.rs`). The audit log is append-only and hash-chained
   (`audit_chain.rs`); write paths use `ledger::begin_write` / `commit_write` so new audit rows are
   sealed before commit.
8. **Write transactions start with `begin_write`** (`BEGIN IMMEDIATE`), so concurrent writers queue
   under WAL instead of failing.

### Schema

9. **Migrations are append-only.** Add `src-tauri/migrations/NNNN_name.sql` with the next free
   number (claim it in the PR description). Never edit a shipped migration; pin its checksum in
   `src-tauri/src/db/released_migrations/`. Tables are `STRICT`.

### Clean room

10. **No third-party proprietary material in the repo:** no vendor installers, binaries, sample
    company files, screenshots, icons or copied sample data. `.gitignore` blocks `*.exe`, `*.dmg`,
    `*.qbw`, `*.qbb` and company databases; never `git add -f` them. Test fixtures are synthetic
    data written for this project.
11. **Other products' names appear only to say what LocalBooks reads or replaces** (for example,
    "opens QuickBooks Desktop backups"), never in LocalBooks' own branding, feature names or
    module names. The trademark notice lives in `README.md`, `site/index.html` and
    `src/components/About.jsx`; keep all three.
12. **Never paste vendor documentation or code into comments.** Describe file formats in your own
    words and cite the public source.

### Privacy and security

13. Nothing leaves the computer unless the user turns on a feature that sends it (email, bank
    sync, updates). No telemetry. Secrets go to the OS credential store (`secrets.rs`), never into
    the company file, logs or error messages.

## Working in parallel

Several branches are usually open at once. Follow `docs/dev/parallel-work.md`: one feature per
branch, new commands in their own `src-tauri/src/commands/<feature>.rs`, screen wrappers in
`src/lib/api/<feature>.js`, merge only when 0 commits behind `main` with CI green on that head.

In a shared checkout, commit only the paths you changed (`git commit -- <paths>`) and leave other
uncommitted work alone.

## Definition of done for a change

- [ ] Tests cover the new behavior; `cargo test --locked` and the Clippy gate pass locally.
- [ ] Docs travel with the feature: user guide (`docs/guide/`), in-app Help
      (`src/components/help/`), README feature row, and `site/index.html` where relevant
      (`docs_checks.rs` enforces the links).
- [ ] A change people will notice has an entry in `CHANGELOG.md` under the next version, written
      for the people using LocalBooks (it becomes the release notes).
- [ ] `docs/gaps.md` is updated: the row you finished is marked `done` with the PR, and any new
      gap you found has a row.
- [ ] The PR description says which migration number it uses, if any.

## Where the plan lives

- `docs/gaps.md`: the open work, one row per gap, with status and acceptance test. Start here.
- `docs/requirements-gap-review.md`: the reasoning and evidence behind most rows.
- `docs/plans/`: design notes for larger features.
- `docs/testing/edge-cases.md`: hands-on checks to run against real company files.
