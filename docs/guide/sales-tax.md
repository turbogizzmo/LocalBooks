# Sales tax

LocalBooks works out sales tax on your sales forms, builds it up in a liability account, and tells
you what to file and pay each period. You set it up once in **Sales › Sales Tax**: turn it on, add
the rates you charge and the agency each is paid to, and mark which products and customers are
taxed.

The same screen opens from **Vendors › Sales Tax › Manage Sales Tax…** and **Lists › Sales Tax Code
List** in the menu bar.

## Turn sales tax on

1. Go to **Sales › Sales Tax**.
2. Tick **Charge sales tax** at the top.

Invoices, sales receipts, estimates, sales orders and credit memos then show a **Tax** column on
their lines and a **Sales tax** box with the rate. Untick it to stop charging tax: new forms lose the
Tax column, and tax already posted doesn't change. A form you open again that was saved with a rate
keeps that rate, so saving it again doesn't drop its tax.

When you create a company, **Customers are charged sales tax** (under the expert setup) turns sales
tax on for you and adds a **Sales Tax Payable** account. You still add your rates afterwards.

## Add a rate and its agency

1. In **Sales › Sales Tax**, click **New rate**.
2. Fill in:
   - **Name**: what you'll see on forms and reports, such as "State" or "Harbor County".
   - **Rate (%)**: the percentage, such as `8.25`. Up to four decimal places; 0% to 100%.
   - **Agency (who you pay)**: the tax office, such as "State Department of Revenue". Type a name;
     LocalBooks uses the vendor of that name, or adds one if there isn't one. Leave it blank if you
     don't need the return split by agency.
   - **Liability account**: where the tax builds up until you pay it. Only Other Current Liability
     accounts are offered. Leave it blank for **Sales Tax Payable**, which LocalBooks adds if the
     company has none.
3. Click **Add rate**.

The **Tax rates** list shows each rate's name, percentage, agency and whether it's **Active**.

### Change or retire a rate

- **Change a rate:** double-click it, or choose **Edit…** in its row menu, change it and click
  **Save**. The new percentage, agency and account apply to sales posted from now on. Sales already
  posted keep the tax they were posted with.
- **Stop using a rate:** choose **Make inactive** in its row menu. It leaves the **Sales tax** box
  on new forms and on customer records, and a form can't be saved with it. **Make active** brings
  it back. Rates aren't deleted, so old sales and reports keep their names.

## Combine rates from several agencies

Where one sale is taxed by more than one agency (a state, a county and a city, say), each agency
gets its own share and you file with each. Set it up like this:

1. Add each agency's rate on its own with **New rate**, each with its **Agency (who you pay)** and,
   if you keep them apart, its own **Liability account**.
2. Click **New combined rate**.
3. Enter a **Name**, such as "Harbor City 8.5%".
4. Under **Rates it adds up (each paid to its own agency)**, tick two or more rates. Only active,
   ordinary rates are listed; a combined rate can't include another one.
5. Check the **Combined rate** shown below the list (the ticked rates added up), then click **Add
   combined rate**.

You choose a combined rate on sales forms like any other rate. In the list its **Agency** column
shows the rates it's made of, such as "Combined: State + Harbor County + Harbor City".

**What it posts.** LocalBooks works out the tax at the combined percentage on the form's taxable
amount. It then splits that tax into one line per rate: each rate gets its own percentage of the
taxable amount, and any cent left over from rounding goes to the rate with the largest share. Each
line is credited to its own rate's liability account, with a memo naming the rate (for example
"Sales tax: Harbor County").

**When a rate changes.** Change a rate in a combined rate and the combined percentage follows, for
sales from then on. Edit the combined rate itself to change which rates it adds up. Sales already
posted keep the split they were posted with. A form can't be saved at a combined rate while one of
its rates is inactive.

## Mark products and customers taxable or not

LocalBooks has no separate list of tax codes. Whether tax is charged depends on two things: each
line's **Tax** box, and the rate in the form's **Sales tax** box.

**Products and services.** Open a product in **Sales › Products & Services**. **Taxable: charge
sales tax when this is sold** is ticked for new products; untick it for things that aren't taxed,
such as most labor in many states. When you add a product in the list itself, the **Tax** box next to
the income account does the same. For a Discount item the box reads **Taxable: the discount lowers
the taxable amount**.

