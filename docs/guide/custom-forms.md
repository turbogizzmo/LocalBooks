# Design your invoices and estimates

**Settings › Custom Forms** sets how invoices and estimates look when you print them or save them as
PDF files. Each form type has its own design, and the design is saved in your company file.

## Change the design

1. Go to **Settings › Custom Forms** and pick **Invoice** or **Estimate**.
2. Change the layout on the left. The preview on the right updates as you go:
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
`{date}`, `{due_date}`, `{total}`, `{balance}` and `{company}`. For example, *Please pay invoice
{number} by {due_date}.*

## Print or save a PDF

In **Sales › Invoices**, **Sales › Estimates** or **Sales › All Sales**, open a row's **Actions** menu
and choose **Print…** or **Save as PDF…**. Both use the saved design, so the PDF matches the
printout.

## Email it

Open the row's **Actions** menu and choose **Email…**. Check the address (it starts as the
customer's email from **Sales › Customers**; separate several with commas), the subject and the
message, then click **Open in mail app**. A new message opens in your mail app with the PDF
attached. Review it there and send it. LocalBooks doesn't send anything itself.

- On a Mac, Apple Mail and Microsoft Outlook get the attachment directly. The first time, macOS asks
  whether LocalBooks may control the mail app: choose **OK**.
- On Windows, mail apps that support Simple MAPI (such as Outlook and Thunderbird) get the
  attachment directly.
- With any other mail app, the message opens with the address, subject and text filled in, and the
  PDF is shown in Finder or File Explorer for you to drag into the message.

Forms print in Helvetica, Times or Courier, the fonts every PDF reader has. Characters those fonts
don't have (for example Chinese or Arabic text) print as **?**.

Next: [Create and send an invoice](invoices.md)
