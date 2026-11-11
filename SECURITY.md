# Security policy

## Reporting a vulnerability

Please report security problems **privately**, not in a public issue:
[report a vulnerability](https://github.com/turbogizzmo/LocalBooks/security/advisories/new)
(GitHub's private vulnerability reporting). Include the LocalBooks version, your operating system,
and steps to reproduce. You should hear back within a week.

Never attach a real company file or backup to any report: they contain financial records.

## Supported versions

Only the latest release receives fixes. Installed copies check for updates shortly after they
start, unless that is turned off (Settings › Updates).

## How LocalBooks is built to be safe

Each claim below links to the code that does it and, where there is one, the test that checks it.
`docs_checks::security_claims_point_at_code_that_exists` fails if a link here stops resolving or a
claim loses its link.

- **Company files stay on the user's computer.** LocalBooks has no server and collects no
  telemetry. It makes network requests only for: the update check (on by default, sends no data
  about the books: [`Updates.jsx`](src/components/Updates.jsx)), exchange rates (off unless turned
  on, sends two currency codes and a date: [`fx_rates.rs`](src-tauri/src/fx_rates.rs)), bank
  downloads ([`simplefin.rs`](src-tauri/src/simplefin.rs)), Stripe payouts from api.stripe.com
  ([`payout_feeds.rs`](src-tauri/src/payout_feeds.rs)) and emailing over SMTP
  ([`smtp.rs`](src-tauri/src/smtp.rs)), the last three only after the user sets them up. Offsite
  backup writes to a folder the user picks and sends nothing itself
  ([`offsite.rs`](src-tauri/src/offsite.rs)).
- **Updates are installed only if signed** with the project's update key (minisign); the public
  key is built into the app (`plugins.updater.pubkey` in
  [`tauri.conf.json`](src-tauri/tauri.conf.json)), and the updater refuses a download whose
  signature does not match.
- **The local AI access (MCP)** is off until turned on. It listens on 127.0.0.1 only, drops
  connections from anything but the loopback interface, checks the `Host` header, and requires a
  256-bit random token generated on this computer, compared in constant time; the token can be
  replaced at any time ([`mcp_server.rs`](src-tauri/src/mcp_server.rs), tested over real sockets
  in `http_transport_admits_only_authorized_local_clients` in
  [`mcp_server/tests.rs`](src-tauri/src/mcp_server/tests.rs)). Its tools only read.
- **The second-launch hand-off** (opening a file in the copy already running) listens on
  127.0.0.1 only and requires a random secret made fresh at each launch and kept in a file only the
  user's account can read ([`instance.rs`](src-tauri/src/instance.rs),
  [`instance/tests.rs`](src-tauri/src/instance/tests.rs)).
- **Credentials are kept outside the company file.** On macOS and Windows the installed app uses
  the operating system's credential store (Keychain, Credential Manager). On Linux, and in
  development builds, they go to a file in LocalBooks' settings folder that only the user's account
  can read ([`secrets.rs`](src-tauri/src/secrets.rs)). This covers the bank connection, the SMTP
  password, the Stripe payout key, remembered sign-ins, the offsite backup key and the audit-trail
  anchor.
- **SimpleFIN bank access is read-only**, and its access URL is kept in the credential store above,
  never in the company file or in error messages
  (`connect_link_and_sync_without_duplicates` and
  `a_company_file_without_saved_access_asks_to_reconnect` in
  [`simplefin/tests.rs`](src-tauri/src/simplefin/tests.rs)).
- **Payout feeds (Stripe)** accept only a restricted key (`rk_`), refusing a full secret key that
  could move money, keep it in the credential store above and never in the company file, and call
  Stripe's API only when the user clicks Get payouts
  (`a_fetched_payout_is_recorded_as_a_gross_deposit_that_lands_at_the_net` in
  [`payout_feeds/tests.rs`](src-tauri/src/payout_feeds/tests.rs)).
- **Email** opens in the user's own mail app by default. Sending over SMTP is optional; a password
  over an unencrypted connection is refused, so a password is only ever sent over STARTTLS or TLS,
  and the password is kept in the credential store, never in the company file (`settings_are_checked`
  and `passwords_stay_out_of_the_company_file` in [`smtp/tests.rs`](src-tauri/src/smtp/tests.rs)).
- **A company file from someone else cannot run code.** Every connection turns off
  `trusted_schema`, so views and triggers stored in a file cannot call functions with side effects
  (`open_for` in [`db.rs`](src-tauri/src/db.rs)). A file whose `-wal` log was written for another
  page size is refused, which blocks RUSTSEC-2026-0459 in the SQLite LocalBooks bundles until it
  can be upgraded. It is checked twice: by path just before opening, for a clear message, and again
  by the VFS every company file opens through, as SQLite opens the log, through SQLite's own handle,
  so a log swapped in between is refused too (`a_log_with_another_page_size_is_refused` and
  `a_log_swapped_after_the_check_is_refused_when_sqlite_opens_it` in
  [`wal_guard.rs`](src-tauri/src/wal_guard.rs); [`vfs.rs`](src-tauri/src/file_cipher/vfs.rs)).

## Users and sensitive data

A company file can have users (Settings › Users & Passwords). Without them, anyone who can open the
file can see and change everything, as in earlier versions.

- **Sign-in:** each user's password is kept only as an Argon2id verifier (64 MiB, 3 passes:
  `PRODUCTION_COST` in [`crypto.rs`](src-tauri/src/crypto.rs)). An unknown user name does the same
  Argon2id work as a wrong password and gets the same answer (`sign_in` in
  [`users.rs`](src-tauri/src/users.rs); `signing_in_checks_the_password_and_unlocks_the_key` and
  `turned_off_users_cannot_sign_in` in [`users/tests.rs`](src-tauri/src/users/tests.rs)).
  Passwords need at least 8 characters.
