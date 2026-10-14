# Import bank transactions (bank feeds)

Download a statement from your bank, import it, and approve each transaction into your books.
LocalBooks reads **OFX**, **QFX** and **QBO** statement downloads and **CSV** files.

## Import a statement

1. Download a statement from your bank's website. Choose OFX, QFX or QBO if offered: those files
   carry the bank's own ID for each transaction.
2. In LocalBooks go to **Banking › Bank Feeds** and choose the **Account** the statement is for.
3. Click **Import statement…** and choose the file. The new transactions appear under **For review**.

You can import overlapping statements safely. Transactions already imported are skipped and
counted as "already imported", while two genuinely identical purchases on the same day are both
kept.

## When LocalBooks asks which columns are which

Most bank CSV files are recognized automatically: a date, a description, and either one amount
column (negative = money out) or separate money-in and money-out columns such as Credit/Debit,
Deposits/Withdrawals or Paid in/Paid out. Otherwise a window opens:

1. Choose the **Date** column and how **Dates are written**: month first (MM/DD, as US banks do)
   or day first (DD/MM, as UK and European banks do).
2. Choose the **Description** column.
3. Under **Amounts are in**, choose **One column** and the **Amount** column, or **Two columns**
   and the **Money in** and **Money out** columns.
4. Check the **Preview**: dates should be right and money out should show as negative (red).
5. Click **Import … transactions**.

LocalBooks remembers your choices, so the next file with the same columns imports directly. The
window also opens when every date in a file could be read either way (such as 03/07), so you can
confirm the day/month order once.

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
