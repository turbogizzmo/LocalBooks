# Users, passwords and sensitive data

A new company file opens for anyone who starts LocalBooks on the computer. Turn on users when more
than one person keeps the books, or when vendor tax IDs (SSNs and EINs) need protecting: everyone then
signs in with their own password, and tax IDs and vendors' bank details are encrypted so only the
people you allow can read them.

## Turn on users

1. Go to **Settings › Users & Passwords**.
2. Enter **Your user name**, optionally **Your name**, and **Your password** twice (at least 8
   characters; a short phrase is easier to remember than a code).
3. Click **Turn On Users**. You become the first **Admin**.
4. LocalBooks says **Users are on. Save your recovery key** and shows the **recovery key**: eight groups of four letters and digits. Write it down or
   store it in a password manager, somewhere apart from this computer, tick **I have saved the
   recovery key somewhere safe** and click **Continue**.

Every tax ID already in the file is encrypted in the same step. The company file now opens on a
sign-in screen.

> **Note:** Backups made before you turned on users still contain tax IDs in plain text. Delete the
> ones you no longer need, or keep them somewhere as safe as the books themselves.

> **Tip:** The recovery key is the only way back in if every Admin forgets their password.
> LocalBooks keeps no copy, and nobody (including LocalBooks' authors) can reset it for you.

## Add a user

1. In **Settings › Users & Passwords**, click **Add User…**.
2. Enter a **User name** and optionally a **Name**.
3. Choose a **Role**:
   - **Admin**: everything, including users and passwords, company-wide settings, imports and
     restores, bank connections and AI access.
   - **Standard**: all the day-to-day bookkeeping, but not those Admin areas.
   - **View only**: looks at the books and runs reports; changes nothing.
   - **External Accountant**: for your outside accountant. Works like Standard, and may also post
     and void entries dated on or before the [closing date](closing-the-books.md), for example
     adjusting entries for a year you've closed. Never sees sensitive data. Everything they post in
     a closed period is listed, with who did it, in **Reports › Closing Date Exceptions**.
4. Tick **Can see sensitive data (vendor tax IDs and bank details)** if this person needs full tax IDs, for example
   to file 1099s, or needs to see and change vendors' bank details and pay bills by ACH file. Admins
   always can; External Accountants never can.
5. Enter a **Temporary password** twice and click **Add User**. Tell the user the password; they
   choose their own the first time they sign in.

## Change, reset or turn off a user

Click **Edit…** next to the user.

- **Set a new temporary password** resets a forgotten password. The user picks a new one at their
  next sign-in.
- Ticking **Can see sensitive data** for someone who didn't have it also needs a temporary password:
  the key to the tax IDs can only be stored for a password.
- Untick **User is turned on (can sign in)** when someone leaves. Users are never deleted, so the
  audit trail can always show who did what.

LocalBooks always keeps at least one active Admin, and you can't change your own role or turn
yourself off: another Admin has to.

## Sign in and sign out

- The sign-in screen appears whenever a company file with users is opened.
- Tick **Remember me on this computer** to have this computer open the company as you next time
  without asking. Anyone who can use your account on this computer could then open the books, so
  leave it off on a shared computer. **Forget on This Computer** in **Settings › Users & Passwords**
  turns it off again. The password is kept in the Mac's Keychain or Windows Credential Manager (on
  Linux, in a file only your account can read), never in the company file.
- **Sign Out** is at the top of **Settings › Users & Passwords**. Your name at the top right of
  every screen leads there.
- LocalBooks signs everyone out after **15 minutes** without typing or clicking in any of its
  windows, including time the computer was asleep. An Admin can change this under **Sign out when idle**
  (or choose **Never**). Anything not yet saved is lost when it signs out.

## Two-step sign-in

Two-step sign-in is **optional and off by default**. When it's on for you, signing in asks for your
password and then a 6-digit code from an authenticator app on your phone (any app that scans a QR
code for one-time codes). Someone who learns your password still can't sign in to LocalBooks
without your phone.

**Turn it on for the company (Admin).** In **Settings › Users & Passwords › Two-step sign-in**,
choose:

- **Off** (the default): nobody is asked for a code. Choosing Off later removes everyone's
  authenticator and backup codes.
- **Optional**: each user may set it up; nobody has to.
- **Required for Admins and External Accountants**: they set it up at their next sign-in and can't
  do anything else until they have. Other users may.
- **Required for everyone**: every user sets it up at their next sign-in.

An Admin held at the set-up step without a phone at hand can click **Make two-step sign-in
optional for this company instead** there.

