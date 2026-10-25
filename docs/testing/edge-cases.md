# Edge-case catalog

Hands-on checks for LocalBooks, beyond what `cargo test` covers. Each case names the test company,
the steps and what must happen. Run them on a debug build with the `run-localbooks` project skill
(`.claude/skills/run-localbooks/SKILL.md`), which also makes the test companies.

**Test companies** (`~/Library/Application Support/com.localbooks.desktop/dev-builds/test-companies/`,
made by `seed_test_companies` in `src-tauri/src/test_companies.rs`):

| File | What's in it |
|---|---|
| Edge Cases | Users on; encrypted tax IDs; sales tax; EUR invoice; partial payments; a credit; a job; odd names; leap-day and year-end dates; a voided entry; 2025 closed |
| No Users | No sign-in; a vendor with a plain tax ID |
| Five Years | ~2,800 documents, 2021–2025, open invoices in Nov–Dec 2025 |

Sign-ins (test values; also in `README.txt` beside the files): `owner` / `owner password` (Admin).
`clerk`, `bookkeeper` and `viewer` sign in with `<name> temp pw` and must then choose their own
password. The recovery key is in `README.txt`; the closing passcode is `close-2025`.

When a case fails, note the steps, what happened and a screenshot, then add the case's ID to the
bug. A fixed bug gets an automated test wherever one can reach it.

## Sign-in and users (Edge Cases)

| ID | Steps | Expected |
|---|---|---|
| U1 | Open the file | Sign-in screen with the company name; nothing else reachable |
| U2 | Sign in as `owner` with a wrong password, then an unknown user name | Same message both times; no hint which part was wrong |
| U3 | Sign in as `viewer` with the temp password | Asked to choose a new password before anything else; Save stays disabled until both fields match and have 8+ characters |
| U4 | As `owner`: Settings › Users & Passwords › Edit `clerk`, tick "Can see sensitive data" | A temporary password is required before Save |
| U5 | Try to change your own role or turn yourself off | Not offered; another Admin must do it |
| U6 | Turn `bookkeeper` off, sign out, try to sign in as `bookkeeper` | Refused as turned off (only after the right password) |
| U7 | Sign out › I forgot my password › Use the recovery key, with `owner` | New password works; old one doesn't; signed in as `owner` |
| U8 | Recovery key typed in lower case with spaces instead of dashes | Accepted |
| U9 | Recovery key for a non-Admin user name | Refused: resets an Admin's password only |
| U10 | New Recovery Key… then use the old key | Old key refused; new one works |
| U11 | Two windows (New window), sign out in one | Both return to the sign-in screen |

## Roles

| ID | Steps | Expected |
|---|---|---|
| R1 | As `viewer`: open an invoice, change a line, save | Refused with "can look at the books but not change them"; banner shows view only |
| R2 | As `viewer`: run P&L, Balance Sheet, export CSV, print | All work |
| R3 | As `clerk` (Standard): create and pay an invoice | Works |
| R4 | As `clerk`: Settings | Only Users & Passwords and Updates tabs |
| R7 | As `owner`, leave Settings open and sign out; sign in as `clerk` | Opens on Users & Passwords; no Admin settings (Close books, company fields) even for a moment |
| R5 | As `clerk`: Accounting › import screen | "Only an Admin can import" |
| R6 | As `owner`: every Settings tab | All present and working |

## Tax IDs (Edge Cases, then No Users)

| ID | Steps | Expected |
|---|---|---|
| T1 | As `owner`: Expenses › Vendors › Pat Contractor | Tax ID `***-**-6789` with Show; Show reveals `123-45-6789` |
| T2 | As `clerk`: same vendor | Masked, read-only, with the "only users who can see sensitive data" note; saving other fields keeps the ID |
| T3 | As `bookkeeper` (after choosing a password) | Full ID visible |
| T4 | 1099 Summary for 2025 | Both vendors listed, IDs masked on screen and in print; CSV has full IDs for `owner`, masked for `clerk` |
| T5 | Type a 5-digit tax ID and save | Masked as all stars (too short to show the last four) |
| T6 | No Users: turn users on | Recovery key shown once; Continue disabled until "I have saved…" is ticked; the vendor's ID is then masked |

## Idle and remember me

