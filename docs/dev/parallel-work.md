# Working on features in parallel

Several feature branches are often open at once. The code is laid out so they rarely change the
same lines, and these rules keep it that way. When two branches do conflict, the one that merges
second rebases and waits for CI again, so every shared line costs time.

## Where a new feature's code goes

| What | Where | Lines touched outside the feature |
|---|---|---|
| Tauri commands | A new file `src-tauri/src/commands/<feature>.rs` that starts with `use super::prelude::*;` and lists its commands in `handlers![...]`, one per line | One line in the alphabetical `features![...]` list in `src-tauri/src/commands/mod.rs` |
| A command added to an existing feature | The `#[tauri::command]` function, and its name in that file's `handlers![...]` | None |
| Screen wrappers for commands | A new file `src/lib/api/<feature>.js` (same name as the commands file) that default-exports its wrappers | None: `src/lib/api.js` picks up every file in `src/lib/api/` |
| Help topics | The area file under `src/components/help/` (`1-sales.js` … `7-company.js`); a screen's help text and main guide go in that file's `screens` | None |
| Step-by-step guide | `docs/guide/<topic>.md`, plus one line in `docs/guide/README.md` | One line in the guide index |
| README | One row in the **Features** table: one row per feature, alphabetical, one short line, with details in its guide | One line |
| Business logic | Its own module, `src-tauri/src/<feature>.rs` | One `mod` line in `src-tauri/src/main.rs` (alphabetical) |

`main.rs` registers `commands::handler()` and doesn't change when a command is added. Each command
name must be registered once, which `commands::tests::every_command_is_registered_once` checks.
Each wrapper name must be defined in only one file under `src/lib/api/`, which
`frontend_checks::backend_wrappers_are_named_once` checks.

Lists that are still shared: the role lists in `src-tauri/src/permissions.rs`. A command not on any
of them is a write, so only a command that opens files, reads without a `get_`, `list_`,
`inspect_` or `preview_` prefix, or needs an Admin is added there. Add it in the list's own order.

## Test builds and secrets

Debug builds (`tauri dev`, `tauri build --debug`) keep their company file and settings under
`dev-builds/` (`data_dirs` in `src-tauri/src/main.rs`), and **never use the system Keychain or
Credential Manager**. Neither does a release build under any bundle identifier other than
`com.localbooks.desktop`. Their secrets (SMTP password, SimpleFIN access, remembered sign-ins, audit
trail record) are owner-only JSON files in that build's config folder, so a test launch never
prompts for the Keychain password and never reads or overwrites the installed app's secrets. The
choice is `store_kind` in `src-tauri/src/secrets.rs`; anything new that stores a secret goes through
`SecretStore::for_this_computer`, never `keyring` directly.

## Migrations

- **Claim a migration number before you start**, in the PR description or the tracking issue, so
  two branches never write the same `NNNN_*.sql`. Take the next free number after the highest one
  on `main` and after any number claimed by an open PR.
- Released migrations are frozen. Once a migration ships, pin its checksum by adding
  `src-tauri/src/db/released_migrations/NNNN.checksum` (the test failure prints the exact content).
  There is one file per migration, so two new migrations never touch the same file.
- Never edit a pinned migration or its pin. Fix mistakes with a new migration.

## Documentation travels with the feature

A feature PR isn't done until people can find out how to use it. In the same PR:

- **User guide:** a section in the right `docs/guide/*.md`, or a new guide. A new guide also needs a
  link in `docs/guide/README.md`, a Help topic with `guide: '<name>'` under
  `src/components/help/`, and a row in the tester page's guide list (`site/index.html`).
  `docs_checks::every_guide_is_linked_from_the_site_help_and_index` fails until all three exist.
- **In-app Help:** add or update the topic for the screen in `src/components/help/<area>.js`.
- **README:** add the feature's row to the feature table, and update *Known limitations* if it
  removes or adds one.
- **Website:** if the feature changes how people install, update, or move their books in, update
  that section of `site/index.html` too. The page is published from `main` by `pages.yml`.

## Merging

Feature PRs reach `main` through batch PRs, which the coordinating session builds and merges.

- **A feature PR is ready for a batch when** its checks are green (Rust tests on Linux and Windows,
  end-to-end, Clippy, JS unit tests) and its Claude review is in. `review:problems` means read the
  review comment and fix or answer each point first; `review:failed` means no review was posted
  (close and reopen the PR to run it again, or review it by hand).
- **The batch:** the green feature branches are merged into `batch/N`, which gets one PR. That PR
  runs everything, macOS and CodeQL included, and is squash-merged (through the REST API) only when
  it is 0 commits behind `main` and every check is green on its head commit. The feature PRs are
  then closed. A green run on an older base proves nothing about the merge result: if `main`
  moved, the batch rebases (or merges `main` in) and waits for CI on the new head.
- **Push a feature branch as few times as you can.** Every push re-runs about ten jobs on the shared
  runners (GitHub Free runs 20 jobs at once, 5 of them macOS), and in Nov 2026 half of all pull
  request runs were cancelled by a newer push. Don't merge `main` into a feature branch only to keep
  it current; the batch does that once. Do it to fix a conflict.
- Merge one batch at a time. After it lands, the next batch updates to the new `main` and goes
  green before it merges.
- Keep a branch to one feature. Moving shared code around, like this layout, is its own PR, landed
  when no feature PRs are open.
