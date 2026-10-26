---
name: run-localbooks
description: Build and launch the LocalBooks desktop app (Tauri) safely on test company files, drive it, and stop it. Use when asked to run, try, screenshot or hands-on test LocalBooks, or to work through docs/testing/edge-cases.md.
---

# Run LocalBooks for testing

LocalBooks is a Tauri app: Rust backend in `src-tauri/`, React screens in `src/`. These steps run
a **debug build on test company files**, never the user's real books.

## Safety first

- Debug builds keep their own settings and company file under
  `~/Library/Application Support/com.localbooks.desktop/dev-builds/` (see `data_dirs` in
  `src-tauri/src/main.rs`). The user's real company file and settings, one folder up, are never
  touched. Never launch a release build (`LocalBooks.app`) for testing.
- Only open files under `dev-builds/test-companies/` (made by the seeder below) or new files
  created inside `dev-builds/`.
- Do not change `HOME` to isolate the app: the macOS Keychain stops working, which breaks
  "Remember me" and the audit trail record. `dev-builds` is already isolated.
- Quit only the app you started (match `target/debug/localbooks`), never another LocalBooks.

## 1. Prerequisites

- If any build fails with "You have not agreed to the Xcode license agreements", ask the user to
  run `sudo xcodebuild -license accept` (it needs their password). Nothing compiles until then.
- Port 1420 (`npm run dev`) may belong to another session's worktree. The debug build below does
  not use it.

## 2. Make the test companies

```bash
cd src-tauri && LOCALBOOKS_SEED_DIR="$HOME/Library/Application Support/com.localbooks.desktop/dev-builds/test-companies" cargo test --release -- --ignored seed_test_companies --nocapture
```

About 10 seconds. It replaces `Edge Cases.localbooks`, `No Users.localbooks` and
`Five Years.localbooks` in that folder and writes `README.txt` with the sign-ins, the recovery key
and the closing passcode (test values). Reseed before each test run so results don't depend on an
earlier session's changes.

## 3. Build the app

```bash
npx tauri build --debug --no-bundle
```

About a minute after the first build. It runs `npm run build` first, so the screens are built into
the binary: `src-tauri/target/debug/localbooks`.

## 4. Launch on a test company

Pass the file as the only argument; the app opens it in place:

```bash
src-tauri/target/debug/localbooks "$HOME/Library/Application Support/com.localbooks.desktop/dev-builds/test-companies/Edge Cases.localbooks" > /tmp/localbooks-run.log 2>&1 &
```

Run it in the background. It is ready when `dev-builds/config.json` lists the file first under
`recent_companies`. Backend errors go to the log (stderr).

## 5. Drive it

- **End-to-end tests (Windows and Linux only):** `e2e/` drives the real debug build through
  `tauri-driver` and WebdriverIO, each spec on a fresh copy of a seeded company. CI runs them on
  every pull request; on a Linux or Windows machine, `npm ci --prefix e2e`, build as in step 3, then
  `xvfb-run --auto-servernum npm test --prefix e2e` (Linux) or `npm test --prefix e2e` (Windows,
  with `NATIVE_DRIVER` set to a matching `msedgedriver.exe`). Setup and how to write specs:
  `docs/testing/end-to-end.md`. They cannot run on macOS.
- **Desktop control (computer use)** is the only way to click through the real window on macOS:
  there is no WebDriver for Tauri's webview there. It must be on in the Claude desktop app (Settings › This
  computer › Computer use) *before* the session starts; request access to the app named
  `localbooks`. Take a screenshot before every click.
- **Screens without the backend:** start the `localbooks-vite-1430` preview (`.claude/launch.json`),
  then before the app loads, set `window.__TAURI_INTERNALS__` to an object whose
  `invoke(cmd, args)` returns test data (also give it `transformCallback` and `metadata`). Load
  React from the exact `node_modules/.vite/deps/react.js?v=…` URL the page uses, or hooks fail
  ("Invalid hook call"). Good for layout and form logic; it proves nothing about the backend.
- **The rules underneath** (ledger, roles, encryption) are covered by `cargo test`; run it
  alongside any hands-on pass.

## 6. Stop

```bash
pkill -f "target/debug/localbooks"
```

## What to report

For each case in `docs/testing/edge-cases.md`: pass or fail, what you saw (a screenshot for a
failure), and for a failure the steps that reproduce it. Note anything you could not reach.
