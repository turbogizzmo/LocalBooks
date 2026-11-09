# Reconcile an account

Reconciling checks your books against a bank or credit card statement. Once it balances,
LocalBooks locks the matched transactions so the reconciled balance can't change.

1. Go to **Banking › Reconcile** and choose the **Account**.
2. From your statement, enter the **Statement ending date** and **Ending balance**. If the
   statement shows a **Service charge** (a bank or card fee) or **Interest earned**, enter each
   one's amount, date and account (an expense account for the charge, an income account for the
   interest), and a class if you track classes. Then click **Start reconciling**. LocalBooks posts
   each as a journal entry ("Service charge" or "Interest earned") and ticks it as cleared, so you
   don't have to record it separately. Its date must be on or before the statement ending date.
3. Tick each transaction that appears on the statement. Only transactions up to the statement date
   are listed.
4. Watch **Difference**. When it reaches $0.00 it reads **✓ Balanced. You can finish now.**
5. Click **Finish now**.

The cleared transactions are now locked: they can't be voided or changed. Past reconciliations are
listed under **Reconciliation history**.

## Keep the statement with the reconciliation

While reconciling, click **Attach statement…** beside **Edit statement info** and choose the
statement file (a PDF or a picture, up to 25 MB). LocalBooks keeps a copy in the company file with
that reconciliation. Finished reconciliations show theirs in the **Statement** column of
**Reconciliation history**, where you can attach one later too. Click a file's name to save a copy,
or **×** to delete it (you can restore it from **Accounting › Attachments Library**, where the
statement files are listed with every other attachment). Cancelling a reconciliation in progress
removes its files with it.

## Undo the last reconciliation

If you finished a reconciliation by mistake (the wrong statement, or a transaction ticked that
wasn't on it), choose the account in **Banking › Reconcile** and click **Undo Last
Reconciliation…** on the newest row of **Reconciliation history**. After you confirm (and,
if you like, type why), its transactions are uncleared and can be ticked again, voided or changed,
and you can reconcile that statement again.

- Only the account's last reconciliation can be undone; undo again to step back one more.
  Cancel a reconciliation in progress first.
- Nothing is deleted. The reconciliation stays on record, and the undo is written to the
  **Audit Log** (as **Reconciliation undone**, with your reason) and to the audit trail. It
  can't itself be undone: reconcile the statement again instead.
- A service charge or interest earned entered when it started stays posted. Void it if it was
  wrong.
- Earlier reconciliations and their reports don't change.

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
