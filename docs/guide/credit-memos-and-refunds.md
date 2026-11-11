# Credit memos and refunds

When you owe a customer money back (a return, an overcharge, a goodwill credit), give them a
**credit memo** to use against their invoices, or a **refund receipt** when you pay the money back
straight away. Both live in **Sales › Credit Memos & Refunds**, together with payments a customer
made that weren't applied to an invoice.

## Give a customer credit (credit memo)

1. Go to **Sales › Credit Memos & Refunds** and click **New credit memo**. Or choose **Customers ›
   Create Credit Memos/Refunds** in the menu bar, **+ New › Credit Memo**, or **New Transactions ›
   Credit Memo / Refund** above the customer list.
2. Enter the **Customer**. A new name is added as a customer when the credit memo posts.
3. Check the **Date**. It starts on today's date, or on the last date you entered when that is your
   preference.
4. Type the **Memo** if you like, such as the reason for the credit.
5. Bill To, Ship To, **P.O. No.** and **FOB** fill in from the customer, as on an invoice. Change them
   if you need to. The P.O. No. can print on the customer's statement (see
   [Customer statements](customer-statements.md#the-po-no-column)).
6. Add a line for each thing you are crediting: pick an item or type a description, and enter the
   quantity, the rate and the income account. Each line needs an income account, a quantity and a
   rate above zero. **Add line** adds another.
7. If you charge sales tax, choose the **Sales tax** rate (it starts on the customer's rate, or **No
   tax** for an exempt customer). Tax is worked out on the lines marked taxable.
8. With class tracking on, choose each line's class, or the form's **Class** when classes are
   assigned one per transaction.
9. Check the **Credit** total at the top right, then click **Save & Close** or **Save & New**.
   **Clear** empties the form.

Tillhouse posts the credit memo straight away and gives it the next credit number:

| Account | Debit | Credit |
|---|---|---|
| Each line's income account | the line amounts | |
| Sales tax payable (the rate's agency account) | the tax | |
| Accounts Receivable | | the credit total |

The customer's balance goes down by the total, and the credit waits to be applied to invoices,
refunded or written off (below). The list shows it as **Open** with its **Credit left**.

### Return stock

The item list on this form leaves out items whose stock you track. To take returned goods back into
stock, record them with **Adjust stock** (see [Count and correct
stock](inventory.md#count-and-correct-stock)), and credit the customer here with a non-stock line
for the amount.

### Subtotals, discounts, charges and groups

The Product/Service list here offers **Subtotal**, discount items, percent other charges and
**Group: …** lines, laid out as on an invoice (see [Subtotals, discounts, charges and
groups](invoices.md#subtotals-discounts-charges-and-groups)). On a credit memo:

- A discount line lowers the credit. Its account is credited, the reverse of a discount on an
  invoice.
- A percent charge raises the credit. Its account is debited, like an item line.
- A subtotal posts nothing. A group's items post like any other lines.

The credit must stay above zero after discounts.

### Attach a file

Click **Attach File** in the toolbar before you save to pick files (the customer's return form, a
photo). They are attached when the credit memo posts. On a saved credit memo, **Attach File** shows
the files and adds more. See [Attach receipts and documents](attachments.md).

## Pay the money back now (refund receipt)

Use a refund receipt when the customer gets their money back at once and keeps no credit.

1. In **Sales › Credit Memos & Refunds**, click **New refund receipt** (or **+ New › Refund
   Receipt**).
2. Fill in the **Customer**, **Date**, **Memo**, addresses and lines as for a credit memo.
3. Choose **Refund from**: the bank or credit card account the money goes out of. It starts on your
   first bank account.
4. Check the **Refunded** total, then save.

| Account | Debit | Credit |
|---|---|---|
| Each line's income account | the line amounts | |
| Sales tax payable | the tax | |
| The **Refund from** account | | the total |

A refund receipt doesn't touch Accounts Receivable, so it has no credit to apply. The list shows it
as **Refunded**. It doesn't write or print a check; write the check by hand, or pay by card, and put
its number in the memo if you want it on record.

## Apply a credit to invoices

1. In the list, open the credit's **▾** menu and choose **Apply to invoices…**. Or open the credit
   memo and click **Apply** in the toolbar.
2. The dialog shows the **Credit left** and lists the customer's open invoices with their **Due**
   date and **Open balance**. The **Apply** amounts are filled in for you, oldest due date first,
   until the credit runs out.
3. Change any amount, or clear it to leave that invoice alone. You can't apply more than the credit
   left, or more than an invoice's open balance.
4. Set the **Date applied** (today to start) and click **Apply**.

Applying a credit posts nothing: the credit memo already reduced Accounts Receivable. It counts
toward what's paid on the invoice, like a payment, so the invoice becomes **Partial** or **Paid**.

Only the same customer's invoices in your home currency are listed. A credit for a customer
doesn't list their jobs' invoices, and a job's credit doesn't list its customer's.

**Already applied** at the bottom of the dialog lists where the credit has gone. Click **Remove**
to take one back off its invoice; the invoice's balance goes up again and the credit comes back.

### When you save an invoice

A new invoice for a customer with credit left says so under **Balance due**, and when you save it
Tillhouse offers to apply the credits. See [Use the customer's
credits](invoices.md#use-the-customers-credits).

## Payments kept as credit (unapplied payments)

When a customer pays more than they owe, or pays before there is an invoice, and you choose to
leave the extra as their credit in **Receive Payments**, it is listed here as an **Unapplied
payment** (see [Overpayments and prepayments](customer-payments.md#overpayments-and-prepayments)).
Its **No.** column shows the payment's reference.

Apply it to invoices, refund it or write it off from its **▾** menu, the same way as a credit memo.
It isn't a form, so **Open** and **Duplicate** are greyed out for it.

## Refund credit that is left

1. Open the credit's **▾** menu and choose **Refund credit…** (or click **Refund** in the toolbar
   of an open credit memo).
2. Choose **Pay from**: a bank or credit card account.
3. Check the **Date** (today) and the **Amount**. The amount starts at all the credit left; you can
   refund part of it.
4. Paying from a bank account, leave **Print a check** ticked to print the refund check: the next
   check number fills in **Reference (check no.)**. Untick it if you pay another way, and type the
   check number yourself if you wrote one by hand.
5. Click **Record refund**. With **Print a check**, the check prints right away, made out to the
   customer for the amount, on the same check layout as **Write Checks**.

The refund posts its own entry: a debit to Accounts Receivable and a credit to the bank or card
account. The credit left goes down by the amount; when it reaches zero the credit shows as **Used**.
To print the check again later, open the credit memo and choose **Print check** on the refund in
its **Refunds and write-offs** list.

## Write off credit that is left

When the customer won't use the credit and won't be paid back, keep it as income:

1. Open the credit's **▾** menu and choose **Write off credit…** (or click **Write Off** in the
   toolbar of an open credit memo).
2. Choose **Write off to (income)**: an Income or Other Income account. It starts on your first
   income account.
3. Check the **Date** and **Amount**, add a **Reference** if you like, and click **Write off**.

The write-off posts a debit to Accounts Receivable and a credit to the income account.

### Undo a refund or write-off

An open credit memo lists its **Refunds and write-offs** under the toolbar: each with its date,
account, check number and amount. Choose **Void** in a row's **▾** menu to undo one; the credit
comes back. (Each is a journal entry, described as *Refund of credit memo #…* or *Write-off of
credit memo #…*, so voiding it in **Accounting › Journal Entries** does the same; see [Undo a
transaction](journal-entries-and-voids.md#undo-a-transaction-void).) For an unapplied payment,
which has no form to open, void it from Journal Entries.

## Open, print and email a saved credit memo

Double-click a row (or choose **Open** in its **▾** menu) to see the saved credit memo or refund
receipt as its printed form. Saved ones can't be changed; void and enter again instead.

The toolbar over it has:

- **Previous** and **Next**: open the one before or after, by date and number. A credit memo steps
  through credit memos, a refund receipt through refund receipts. On a new form, **Previous** opens
  the newest; it waits until you save or clear what you have typed.
- **Find**: opens **Find Transactions**.
- **New**: goes back to a blank form.
- **Void**: see below.
- **Duplicate**: see below.
- **Print**: **Print…** or **Save as PDF…**.
- **Email**: **Email…**, or **Email later** to send it with others from **Sales › Send Forms**.
  Emailed credit memos go to the customer's contact for invoices, or the customer's email.
- **Attach File**: the files kept with it.
- **Apply**, **Refund** and **Write Off** (credit memos only): as above, while credit is left.
- **History** and **Journal**: the transactions linked to it, and the debits and credits it posted.

Under the title, a credit memo says how much of its total is left to apply.

Credit memos and refund receipts print with the **Credit memo** design in **Settings › Custom
Forms** (see [Design your forms](custom-forms.md)). To print several at once, use **File › Print
Forms › Credit Memos…** (see [Print other forms in a
batch](invoices.md#print-other-forms-in-a-batch)).

## Copy a credit memo (Duplicate)

Choose **Duplicate** in a row's **▾** menu, or in the toolbar of an open one. A new, unsaved form
opens with the original's customer, memo, lines (with its subtotal, discount, charge and group
lines, so it totals the same), sales tax rate and (for a refund receipt) the
**Refund from** account. It is dated today and gets the next number when you save it. Check it,
change what differs, and save.

## Void a credit memo or refund receipt

Choose **Void Transaction** in the row's **▾** menu, or **Void** in the toolbar of an open one, and
confirm. Tillhouse posts a reversing entry dated the same day; the original stays in the books
and the audit log, and the row shows **Voided**.

A credit memo that has been applied to invoices or refunded can't be voided. Remove its
applications (in **Apply to invoices…**) and void its refunds first. An unapplied payment that is in
a bank deposit can't be voided until the deposit is.

## The list

Each row shows the **No.**, **Date**, **Customer**, **Type** (**Credit memo**, **Refund receipt** or
**Unapplied payment**), **Total**, **Credit left** and **Status**:

- **Open**: none of the credit used yet.
- **Partly used** and **Used**: some or all of it applied, refunded or written off.
- **Refunded**: a refund receipt.
- **Voided**.

Click a column heading to sort by it.
