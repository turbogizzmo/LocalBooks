# Encryption at rest for the whole company file (R1)

Status: **design, not built.** Gap R1 in [`docs/gaps.md`](../gaps.md) stays `partial` until the
phases at the end land. This note is the next step for whoever picks R1 up.

## The goal

R1's acceptance test: opening a company file, a backup or an attachment with plain SQLite or a hex
dump shows no readable business data, and the key is wrapped by a password.

## What exists today

- **Field encryption** ([`crypto.rs`](../../src-tauri/src/crypto.rs), migration 0042): one random
  256-bit **data key** per company file. It is never stored as is: each user allowed to see
  sensitive data, and the recovery key, has a **key slot** in the `key_slots` table holding the
  data key sealed with AES-256-GCM under a key that Argon2id (64 MiB, 3 passes) derives from their
  password or recovery key. Vendor tax IDs ([`tax_ids.rs`](../../src-tauri/src/tax_ids.rs)) and
  vendors' bank details ([`ach.rs`](../../src-tauri/src/ach.rs)) are sealed with that key, each
  bound to its table, column and row.
- **Users and sign-in** ([`users.rs`](../../src-tauri/src/users.rs)): the users table and the
  sign-in check live inside the company file, so today the file is opened (and migrated) first and
  the sign-in happens afterwards.
- **Offsite backups** ([`offsite.rs`](../../src-tauri/src/offsite.rs)) are already encrypted as a
  whole: a header with an Argon2id salt and cost, then 1 MiB chunks sealed with AES-256-GCM.
- **Attachments are stored inside the company file** (`attachments`, `record_attachments`), so
  encrypting the file encrypts them. No separate attachment store needs its own scheme.
- `secure_delete` is on for every connection, and turning users on compacts the file
  ([`db.rs`](../../src-tauri/src/db.rs) `compact`).
- Everything else in the file, its local backups (`VACUUM INTO` snapshots in
  [`backup.rs`](../../src-tauri/src/backup.rs)), Condense archives and pre-upgrade copies is plain
  SQLite. [`SECURITY.md`](../../SECURITY.md) says so and points to full-disk encryption.

## Threat model

Protects against: someone who gets a copy of the company file or a backup **without** a password or
the recovery key (a lost laptop without full-disk encryption, a backup drive, a synced folder, a
file sent to the wrong person). Under the FTC Safeguards Rule, information whose key was not taken
is not "unencrypted", so this also narrows what counts as a notification event
([`docs/guide/safeguards-rule.md`](../guide/safeguards-rule.md)).

Does not protect against: malware or a person using the computer while the books are unlocked
(the key is in memory), a keylogger, or someone who has a user's password. Exports (CSV, Excel,
PDF) stay plain by design and LocalBooks already warns when it writes one.

## Options

### A. SQLCipher, through the SQLite library sqlx already links (recommended)

`libsqlite3-sys`, which sqlx 0.8 uses, can build SQLCipher instead of plain SQLite with its
`bundled-sqlcipher` feature. Cargo merges features, so declaring
`libsqlite3-sys = { version = "=0.30.1", features = ["bundled-sqlcipher"] }` (or
`"bundled-sqlcipher-vendored-openssl"`, see below) in our own `Cargo.toml` switches the library under sqlx **without changing sqlx or any query**. sqlx 0.8.6
already supports it: `SqliteConnectOptions` runs a `key` pragma before every other pragma, and has
slots for the other SQLCipher pragmas (`sqlx-sqlite` `options/mod.rs`).

- **What it encrypts:** every database page (AES-256-CBC with a random IV per page and an
  HMAC-SHA512 per page in SQLCipher 4), and the WAL and rollback journal pages. A file without a key
  opens as plain SQLite, so imports of other people's files, plain backups and files from earlier
  versions keep working unchanged.
- **Key:** we pass a raw 256-bit key (`PRAGMA key = "x'…'"`), so SQLCipher's own PBKDF2 is skipped
  and opening costs nothing extra beyond the Argon2id slot we already have.
