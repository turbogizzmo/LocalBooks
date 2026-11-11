# Release check

`.github/workflows/release-check.yml` is the gate between a test build and **latest**. It checks
the files a release actually ships, on macOS, Windows and Linux, and replaces the manual Mac check.
It runs when a pre-release or draft is published or edited, and by hand (Actions › Release check ›
Run workflow, with the tag and optionally the tag to upgrade from). A pull request that changes the
release check itself runs it against the newest release. Promote a release to latest only
when all three jobs are green. The whole release procedure, including re-running a Build job after
publishing, is in [`docs/dev/releasing.md`](../dev/releasing.md).

## What each job checks

Each platform's job summary has one section per step.

| Step | Passes when |
| --- | --- |
| **Artifacts** | Every installer and update bundle for the platform verifies against the updater public key in the release's `tauri.conf.json`, and that key is the same as the previous release's (installed copies check updates against their own key). `latest.json` has the release's version and an entry for each platform. Each of its URLs downloads, the way the updater fetches it, a file identical to a release asset with a valid signature. |
| **Code signature** (macOS) | Report only, never fails the job. It shows `codesign --verify` of the app from the `.dmg` and its signature details. Builds are signed ad hoc for now (`Signature=adhoc`, no Team ID), which verifies but still makes Gatekeeper warn. A Developer ID signature would show a Team ID here. |
| **Seed** | The **previous** release, built from its tag, runs its test-company seeder (`seed_test_companies` in `src-tauri/src/test_companies.rs`). This gives `Edge Cases`, `No Users` and `Five Years` as that version wrote them. Their balances are recorded. |
| **Upgrade with `--check-file`** | The new release's app opens a copy of each company headlessly (see below). The schema reaches the release's newest step. A pre-upgrade copy is saved and opens at the old schema with the same entries. File Check & Repair reports no failure, and every account's balance and the entry and line counts are unchanged. |
| **Launch smoke** | The installed app starts on a copy of each seeded company and is still running after 30 s. It leaves no crash report, panic or `startup-error.log`, and it quits. The file it opened is upgraded, has a pre-upgrade copy in the app's backups folder, passes `integrity_check` and `foreign_key_check`, and has unchanged balances. |
| **Installer upgrade** | Run last, after removing the copy the Install step made. The **previous** release's installer is installed, then the new one is applied over it the way the in-app updater (tauri-plugin-updater 2.14.1) does. **Windows:** previous `-setup.exe /S`, then new `-setup.exe /P /UPDATE` (installMode `passive`). Exactly one uninstall entry is left, under the new product name; when the name changed, the old install folder, registry keys and Start menu shortcut are gone; the new name's shortcut exists. **Linux:** previous `.deb` then new `.deb` with `dpkg -i`, and previous `.rpm` then new `.rpm` with `rpm -U` in a scratch root; the new package is installed and, after a rename, the old one is removed. **macOS:** the previous `.app.tar.gz` unpacked into a temp Applications folder, then the new one unpacked without its top folder and moved into the old bundle's path, as the updater does; the bundle keeps its old folder name and shows the new `CFBundleName` with the same identifier. On every platform the app data folder (`com.localbooks.desktop`) keeps every file, and the upgraded app runs `--check-file` on each seeded company with unchanged balances (on macOS it also passes the launch smoke). |

The seeded and upgraded company files, the balances and each `--check-file` result are kept as a
workflow artifact (`release-check-<platform>`) for 7 days.

Releases from before `--check-file` existed (v0.6.0 and earlier) skip the Upgrade step, with a
notice. Their upgrade is still checked by the launch, without File Check & Repair.

## `--check-file` (internal)

A hidden mode of the app itself, for this workflow. It is not a user feature and has no menu item.

```
LocalBooks --check-file <company file> [--backups-dir <folder>] [--output <result.json>]
```

It opens the file the way a launch with that file does: the restore checks, then the schema upgrade
with its verified pre-upgrade copy. Then it runs the file check-up and prints the JSON result
(`ok`, `schemaBefore`, `schemaAfter`, `preUpgradeBackup`, `checkup`, `problems`). No window opens,
and it doesn't hand off to a running copy, take an automatic backup, start the MCP server or write
audit-trail anchors to the keychain. The exit code is 0 when everything passes, 1 on any problem
and 2 on bad arguments. A File Check & Repair `WARN` (only ever unused space) is not a problem.

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
--help` to list them. The summary always prints to the terminal (and the run log), and also goes to `GITHUB_STEP_SUMMARY` when that is set.