**Set it up for yourself.** In the same section click **Set Up…**, enter your password, scan the QR
code with your authenticator app (or type the key shown under it), and enter the code the app now
shows. LocalBooks draws the QR code on your computer; nothing is sent anywhere. You then get **ten
backup codes**, shown only once: save them somewhere apart from your phone. Each one signs you in
once in place of a code. **Set Up Again…** replaces your authenticator and backup codes;
**Turn Off…** removes it, unless the company requires it for you.

**Signing in.** After your password, enter the code your app shows for LocalBooks, or click **Use a
backup code**. A code works once. A phone clock up to 30 seconds off is fine. **Remember me on this
computer** fills in your password but still asks for the code, and so does signing in again after
an idle sign-out. Wrong codes wait the same way as wrong passwords: four free tries, then 30
seconds, 1, 2 and at most 5 minutes. Past the free tries, wrong codes are recorded in the audit
trail.

**Lost your phone?**

- Use one of your backup codes.
- Ask an Admin: **Edit…** your user and click **Reset Two-Step Sign-In…**. You sign in with your
  password and set it up again (at once, if the company requires it).
- A temporary password from an Admin resets two-step sign-in too: your authenticator's secret is
  locked with your old password, which the Admin doesn't know.
- The only Admin, with no backup code: **I forgot my password › Use the recovery key** sets a new
  password and turns off your two-step sign-in.

**What it protects.** Two-step sign-in stops someone who has a password but not the phone from
signing in to LocalBooks. It doesn't stop someone who copies the company file and reads it with
another program, so keep full-disk encryption (or whole-file encryption) on. The authenticator's
secret is kept in the company file locked with your password (AES-256, the key made from your
password by Argon2id), so the file alone can't produce codes; changing your password re-locks it.
On an encrypted company file your password opens the file first, then LocalBooks asks for the code.

### Touch ID or Windows Hello on this computer

Once you have an authenticator app set up, you can also let **this computer** confirm it's you with
**Touch ID** (on a Mac) or **Windows Hello** (fingerprint, face or PIN on Windows) in place of typing
the code. The app stays: other computers, and copies of the company file, still ask for its code.

- **Add this computer.** In **Settings › Users & Passwords › Two-step sign-in**, click **Use Touch ID
  on This Computer…** (or **Use Windows Hello on This Computer…**), enter your password and confirm
  with Touch ID or Windows Hello. The list under it shows each computer added for you, when it was
  last used, and which one you're on.
- **Signing in.** After your password, click **Use Touch ID** (or **Use Windows Hello**) on the code
  step. The code box and backup codes still work there too.
- **Remove a computer** with **Remove** beside it in that list, for example a laptop you sold. Your
  authenticator app stays. Anything that removes the app also removes every computer: turning
  two-step sign-in off, an Admin's reset, a temporary password from an Admin, the recovery key and
  the company setting going to Off. **Set Up Again…** for the app keeps your computers.
- **How it works.** Adding the computer saves a random secret in the computer's own credential
  store (the macOS Keychain or Windows Credential Manager); the company file keeps only a one-way
  fingerprint of it. Touch ID or Windows Hello is asked before LocalBooks reads the secret. No
  fingerprint, face or PIN ever reaches LocalBooks: the system checks them.
- **What it's worth.** Touch ID and Windows Hello let the computer's own password or PIN stand in
  for a fingerprint, so on this computer the second step is as strong as the computer's sign-in.
  Someone who knows both your LocalBooks password and your computer's password can sign in here.
  If that matters for you, don't add the computer, and keep using the app's code.
- **Not on Linux.** Linux has no common fingerprint or face check that LocalBooks can use, so the
  option isn't offered there; use the authenticator app. It also needs the computer's check to be
  set up: Touch ID (or a Mac password) in System Settings, or Windows Hello in Windows Settings.

## Forgot your password?

After four wrong passwords for a user name, LocalBooks makes each next try wait: 30 seconds, then 1,
2 and at most 5 minutes. It never locks anyone out for longer, so the only Admin can't be shut out by
someone else's guessing, and other user names aren't affected. Wrong recovery keys wait the same
way, and so does a wrong current password under **Change Password**. A right password clears the
count, and the wait lasts across restarts.

Click **I forgot my password** on the sign-in screen.

- **Not an Admin:** ask an Admin to set a temporary password for you.
- **An Admin:** another Admin can do the same. Otherwise click **Use the recovery key**, enter the
  **Recovery key**, your **Admin user name** and a new password, and click **Reset Password and Sign
  In**.
