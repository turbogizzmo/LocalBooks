# Import bank transactions (bank feeds)

Download a statement from your bank as a CSV file, import it, and approve each transaction into
your books.

## Import a statement

1. Download a CSV statement from your bank's website.
2. In LocalBooks go to **Banking › Bank Feeds** and choose the **Account** the statement is for.
3. Click **Import CSV…** and choose the file. The new transactions appear under **For review**.

LocalBooks reads files that have a header row with a **Date**, a **Description** (or Memo, Payee or
Name) and either one **Amount** column or separate **Debit**/**Credit** (Withdrawal/Deposit)
columns. Dates such as `2026-03-07`, `03/07/2026`, `3/7/26` and `Mar 7, 2026` are recognized.

## Approve or exclude each transaction

1. Click a transaction under **For review**.
2. In **Categorize**, check the suggested **Expense category** (money out) or **Income category**
   (money in). The **Journal entry preview** shows exactly what will post.
3. Click **Approve** to post it, or **Exclude** to skip it (for example a duplicate, or a transfer
   you already recorded).

Approved money out becomes an expense; approved money in becomes a deposit. LocalBooks learns from
what you approve and suggests the same category for that payee next time.

> **Tip:** Approving a bank transaction that you already entered by hand records it twice. Exclude
> it instead.

Next: [Reconcile an account](reconciling.md)
