# CI pipeline review (2026-11-10)

An end-to-end check of LocalBooks' GitHub pipeline: settings, pull requests, reviews, tests and
releases. Every number below comes from the Actions API for Nov 1-10, 2026 (2,730 workflow runs,
6,726 jobs) or from timing the test suite on a 10-core M-series Mac. Nothing was re-run by hand.

## Summary

- **The Rust suite was slow because of one lock, not because it grew.** From Nov 2 to Nov 10 the
  tests went from 590 to 872 (+48 %) but the test step went from 16 to 62 minutes on macOS (×4) and
  from 9 to 40 minutes on Linux. Tests running side by side waited on SQLite's process-wide
  memory-statistics mutex, and each test applied all 110 migrations again, which is exactly the
  allocation-heavy work that lock serialises. Turning the statistics off and building the migrated
  test database once makes the full suite run in **70 s on three threads** locally, where the
  same tests on the original build were 27 times slower. CI should drop from about 66 minutes to
  under 10 on macOS; confirm on the first batch run.
- **The rust-cache was over GitHub's 10 GB limit** (10.5 GB in 19 entries). Each pull request
  saved its own 600-850 MB copy that only it could read, and those pushed main's caches out. Pull
  requests now only read the cache.
- **CodeQL ran on every push to every feature PR**: 1,580 jobs, more than any other workflow. It
  now runs on batch PRs, main, weekly and on request.
- **The Claude review can no longer fail silently.** On batch #330 it ran, posted nothing and
  reported success. Each review now ends in a label (`review:clean`, `review:problems`,
  `review:failed`), and the job fails when no review was posted.
- **Windows end-to-end failed on nine runs** (twice on main) because `winget install gsudo` got
  HTTP 504 before any test ran. The install now retries and falls back to Chocolatey.