- **Version:** `libsqlite3-sys` 0.30.1 bundles **SQLCipher 4.5.7, built on SQLite 3.45.3**, while the
  plain build we ship is SQLite 3.46.0. Switching is a small SQLite downgrade; the migrations use no
  3.46 feature that we know of, and the full suite must pass on it before anything ships.
- **Crypto library:** on macOS it uses Apple's CommonCrypto (no OpenSSL). On Windows and Linux it
  needs OpenSSL's libcrypto; the `bundled-sqlcipher-vendored-openssl` feature compiles OpenSSL from
  source (`openssl-src`), which needs Perl on the build machine (GitHub's Windows runners have it)
  and adds a few minutes to a clean build. The feature can be set per target so macOS keeps
  CommonCrypto.
- **Licences:** SQLCipher (BSD-style, notice required) and OpenSSL 3 (Apache-2.0) join
  `THIRD-PARTY-NOTICES.txt` through `scripts/third-party-notices.mjs`.

### B. Our own encrypting SQLite VFS

Register a virtual file system through `libsqlite3-sys` that encrypts each page with AES-256-GCM
(RustCrypto, already a dependency) and select it with `SqliteConnectOptions::vfs`. No OpenSSL and no
SQLite downgrade, but LocalBooks would own storage-level crypto: page reserve bytes for nonces and
tags, the WAL, the journal, torn writes and every SQLite file-control. That is the code most likely
to lose data or leak a page, and it would need an outside review. Not recommended while SQLCipher
does the same job and has years of use.

### C. Encrypt the whole file outside SQLite

Decrypt to a working copy on open, re-encrypt on close (like offsite backups). Rejected: the books
sit in plain text on disk the whole time they are open and after a crash, WAL durability is lost,
and large files take seconds to open and close.

### D. Rely on full-disk encryption (today)

Keeps working and remains the advice for exports, but does not protect copies that leave the disk.

## Where the key slots live

SQLCipher encrypts the whole file, including the `key_slots` table, so the slots that unlock the key
can't be read from inside it. They must be readable before the database is opened. Choices:

1. **A header in front of the database (recommended).** The company file becomes
   `[LocalBooks key header][SQLCipher database]`. A thin "offset" VFS (pass-through to SQLite's
   default VFS; it adds the header length to every read and write of the main database file and
   leaves the WAL and journal alone) lets SQLite see the database at offset 0. It does no
   cryptography, so it is far smaller and safer than option B: open, read, write, truncate, size
   and sync with an offset, plus lock pass-through. SQLite ships a similar design as its "append"
   VFS. The header holds:
   - a magic tag and format version, so LocalBooks (and `wal_guard`) can tell an encrypted file at
     a glance;
   - the SQLCipher page size, so `wal_guard` can still compare it with a WAL's page size (the
     database's own header is encrypted);
   - the key slots: for each, the user id and user name (needed to pick the slot before the users
     table can be read), the Argon2id salt and cost, and the sealed key; one recovery-key slot;
   - two copies (A and B) with a sequence number, written alternately, so a crash while a password
     changes leaves the previous copy readable.
   The `key_slots` table stays inside the database as the authoritative copy; the header mirrors it
   and is rewritten in the same write path whenever a slot changes.
2. **A sidecar file** (`localbooks.db.keys`). Simpler, but a company file copied without its
   sidecar can never be opened again, and LocalBooks is built on "a company is one file".
3. **SQLCipher's plaintext header** (`cipher_plaintext_header_size`) leaves at most the first
   page's SQLite header in clear; there is no room for slots. Not usable.

The page key is **not** the data key itself: it is derived from it with HKDF-SHA256 (label
`localbooks file key v1`), so field encryption and page encryption never share a key.

## Who can open an encrypted file

Today a user without "can see sensitive data" has no key slot. Page encryption needs a key for
everyone who can open the books, so with whole-file encryption **every user gets a slot for the page
key**, and only users allowed sensitive data get the field key as well. Two keys, both from the one
data key:

- page key = HKDF(data key, "file"), in every user's slot;
- field key = the data key itself, only in sensitive-data users' and the recovery slot.

Turning whole-file encryption on therefore needs each existing user's password once (the same rule
as granting sensitive access today), or an Admin sets a temporary password for them.

