# Release check

`.github/workflows/release-check.yml` is the gate between a test build and **latest**. It checks
the files a release actually ships, on macOS, Windows and Linux, and replaces the manual Mac check.
It runs when a pre-release or draft is published or edited, and by hand (Actions › Release check ›
Run workflow, with the tag and optionally the tag to upgrade from). A pull request that changes the
release check itself runs it against the newest release. Promote a release to latest only
when all three jobs are green.

## What each job checks

Each platform's job summary has one section per step.

| Step | Passes when |
| --- | --- |
| **Artifacts** | Every installer and update bundle for the platform verifies against the updater public key in the release's `tauri.conf.json`, and that key is the same as the previous release's (installed copies check updates against their own key). `latest.json` has the release's version and an entry for each platform. Each of its URLs downloads, the way the updater fetches it, a file identical to a release asset with a valid signature. |
| **Code signature** (macOS) | Report only. `codesign --verify` of the app from the `.dmg` is expected to fail until a Developer ID certificate is set up. The step never fails the job. |
| **Seed** | The **previous** release, built from its tag, runs its test-company seeder (`seed_test_companies` in `src-tauri/src/test_companies.rs`). This gives `Edge Cases`, `No Users` and `Five Years` as that version wrote them. Their balances are recorded. |
| **Upgrade with `--check-file`** | The new release's app opens a copy of each company headlessly (see below). The schema reaches the release's newest step. A pre-upgrade copy is saved and opens at the old schema with the same entries. File Dentist reports no failure, and every account's balance and the entry and line counts are unchanged. |
| **Launch smoke** | The installed app starts on a copy of each seeded company and is still running after 30 s. It leaves no crash report, panic or `startup-error.log`, and it quits. The file it opened is upgraded, has a pre-upgrade copy in the app's backups folder, passes `integrity_check` and `foreign_key_check`, and has unchanged balances. |

The seeded and upgraded company files, the balances and each `--check-file` result are kept as a
workflow artifact (`release-check-<platform>`) for 7 days.

Releases from before `--check-file` existed (v0.6.0 and earlier) skip the Upgrade step, with a
notice. Their upgrade is still checked by the launch, without File Dentist.

## `--check-file` (internal)

A hidden mode of the app itself, for this workflow. It is not a user feature and has no menu item.

```
LocalBooks --check-file <company file> [--backups-dir <folder>] [--output <result.json>]
```

It opens the file the way a launch with that file does: the restore checks, then the schema upgrade
with its verified pre-upgrade copy. Then it runs File Dentist's check-up and prints the JSON result
(`ok`, `schemaBefore`, `schemaAfter`, `preUpgradeBackup`, `checkup`, `problems`). No window opens,
and it doesn't hand off to a running copy, take an automatic backup, start the MCP server or write
audit-trail anchors to the keychain. The exit code is 0 when everything passes, 1 on any problem
and 2 on bad arguments. A File Dentist `WARN` (only ever unused space) is not a problem.

The pre-upgrade copy goes to `--backups-dir`. By default that's a `localbooks-check-backups` folder
next to the company file, never the installed app's backups. On Windows the release build has no
console, so use `--output` to get the result.

**It upgrades the file it is given.** Point it at a copy. To try it on a Mac:

```bash
cp "Edge Cases.localbooks" /tmp/check.localbooks
/Applications/LocalBooks.app/Contents/MacOS/LocalBooks --check-file /tmp/check.localbooks
```

## Running the driver by hand

The steps are subcommands of `.github/release-check/release_check.py` (Python 3.10+, with the
`cryptography` package for the Artifacts step). Run `python3 .github/release-check/release_check.py
--help` to list them. Without `GITHUB_STEP_SUMMARY` set, the summary prints to the terminal.