**On a form.** Each line's **Tax** box starts from its product's setting. Tick or untick it on one
line to change just that sale. Subtotal and group heading lines don't have one.

**Customers.** Open a customer in **Sales › Customers**. With sales tax on, the record has:

- **Default sales tax**: the rate their sales start with. **Company default** (the starting choice)
  means the **Default rate** chosen at the top of **Sales › Sales Tax**. With no default rate
  chosen, it's the company's only active rate if it has exactly one; otherwise no rate until you
  pick one on the form.
- **Tax exempt**: their sales start with no tax. **Exemption (reason or certificate no.)** keeps the
  reason or the resale or exemption certificate number on file.

## Choose the rate on a sale

1. Pick the customer. The **Sales tax** box fills in from their record: their **Default sales tax**,
   **No tax** if they're exempt, or for **Company default** the company's **Default rate** (or its
   only rate, when no default rate is chosen).
2. Change the rate in **Sales tax** if this sale is taxed differently, or choose **No tax**. Once
   you change it, picking another customer doesn't change it back.
3. Check each line's **Tax** box.

The tax is the taxable lines' total times the rate, rounded to the nearest cent (half a cent rounds
up). A **Discount** off the whole form comes off before tax: it lowers the taxable amount by its
share of the taxable lines. Taxable and non-taxable discount lines work as described in
[Subtotals, discounts, charges and groups](invoices.md#subtotals-discounts-charges-and-groups).

What it posts:

- **Invoice:** Dr Accounts Receivable for the total; Cr each income account; Cr the rate's liability
  account for the tax (one line per rate for a combined rate).
- **Sales receipt:** the same, with the deposit account debited instead of Accounts Receivable.
- **Credit memo or refund:** the reverse. The tax is debited back to the liability account.
- **Estimate and sales order:** nothing. They show the tax at their rate; each invoice made from
  them works it out again.

When Preferences mark taxable amounts, printed and emailed invoices, sales receipts and estimates
show a **T** beside each taxable line.

## See what you owe

The right side of **Sales › Sales Tax** is the **Sales tax liability** for a period. It opens on the
last filing period that ended (the return due now), going by **File returns** in Preferences. Change
the two dates to look at another period.

For each rate it shows:

- **Taxable sales** and **Non-taxable** sales in the period.
- **Tax** on those sales.

Sales with no rate count as non-taxable, under **No tax rate**, whatever their lines say. Below the
rates:

- **Tax for the period**: the tax on the period's sales.
- **Paid in the period**: sales tax payments dated in it.
- **Owed as of** the end date: the balance of your sales tax liability accounts on that date, from
  all periods, not only this one.

A note under the table says whether it's on the accrual or cash basis, how often you file, and when
the last return is usually due.

**Accrual or cash.** On the accrual basis (the default), invoices and sales receipts count on their
date and credit memos are taken off on theirs. On the cash basis, an invoice counts as it is
settled: in proportion to each payment and each credit applied to it, on their dates, less checks
that bounced. Sales receipts and credit memos still count on their own dates. On cash basis **Owed**
also leaves out the tax on the unpaid part of invoices, which the ledger already holds in the
liability account but you don't owe yet. Invoices in another currency aren't included.

## File the sales tax return by agency

**Sales tax return by agency**, at the bottom of **Sales › Sales Tax**, is what you copy onto each
agency's return. It uses the same dates as the liability above it.

1. Set the period in the liability's date boxes.
2. Choose the basis in the box on the right: **Accrual (when invoiced)** or **Cash (when paid)**. It
   starts on the basis set in Preferences; changing it here changes only this table.
3. Read each agency's block:
   - The agency's name with its **Tax** for the period and what you **Paid** it in the period.
   - Under it, each of its rates with **Taxable sales** and **Tax**.

A sale at a combined rate shows under every agency in it, each with its own share of the tax. Rates
without an agency are under **No agency**. **All agencies** at the bottom adds them up.

A payment counts for the agency of the rate it was recorded for. A payment recorded for **All
rates** counts under **No agency**.

## Pay sales tax

1. In **Sales › Sales Tax**, click **Pay sales tax** (or choose **Vendors › Sales Tax › Pay Sales
   Tax…**). The dialog shows what's **Owed now**, from the liability's end date.
2. Fill in:
   - **For**: the rate you're paying. Choose a rate so the payment comes off that rate's liability
     account and counts for its agency on the return. **All rates (Sales Tax Payable)** pays down
     Sales Tax Payable and counts under **No agency**. With only one active rate, it's chosen for you.
   - **Paid from**: a bank or credit card account. It starts on the first active bank account.
   - **Payment date**: today, unless you change it.
   - **Amount**: starts at the amount owed; change it to what you're paying.
   - **Reference (confirmation or check no.)**: optional, such as the agency's confirmation
     number.
3. Click **Record payment**.

The payment posts Dr the rate's liability account (or Sales Tax Payable for **All rates**) and Cr
the account it was paid from. Its description is "Sales tax payment" and the agency's name. It's
listed under **Payments** on the screen, and in **Paid in the period** on the liability and the
return.

When you file with several agencies, record one payment per agency, each for that agency's rate.

Paying a **combined rate** splits the payment among the rates it adds up, in proportion to their
percentages (any rounding cent going to the largest): one entry debits each rate's own liability
account for its share, and the return lists each share under that rate's agency. A combined-rate
payment recorded in 0.14.0 went entirely to its first rate's account and counts for that rate's
agency; void it and record it again to spread it across the agencies.

To undo a payment, choose **Void Transaction** in its row menu. The voided payment stays in the list,
greyed, and no longer counts as paid (see [Undo a transaction
(void)](journal-entries-and-voids.md#undo-a-transaction-void)).

## Adjust what you owe

When what you owe the agency differs from what LocalBooks built up (a discount for filing on time,
a penalty or interest, or a rounding difference), record a sales tax adjustment:

1. On **Sales › Sales Tax**, click **Adjust…** next to **Pay sales tax**.
2. Choose the **Rate**, the **Adjustment date**, the **Amount**, and **Reduce sales tax owed** or
   **Increase sales tax owed**.
3. Choose the account on the other side: an income account for a reduction (such as a
   timely-filing discount), an expense account for an increase (such as penalties).
4. Add a **Memo** if you like, then click **Record adjustment**.

A reduction posts Dr the rate's liability account / Cr the income account; an increase posts Dr
the expense account / Cr the liability account (pay it with **Pay sales tax**). The adjustment
changes **Owed as of**, and the [sales tax return](#file-the-sales-tax-return-by-agency) adds it to
the rate's tax under its agency for the period it is dated in, marked *adj.* A combined rate is
adjusted one part at a time, so each agency's figure is right. Adjustments are listed under
**Adjustments** on the Sales Tax screen; choose **Void** in a row's **▾** menu to undo one.

To correct the tax on one sale, edit or void that invoice, sales receipt or credit memo instead.

## Sales tax reports

- **Sales Tax Liability** (**Vendors › Sales Tax › Sales Tax Liability**, **Reports › Vendors &
  Payables**, or **What you owe** in the Report Center): the same figures as the liability on the Sales Tax screen, per rate with **Tax rate**,
  **Rate**, **Taxable sales**, **Non-taxable sales** and **Tax**, then a total, **Paid in the
  period** and **Owed as of** the end date. It opens on the last filing period that ended, on the
  basis set in Preferences. Print it or export it to CSV like any report.
- **Sales Tax Revenue Summary** (**Reports › Vendors & Payables › Sales Tax Revenue Summary** in the
  menu bar, or **What you owe** in the Report Center): for each rate, **Total Sales**,
  **Non-Taxable Sales** and **Taxable Sales**, the same sales the liability counts. See [Lists and
  other reports](reports.md#lists-and-other-reports).

## Sales tax preferences

**Edit › Preferences › Sales Tax › Company Preferences** (Admins only) sets when you owe the tax
(accrual or cash), how often you file (which sets the period the screen and reports open on, and a
reminder on the 20th of the following month), and whether printed forms mark taxable lines with a
**T**. With sales tax on, **Reminders** lists the return under tax dates. See [Sales
Tax](preferences.md#sales-tax) in the Preferences reference for each option and its default.
