# Close a period

Setting a closing date locks the books through that date. Nothing dated on or before it can be
posted or voided from any screen, so filed numbers can't change by accident.

## Close the books

1. Go to **Settings › Preferences Console**, click **Accounting**, then **Company Preferences**, and scroll to **Closing date**.
2. Enter the **Closing date**, usually the last day of the period you've filed or finalized.
3. Optionally enter a **Passcode (optional)** of at least 4 characters. From then on, moving or
   removing the date needs it.
4. Click **Close books**.

The box shows **Closed through** the date, and the change is added to the history below it.

To shrink a file with years of closed history, see
[Condense data](backups-and-company-files.md#condense-data-archive-closed-years).

## Reopen or move the date

1. Enter the new **Closing date**, or click **Reopen all periods** to remove the lock.
2. If the books are protected, type the **Current passcode**. You can set a different one in
   **New passcode (optional)**.
3. Click **Close books** (or **Reopen all periods**).

> **Tip:** Keep the passcode with your accountant. It is stored securely and cannot be recovered;
> without it the closing date can't be moved.

## Work with your accountant at the same time (Accountant's Copy)

When your accountant reviews a period (the year to December 31, say), you don't have to stop
working. In **Settings › File & Data Controls › Accountant's Copy** (Admins):

1. Choose the **dividing date**, the last day of the period they review, and click **Create
   Accountant's Copy…**. Save the copy and send it to them however you like.
2. Keep working. While the copy is out, nothing can be posted **on or before** the dividing date in
   your books (the accountant owns that period); everything after it works as usual.
3. Your accountant opens the copy in LocalBooks. It says it's an Accountant's Copy and shows the
   dividing date; they can post journal entries (adjusting or not) and add accounts **on or before**
   that date, and nothing after it. When they're done, **Export Changes for Client…** saves a changes
   file to send you.
4. Click **Import Accountant's Changes…** and choose the file. LocalBooks checks it came from the copy
   that's out and lists the new accounts and journal entries. **Apply the changes** adds the
   accounts and posts the entries (each memo says it came from the accountant), all or nothing, and
   ends the copy: the period opens again.

The list under it is the record of each copy: when it was made and by whom, and when its changes
were imported (or the copy cancelled), by whom, and how many entries came back. The entries are in
the audit log like any posting.

Only **journal entries and new accounts** come back. Anything else the accountant does in the copy
(invoices, bills, reconciliations, changes to existing accounts, voiding your entries) stays in the
copy: the export lists what it left out so they can tell you. **Cancel the copy** ends it without
importing anything; its changes are then refused.

## Let your accountant work in a closed period

Add your accountant as an **External Accountant** user (see
[Users and passwords](users-and-passwords.md#add-a-user)). They can post and void entries dated
on or before the closing date without moving it; everyone else still can't.

## Closing Date Exceptions

Under the closing date, **Closing Date Exceptions** lists everything posted or voided in a period
that was already closed at the time: when, by whom, the entry's date, type, memo and amount, and
the closing date then in force. Entries made before a closing date was set aren't listed.