- **Needs you:** make `Clippy` and `JS unit tests` required checks, and two smaller settings
  (see [Needs you](#needs-you)).

## What was checked

| Area | How |
|---|---|
| Workflows | All nine files in `.github/workflows/`, against `AGENTS.md` and `docs/dev/parallel-work.md` |
| Run history | `GET /actions/runs` for each day Nov 1-10 (2,730 runs) and the jobs of every run (6,726 jobs): durations, queue times, conclusions, step timings |
| Test time | Job logs of the main runs on Nov 2 and Nov 10 on all three platforms; the suite run locally with `--report-time` on four builds |
| Caches | `GET /actions/cache/usage` and `/actions/caches` |
| Settings | Ruleset "Protect main" (`/rules/branches/main`; classic branch protection is not used), environments, Actions permissions, secrets (names only), labels |
| Reviews | The 13 Claude Code Review runs: their comments, cost and turn counts from the logs |
| Failures | Every failed E2E job's failing step and log |

## What works

- **Release path.** Tag `v*` → tests on all three platforms → `build` waits for the `release`
  environment, which allows tags `v*` only and requires turbogizzmo's approval → signed updater
  artifacts and `latest.json` uploaded to a draft release. The signing key is an environment secret
  that only `build` sees, the frontend is built before the key is in the environment, and npm
  install scripts are skipped. `release-check.yml` then checks the published files on all three
  platforms.
- **Least privilege.** Default workflow token is read-only; only `build`, Pages, CodeQL uploads, the
  release-check downloads and the Claude review get write scopes, each on the job or workflow that
  needs it. Every third-party action is pinned to a full commit SHA with a version comment.
- **Timeouts.** Every job has one except `Release check › Plan`, which now has 5 minutes.
- **Required check names match.** The ruleset requires `Test (macos-latest)`, `Test
  (ubuntu-latest)`, `Test (windows-latest)`, `E2E (ubuntu-latest)` and `E2E (windows-latest)`,
  with "up to date before merging" on and squash as the only merge method. The macOS job has no
  matrix, so its name stays `Test (macos-latest)` and a skipped run counts as passing.
- **Concurrency.** `Release`, `Lint`, `End-to-end` and `Release check` cancel a superseded run of
  the same ref; tag builds are never cancelled.
- **Claude review cost.** $0.05 to $0.33 of subscription usage per review (median about $0.14),
  10 s to 2 minutes, 3 to 26 turns. One run per PR, as intended.
- **Rust tests on retry.** No Rust test failed and then passed on the same commit in this period.

## What was wrong, and what changed

### 1. The Rust test suite (macOS 16 → 66 minutes)

`Test (macos-latest)` on pushes to main, test step only:

| Date | Tests | macOS | Linux | Windows |
|---|---|---|---|---|
| Nov 2 | 590 | 17-18 min | 9-13 min | 10-12 min |
| Nov 7 | — | 18-34 min | 12-21 min | 8-29 min |
| Nov 9 | — | 35-65 min | 19-36 min | 17-37 min |
| Nov 10 06:10 | 872 | 62 min (3,714 s in libtest) | 38 min | 17 min |

Compiling took about 2 minutes in every run, and every cache restore was a full hit, so the time
was all in running tests.

**Cause.** Run alone, a typical test took 1.4 s; run three or six at a time, the same 26 tests got
*slower* in total (30 s on one thread, 56 s on three, 83 s on six). A profile (`sample`) of the
six-thread run showed most thread time in `__psynch_mutexwait`, called from `sqlite3Malloc` and
`sqlite3_free` through `sqlite3_mutex_enter`: SQLite's memory statistics
([`SQLITE_DEFAULT_MEMSTATUS`](https://sqlite.org/compile.html#default_memstatus), on unless turned
off) take one mutex for the whole process on every allocation. Every test opens its own in-memory
company and applies the 110 migrations, which parses the growing schema again and again, so each
new migration and each new test made the contention worse. The runners have 3 (macOS) or 4 cores,
so the suite ran three or four tests at a time, each waiting on the others.

**Changes.**

- `src-tauri/.cargo/config.toml` sets `LIBSQLITE3_FLAGS = "-DSQLITE_DEFAULT_MEMSTATUS=0"` for every
  build of the bundled SQLite. LocalBooks never reads SQLite's memory statistics or sets heap
  limits, and SQLite lists this among its recommended options.
  Cargo reads that file when it runs inside `src-tauri`, as `cargo test` does; whether `tauri
  build` (installers, the e2e app) also picks it up was not checked. It only affects speed.
  rust-cache hashes `.cargo/config.toml` into its key, so main's caches are rebuilt with the flag.
- `db::test_pool` applies the migrations once per test binary, keeps the database's bytes
  (`sqlite3_serialize`), and gives each test its own copy (`sqlite3_deserialize`). A copy is the same
  file a fresh migration writes, audit trail included; the only difference is that every copy has
  the same random `ledger_chain.file_id`, which no test compares.

**Measured** (full suite, `--test-threads 3`, 10-core M-series Mac, debug build):

| Build | Time |
|---|---|
| Original | not run to the end (first 66 tests: 401 s, against 15 s on the last row) |
| Statistics off only | 314 s |
| Migrated once only | 220 s |
| Both | **70 s** (839 passed, 33 ignored) |

Slowest tests now: `condense::tests::five_years_condenses_its_first_three_years` 40 s, everything
else 7 s or less.

**Expected in CI:** compile about 2 minutes plus a few minutes of tests on each platform, against
62/38/17 minutes before. The macOS job's `timeout-minutes` stays at 110 in this PR, matching
`batch/11`, so the two merge cleanly; lower it to 30 once a few runs confirm the new time.

**Considered, not done.** Optimizing dependencies in test builds (`[profile.dev.package."*"]
opt-level = 2`) made a typical test 2.3 times faster but more than doubled a clean build (87 s →
222 s locally); after the lock fix it is not worth the compile time. Splitting the macOS suite into
shards, or cargo-nextest, would double the macOS runners each run takes; not needed at 70 s.

### 2. The rust-cache filled the 10 GB limit

On Nov 10 the repository held 10.5 GB of caches in 19 entries: eight for `main`, and eleven for
pull requests (`refs/pull/N/merge`), 600-850 MB each. A pull request's cache can only be read by
that pull request, so every PR run that saved one pushed out older caches, `main`'s included.
Saving also costs time at the end of the job; on #330 saving the macOS cache hung for 30 minutes
after the tests passed and timed the job out.

**Change:** `save-if: ${{ github.event_name != 'pull_request' }}` on every `rust-cache` step in
`Release` (test jobs), `Lint` and `End-to-end` (the macOS line is the same as on `batch/11`), and
`save-if: ${{ github.ref == 'refs/heads/main' }}` on the release `build` job, whose tag-scoped
caches no later tag could read. Pull requests still restore `main`'s caches.

### 3. The Claude review could fail silently, and its findings were easy to miss

- **#330 (batch eleven, 309 files):** `gh pr diff` failed (over GitHub's 300-file limit), the
  reviewer quit after 9 turns, posted nothing and the job reported success. #331 skips `batch/*`
  branches; this PR carries the same lines, so the two merge cleanly in either order.
- **No visible verdict.** The review was one comment among many; nothing told the session that
  batches PRs whether it had found problems.
- **Stacked PRs:** a PR whose base is another feature branch gets no review and no CI at all,
  because every workflow triggers on `pull_request` to `main` only. That is by design; open stacked
  PRs against `main` (or rebase them onto it) when they should be checked.

**Changes in `claude-code-review.yml`:**

- The comment's first line is a fixed verdict (`Claude review: no problems found`, `N problems
  found` or `not reviewed`), and a new step turns it into a label: `review:clean`,
  `review:problems` or `review:failed`. The labels are created on first use.
- If the run posted no comment at all, the step posts "Claude review: not reviewed" with a link to
  the run, labels `review:failed` and fails the job, so a silent run shows as a red check.
- A per-PR concurrency group, so closing and reopening quickly reviews once.
- Triggers are unchanged (opened, ready for review, reopened; owner's non-draft PRs).

Labels added with the workflow's own token start no other workflow runs.

### 4. Queue contention and wasted runs

Runner minutes, Nov 1-10:

| Workflow (event) | Minutes | Jobs |
|---|---|---|
| Release (pull request) | 29,297 | 1,839 |
| CodeQL (pull request) | 6,458 | 1,580 |
| End-to-end (pull request) | 5,474 | 1,051 |
| Release (push to main) | 4,259 | 205 |
| Lint (pull request) | 1,802 | 943 |
| Everything else | about 2,100 | |

In all, 49,400 runner minutes, of which **11,100 (22 %) were spent on runs that were later
cancelled**, 5,400 of them on macOS. 282 of the 528 `Release` runs on pull requests (53 %) were
cancelled by a newer push to the same PR. macOS jobs waited up to 128 minutes in the queue on Nov 9,
and batch PRs waited up to 70 minutes for a macOS runner behind feature-PR runs, before the macOS
suite was limited to batch PRs (in #305, on Nov 10).

GitHub Free runs at most 20 jobs at a time, 5 of them on macOS, so every feature-PR job queues
ahead of the batch that actually gates a merge.

**Changes:**

- **CodeQL** runs on batch PRs, PRs labelled `codeql`, pushes to `main` and weekly, with a
  concurrency group that cancels a superseded PR run. The Rust analysis alone was 8 minutes a run.
  Feature PRs reach `main` only through a batch, which is analysed before it merges.
- **The test time fix** (section 1) shortens every remaining Rust job by most of its length.
- **`docs/dev/parallel-work.md`** described merging each feature as soon as it is green and
  advised against batches, the opposite of how PRs now land. It now describes the batch flow,
  the review labels, and asks sessions not to merge `main` into a feature branch only to keep it
  current: each push re-runs about ten jobs, and the batch updates to `main` once.

Left as is: Linux and Windows Rust tests, end-to-end, Clippy and the JS checks still run on every
feature PR. They are the signal a session needs before its PR goes into a batch, and after the
test-time fix they are short.

Adding the `codeql` or `macos` label to an open PR starts that run at once (the `labeled` trigger,
found by the Claude review of #332). Only that label starts anything: each job checks the label's
name, and any other label (the `review:*` ones included) runs in a concurrency group of its own, so
it cancels nothing. CodeQL takes `labeled` in `codeql.yml`. The macOS run is a separate workflow,
`macos-label.yml`, with its own job name (`Test on macOS (label)`): a `labeled` trigger in
`release.yml` would start a whole Release run, cancel the PR's run in progress, and post skipped
`Test (ubuntu-latest)` / `Test (windows-latest)` checks that could stand in for a failed required
check. After the label is on, `release.yml` runs `Test (macos-latest)` on each later push as before.

### 5. Other findings

| Finding | Evidence | Status |
|---|---|---|
| Windows E2E failed before testing: `winget install gsudo` got "Gateway timeout (504)" | 9 of 52 failed E2E jobs, including main on Nov 8 and Nov 9 | Fixed: three tries with a pause, then `choco install gsudo` |
| Other Windows-only E2E failures (an element not clickable or not displayed after 15 s) | Nov 7, three PR runs, Linux passed | Not fixed: could be timing on WebView2; no failure repeated on the same commit, so nothing proves a flake yet. Worth watching |
| `Lint` failed on batch/10 | JS unit tests on its first head, green on the next push | A real failure, fixed in the batch |
| The test job's comment still said "macOS takes about 41 minutes" | `release.yml` | Updated |
| `workflow_dispatch` of `Release` builds installers in the `release` environment, whose policy allows tags `v*` only | Environment policy | A manual run from a branch will wait on the environment and be rejected; run it from a tag, or add a branch rule if you want branch builds |
| `AGENTS.md` CI paragraph | | Updated for CodeQL and the review labels |
| `Third-party notices` and `Security audit` failed on Dependabot and release PRs, Nov 2-9 | 34 runs | Not investigated here: they run only when a lockfile changes, aren't required, and cost 200 minutes in all |

## Follow-up: time to ship (2026-11-11, gap A20)

Measured on PR #408 (the 0.16.0 release PR) and the v0.16.0 tag, from the Actions API.

| Where | Before | What it was |
|---|---|---|
| PR CI, end to end | 22.7 min (21:34 to 21:56) | `Test (windows-latest)` the long pole: 18 min, of which 4.3 min compiling and 776 s (13 min) running the tests; Linux ran the same tests in 407 s |
| Tag run before any build | 19.2 min (21:57 to 22:16) | `Test` ×3 again on the commit that main's own run (started the same second) was already testing |
| `Build (Windows)` | 32.5 min | no Rust cache, ever (below) |
| `Build (macOS universal)` | 35 to 41 min (v0.14.1, v0.15.0); over 50 for v0.16.0 | compiling the app crate with fat LTO and one codegen unit: 20 min for aarch64, then 14.5 min for x86_64 (v0.14.1) |
| Release check, per platform | 7 to 14 min | the seeder of the previous release built with fat LTO: 3.5 to 11 min |
| Dependabot | 9 PRs × 6 workflows, twice on Nov 11 | every push to main rebased every open update PR; at 21:59 that queued about 54 runs beside the v0.16.0 build |

**What changed:**

- **A tag doesn't test its commit a third time.** Its commit passed on the PR and is being tested
  by its push to main. The tag run's `Tests passed on main` job (one small Linux runner) waits for
  that run's three `Test` jobs and fails unless they all passed. The `Build` jobs no longer wait on
  tests: they start once the `release` environment is approved, so the tests run beside the
  builds instead of 19 minutes before them. Publishing stays gated: the draft is invisible to
  installed copies, the tag run is red if main's tests failed, and the release check's first job
  refuses a tag whose commit didn't pass on main.
- **Pushes to main are no longer cancelled by a newer push** (in `release.yml`), so every commit
  on main has a complete test run a release can rely on. These runs are still needed: they are the
  release's test evidence, and they save the Rust caches that pull requests read.
- **SQLite/SQLCipher is built optimized in tests** (`[profile.dev.package.libsqlite3-sys]` in
  `Cargo.toml`). It was compiled unoptimized like the rest of the test build, and every test runs
  through it. Locally the encrypted-file tests went from 13.8 to 8.3 s and the condense tests from
  48 to 39 s. Optimizing every dependency gained almost nothing more and made a cold test build
  three times longer, so only this package is.
- **On Windows the tests' temp folder is on the work disk** (D:, beside the workspace) instead of
  the system disk. (Defender's real-time scan is already off on the runners, so nothing to change
  there.)
- **No dead caches.** No `build-*` cache ever existed: the Build job saved only on `main`, where it
  never runs, and a tag's cache can't be read by any later tag. The release check's cache was saved
  under the release tag for the same reason. Both steps are gone. (A cache can't help the macOS
  build anyway: the dependencies take about 3 of its 35 minutes.)
- **The release check's seeder builds without LTO.** It only writes the test companies, so it keeps
  `opt-level = 3` but skips the whole-program link.
- **Dependabot no longer rebases on every push to main** (`rebase-strategy: disabled`). Comment
  `@dependabot rebase` on an update PR before it goes into a batch.

**After** (PR #409, the first run with these changes; its cache was a partial match, as for #408):

| Where | Before | After |
|---|---|---|
| `Test (windows-latest)`: test run | 776 s | 367 s |
| `Test (windows-latest)`: job | 18.0 min | 11.1 min |
| `Test (ubuntu-latest)`: test run | 407 s | 307 s |
| Tag pushed → Build jobs start | 19.2 min | at once (after the approval) |
| Release time, tag → all builds done | tests 19 min + slowest build | slowest build (macOS, 35+ min) |

Thin LTO, measured for the record (aarch64, `-j 3` on an M-series Mac): fat LTO with one codegen
unit took 8 min 16 s and made a 45 MB binary; thin LTO with 16 codegen units took 4 min 55 s and
made a 70 MB one. On CI that would save roughly 15 of the macOS build's 35 minutes, at 55 % more
binary.

Left as is:

- **cargo-nextest:** it runs each test in its own process, which would undo the "migrate once per
  test binary" template (gap A18) and make every test apply all migrations again.
- **The shipped release profile** (`lto = true`, `codegen-units = 1`), which is what makes the
  macOS universal build long. Changing it changes the shipped binary, so it is a separate
  decision; see gap A20's note.
- **Path filters for the Rust tests:** `docs_checks.rs` reads the docs and the Help screens, so a
  docs-only PR can still fail them.

## Needs you

These are repository settings; agents can't change them.

1. **Make the lint checks required (gap A4).** Settings › Rules › Rulesets › **Protect main** ›
   *Require status checks to pass* › **Add checks** → type `Clippy`, pick it (source GitHub
   Actions); again for `JS unit tests` → **Save changes**. Both run on every PR to `main` already.
2. **Require pinned actions (gap A19).** Settings › Actions › General › *Actions permissions* →
   tick **Require actions to be pinned to a full-length commit SHA** → **Save**. Every workflow
   already pins them, so nothing breaks; it stops an unpinned action being added by mistake.
3. **Delete the `copilot` environment (gap A19).** Settings › Environments › **copilot** ›
   **Delete environment**. It has no rules or secrets and was used only by Copilot reviews.

Optional, once the new test times are confirmed: lower `Test (macos-latest)`'s `timeout-minutes`
from 110 to 30 (and the Linux/Windows test jobs' from 75 to 30), so a hang fails in half an hour
instead of two.

## How to re-check

```bash
# Test-step minutes per platform for the last pushes to main
gh run list --workflow release.yml --branch main --event push -L 5 --json databaseId \
  --jq '.[].databaseId' | xargs -I{} gh run view {} --json jobs \
  --jq '.jobs[] | select(.name|startswith("Test")) | "\(.name) \(.steps[] | select(.name=="Run ledger test suite") | .completedAt) \(.startedAt)"'
```

```bash
# Cache use against the 10 GB limit
gh api repos/turbogizzmo/LocalBooks/actions/cache/usage
```

```bash
# Per-test durations locally (stable toolchain)
cd src-tauri && RUSTC_BOOTSTRAP=1 cargo test --locked -- -Zunstable-options --report-time
```