| ID | Steps | Expected |
|---|---|---|
| S1 | As `owner`: set Sign out when idle to 5 minutes; leave the app untouched 6 minutes | Back at the sign-in screen |
| S2 | Type in a form for 6 minutes without saving | Still signed in |
| S3 | Close the lid (sleep) longer than the limit, wake, click anything | Signed out |
| S4 | Sign in with Remember me ticked; quit; reopen the file | Opens without asking |
| S5 | After S4: Sign Out | Sign-in screen stays; no automatic sign-in until the file is reopened |
| S6 | Settings › Forget on This Mac; reopen | Asks for the password |

## Last resort (Edge Cases; reseed afterwards)

| ID | Steps | Expected |
|---|---|---|
| X1 | Sign-in screen › I forgot my password › No Admin password and no recovery key? › type REMOVE ALL USERS | File opens without sign-in; vendors' tax IDs are gone; invoices, bills and reports unchanged |
| X2 | After X1: Accounting › Audit Log › Verify ledger integrity | Intact |

## Accounting (Edge Cases, signed in as `owner`)

| ID | Steps | Expected |
|---|---|---|
| A1 | Trial balance at 2025-12-31 and 2026-12-31 | Debits equal credits |
| A2 | Balance sheet at 2025-12-31 | Assets = liabilities + equity |
| A3 | Post an expense dated 2025-06-30 | Refused: closed through 2025-12-31 |
| A4 | Move the closing date without the passcode, then with `close-2025` | Refused, then allowed |
| A5 | Journal entries: the voided "Posted by mistake" entry | Shown as voided with its reversal; no effect on balances |
| A6 | Invoice dated 2024-02-29 | Shows that date everywhere; P&L for Feb 2024 includes it |
| A7 | Café Olé invoice (2024-12-31) | Fractional quantity and tax round to whole cents; tax only on the taxable line; $1.01 discount applied; lands in 2024, not 2025 |
| A8 | EUR invoice to the long-named customer | Open balance reflects the partial payment at 1.10; exchange gain/loss posted |
| A9 | Zed's invoice with the applied credit | Balance reduced by $25.00 |
| A10 | Acme Corp:Kitchen Remodel | Listed under Acme Corp; A/R aging shows the part-paid balance |
| A11 | Record a payment larger than an invoice's balance | Refused or handled as a credit, never an overpaid invoice |
| A12 | Sales tax liability report for 2024 | City 8.875% on the taxable amount only |
| A13 | Bill OB-77 | Part-paid: $500.00 still open in A/P aging |
| A14 | Sales tax is off in this file: reopen 2024 with the passcode, edit invoice 1003, save without changes | Still $134.52 with its $0.98 tax |
| A15 | Every "Are you sure?" (New Recovery Key, delete a budget, Reset a form, Start over an import) | An in-app dialog asks; Cancel changes nothing, confirming does it (on macOS a browser dialog silently cancelled) |

## Names and input

| ID | Steps | Expected |
|---|---|---|
| N1 | Lists, forms and printed invoices for `Café Olé & Søn's "Best" Bakery` and `Zed's 50% Off <Outlet>` | Shown exactly, no broken characters or HTML |
| N2 | The very long customer name | Wraps or truncates with the full name on hover; never breaks the layout |
| N3 | Paste a 2,000-character password | Refused as too long, no freeze |
| N4 | Amount fields: `1,234.5`, `-5`, `abc`, `0.001` | Clear errors or correct parsing; never a silent wrong amount |

## Files

| ID | Steps | Expected |
|---|---|---|
| F1 | File › Close Company, then reopen from the list | Back to the sign-in screen (users on) or the books |
| F2 | Settings › File & Data Controls › Back up now; restore it as a new file | Restored file opens and verifies |
| F3 | File Dentist check-up, then Rebuild on Edge Cases | Check-up passes; rebuild keeps users, sign-in and tax IDs |
| F4 | Open a company file made by an older LocalBooks (keep one per release) | Upgrades after a backup; verifies |

## Speed and size (Five Years)

| ID | Steps | Expected |
|---|---|---|
| P1 | Open the file; Dashboard | Usable within a few seconds |
| P2 | P&L for 2021–2025 by month; General Ledger for 2025 | Under ~5 seconds; numbers add up |
| P3 | A/R aging | Only Nov–Dec 2025 invoices (and every ninth) open |
| P4 | Global search for "Customer 07" | Results in under a second |

## Imports (bring your own files)

| ID | Steps | Expected |
|---|---|---|
| I1 | Import an IIF and a QIF from the import kit samples | Lists and transactions match the source |
| I2 | Import a truncated or garbage file renamed `.iif` | Clear error; nothing posted |
| I3 | Import a recent QuickBooks `.qbb` | Vendors come in without QuickBooks' encrypted tax IDs; trial balance matches QuickBooks' own |
