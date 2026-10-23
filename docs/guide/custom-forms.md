# Design your forms

**Settings › Custom Forms** sets how invoices, estimates, sales receipts and customer statements
look when you print them or save them as PDF files. Each form type has its own design, and the
design is saved in your company file.

## Change the design

1. Go to **Settings › Custom Forms** and pick **Invoice**, **Estimate**, **Sales receipt** or
   **Statement**.
2. Change the layout on the left. The preview on the right updates as you go:
   - **Layout**: **Classic** (company and title side by side over a rule), **Modern** (the title in
     a colored band, banded rows) or **Compact** (smaller type, more lines per page).
   - **Title**, **Paper** (US Letter or A4), **Font** and **Accent color** (the title, rules and
     table header).
   - **Logo**: top right, top left or hidden, in three sizes. Upload the logo itself in
     **Preferences › Company**.
   - **Header details**: which company details print, and whether the customer's email prints
     under the billing address.
   - **Line items**: the shaded header, line numbers, the quantity and rate columns, and the column
     labels. On invoices you can also hide the payments and credits listed above the balance due.
   - **Messages**: a message to the customer, terms or payment instructions, and a footer for
     every page.
   - **Email**: the subject and message used when you email the form.
3. To see a real document, choose it under **Preview with**.
4. Click **Save form**. **Undo changes** goes back to the last saved design, and **Reset to
   standard** restores the built-in one.

Messages can include placeholders that are filled in for each document: `{number}`, `{customer}`,
`{date}`, `{due_date}`, `{total}`, `{balance}` and `{company}`. `{invoice_number}`, `{amount}` and
`{balance_due}` work too, as other names for `{number}`, `{total}` and `{balance}`. For example,
*Please pay invoice {number} by {due_date}.*

## Styles: several designs, one per customer

A **style** is a full set of forms (invoice, estimate, sales receipt and statement). You start with
one, **Standard**, which is the default. Use the **Style** row at the top of Custom Forms to add a
**New** style, **Copy** the one you're looking at, **Rename** it, **Make default**, or **Delete** it.

To give a customer their own style, open them in **Sales › Customers** and choose a **Form style**.
Their forms then print, save and email in that style; every other customer gets the default.
Deleting a style moves its customers back to the default.

## Print or save a PDF

In **Sales › Invoices**, **Sales › Estimates** or **Sales › All Sales** (invoices and sales
receipts), open a row's **Actions** menu and choose **Print…** or **Save as PDF…**. Both use the
saved design, so the PDF matches the printout.

For statements, go to **Sales › Customer Statements**, pick the period, and click a customer to
preview their statement; **Print…** and **Save as PDF…** are above the preview. Tick several
customers and click **Print statements** to print them all at once.

## Email it

Open the row's **Actions** menu and choose **Email…** (invoices, estimates, and sales receipts in
**All Sales**). For a statement, click **Email…** above its preview in **Sales › Customer
Statements**: it sends the statement for the period shown. Check the address (it starts as the
customer's email from **Sales › Customers**; separate several with commas), the subject and the
message, then click **Open in mail app**. A new message opens in your mail app with the PDF
attached. Review it there and send it. LocalBooks doesn't send anything itself.

- On a Mac, Apple Mail and Microsoft Outlook get the attachment directly. The first time, macOS asks
  whether LocalBooks may control the mail app: choose **OK**.
- On Windows, mail apps that support Simple MAPI (such as Outlook and Thunderbird) get the
  attachment directly.
- With any other mail app, the message opens with the address, subject and text filled in, and the
  PDF is shown in Finder or File Explorer for you to drag into the message.

## See what was sent

Every time a form is emailed, LocalBooks adds it to the form's sent history: **✉** beside its
number in the list (point at it to see *Emailed 10/21/2026 to ap@acme.com*), and the latest sends
in the **Email…** dialog. The history is a permanent record: it can't be edited or deleted, and it
doesn't change the posted invoice. When a message is opened in your mail app, it's recorded as
*opened in mail app*, because LocalBooks can't see whether you then sent it.

## Email later and Send Forms

To send forms in a batch, mark them **Email later**:

- Tick **Email later** next to **Save & Post to Ledger** (invoices) or **Save Estimate**, or
- choose **Email later** in a row's **Actions** menu (also for sales receipts in **All Sales**).

Statements aren't queued, because their period is chosen when you send them. Email them from
**Sales › Customer Statements**.

Marked forms show an **Email later** tag, and **Unsent only** above the list shows just those.
Then go to **Sales › Send Forms**:

1. Check the address for each form (fill in any that are missing) and tick the ones to send.
2. Click **Send selected**. With sending from LocalBooks turned on (below), they all go out at once.
   Otherwise one new message per form opens in your mail app.
3. Sent forms leave the list. Any that couldn't be sent stay, with the reason in **Result**.

Use **Review and email…** in a row's menu to check or change one message before it goes.

## Send from LocalBooks instead (SMTP)

If you'd rather LocalBooks sent the message itself, go to **Settings › Email**:

1. Choose **Send them from LocalBooks (SMTP)**.
2. Enter your email provider's SMTP server, security and port (usually STARTTLS on 587, or SSL/TLS
   on 465), your username and password, and the from address and name. Many providers (Gmail,
   iCloud, Outlook.com) require an *app password* created in your account's security settings.
3. Click **Test connection** to check the sign-in, or **Send test email** to send yourself a short
   message, then **Save**.

Gmail works with an app password (it needs 2-Step Verification on). Microsoft 365 work and school
accounts may have password sign-in for SMTP turned off: Microsoft is retiring it, and an
administrator can disable it. If Test connection is refused there, use the mail-app option.

**Email…** then shows **Send**, which sends the message with the PDF attached. You can still choose
**Open in my mail app instead** for any message. With **Copy to me** on, you get a blind copy of
every message you send. The password is kept in this computer's Keychain or Credential Manager, not
in the company file, so on another computer you enter it again.

Forms print in Helvetica, Times or Courier, the fonts every PDF reader has. Characters those fonts
don't have (for example Chinese or Arabic text) print as **?**.

Next: [Create and send an invoice](invoices.md)