## Opening, sign-in and idle sign-out

Today: open, migrate, then sign in. With an encrypted file:

1. Read the header. No header: open as today.
2. Show the sign-in screen with the header's user names (the names leak; amounts don't). An unknown
   name does the same Argon2id work as a wrong password, as now.
3. Unlock the slot, open the pool with the page key, then migrate (the pre-upgrade backup is taken
   from the unlocked file and is itself encrypted), then finish the sign-in as today (verifier,
   turned-off users, must-change-password).
4. **Remember me on this computer** keeps working: it supplies the password before step 3.
5. **Idle sign-out closes the pool** instead of only refusing commands, so SQLCipher wipes the key
   from memory; signing back in reopens it. Windows that call commands meanwhile already get
   `IDLE_SIGNED_OUT`. The MCP server's `readable_pool` already returns the closed pool then.

## Backups, copies and restore

Every copy LocalBooks makes of an encrypted file must be encrypted with a header:

- **Snapshots** (`backup::snapshot`, automatic and manual backups, pre-upgrade, pre-restore, Before
  condense): `VACUUM INTO` on an SQLCipher connection, or `ATTACH … KEY` plus `sqlcipher_export`,
  then the current header written in front. What `VACUUM INTO` writes under SQLCipher 4.5.7 is
  measured below. A backup carries the slots of the moment it was taken: an old password opens an
  old backup, and the recovery key opens all of them until it is replaced.
- **`backup::inspect` and restore** read the header, ask for a password or the recovery key if the
  open session's key does not open it, and check it as today.
- **Condense** archives and staging files, the **File Dentist** (`VACUUM`, `integrity_check`) and
  **release checks** open with the key. Imports of QuickBooks for Mac or GnuCash SQLite files open
  without a key, as now.
- **Offsite backups** wrap the (already encrypted) snapshot in their own container, unchanged.

## Turning it on and off, and existing files

- **On** (Settings › Users & Passwords, Admin only, users must be on): export the database into a
  new encrypted file beside it (`ATTACH … KEY`, `sqlcipher_export`), write the header, check it
  (integrity check, row counts per table, the audit chain), then swap it in with the same replace
  path restore and Condense use (including the Windows swap fix). Then list the plain backups and
  archives that still exist and offer to delete them, saying plainly that deleted files may survive
  on SSDs and in synced folders' history.
- **Off**: the reverse export to a plain file, after the same checks.
- Files from earlier versions are untouched until someone turns it on.

## What the user can lose

Today, with no Admin password and no recovery key, **Remove all users** keeps the books and erases
only the encrypted tax IDs and bank details. With whole-file encryption there is nothing to keep:
**losing every Admin password and the recovery key loses the books and every encrypted backup.**
That is a different promise, so whole-file encryption must be a separate, opt-in setting (default
off) with that warning, and the recovery key screen must say so.

**Decided (2026-11-09, by the project owner):** whole-file encryption is a choice, off by default.
Turning it on shows the plain warning that losing every Admin password and the recovery key loses
the books and every encrypted backup, and the recovery key screen says the same while it is on.
Files with users but without whole-file encryption keep today's **Remove all users** way back.

## Performance

Measured on an Apple-silicon Mac with a throwaway program linking sqlx 0.8.6 and
`libsqlite3-sys` 0.30.1 with `bundled-sqlcipher-vendored-openssl`, WAL mode, `secure_delete` on
(the machine was heavily loaded, so treat the numbers as ratios):

