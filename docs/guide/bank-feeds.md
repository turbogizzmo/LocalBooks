# Import bank transactions (bank feeds)

Bring your bank's transactions into LocalBooks, then approve each one into your books. There are two
ways to get them:

- **Import a statement file** you download from your bank: **OFX**, **QFX**, **QBO** or **CSV**.
  Free, and works with almost any bank.
- **Connect a bank** through SimpleFIN to fetch new transactions with one click. This needs a
  SimpleFIN Bridge subscription (about $15 a year, paid to SimpleFIN, not LocalBooks).

Either way, transactions wait under **For review** until you approve them. Nothing posts on its own.

## Import a statement

1. Download a statement from your bank's website. Choose OFX, QFX or QBO if offered: those files
   carry the bank's own ID for each transaction.
2. In LocalBooks go to **Banking › Bank Feeds** and choose the **Account** the statement is for.
3. Click **Import statement…** and choose the file. The new transactions appear under **For review**.

You can import overlapping statements safely. Transactions already imported are skipped and
counted as "already imported", while two genuinely identical purchases on the same day are both
kept.

## Connect a bank (SimpleFIN)

SimpleFIN Bridge signs in to your banks and shares read-only transaction data with apps you
approve. LocalBooks never sees your bank password.

1. Go to **bridge.simplefin.org**, create an account, and connect your banks there.
2. In SimpleFIN Bridge, create a **setup token** for a new app connection and copy it.
3. In LocalBooks go to **Banking › Bank Feeds** and click **Connect a bank…**.
4. Paste the token and click **Connect**. Your SimpleFIN accounts and their balances are listed.
5. For each account you want, choose the LocalBooks bank or credit card account under **Feeds
   into**, check the **from** date, and click **Link**. The suggested date is the day after the
   last transaction already in Bank Feeds for that account, so nothing is fetched twice.
6. Click **Sync now**. New transactions appear under **For review**.

After that, click **Sync banks** on the Bank Feeds toolbar whenever you want new transactions.

Good to know:

- **A setup token works once.** If LocalBooks says the token was already used and you didn't use
  it, someone else may have: disable it in SimpleFIN Bridge and create a new one.
- **The access is kept in this computer's keychain**, not in the company file or its backups. If
  you open the company file on another computer, the connection shows **Reconnect**: paste a new
  setup token and your account links are kept.
- **SimpleFIN allows about 24 updates a day**, so LocalBooks syncs only when you ask, and at most
  every 10 minutes. Each sync re-checks the last 5 days to catch transactions that post late.
- **Only posted transactions are fetched.** Pending ones arrive in a later sync once they post.
- **Up to 90 days back.** For older history, import statement files.
- **Accounts in another currency can't be linked**, because LocalBooks bank accounts are kept in
  your home currency. Import their statements as files instead.
- Messages from SimpleFIN (for example "reauthenticate with your bank") appear on the connection.
  Fix those in SimpleFIN Bridge.
- **Remove** forgets the connection on this computer. Transactions already fetched stay in Bank
  Feeds. To stop SimpleFIN sharing data, also disable the app connection in SimpleFIN Bridge.

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

Approved money out becomes an expense; approved money in becomes a deposit. For money out you can
also fill in the **Payee**, which becomes the expense's vendor (a new name adds a vendor).

## How categories are suggested

LocalBooks suggests a category for every transaction, in this order:

1. **Rules.** A built-in vendor list (for example fuel stations go to Fuel & Auto), and a rule
   learned from each transaction you approve, matching the first distinctive words of its
   description. The note under the category says which kind of rule matched.
2. **Similar past transactions.** When no rule matches, LocalBooks looks for the past transactions
   in this company file that are most like this one: words in the description, the exact amount,
   roughly the same amount, the same bank account and the same time of the month. It suggests the
   category most of them used, with a **High**, **Medium** or **Low** confidence badge and a
   *Because similar to …* line naming the closest one. A Target purchase at a store you haven't
   used before, or this month's rent check, usually gets the right category this way.
3. **Uncategorized** Expense or Income, when nothing is similar enough.

The **Payee** is suggested from similar past expenses as well, including when a rule chose the
category.

Good to know:

- **Suggestions never post on their own.** You still approve every transaction, and you can change
  the category or payee first.
- **It learns as you go.** Each approval is learned immediately. Excluded transactions teach it
  nothing. Expenses and deposits you enter directly count as past transactions too, so a file
  imported from QuickBooks gets useful suggestions from the start.
- **It stays on your computer.** Suggestions are worked out from your company file in memory. Nothing
  is sent anywhere and nothing extra is stored.
- **Voiding** an approved transaction sends it back for review, and LocalBooks forgets it.
- Inactive accounts are never suggested.

> **Tip:** Approving a bank transaction that you already entered by hand records it twice. Exclude
> it instead.

Next: [Reconcile an account](reconciling.md)
