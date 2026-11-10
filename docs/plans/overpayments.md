# Overpayments, prepayments and unapplied payments (T24)

Receive Payments used to refuse any money it could not apply to an open invoice
(`INV_OVERPAYMENT`), and listed only customers with open invoices. This note says how money that is
not applied to an invoice is posted and tracked; the gaps row (`docs/gaps.md`, T24) tracks status.

## What the person sees

- **Received from** lists every active customer, not only those with open invoices. A customer with
  none can still be paid: a prepayment or a deposit before any work is billed.
- When the amount received is more than what is applied to invoices, the form shows the
  **Unapplied amount** and asks what to do with it:
  - **Leave it as a credit** (the default): the customer keeps it, to use on a later invoice.
  - **Refund the amount**: pay it straight back, from a bank or credit card account.
  - **Write it off**: keep it as income, posted to an income account the person picks.
- In the Customer Center and on Credit Memos & Refunds the kept amount is listed as an
  **Unapplied payment**, with its **Remaining credit**. It is applied to invoices, refunded or
  written off later exactly like a credit memo's credit.

## How it posts

The money applied to invoices posts as before: one `invoice_payments` row and one entry per invoice.
The extra becomes a customer credit of its own: a row in `credit_memos` with the money's account
and payment method on it (`payment_account_id`, `payment_method`, `payment_method_id`,
`payment_reference`) and no lines.

| Step | Entry |
|---|---|
| Unapplied payment received | Dr the deposit account (Undeposited Funds or a bank) / Cr Accounts Receivable |
| Applied to an invoice later | nothing; both sides are already in A/R (`credit_applications`) |
| Refunded | Dr A/R / Cr the bank or card account (`credit_refunds`) |
| Written off | Dr A/R / Cr the income account (`credit_refunds` with `is_write_off = 1`) |

Why a credit memo row rather than a new table: everything a credit already does (remaining credit,
applying to invoices, available credits on a new invoice, refunds, statements, the A/R aging, void
rules) then works for unapplied payments with no second copy of it. The row is told apart by
`payment_account_id IS NOT NULL`. Reports of sales and sales tax read credit memo *lines*, and an
unapplied payment has none, so it adds nothing to sales. It takes the next credit number, like a
refund receipt does; lists show it as "Payment" rather than "Credit memo".

All of it (the invoice payments, the unapplied credit and its refund or write-off) is one write
transaction: it all posts, or none of it does.

## Deposits

An unapplied payment received into Undeposited Funds shows in Make Deposits next to the other
payments, as kind `CREDIT`. `deposit_lines.credit_memo_id` records it, and a partial unique index
keeps it in one live deposit at a time. While it is in a live deposit it can't be voided, the same
rule as for invoice payments.

## Limits

- Credits are in the home currency, so a foreign-currency payment can't leave an unapplied amount;
  apply all of it to invoices.
- Underpayments are unchanged: an invoice paid short stays open for the rest.
- Clean Up Books' undeposited-funds matching and transfers handle invoice payments and sales
  receipts only; an unapplied payment is deposited through Make Deposits.

## Schema

Migration `0103_unapplied_payments.sql`: the four `credit_memos` columns, `credit_refunds.is_write_off`,
`deposit_lines.credit_memo_id` with its unique index, and triggers that keep a deposit line to one
source and stop a deposited unapplied payment from being voided.
