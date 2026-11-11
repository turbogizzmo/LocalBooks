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

**Edit…** and **Duplicate** in a row's menu are offered only for entries typed in Make General
Journal Entries. Other screens also post journal entries: customer refunds and write-offs, vendor
refunds, bounced checks, sales tax payments, class reclassifications, item receipts and currency
revaluations. Those are changed or voided from the screen that made them (their row offers only
**Void**), and the **Manual journal entries** filter leaves them out.

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

## Find a transaction

For a quick lookup, type in the search box at the top of the window (**⌘K** or **Ctrl+K**). It
matches numbers, names, memos, references and exact amounts.

For more, choose **Advanced search…** at the bottom of its results, or go to **Accounting › Find
Transactions**. Combine any of:

- **Name, memo or number contains**: text anywhere in the name, the number or a line's memo.
- **Dates**: a period such as *This month* or *Last fiscal year*, or your own From and To.
- **Number**: the exact invoice, check, bill or reference number (a leading `#` is ignored).
- **Amount**: equal to, at least, at most or between, compared with each transaction's total.
- **Transaction types**, **Customers and vendors**, and **Accounts** (an account includes its
  subaccounts).
- **Include voided**: voided transactions and their reversals are left out unless this is ticked.

The list shows one row per transaction with how many match and their total. Click a column heading
to sort and a row to open the transaction. The first 500 matches are listed, newest first; narrow
the filters to see older ones.

## Reclassify transactions

Use **Reclassify Transactions** to move income and expense lines that are already posted to
another class, many at once: for example, when a month of expenses went to the wrong department.
It changes classes only. To move an amount to another account, post a journal entry (see [Post a
manual journal entry](#post-a-manual-journal-entry)).

1. Go to **Accounting › Reclassify Transactions** (or **Company › Reclassify Transactions…**).
2. Narrow the list:
   - **From** and **To**: the transaction dates. The default is this year to date.
   - **Account**: one income or expense account, or **All income and expense accounts** (the
     default).
   - **Current class**: **Any class** (the default), one class (its subclasses are included), or
     **No class** for lines that have none.
3. The list shows each matching line with its **Date**, **Type**, **No.**, **Account**, **Memo**,
   current **Class** and **Amount** (income and expenses both as positive amounts). Tick the lines
   to move, or tick the box in the heading to tick them all. Clicking a row ticks it too.
4. In **Move ticked lines to**, choose the new class, or **—** for no class. Only active classes
   are offered; **＋ Add new class…** adds one.
5. Click **Reclassify**.

Posted lines are never edited. Instead, LocalBooks posts a reclassifying journal entry for each
date among the ticked lines, with the memo *Reclassify to* and the class name. For each line it
takes the amount out of the old class and puts it into the new one, on the same account, so
account balances and totals don't change; only reports by class do. The list then shows the lines
in their new class, and you can move them again later. Lines already in the chosen class are
skipped.

To undo a move, void its reclassifying entry in **Accounting › Journal Entries** (see [Undo a
transaction (void)](#undo-a-transaction-void)). The lines go back to the class they had before.

Some lines aren't listed and can't be moved: voided transactions, void reversals, and the
reclassifying entries themselves.

All the ticked lines move together, or none do. A reclassifying entry is dated on its lines' own
date, so lines dated on or before the closing date can't be moved unless you're allowed to post in
a closed period (see [Close a period](closing-the-books.md#let-your-accountant-work-in-a-closed-period)).
Untick those lines, or reopen the period first.
