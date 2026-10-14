# Verify and correct the ledger

LocalBooks saves a snapshot of every entry at the moment it's posted. The integrity check compares
your books with those snapshots, which catches anything changed or deleted outside the app (for
example by editing the company file with another tool).

## Run the check

1. Go to **Accounting › Audit Log** and click **Verify ledger integrity**.
2. A green message means every posted entry matches its snapshot, every void links to its reversal,
   and debits equal credits.
3. Otherwise each problem is listed by entry number, such as "Differs from its posting snapshot" or
   "Snapshotted at posting but no longer in the ledger (deleted)".

## Review corrections

1. Under the list of problems, click **Review corrections…**. Each affected entry shows one of:
   - **Correctable**: a balanced correcting entry would restore every account to the amount
     recorded at posting. Its lines are shown so you can check them.
   - **Restore a backup**: the change left debits and credits unequal. No entry can repair that;
     restore a backup taken before the change ([Back up, restore and open company files](backups-and-company-files.md)).
   - **Review entry**: only a detail such as the date or memo changed; balances are unaffected.
   - **Corrected**: already fixed by an earlier correction.
2. For a correctable entry, check the lines and click **Post correction**, then confirm.

The correction is dated today, follows the closing date like any entry, and is recorded in the audit
trail. The altered rows stay in the books as evidence.

> **Tip:** Nothing posts automatically. The snapshots live in the same file as your books and could
> also have been altered, so check every proposed correction before posting it.