- **No Admin password and no recovery key:** click **No Admin password and no recovery key?**. Typing
  **REMOVE ALL USERS** removes every user and erases every encrypted tax ID for good. Transactions,
  lists and reports are kept, and the file opens without signing in again, as before users were
  turned on. This is recorded in the audit trail, with the names of the users removed.

An Admin can issue a new recovery key at any time with **New Recovery Key…**; the old one stops
working.

## Tax IDs

- On a vendor's record the tax ID shows as `***-**-6789`. People who can see sensitive data click
  **Show** to see or change it; everyone else sees only the last four digits and can't change it.
- The **1099 Summary** report shows tax IDs masked on screen and in print. Its Excel and CSV exports
  have them in full for people who can see sensitive data, so keep those files safe.
- Tax IDs brought in by an import are encrypted as soon as the import finishes (or at the next
  sign-in of someone who can see sensitive data).
- Newer QuickBooks files keep tax IDs encrypted with QuickBooks' own key, which LocalBooks can't
  read. Those vendors come in without a tax ID, and an unreadable value left by an earlier import is
  removed automatically. Enter the ID again from the vendor's W-9.

## Encrypt the whole company file

Off unless an Admin turns it on. With it on, the whole company file, its attachments and every
backup made from it are encrypted, so a copy that leaves your computer (a lost laptop, a backup
drive, a synced folder) shows nothing readable without a user's password or the recovery key.

> **Warning:** With it on, if every Admin forgets their password and the recovery key is lost,
> nobody can open these books or any backup of them: not you, not LocalBooks' authors. Without it,
> **Remove all users** can still get you back in.

1. Sign in as an Admin and go to **Settings › Users & Passwords**.
2. Under **Encrypt the whole company file**, tick **I understand that losing every Admin password
   and the recovery key means losing these books**.
3. Enter **Your password** again, and a **temporary password** for each other user. They choose
   their own the next time they sign in.
4. Click **Encrypt Company File**. LocalBooks makes an encrypted copy, checks it against your books,
   and switches to it.
5. Under **The company file is encrypted. Save the new recovery key**, save the key it shows (write
   it down or store it in a password manager), then tick that you've saved it. The recovery key you
   had before no longer opens this file, and this one is shown only once.

The company file then opens on the sign-in screen and nothing in it can be read until someone signs
in. Signing out, or being signed out when idle, closes it again.

- **Backups made before** you turned it on are still unencrypted. Users & Passwords lists them:
  click **Delete Them…** once you no longer need them. Copies in a synced folder's history or on
  other drives aren't affected.
- **Backups made since** are encrypted with the same passwords, and restore as usual. A backup of
  another encrypted company can't be restored into this one: open it with **File › Open Company
  File** and sign in to it.
- **Turning a user off** removes their access to the encrypted file. Turning them back on needs a
  new temporary password.
- **To turn it off,** enter your password under **Turn Off Encryption…**. The file goes back to an
  ordinary, unencrypted one.

## What this protects, and what it doesn't

- Tax IDs are encrypted (AES-256) with a key that is itself locked by each allowed user's password
  and by the recovery key. Copies of the company file and its backups carry the encrypted values,
  and the file opens on any computer with the right password.
- Roles are enforced by LocalBooks. The rest of the company file is a regular database that another
  program could read or change; changes made outside LocalBooks show up in
  [Verify and correct the ledger](ledger-integrity.md).
- Unless the whole company file is encrypted (above), vendor tax IDs and bank details are the only
  values encrypted inside it. The rest of the file, its local backups and its attachments are
  readable by anyone who has them, so turn on whole-file encryption, or full-disk encryption on every
  computer that holds them: FileVault on a Mac (System Settings ›
  Privacy & Security), BitLocker or Device Encryption on Windows. Offsite backups are encrypted as a
  whole.
- [SECURITY.md](https://github.com/turbogizzmo/LocalBooks/blob/main/SECURITY.md) lists every security claim with the code and test behind it.

### Copies outside the company file

- **Exports are plain copies.** A report, list or register saved as CSV, Excel or PDF, a form or
  voucher saved as PDF, and attachments saved to a folder or zip are ordinary files: the company
  file's password doesn't protect them. LocalBooks says so each time it saves one. Keep them
  somewhere safe, or delete them when you're done.
- **Temporary copies are deleted.** A PDF handed to your mail app is kept in LocalBooks' cache
  folder only long enough for the mail app to read it: it's deleted after an hour, when you next
  send something, start LocalBooks or quit it. A QuickBooks backup unpacked to be read is deleted
  as soon as the import has read it (one left behind by a crash goes at the next start). Imports
  staged for review are kept inside the company file, not beside it.
