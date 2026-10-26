# Changelog

What changed in each LocalBooks release. A release's section here becomes its release notes on
GitHub and in the app's update window (see `.github/workflows/release.yml`), so write it for the
people using LocalBooks.

## [0.7.0]

- **Export to Excel.** Every report, register, list and the audit log can now be saved as an Excel
  workbook, laid out like QuickBooks' own export: real numbers and dates, bold totals, frozen
  headers, and indented accounts.
- **Import PDF bank statements.** Bank Feeds can read the transaction table from a PDF bank or card
  statement. You confirm the columns, and a balance check shows whether the statement adds up. PDFs
  that are only scanned images can't be read.
- **Import from GnuCash.** The Desktop Migration Wizard reads GnuCash books (XML or SQLite) into
  LocalBooks, including customers, vendors, invoices and bills.
- **Smarter Bank Feed suggestions.** When no rule matches, LocalBooks suggests a category and payee
  from similar past transactions, with a confidence level and the transaction it looked like.
  Nothing is approved automatically, and nothing leaves your computer.
- **Daily exchange rates (optional).** Turn on **Look up daily exchange rates** (Settings ›
  Preferences Console › Accounting & tax) to have foreign-currency rates filled in from the European
  Central Bank. Only the currency codes and the date are sent. A rate you type, or one already
  saved, is never replaced.
- **Pay bills by ACH file.** In Pay Bills, choose **Pay by ACH file** to record the payments and
  save a NACHA file to upload to your bank. Vendor bank details are encrypted like tax IDs. Check
  your first file with your bank before relying on it.
- **License notices.** Help now lists the open-source components LocalBooks uses. QuickBooks® is a
  registered trademark of Intuit Inc.; LocalBooks is independent and not affiliated with, endorsed
  by, or sponsored by Intuit.
- Behind the scenes: every release is now checked automatically on macOS, Windows and Linux before
  it's offered as an update. The checks cover the update signatures, upgrading company files from
  the previous version, and launching the app.

## [0.6.0]

- **Import QuickBooks reports.** No working QuickBooks? Every QuickBooks (Desktop for Windows,
  Desktop for Mac and Online) can still export its **Journal** or **General Ledger** report and its
  account, customer, vendor and item lists to Excel or CSV, even in view-only mode. Drop those files
  into the Desktop Migration Wizard and LocalBooks rebuilds your books from them. See the guide for
  the exact report settings.
- **Moving from QuickBooks for Mac.** The migration guide now explains how to bring Mac books over:
  export a copy for QuickBooks for Windows, with no password, and import that.
- **Credit card credits.** Enter a vendor's refund onto a credit card (Expenses & Checks › Card credit,
  or + New). Card credits in QuickBooks, IIF and QIF imports now come in as card credits instead of
  journal entries.
- **Finance charges.** Set a rate, minimum charge and grace period (Settings › Preferences Console ›
  Finance charges), then use **Assess Finance Charges** to add late fees to overdue invoices.
- **Change Item Prices.** Raise or lower the prices of many products and services at once, by an
  amount or a percentage, with a preview before you apply. Every change is in the audit trail.
- **Sales reps, Ship Via and custom fields.** Optional lists for sales forms: a sales rep per
  customer and document (with a Sales by Rep Summary report), shipping method and ship date, and up
  to 15 custom fields for customers, vendors and items, which can print on your forms. They come in
  from QuickBooks and IIF imports too. Nothing changes until you add an entry.
- **New reports:** Profit and Loss and Balance Sheet Previous Year Comparison, Balance Sheet Budget
  vs. Actual (budgets can now include balance sheet accounts), Inventory Valuation Summary and
  Detail, Inventory Stock Status by Item, and Missing Checks.
- **Fixed:** on macOS, "Are you sure?" questions (such as making a new recovery key or deleting a
  budget) cancelled themselves without asking. They now ask inside LocalBooks.
- **Fixed:** editing an invoice that had sales tax, after sales tax was turned off, removed its tax.
- **Fixed:** a View only or Standard user could briefly see Admin settings when LocalBooks reopened
  on the Settings page. Changes were always refused.

## [0.5.1]

- **Tax IDs from newer QuickBooks files.** QuickBooks keeps vendor tax IDs encrypted with its own
  key, which LocalBooks can't read. Books imported before 0.5.0 could show that unreadable text as a
  vendor's tax ID (and on the 1099 report). It is now removed automatically when you open the file,
  or at your next sign-in if users are on. Re-enter those IDs from the vendors' W-9 forms. Real tax
  IDs and notes are never touched.

## [0.5.0]

- **Users and passwords (optional).** Turn them on in Settings › Users & Passwords: everyone signs
  in with their own password and a role (Admin, Standard or View only). Save the recovery key you
  are shown; it is the only way back in if every Admin forgets their password. LocalBooks signs
  everyone out after 15 minutes without activity (an Admin can change this), and can remember your
  sign-in on your own Mac. Files without users work exactly as before.
- **Encrypted tax IDs.** With users on, vendor tax IDs (SSNs and EINs) are encrypted and shown as
  `***-**-6789` to anyone not allowed to see sensitive data. The 1099 report masks them on screen;
  its CSV has them in full for people who may see them.
- **Accountability.** The audit trail now records who did what, including changes to users and
  passwords.
- **Fixed:** a foreign-currency payment could be refused after earlier partial payments on the same
  invoice were voided.
- Behind the scenes: much stronger automated testing of the ledger (thousands of random
  transaction sequences checked against the accounting rules) and saved copies of every report to
  catch unintended changes.
