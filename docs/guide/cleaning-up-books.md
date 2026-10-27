# Clean up the books (Client Data Review)

**Accounting › Clean Up Books** puts the cleanup tools from QuickBooks Accountant's Client Data
Review on one screen. Run them before you close a year or hand the books to your accountant.

Each tool lists what it found first. Nothing changes until you choose what to fix and confirm a
message that shows the totals. Every fix goes through the usual rules: entries must balance,
nothing can be dated on or before the [closing date](closing-the-books.md), and each fix is written
to the audit log as a `CLEANUP` event that names everything it changed.

## Apply unapplied credits

Credit memos and vendor credits that still have credit left are listed by customer or vendor, next
to that customer's open invoices or that vendor's open bills.

1. Open **Unapplied credits**. The amounts are filled in for you, applying each credit to the
   oldest due invoice or bill first.
2. Change any amount, or clear it to leave that credit where it is.
3. Choose the **Apply on** date and click **Apply credits…**.
4. Check the number of credits and the total, then click **Apply credits**.

Applying a credit posts nothing, because the credit memo already reduced Accounts Receivable (or
the vendor credit reduced Accounts Payable). It marks the invoice or bill as paid by that amount.
To take one back off, open the credit memo or vendor credit and remove the application.

A credit with nothing open to apply it to can be refunded instead, from **Sales › Credit Memos** or
**Expenses › Vendor Credits**.

### Unapplied payments

In LocalBooks a payment is always recorded against its invoice or bill, so a payment itself is
never left unapplied. An import can still bring in money that QuickBooks held as unapplied. It
arrives as a journal entry that credits Accounts Receivable (or debits Accounts Payable) with no
invoice or bill behind it. Those entries appear under **Unapplied payments from journal entries**
for review only. A journal entry names no customer and can't pay an invoice. To fix one, void it
and record the payment against its invoice in **Receive Payment** (or **Pay Bills**).

## Clear up Undeposited Funds

A payment that sits in Undeposited Funds even though the money is already in the bank usually
means it was deposited twice. The payment went into Undeposited Funds, and the deposit was also
recorded directly, often to an income account from a bank feed. Income is overstated and
Undeposited Funds never empties.

1. Open **Undeposited Funds**. Payments are grouped by customer.
2. For each payment, the **Already banked by** list shows deposit lines that could hold it. Lines
   whose memo names the customer come first, then lines that name no customer. A line with the
   exact amount starts with **=**.
3. Choose the deposit line that banked the payment. Leave the rest on **Leave in Undeposited
   Funds**.
4. Click **Match to deposits…**, check the total, and click **Match payments**.

Each match posts a correcting entry on the deposit's date: a debit to the account the deposit line
used (usually income) and a credit to Undeposited Funds. If the line already took the money out of
Undeposited Funds, no entry is needed and only the link is recorded. The payment then counts as
deposited. The deposit itself doesn't change, so its bank line and any reconciliation stay as they
are.

To undo a match, click **Undo** under **Matched earlier**. Its correcting entry is voided, and the
payment goes back into Undeposited Funds. Voiding the correcting entry from **Journal Entries**
does the same. While a match stands, neither the payment nor the deposit can be voided.

> **Tip:** If the deposit's date is in a closed period, the match is refused. Ask whoever holds the
> closing passcode whether to reopen that period, or date the correction in the open period.

## Write off invoices

1. Open **Write off invoices**.
2. Set the **Write-off date**, how many days past due an invoice must be (**At least days past
   due**), and optionally **Open balance up to** to find small leftover balances.
3. Choose **Write off to**: an expense account such as Bad Debt, or an income account such as
   Discounts Given.
4. Tick the invoices to write off. The footer shows how many you picked and their total, beside the
   total of everything listed.
5. Click **Write off…**, check the total, and click **Write off**.

Each invoice gets a credit memo for its open balance to the chosen account, applied to that invoice
at once: a debit to that account and a credit to Accounts Receivable. The credit memo's memo reads
"Write-off of invoice #…", followed by your memo. To reverse a write-off, remove the credit
application and void the credit memo.

Foreign-currency invoices aren't listed, because credit memos are in the home currency. Sales tax
already reported on a written-off invoice isn't given back automatically. Adjust it in
**Sales › Sales Tax** if your tax agency allows a bad-debt deduction.

## Review list changes

**List changes** shows accounts, items, customers and vendors that were added, renamed, edited, made
inactive or made active again during the dates you choose. Pick a list to narrow it down.

- **Added** comes from the date each record was created.
- Edits come from the audit log. Account changes have been recorded since LocalBooks first had an
  audit log. Customer, vendor and item edits are recorded from the release that added Clean Up Books
  on, so earlier edits to them aren't shown.
- For customers and vendors, **Details** names the fields that changed without their values, so
  sensitive details such as tax IDs never enter the audit log. Renames show the old and new name,
  and item edits show the old and new price.
- LocalBooks can't merge two list entries, so there are no merges to review.

## Compare the Balance Sheet with inventory

**Inventory vs. Balance Sheet** compares each inventory asset account's balance with the
**Inventory Valuation** total for the items that use it, as of the date you choose. **Last year end**
sets the date to the end of last year.

When they differ, **Transactions that cause the difference** lists every transaction whose amount on
an inventory account doesn't match the value of stock it moved, usually a journal entry, expense or
check posted straight to the inventory account. Click one to see its lines. To fix it, void it and
record the stock with **Adjust stock…** in **Sales › Products & Services**, or move the amount to the right account with a
journal entry.

## See also

- [Close a period](closing-the-books.md)
- [Bank deposits and transfers](deposits-and-transfers.md)
- [Record customer payments](customer-payments.md)
- [Track inventory](inventory.md)
- [How LocalBooks keeps the books accurate](ledger-integrity.md)
