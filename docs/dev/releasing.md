# Releasing

How a version gets from a tag to every installed copy, and what to do when one platform's build
has to be run again. The workflows are `.github/workflows/release.yml` and
`.github/workflows/release-check.yml`; how the release check reads its results is in
[`docs/testing/release-check.md`](../testing/release-check.md).

## The normal path

1. Bump `version` in `src-tauri/tauri.conf.json`, `package.json` and `src-tauri/Cargo.toml`, and
   write the `## [<version>]` section of `CHANGELOG.md` (it becomes the release notes).
2. Merge that commit to `main`, then tag it: `git tag v0.16.0 && git push origin v0.16.0`. Tag
   only a commit on `main`.
3. `release.yml` doesn't test the tagged commit again: its push to `main` already ran the Rust
   suite on all three platforms (a newer push to `main` doesn't cancel that run). The tag run's
   **Tests passed on main** job waits for that run and fails unless its three `Test` jobs passed.
   Meanwhile the three `Build` jobs (macOS universal, Windows, Linux) start as soon as they are
   approved: they run in the `release` environment, so someone approves them; only that
   environment holds the updater signing key. If **Tests passed on main** fails, re-run the
   failed jobs of main's run (the job's error links it), then re-run **Tests passed on main**;
   don't publish the draft until the whole tag run is green.
4. The first Build job to finish creates a **draft** release for the tag; every Build job uploads
   its installers and update bundles to it and merges its platforms into the draft's
   `latest.json`.
5. When the tag run is green, publish the draft as a **pre-release**. `release-check.yml` runs on
   that and checks the files on all three platforms. Its first job also refuses a tag whose
   commit's `Test` jobs on `main` did not all pass.
6. When the release check is green, promote the release to **latest**. Installed copies read
   `releases/latest/download/latest.json`, so they see the update only from this point.

## Re-running a Build job

A Build job can be re-run (Actions › the tag's Release run › Re-run failed jobs, or re-run that one
job) at any point: while the release is a draft, after it is published as a pre-release, or after
it is latest. The job adds its files to the release that already exists.

What the job does on a re-run:

- **Find the release for this tag** lists the repository's releases (drafts included, since a
  draft can't be fetched by tag) and picks the one for the tag: the published release if there is
  one, otherwise the single draft. It records which platforms that release's `latest.json` already
  lists. The job fails here if the tag has more than one draft and no published release; delete
  the extra drafts and re-run.
- **Build and package with Tauri** gets that release's id (`releaseId`), so tauri-action uploads to
  it instead of looking for a draft. Without the id, tauri-action reuses only a *draft* release
  and creates a new draft next to a published one, which is what happened on v0.15.0. The
  release's name and notes are left alone. Files with the same name are replaced (deleted, then
  uploaded again), and `latest.json` is rewritten with this platform's entries merged into the
  ones it already had.
- **Check latest.json kept every platform** reads the release's `latest.json` back and fails if a
  platform recorded before the build is missing, this job's platform is missing, the version is
  wrong, or any URL points to a file that isn't an asset of this release.

On a published release, for the minute or so while the job uploads, that platform's installers
and `latest.json` are briefly missing or replaced: an installed copy that checks for updates right
then finds nothing and tries again later. Re-run during a quiet moment if you can.

After a re-run on a published release:

1. Check the job summary: all three steps above are green.
2. The rebuilt files are new binaries, so run the release check again by hand: Actions › Release
   check › Run workflow, with the tag. (Uploading files doesn't trigger it.)
3. Promote nothing until it is green; if the release is already latest, it stays latest.

## If a duplicate draft was created anyway

This is how v0.15.0 was repaired, and the fix if the check step fails:

1. On the duplicate draft, note which files it has; delete the draft (its `latest.json` lists only
   the re-run platform and must not be published).
2. Re-run the Build job for that platform. With the published release in place it uploads there
   and merges `latest.json`.
3. If you can't re-run, fix it by hand: upload the platform's files to the published release, then
   download its `latest.json`, copy that platform's entries (`linux-x86_64`, `linux-x86_64-deb`,
   and so on) into `platforms` with each `url` pointing at the new asset's API URL
   (`https://api.github.com/repos/<owner>/<repo>/releases/assets/<id>`) and the matching `.sig`
   contents as `signature`, and upload it in place of the old one.
4. Run the release check by hand as above.
