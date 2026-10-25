# End-to-end tests

The specs in `e2e/specs/` drive the real debug build of LocalBooks, screens and backend together,
through Tauri's WebDriver support: WebdriverIO talks to `tauri-driver`, which starts the app and
forwards to the platform's own driver (msedgedriver for WebView2 on Windows, WebKitWebDriver for
WebKitGTK on Linux). macOS has no WebDriver for its webview, so these tests run on Windows and
Linux only. CI runs them on every pull request (`.github/workflows/e2e.yml`).

## What they cover

| Spec | Company | Steps |
| --- | --- | --- |
| `users-from-scratch` | `No Users` | Post an invoice without signing in; turn users on and save the recovery key; add a View only user; sign in as them and choose a new password; their vendor tax ID shows masked (`***-**-4321`) and read-only, and saving is refused ("view only"); Settings shows only Users & Passwords and Updates; sign out and recover the Admin with the recovery key. |
| `seeded-users` | `Edge Cases` | The seeded View only user sees `***-**-6789`, read-only, and cannot save; the Admin sees the full tax ID with Show. |

The rules underneath (roles, encryption, the ledger) are covered by `cargo test`; these specs check
that the screens and the role check in front of every command (`guarded` in `src-tauri/src/main.rs`)
agree.

## How each spec gets its own company

`npm test` first runs the test-only seeder (`seed_test_companies` in
`src-tauri/src/test_companies.rs`) into `e2e/.work/seeds`, skipping the large `Five Years` file.
Before each spec file it copies that spec's company (the table in `e2e/wdio.conf.js`) to
`e2e/.work/runs/<spec>/company/localbooks.db`, and points the debug build's `config.json`
(`database_directory`) at that folder. Every spec file starts its own `tauri-driver` and app.

- **Linux:** each spec also gets its own settings and data folders (`XDG_CONFIG_HOME`,
  `XDG_DATA_HOME` under `e2e/.work/runs/<spec>/`), so nothing on your machine is touched.
- **Windows:** the app uses the usual debug-build folder, `%APPDATA%\com.localbooks.desktop\dev-builds`.
  Your own `config.json` there is saved before the run and put back after it; daily backups of the
  test companies may appear in `dev-builds\backups`.

The seeded sign-ins and recovery key are test values (see `e2e/.work/seeds/README.txt`).

## Run them on Linux

Once: the Tauri build dependencies (see `LINUX_DEPS` in `.github/workflows/e2e.yml`), which include
`webkit2gtk-driver` and `xvfb`, then

```bash
cargo install tauri-driver --version 2.0.6 --locked
```

Each run, from the repository root:

```bash
npm ci && npm ci --prefix e2e
```

```bash
npx tauri build --debug --no-bundle
```

```bash
xvfb-run --auto-servernum npm test --prefix e2e
```

With a desktop session, leave out `xvfb-run` to watch the window.

## Run them on Windows

Once:

```powershell
cargo install tauri-driver --version 2.0.6 --locked
```

and an `msedgedriver.exe` whose version matches the installed WebView2 Runtime exactly (as the
workflow's "Install msedgedriver matching WebView2" step does):

```powershell
$version = (Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}').pv
Invoke-WebRequest "https://msedgedriver.microsoft.com/$version/edgedriver_win64.zip" -OutFile "$env:TEMP\edgedriver.zip"
Expand-Archive "$env:TEMP\edgedriver.zip" -DestinationPath "$env:TEMP\msedgedriver" -Force
$env:NATIVE_DRIVER = "$env:TEMP\msedgedriver\msedgedriver.exe"
```

Redo this when WebView2 updates itself (the run fails with a "session not created" version error).

Run the tests from a normal, **not elevated**, terminal (not "Run as administrator"). In an
elevated process WebView2 ignores the debugging port msedgedriver asks for, and every session fails
with "DevToolsActivePort file doesn't exist" ([wry#1782](https://github.com/tauri-apps/wry/issues/1782)).
GitHub's Windows runners are elevated, so CI runs the tests through `gsudo --integrity Medium`.

Then, from the repository root, the same three commands without `xvfb-run`:
`npm ci; npm ci --prefix e2e`, `npx tauri build --debug --no-bundle`, `npm test --prefix e2e`.

## Useful settings

- `TAURI_DRIVER`: path to `tauri-driver` if it is not in `~/.cargo/bin`.
- `NATIVE_DRIVER`: path to `msedgedriver.exe` (Windows) or `WebKitWebDriver` (Linux) if it is not on
  `PATH`.
- `LOCALBOOKS_E2E_KEEP_SEEDS=1`: reuse the companies already in `e2e/.work/seeds` instead of
  reseeding (faster when rerunning one spec). `npm run seed --prefix e2e` reseeds on its own, as CI
  does before the tests.
- One spec: `npm test --prefix e2e -- --spec specs/seeded-users.e2e.js`.

A failing step saves a screenshot to `e2e/.work/screenshots/` (in CI, the
`e2e-screenshots-<platform>` artifact).

## Writing specs

- Find things the way a person does, with the helpers in `e2e/lib/ui.js`: field labels, button
  text, roles (`role="alert"` for an error notice, `role="status"` for others). Add a `data-testid`
  only for something without a label, like the recovery key.
- Never sleep. Every helper waits for its element; for an action that reloads the window (signing
  in or out, turning users on), wrap it in `reloading(...)` or wait for the next screen
  (`waitForApp`, `waitForSignIn`).
- A new spec file needs a company in `COMPANIES` in `e2e/wdio.conf.js`.
