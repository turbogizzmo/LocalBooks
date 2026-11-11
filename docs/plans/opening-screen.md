# Opening a company file without a blank window (R28)

## The problem

LocalBooks opens the company file inside Tauri's `setup` hook, with `block_on` on the UI thread.
Tauri builds the window *before* it calls `setup` (tauri 2.12, `app.rs` `setup()`), so the
window exists but can't paint until the file is open. Several things can make that slow:

- a schema upgrade, which first saves a pre-upgrade copy of the file;
- a network folder, or a virus scanner holding a freshly copied file;
- the single-instance check, which waits up to about 4 s on a stale instance record.

While it lasts, the window is blank white, with no name, no progress and no way to tell it apart
from a hang. A Windows E2E run hit more than 15 s of it (run 42879167108; #344 logs each startup
step).

Opens from inside the app (File › Open, the recent list, a file handed over by a second launch,
Restore) already run off the UI thread as async commands. But the window shows nothing while they
work: the old company stays on screen until the window reloads.

## The design

**One open at a time, announced.** A small module, `opening.rs`, owns three things:

- `Opening`, managed state: which file is opening and the current step (`OpeningState { fileName,
  step }`), and an async lock that lets only one open run at a time.
- `begin(app, path)`: takes the lock, sets the state and emits `company-opening` with it. It returns
  a guard; dropping the guard clears the state and emits `company-opening` with `null`.
- `note(step)`: changes the step of the open in progress and emits it again. The open path calls it
  without an `AppHandle`, and it does nothing when no open is in progress.

Every path that makes a file live holds the guard:

- the launch;
- File › Open and the recent list (`mount_company_file`);
- a hand-off from a second launch, which also goes through `mount_company_file`;
- Restore (`restore_backup_as`, through `mount_company_file`);
- File › New Company;
- Change Company File Location;
- Close Company (so nothing closes the file halfway through an open).

Because they all queue on the same lock, a second launch's hand-off that arrives during a slow
startup waits for that open to finish, then opens its own file. It never races it.

**The launch.**

1. `setup` manages `Db::closed(config)` straight away. Its permission gate stays `Unchecked`, so
   only the commands listed as safe before sign-in run, and those that need the books say "still
   opening".
2. `setup` starts listening for hand-offs before the open, so a second launch during a slow open
   hands its file over instead of starting another process that fights for the file lock.
3. `setup` spawns the open and returns, and the window paints.
4. The spawned task holds the guard. It runs the same `Db::startup_with_file` as before, then
   moves the result into the managed `Db` (`Db::adopt`), refreshes the gate, and only then drops
   the guard.
5. If the open fails, the task manages `StartupFailure` *before* dropping the guard. The window
   shows the failure screen exactly as it does today.
6. When nothing is to be opened (the company was closed at last quit), no guard is taken, and the
   window goes straight to No Company Open.

The work that used to follow the open in `setup` now runs after the adopt, in the same task: the
window title and recent list (`company_changed`), and the daily automatic backup.

`company_changed` records whether the company was left closed, so it must not run while the managed
`Db` is still empty.

**Progress notes.** The open path reports its slow steps with `opening::note`:

- "Saving a copy before updating the company file" (the pre-upgrade backup);
- "Updating the company file for this version" (the migrations).

**The screens.**

- At launch, `main.jsx` asks `get_opening_state` before anything else. While a file is opening it
  shows *Opening <name>…* with the step and a spinner. It subscribes to `company-opening` before
  asking, so the end can't be missed. When the open ends, the window starts as usual, in place:
  - the failure screen, if it failed;
  - sign-in or unlock, if the file has users (or is encrypted, #338: an encrypted file opens locked
    and its unlock happens at sign-in, after this screen);
  - otherwise the app.

  It does not reload the page for this: a reload landing in the middle of a WebDriver command
  leaves WebKit's driver (Linux end-to-end tests) waiting on it for good.
- For in-app opens, a small overlay on every screen shows the same *Opening <name>…* panel while a
  `company-opening` state is set. It blocks clicks so nothing is changed in the company being
  closed. The window already reloads on `company-file-changed` once the new file is live. On
  failure, the overlay goes away and the screen that asked shows the error, as today.

**The name shown.** The file is not open yet, so its company name comes from the recent list when
the file is on it. Otherwise the file name is shown.

## Single instance and file lock

These are unchanged, only moved off the UI thread:

- `instance::hand_off` still runs first in `setup`, synchronously. A second launch must decide to
  exit before it opens anything.
- The shared-folder lock (`FileLock`) is still taken inside `open_live`. A file open elsewhere still
  fails with `STORAGE_IN_USE`, now shown on the failure screen or in the banner after the opening
  screen.

## Not in scope

- Cancelling an open part way.
- A percentage for the upgrade. Migrations and the copy don't report progress, so the note names
  the step only.
