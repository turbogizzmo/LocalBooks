# Rebrand: LocalBooks → Tillhouse

Decided 2026-11-11. Another desktop bookkeeping app was already selling under the LocalBooks name
(on sale by April 2026, before this project's first commit), and localbooks.com and localbooks.app
are taken. The name is now **Tillhouse**, the mark is **Keystone** (a capital T under a roof line),
and the palette is **Spruce & Brass**. The brand review with the evidence and the other options:
https://claude.ai/artifact/BHgvY6tz4kUG3QjL3UhBFo (private to the maintainer).

## What changed

- **Name in everything people read**: window title, menus (macOS takes the app menu's name from
  `productName`), About, dialogs, every screen string and error message, in-app Help, the user guide
  (`docs/guide/`), README, SECURITY.md, the tester site, the release name and notes, the MCP
  server's display title, the TXF export header, PDF producer/creator, the test email's subject and
  body, the authenticator-app label for new two-step enrollments, and HTTP user agents.
- **Product name** in `tauri.conf.json` (`productName`, window title, descriptions, the company
  file type's display name). Installer, bundle and release files are now named Tillhouse.
- **Icons**: `src-tauri/icons/icon.svg` is the source; every PNG, `icon.icns` and `icon.ico` were
  made from it with `npx tauri icon src-tauri/icons/icon.svg` (the android/ and ios/ folders it
  also writes are deleted). `site/icon.png` and `site/favicon.png` are scaled from `icon.png`.
  `src/components/BrandMark.jsx` draws the same mark in the sidebar and Open Window List.
- **Palette**: the values of the `qb` tokens in `tailwind.config.js` (the borrowed `#2ca01c` green
  and the greys and blue modelled on it are gone), the site's CSS variables, and the few colors
  written into components (charts, money bars, print styles, the list-order drop marker). The
  default accent of new invoice and estimate designs is spruce `#0b5d57`; saved designs keep theirs.
  Every text pair meets WCAG AA; the inline comment in `tailwind.config.js` has the ratios.

## Kept on purpose, so existing installs keep working

| Kept | Why |
|---|---|
| Bundle identifier `com.localbooks.desktop` | Names the data and log folders, the WebView storage, and the Windows install identity. Changing it strands settings and company-file locations. |
| `.localbooks` extension, `application/x-localbooks`, `com.localbooks.company-file` | Every existing company file and backup. A new extension, if ever, must keep opening these. |
| Updater endpoint and public key | Points at `turbogizzmo/LocalBooks` releases; installed copies find updates there. |
| Repo name `turbogizzmo/LocalBooks`, the tester-site URL, `rollback::REPO` and every GitHub link | Renaming the repo is a later step; GitHub redirects the old URLs when it happens. |
| Rust crate and binary `localbooks`, module names, `localbooks.exe` | Internal. The installer hook below relies on the executable name. |
| Migrations | Never edited (AGENTS.md rule 9). |
| Keychain / credential-store service names (`LocalBooks SMTP`, `LocalBooks SimpleFIN`, `LocalBooks offsite backup`, `LocalBooks audit trail`, `LocalBooks payout feeds`, `LocalBooks sign-in`, `LocalBooks report passwords`) | Saved passwords and keys are looked up by these. |
| The encrypted-file signature `b"LocalBooks file\x01"` (`file_cipher/vfs.rs`) | Part of the file format. |
| The QuickBooks SDK app name `LocalBooks` (`installed_qb/win.rs`) and the Help text that quotes QuickBooks' prompt | QuickBooks remembers which app it allowed by this name; changing it would make every user authorize again. |
| `PLACEHOLDER_COMPANY_NAME = "LocalBooks Ledger"` | Stored in existing company files and compared to tell an unnamed company. |
| The `qb` Tailwind token names and `qb-*` classes | About 6,000 uses; only the values changed. Renaming them is optional cleanup. |
| Error codes such as `RESTORE_NOT_LOCALBOOKS` | Codes the screens match on; only the message text after the colon changed. |
| Code comments, `docs/plans/`, `docs/testing/`, `docs/dev/`, e2e test comments, past CHANGELOG entries | Not shown to users; history stays as written. |
| The release-signing Keychain item name in `package.json` (`build:mac`) | A secret on the release Mac. |

## Upgrading an installed LocalBooks

- **Windows.** Tauri's installer keys the install folder, uninstall entry and shortcuts on the
  product name, so the first Tillhouse installer would otherwise install a second copy beside
  LocalBooks. `src-tauri/windows/hooks.nsh` (`NSIS_HOOK_POSTINSTALL`) removes the LocalBooks copy
  it replaces: `localbooks.exe`, its uninstaller and notices file, the folder if then empty, the
  Start menu and desktop shortcuts, and the `Uninstall\LocalBooks` and `Software\localbooks\LocalBooks`
  registry keys. App data is keyed on the identifier and is not touched.
- **macOS.** The updater replaces the bundle where it is, so an updated copy keeps the folder name
  `LocalBooks.app` while showing Tillhouse in the menu bar and Dock. A fresh download is
  `Tillhouse.app`; both are the same app with the same data.
- **Linux.** The package name follows the product name (`local-books` → `tillhouse`). The `.deb`
  conflicts with and replaces `local-books`; the `.rpm` obsoletes it. The AppImage updates in place.

**Before releasing:** install 0.15.0 on Windows, run the in-app update to the first Tillhouse
build, and check that one copy remains (Start menu, Installed apps, `%LOCALAPPDATA%`), the company
file opens, and saved email and bank passwords still work. Then the same from the `.msi`, from a
macOS DMG and from a Linux `.deb`. CI's release check covers a fresh install, not this upgrade.

## Later

- Rename the GitHub repo (update `rollback::REPO`, the updater endpoint as a second entry first,
  the site URL and links), and move the tester site to its own domain.
- Optionally rename the `qb` Tailwind tokens.