- **Roles:** Admin, Standard, View only and External Accountant, checked by LocalBooks before every command runs (the
  `guarded` wrapper in [`main.rs`](src-tauri/src/main.rs) and [`permissions.rs`](src-tauri/src/permissions.rs)).
  A command not yet classified is treated as a change, so View-only users cannot run it
  (`commands_are_classified` and `every_role_gets_what_it_should` in
  [`permissions/tests.rs`](src-tauri/src/permissions/tests.rs)).
- **Sensitive data:** vendor tax IDs and vendors' bank details (routing and account numbers) are
  encrypted with AES-256-GCM using a random key for each company file, and each value is bound to
  its table, column and vendor and to that file ([`crypto.rs`](src-tauri/src/crypto.rs),
  [`tax_ids.rs`](src-tauri/src/tax_ids.rs), [`ach.rs`](src-tauri/src/ach.rs);
  `fields_are_bound_to_their_place_and_tamper_evident` and
  `another_file_s_key_cannot_read_a_field` in [`crypto/tests.rs`](src-tauri/src/crypto/tests.rs)).
  The key is never stored as is: each user allowed to see sensitive data, and the file's recovery
  key, has a copy locked with a key derived from their password (Argon2id). Users without that
  permission have no copy and cannot decrypt; they see a masked form (`***-**-6789`)
  (`users_without_sensitive_access_get_no_key` in [`users/tests.rs`](src-tauri/src/users/tests.rs),
  `protected_ids_show_only_with_the_key` in [`tax_ids/tests.rs`](src-tauri/src/tax_ids/tests.rs),
  `protected_bank_details_need_the_key` in [`ach/tests.rs`](src-tauri/src/ach/tests.rs)).
- **Recovery:** the recovery key is shown once when users are turned on, and LocalBooks keeps no
  copy. There is no reset service. With neither an Admin password nor the recovery key, the only way
  back is removing all users, which erases every encrypted value and keeps the books
  (`the_recovery_key_resets_an_admin_password` and
  `removing_all_users_keeps_the_books_and_drops_protected_values` in
  [`users/tests.rs`](src-tauri/src/users/tests.rs)).
- **Idle sign-out:** after 15 minutes without typing or clicking in any window (an Admin can change
  the limit or turn it off), measured by the wall clock, so time asleep counts. Only changes and
  window activity count as use, not screens refreshing themselves (`Db::check` in
  [`db.rs`](src-tauri/src/db.rs); `the_gate_follows_the_open_file_and_its_sign_in` in
  [`permissions/tests.rs`](src-tauri/src/permissions/tests.rs)).
- **Two-step sign-in** (optional, off by default; an Admin turns it on for the company, and can
  require it for Admins and External Accountants or for everyone): after the password, a 6-digit
  TOTP code (RFC 6238, HMAC-SHA-1, 30-second steps) from an authenticator app, or a one-time backup
  code ([`mfa.rs`](src-tauri/src/mfa.rs); `codes_match_the_rfc_6238_test_vectors` and
  `off_by_default_and_an_admin_turns_it_on` in [`mfa/tests.rs`](src-tauri/src/mfa/tests.rs)). A
  code works once on the file, and one step of clock drift either way is accepted
  (`a_code_works_once_and_one_step_of_drift_is_allowed`). The secret behind the codes is sealed with
  AES-256-GCM under a key derived by Argon2id from the user's password and bound to the user, so
  the company file alone can't make codes, and changing the password re-seals it
  (`the_secret_is_sealed_under_the_password_and_never_written_in_clear`,
  `changing_your_password_reseals_the_secret`). Backup codes are kept as salted SHA-256 hashes and
  work once (`a_backup_code_signs_in_once`). On an encrypted file the code is asked after the
  password unlocks it, and a code step left alone locks the file again
  (`the_code_is_asked_after_the_password_unlocks_the_file` in
  [`commands/mfa/tests.rs`](src-tauri/src/commands/mfa/tests.rs)); a remembered sign-in goes
  through the same step. Until the code is right
  nobody is signed in, so every other command is refused, and a company that requires it holds a
  user without one at the set-up step (`every_role_gets_what_it_should` in
  [`permissions/tests.rs`](src-tauri/src/permissions/tests.rs),
  `a_stricter_setting_makes_the_right_roles_set_it_up`). Wrong codes wait like wrong passwords
  (four free tries, counted apart from passwords) and are audited past the free tries. An Admin's
  reset, a password reset by an Admin, the recovery key, turning the setting off and removing all
  users each remove the factor (`every_way_back_removes_the_factor`). It guards the LocalBooks
  sign-in, not the file: someone with a copy of the file and another program isn't stopped by it.
