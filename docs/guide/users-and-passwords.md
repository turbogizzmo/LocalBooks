# Users, passwords and sensitive data

A new company file opens for anyone who starts LocalBooks on the computer. Turn on users when more
than one person keeps the books, or when vendor tax IDs (SSNs and EINs) need protecting: everyone then
signs in with their own password, and tax IDs are encrypted so only the people you allow can read them.

## Turn on users

1. Go to **Settings › Users & Passwords**.
2. Enter **Your user name**, optionally **Your name**, and **Your password** twice (at least 8
   characters; a short phrase is easier to remember than a code).
3. Click **Turn On Users**. You become the first **Admin**.
4. LocalBooks shows a **recovery key**: eight groups of four letters and digits. Write it down or
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
4. Tick **Can see sensitive data (vendor tax IDs)** if this person needs full tax IDs, for example
   to file 1099s. Admins always can.
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
- Tick **Remember me on this Mac** to have this Mac open the company as you next time without asking.
  Anyone who can use your Mac account could then open the books, so leave it off on a shared
  computer. **Forget on This Mac** in **Settings › Users & Passwords** turns it off again.
- **Sign Out** is at the top of **Settings › Users & Passwords**. Your name at the top right of
  every screen leads there.
- LocalBooks signs everyone out after **15 minutes** without typing or clicking in any of its
  windows, including time the Mac was asleep. An Admin can change this under **Sign out when idle**
  (or choose **Never**). Anything not yet saved is lost when it signs out.

## Forgot your password?

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
- The **1099 Summary** report shows tax IDs masked on screen and in print. Its CSV export has them in
  full for people who can see sensitive data, so keep that file safe.
- Tax IDs brought in by an import are encrypted as soon as the import finishes (or at the next
  sign-in of someone who can see sensitive data).

## What this protects, and what it doesn't

- Tax IDs are encrypted (AES-256) with a key that is itself locked by each allowed user's password
  and by the recovery key. Copies of the company file and its backups carry the encrypted values,
  and the file opens on any computer with the right password.
- Roles are enforced by LocalBooks. The rest of the company file is a regular database that another
  program could read or change; changes made outside LocalBooks show up in
  [Verify and correct the ledger](ledger-integrity.md).
- For the whole file, turn on FileVault (System Settings › Privacy & Security) so the disk is
  encrypted too.
