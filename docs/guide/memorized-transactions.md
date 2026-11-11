# Memorized (recurring) transactions

A memorized transaction is a saved copy of a journal entry, expense or check, bill or invoice, with
a schedule. When its date comes round, Tillhouse reminds you, and you enter it with one click. It
posts nothing until you enter it, and each one you enter posts like the form it came from.

## Memorize a transaction

1. Fill in a journal entry, an expense or check, a bill or an invoice as usual.
2. Click **Memorize** in the form's toolbar, or **Memorize…** next to the save buttons (on journal
   entries, bills, expenses and checks). You can also press **Ctrl+M** or choose **Edit › Memorize
   Transaction**. The form must be complete enough to post: a journal entry must balance, for
   example.
3. In the **Memorize** window (titled *Memorize bill*, *Memorize invoice* and so on):
   - **Name**: how it shows in the list. It starts as the memo (journal entry), payee (expense or
     check), vendor (bill) or customer (invoice). Each name can be used once.
   - **Repeat every**: a number and **Week**, **Month**, **Quarter** or **Year**. The default is
     every 1 month.
   - **Next date**: the date of the first copy to enter. It starts one period after the form's
     date, since you'll usually post this one now.
   - **End date (optional)**: the last date a copy can be entered. Leave it blank to repeat with no
     end.
4. Click **Memorize**.

Memorizing doesn't post the form. To post this one as well, click the form's save button as
usual.

What is saved is everything on the form except its date: accounts, amounts, lines, classes,
customer or vendor, terms and memo. Two things are left out so each copy gets its own:

- a bill's **Bill no.** and a check's number are blank on every copy;
- invoices are numbered when each copy posts.

A bill's or invoice's due date stays the same number of days after its date as on the form.

## The list

Go to **Accounting › Recurring Transactions** (or **Lists › Memorized Transaction List**,
**Ctrl+T** or **⌘T**). Each row shows:

- **Name**, **Type** (Journal entry, Expense / check, Bill or Invoice) and **Payee / customer**;
- **Schedule**, such as *Every month* or *Every 2 weeks*;
- **Next date** and **End**;
- **Last entered**: the latest date entered and, in brackets, how many copies have been entered
  (**Never** if none);
- **Amount**: the transaction's total.

A **Due** tag marks a transaction whose next date has come. A **Paused** tag marks one you
paused, or one that has passed its end date. Click a column heading to sort.

## Enter what is due

Tillhouse never enters a memorized transaction by itself. You choose when:

1. Go to **Accounting › Recurring Transactions**.
2. **Enter transactions due through** is today's date. Change it to look further ahead (for
   example, to enter next week's bills now).
3. Click **Enter all due** (it shows how many are due). To enter just one, open its row's **▾**
   menu and choose **Enter now (through date above)**.

Each copy is dated its own scheduled date, not today. If a transaction is behind by several
periods, one copy is entered for each missed date (up to 120 at a time). Each copy posts through
the same path as the form it came from, so it shows on its own screen (Journal Entries, Expenses
& Checks, Bills or Invoices) and in your reports like any other. The next date then moves along
the schedule.

If a copy can't post, for example because its date is on or before the [closing
date](closing-the-books.md#close-the-books), that copy is left out and listed under **Not entered**
with the reason, and the others are still entered. Its memorized transaction stays due on that
date, so nothing is entered twice: fix the cause, or move its next date with **Edit schedule…**
(see below), and enter again.

Monthly, quarterly and yearly schedules keep their day of the month. When that day falls past the
end of a shorter month, the copy is dated the last day of that month, and the next one goes back
to the schedule's day: a transaction due on January 31 is entered on January 31, February 28,
March 31, April 30 and so on. Memorizing a transaction dated the 31st keeps the 31st even though
its first repeat falls on the 28th. A schedule that had already slipped to the 28th in an earlier
version goes back to the day of its first run from its next date on.

When the next date passes the end date, the transaction is paused for good: it stays in the list
for reference.

## Reminders

- When anything is due today or earlier, a green **recurring transactions due** button appears at
  the top of the window. Click it to open the list.
- The Dashboard's **Reminders** panel has a **Recurring transactions to enter** line. It looks a
  number of days ahead, which you set with **Customize** (see [Choose your
  reminders](reminders-and-todos.md#choose-your-reminders)).

## Change, pause, resume or delete

From a row's **▾** menu:

- **Edit schedule…** changes its name, how often it repeats, its next date and its end date.
  Moving the next date makes its day of the month the one later copies fall on.
- **Edit transaction…** opens the memorized check, expense, bill, invoice or journal entry in its
  form. Change its lines, amounts, accounts or anything else and save: the copies entered from then
  on use the changes, and copies already entered stay as they were.

- **Pause** stops it from coming due or showing in reminders. **Resume** brings it back with the
  next date it had; if that date has passed, the missed copies are due at once.
- **Delete** removes it from the list after you confirm. Transactions already entered from it stay
  posted; void them on their own screens if you need to (see [Undo a transaction
  (void)](journal-entries-and-voids.md#undo-a-transaction-void)).

There are no groups of memorized transactions.

## Memorized Transaction Listing

**Reports › Accountant & Taxes › Memorized Transaction Listing** lists every memorized transaction
by name with its **Type**, **How Often**, **Next Date**, **Ends** and **Status** (**Active** or
**Paused**). It prints and exports like any report (see [Find a report in the report
center](reports.md#find-a-report-in-the-report-center)).