- **Remember me on this computer** (optional, off by default) keeps the user's password in the
  credential store above and is used once per opening of the file, never after a sign-out or idle
  sign-out (`sign_in_remembered` in [`commands/users.rs`](src-tauri/src/commands/users.rs)).
- **Audit trail:** ledger entries record the signed-in user together with the computer's user.
  Turning users on, adding or changing a user, password resets, using or replacing the recovery key,
  two-step sign-in changes and resets, used backup codes, repeated wrong codes and removing all
  users are recorded in the same hash-chained trail (never with a password or key).
  The newest link is also recorded outside the file, so a rebuilt or shortened trail is detected
  ([`audit_chain.rs`](src-tauri/src/audit_chain.rs);
  `every_write_seals_its_audit_rows_with_an_actor` and
  `the_record_outside_the_file_catches_a_rebuilt_chain_and_truncation` in
  [`audit_chain/tests.rs`](src-tauri/src/audit_chain/tests.rs)).
- **No plain copies left in the file:** turning users on encrypts existing tax IDs and then compacts
  the file and empties its write-ahead log (`compact` in [`db.rs`](src-tauri/src/db.rs)); every
  connection sets `secure_delete`, so deleted content is overwritten from then on
  (`no_plain_tax_id_is_left_in_the_file_after_turning_users_on` in
  [`tax_ids/tests.rs`](src-tauri/src/tax_ids/tests.rs)).
- **The local AI access (MCP) sees no books** while a file with users has nobody signed in
  (`Db::readable_pool` in [`db.rs`](src-tauri/src/db.rs)).
- **Whole-file encryption** (optional, off by default, Admins turn it on): the whole company file,
  its attachments and every backup made from it are encrypted page by page (SQLCipher, AES-256)
  under a random page key. A key header in front of the database seals that key once per user
  (their password, Argon2id) and once for the recovery key. The file opens locked, and nothing in it
  is read until a password unlocks it; sign-out and idle sign-out close it again
  ([`file_cipher.rs`](src-tauri/src/file_cipher.rs), [`whole_file.rs`](src-tauri/src/file_cipher/whole_file.rs),
  [`user_slots.rs`](src-tauri/src/file_cipher/user_slots.rs); `turning_encryption_on_and_off` and
  `every_kind_of_snapshot_restores_from_an_encrypted_file` in
  [`whole_file/tests.rs`](src-tauri/src/file_cipher/whole_file/tests.rs),
  `a_locked_file_refuses_every_command_except_unlock` in
  [`user_slots/tests.rs`](src-tauri/src/file_cipher/user_slots/tests.rs)).
- **Offsite backups are encrypted** on this computer (AES-256-GCM, key from a backup password by
  Argon2id) before they are written to the folder the user picks
  ([`offsite.rs`](src-tauri/src/offsite.rs) and its tests in
  [`offsite/tests.rs`](src-tauri/src/offsite/tests.rs)).

What this does not cover:

- Unless whole-file encryption is on, only vendor tax IDs and vendors' bank details are encrypted
  inside the company file; the rest of the file, its local backups (Settings › File & Data
  Controls) and attachments are readable by anyone who has the file. Use full-disk encryption
  (FileVault, BitLocker, LUKS) on every computer that holds them. Offsite copies are always
  encrypted as a whole. Anything exported (CSV, Excel, PDF) is a plain file either way.
- With whole-file encryption on, losing every Admin password and the recovery key loses the books
  and every backup of them: there is no **Remove all users** way back for an encrypted file.
  Backups made before it was turned on stay unencrypted until deleted (Settings › Users &
  Passwords lists them). While the books are open, the key is in memory, and anyone using the
  computer sees what the signed-in user sees.
- Backups made *before* users were turned on still hold tax IDs in plain text. Delete them, or keep
  them somewhere as safe as the books themselves.
- Anyone at the sign-in screen can remove all users (the way back when no Admin password or recovery
  key is left). It erases every encrypted value and is recorded in the audit trail, but it does
  give that person the rest of the books.
- Roles are enforced by LocalBooks, not by the file: another program can read or change the
  database directly. The ledger audit trail is hash-chained and anchored outside the file, so such
  changes are detected (Accounting › Audit Log › Verify ledger integrity), not prevented.
- On Linux there is no system credential store in use: saved credentials are protected only by the
  file permissions of the user's account.
- A tax ID read from a file that is staged for import but not yet imported stays unencrypted until
  the import is carried out or discarded.
- Newer QuickBooks files encrypt tax IDs with QuickBooks' own key; LocalBooks cannot read those and
  imports the vendor without one. Files imported before LocalBooks 0.5.1 may hold that unreadable
  value; it is cleared when the file is opened (or, if already encrypted, at the next unlock).
