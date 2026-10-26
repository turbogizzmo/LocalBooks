# Journal entries and voiding

## Post a manual journal entry

Use a journal entry for adjustments that no form covers, such as depreciation, accruals or opening
balances.

1. Go to **Accounting › Journal Entries** and click **New journal entry** (or **+ New › Journal
   Entry**).
2. Set the **Journal date** and an optional **Memo**.
3. Add at least two lines. On each, choose the account and enter a debit or a credit.
4. Total debits must equal total credits. LocalBooks won't post an entry that doesn't balance.
5. Tick **Adjusting entry** if this is a period-end adjustment (depreciation, an accrual, a correction
   from your accountant).
6. Click **Save & Post**.

Adjusting entries are tagged **Adjusting** in the list; choose **Adjusting entries** in the filter
to see only them. **Reports › Accountant & Taxes › Adjusting Journal Entries** lists them with their
lines (see [Accountant & Taxes reports](reports.md#adjusting-journal-entries)). Editing an entry
keeps its mark unless you untick it.

## Undo a transaction (void)

Posted transactions are never deleted or edited. Voiding posts an exact reversing entry instead, so
the history stays complete.

1. Find the transaction on its screen (Invoices, Bills, Expenses & Checks, Deposits & Transfers or
   Journal Entries).
2. Open the row's **Actions** menu and choose **Void Transaction**, then confirm.

Both the original and its reversal appear in **Journal Entries**, marked **Voided by #…** and
**Void reversal**. Choose **Voids and reversals** in the filter to list them.

A void is refused when:

- the transaction is reconciled, or ticked in a reconciliation in progress
  ([Reconcile an account](reconciling.md));
- it's dated on or before the closing date ([Close a period](closing-the-books.md));
- an invoice or bill still has payments (void the payments first), or a payment is already part of a
  bank deposit (void the deposit first).
