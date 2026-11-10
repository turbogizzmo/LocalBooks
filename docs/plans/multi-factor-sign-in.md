# Multi-factor sign-in (R26)

Status: **design, not built.** Gap R26 in [`docs/gaps.md`](../gaps.md). The FTC Safeguards Rule guide
([`docs/guide/safeguards-rule.md`](../guide/safeguards-rule.md), § 314.4(c)(5)) names this as the
main limit LocalBooks has today.

## The goal

R26's acceptance test: a user can add a second factor (an authenticator app, or a platform
authenticator such as Touch ID), LocalBooks asks for it at sign-in, and the recovery key still works
when the factor is lost.

The Safeguards Rule defines multi-factor authentication as checking at least two of: something you
know (a password), something you have (a token or phone), something you are (a fingerprint). Today a
LocalBooks user proves only the first.

## What exists today

- **Sign-in** ([`users.rs`](../../src-tauri/src/users.rs) `sign_in`): a user name and password,
  checked against an Argon2id verifier. An unknown name does the same work as a wrong password.
  Users who may see sensitive data also unlock their key slot with the same password.
- **Roles:** Admin, Standard, View only and, with #302, External Accountant (can post in closed
  periods, never holds the data key).
- **Recovery:** an Admin sets a temporary password for another user. The recovery key resets an
  Admin's password. With no Admin password and no recovery key, **Remove all users** keeps the
  books and erases the encrypted values.
- **Remember me on this computer** keeps the password in the credential store and signs in once per
  opening of the file.
- **Idle sign-out** after 15 minutes (configurable, or off).
- **User changes** go into the hash-chained audit trail as `USER_CHANGE` events, never with a
  password or key.

## What a second factor can and can't protect, on one computer

LocalBooks has no server. Everything that checks a second factor runs on the same computer, from
data in the company file. That sets hard limits, and the design should say them plainly rather than
promise what a cloud service promises.

