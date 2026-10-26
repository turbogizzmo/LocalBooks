# Reconcile an account

Reconciling checks your books against a bank or credit card statement. Once it balances,
LocalBooks locks the matched transactions so the reconciled balance can't change.

1. Go to **Banking › Reconcile** and choose the **Account**.
2. From your statement, enter the **Statement ending date** and **Ending balance**, then click
   **Start reconciling**.
3. Tick each transaction that appears on the statement. Only transactions up to the statement date
   are listed.
4. Watch **Difference**. When it reaches $0.00 it reads **✓ Balanced. You can finish now.**
5. Click **Finish now**.

The cleared transactions are now locked: they can't be voided or changed. Past reconciliations are
listed under **Reconciliation history**.

## Reprint a past reconciliation

Go to **Reports › Accountant & Taxes › Previous Reconciliation**, choose the account and the
statement, and pick **Detail** or **Summary**. Detail lists every cleared transaction and those still
outstanding on the statement date (or cleared on a later statement); Summary shows the totals. See
[Accountant & Taxes reports](reports.md#previous-reconciliation).

## Check for changes after reconciling

**Reports › Accountant & Taxes › Reconciliation Discrepancy** shows cleared transactions that were
changed, voided or deleted after their reconciliation was finished. When you finish a reconciliation,
LocalBooks keeps a copy of each cleared transaction (its date, account and amount). The report
compares those copies with the books and lists anything:

- **Deleted**, **Voided** or **Moved to another account**;
- **No longer cleared**;
- with a changed amount (**Amount changed**) or date (**Date changed to …**);
- cleared into the reconciliation afterwards.

It also checks that each statement begins where the one before it ended.

LocalBooks blocks all of these changes itself, so the report is normally empty. It is there to catch
a company file changed by another program.

Reconciliations finished with an earlier version of LocalBooks, before it kept these copies, can't
be checked line by line: the report checks that their cleared transactions still add up to the statement and
says how many were checked this way. **Previous Reconciliation** works for every reconciliation,
old or new.

## If the difference won't reach $0.00

- Look for a transaction on the statement that isn't in LocalBooks yet (bank fees and interest are
  common). Record it, then come back.
- Check that you didn't tick something that isn't on this statement.
- Use **Edit statement info** if you typed the ending date or balance wrong.
- **Cancel reconciliation** discards the ticks so you can start again. Nothing is locked until you
  finish.