| | Same build, no key (plain file) | With a key (encrypted file) |
|---|---|---|
| Insert 200,000 rows in one transaction | 4.79 s | 4.52 s |
| 20 full-table scans with `LIKE` (6.5 MB table, larger than the page cache) | 0.37 s | 0.76 s |
| File size | 6,549,504 bytes | 6,656,000 bytes (+1.6%: per-page IV and HMAC) |
| Encrypt an existing plain file (`ATTACH … KEY`, `sqlcipher_export`) | | 0.05 s |

What the spike also showed:

- With the key, sqlx opened, wrote and read the file normally: SQLCipher reported version
  `4.5.7 community` on SQLite 3.45.3.
- The encrypted file did not start with `SQLite format 3`, and a byte search for a string written
  into 200,000 rows found nothing in it.
- No key, or a wrong key, fails at connect with SQLite error 26 (`file is not a database`), a clean
  error LocalBooks can map to "wrong password" without reading anything.
- `VACUUM INTO` from a keyed connection wrote an **encrypted** copy (no plain header, no plaintext),
  so snapshots stay encrypted with no extra step. Whether that copy opens with the same key, and its
  cost per page, still needs a test in phase 4.
- `sqlcipher_export` into an attached database with an empty key wrote a **plain** copy, the path
  for turning encryption off; into one with a key, an encrypted copy that passed `integrity_check`
  with every row.

Reading pages that are not cached costs about twice as long (each page is decrypted and its HMAC
checked); writes are dominated by syncing to disk and did not change. Company files are far smaller
than the test table, and most screens read a few pages, so the difference should not be noticeable.
Phase 1 measures it on the test companies before anything ships.

## The SQLite pin and `wal_guard` (R24)

- RUSTSEC-2026-0459 affects SQLite before 3.54. Plain SQLite 3.46.0 and SQLCipher 4.5.7 (SQLite
  3.45.3) are both affected; switching to SQLCipher neither fixes nor worsens it. Both come from the
  same `libsqlite3-sys` pin, so the upgrade R24 waits for (a sqlx release that allows
  `libsqlite3-sys` 0.39 or later) brings a newer SQLCipher too. Nothing extra to wait for.
- `wal_guard` reads the page size from the database header at offset 16. An encrypted file has the
  LocalBooks header there instead, and SQLCipher's own first page is encrypted, so the guard must
  take the page size from the LocalBooks header for encrypted files. The WAL's header stays
  plaintext under SQLCipher, so the comparison still works.
- The offset VFS shifts only the main database file; the `-wal` path and format are unchanged.

## Decision

The design holds up **without swapping the database layer**: sqlx, every query and every migration
stay as they are; only the SQLite library under sqlx and a small VFS change. It is still several
pull requests of work that change the file format, the startup and sign-in order, every backup and
restore path and the build chain on three platforms, and it changes what users can lose. So R1 stays
`partial` with this design as its next step, built in this order (the owner has asked for all five):

1. **Build chain.** Turn on SQLCipher (CommonCrypto on macOS, vendored OpenSSL on Windows and Linux);
   no behaviour change, since files without a key open as plain SQLite. Full suite on all three CI
   platforms on SQLite 3.45.3, notices regenerated, build-time and size recorded.
2. **Header and offset VFS.** Header format with A/B copies, the VFS, `wal_guard` reading the
   header, tests that a header-less file still opens and a crafted header is refused.
3. **Open and sign-in order.** Unlock before open and migrate; idle sign-out closes the pool; the
   opt-in setting (off by default) with its warning, as decided above.
4. **Turning it on and off; copies.** Export, check and swap; encrypted snapshots, restore, Condense,
   File Dentist; offering to delete plain backups. The acceptance test (no readable business data in
   the file, a backup or an attachment by hex dump) lands here.
5. **Docs.** `SECURITY.md`, the users guide, Help, the Safeguards Rule guide, the CHANGELOG.