**It protects** the LocalBooks front door: someone at the computer (or using a remembered sign-in, or
an External Accountant's shared password) who knows or guesses a password but doesn't have the
user's phone can't sign in through LocalBooks.

**It does not protect:**

- **The file itself.** Roles and sign-in are enforced by LocalBooks, not by the file. Someone with a
  copy of the company file can read it with another program, or edit the users table to switch the
  factor off; the audit trail detects that afterwards, it doesn't prevent it. Whole-file encryption
  (R1) closes this for an encrypted file, because its key slots need the password, but a TOTP code
  can't be part of a key: it changes every 30 seconds, and the secret behind it has to be readable
  by LocalBooks to check it. So a second factor never makes an encrypted file harder to open for
  someone who already has the password.
- **A compromised computer.** Malware running as the user while the books are open sees what the
  user sees.
- **The phone and the computer being the same device**, for a user who keeps the authenticator app
  on the computer itself.

What it adds for the Safeguards Rule: LocalBooks sign-in becomes multi-factor for the people the
rule is about (staff and outside accountants using the books). The Qualified Individual still has
to cover the file itself with full-disk encryption or R1, as the guide already says.

## Options

### A. TOTP with an authenticator app (recommended first)

Time-based one-time passwords (RFC 6238): a shared secret, enrolled by scanning a QR code into any
authenticator app (phone or password manager), and a 6-digit code that changes every 30 seconds.

- Works offline on every platform, needs nothing from the operating system, and every
  authenticator app supports it.
- The QR code is drawn by LocalBooks itself; nothing leaves the computer.
- It is a real "something you have" when the app is on a phone.

### B. A platform authenticator (Touch ID, Windows Hello)

The operating system checks a fingerprint, face or PIN. Native APIs (LocalAuthentication on macOS,
Windows Hello on Windows) are reachable from Tauri through a small plugin; WebAuthn passkeys in the
webview are uneven across the three platforms' webviews.

- Convenient, but bound to **this computer**: a file opened on another computer, or by an External
  Accountant on their own machine, can't use it, and Linux has no common API.
- Against the threat that matters most locally (someone at this computer with the password), a
  fingerprint is strong; against someone who knows the computer's login PIN, Windows Hello's PIN
  fallback adds little.

Later, as an alternative factor alongside A, not instead of it.

### C. A hardware key (FIDO2 / YubiKey)

Strongest, and with the `hmac-secret` extension it could even feed a key slot. But it needs USB HID
access from the app, a key per person, and a recovery story for lost keys. Out of scope until users
ask.

### D. Binding a factor to this computer's credential store

A secret in the Keychain or Credential Manager mixed into the sign-in: the file then only opens on
computers the user enrolled. It breaks moving the file, restoring a backup on a new computer, and the
External Accountant working on a copy. Rejected.

## Design (option A)

### Enrolment

1. **Settings › Users & Passwords › Two-step sign-in › Set up…** for the signed-in user. LocalBooks
   asks for the user's password again.
2. It makes a 160-bit random secret, shows it as a QR code (`otpauth://totp/LocalBooks:<user>?secret=…
   &issuer=LocalBooks`) and as text, and asks for one code from the app to prove it was saved.
3. It shows **ten one-time backup codes** once, like the recovery key, with the same "I have saved
   them" step.
4. Recorded in the audit trail as `MFA_ENROLLED` (no secret, no codes).

### Storage (a new migration)

A `user_factors` table, one row per user and factor, `STRICT`:

- `user_id`, `kind` (`TOTP`), `enrolled_at`;
- the TOTP secret **sealed with a key derived from the user's password** (AES-256-GCM, the same
  sealed format as `key_slots`). What that buys: the file alone, without the password, can't be
  used to generate codes. It does not protect against someone who has the file and the password
  (they don't need LocalBooks at all). The factor key comes from the same Argon2id run that checks
  the password (a second output via HKDF with its own label and salt), so sign-in doesn't pay for
  another 64 MiB derivation;
- `last_step`, the last 30-second step accepted, so a code can't be used twice **on this file**. A
  restored backup or a second copy of the file has its own `last_step`, so a code could be reused
  against another copy within its 90-second window; that is accepted, since the copy already gives
  its holder the books;
- the backup codes as salted SHA-256 hashes (each code has 50 bits of randomness, so a slow hash
  adds nothing), each with a `used_at`.

A `mfa_policy` company preference: `OFF` (default), `ADMINS` (Admins and External Accountants must
enrol), `ALL`.

### Sign-in

1. User name and password, as today. A wrong password stops here, with the same answer and timing
   as now. Asking for the code only after a right password tells someone at the sign-in screen
   that their guess was right, so **passwords are throttled too** (the same growing delay after
   five wrong tries, per user name), which today they aren't. Asking for the code first instead
   was considered: it hides nothing, because the code check needs the secret, which is sealed
   under the password.
2. If the user has a factor: **Enter the 6-digit code from your authenticator app** (or **Use a
   backup code**). LocalBooks derives the factor key from the password it just checked, opens the
   secret and accepts the current step or one step either side (clock drift), never a step at or
   before `last_step`.
3. Wrong codes: five tries, then a growing delay (30 s, 1 min, 5 min) kept in memory and in the
   file (a restored backup starts the count again). After the fifth, an `MFA_FAILED` event goes
   into the audit trail so it shows guessing. Nobody is signed in yet, so its actor is the user
   name tried plus the computer's user, the same "<user> as <os user>" form ledger rows use, and
   it is the only write the sign-in screen makes.
4. If the policy requires a factor and the user has none, they sign in and go straight to
   enrolment; nothing else works until they finish, like `must_change_password` today.

### Remember me, idle sign-out

- **Remember me on this computer** still fills in the password, but **always asks for the code**
  when the user has a factor. Otherwise a remembered password would skip the second factor
  entirely.
- **Idle sign-out** asks for the password and the code again. A short grace (sign back in within 5
  minutes without the code) was considered and rejected: it is exactly the walk-up case MFA is for.

### Recovery

- **Lost phone, other Admins exist:** an Admin chooses **Reset two-step sign-in** for that user,
  which removes the factor and the backup codes. The user signs in with their password and enrols
  again (forced if the policy requires it). Audit: `MFA_RESET` with the Admin's name.
- **Lost phone, the user is the only Admin:** a backup code signs in once. Without one, the
  existing **Use the recovery key** path resets the Admin's password **and** removes their factor,
  so the recovery key stays the way back for everything, as R26's acceptance test asks.
- **Changing your own password** re-seals the factor under the new password in the same
  transaction, as the key slot is re-sealed today; the old password and the current secret are
  both at hand there.
- **A temporary password set by an Admin:** the secret is sealed under the user's old password,
  which the Admin doesn't know, so it can't be re-sealed. A password reset **also resets the
  factor**, and the user enrols again at their next sign-in. The reset screen says so.
- **Remove all users** removes every factor with the users.

### External Accountant (#302)

- The role most likely to use the books from someone else's computer or with a password the
  business set up, so `ADMINS` policy includes it.
- It never holds the data key; its factor is sealed under its own password like any user's, so
  nothing about the data key changes.
- When an accountant works on a copy of the file (a future Accountant's Copy, R14), TOTP moves with
  the file, unlike options B and D. That is the main reason TOTP comes first.

### Whole-file encryption (R1)

Independent of it. An encrypted file opens with the password (page-key slot) before LocalBooks can
read `user_factors`, then asks for the code. The code never feeds the page key, for the reasons
under "What a second factor can and can't protect". With R1 on, editing `user_factors` from outside
LocalBooks needs the page key, so switching the factor off by editing the file needs the password
too.

### What the user is told

The Users & Passwords screen and the guide say in plain words: two-step sign-in stops someone who
has a password but not the phone from signing in to LocalBooks; it doesn't stop someone who copies
the company file, so keep full-disk encryption (or whole-file encryption) on. The Safeguards guide's
(c)(5) entry changes from a limit to a feature with that caveat.

## Building it

One pull request after R1 phase 3 (both touch sign-in):

- Rust: `sha1` (RustCrypto, for RFC 6238 compatibility with every app; `hmac` is already a
  dependency) and a QR encoder (`qrcode`, no network). TOTP against the RFC 6238 test vectors.
- Migration for `user_factors` and the policy preference; pinned checksum.
- `users.rs`: enrolment, the second step of `sign_in`, resets; `permissions.rs`: the new commands
  (enrolling is any signed-in user; resetting another user's factor is Admin).
- Screens: the code step on the sign-in screen, enrolment with QR and backup codes, the policy in
  Users & Passwords.
- Tests: RFC vectors; a code is refused twice; drift of one step accepted, two refused; backup codes
  work once; remembered sign-in still asks for the code; the recovery key removes the factor; Remove
  all users removes factors; an External Accountant enrols and signs in; failures are throttled and
  audited.
- Docs: users guide, Help, `SECURITY.md` (with code and test links), the Safeguards guide, the
  CHANGELOG.
