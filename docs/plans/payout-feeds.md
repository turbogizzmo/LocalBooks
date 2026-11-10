# Payout feeds from payment processors (gaps row T68)

A payment processor (Stripe first; PayPal and Shopify Payments later) collects card sales, takes
its fees, pays back refunds and disputes, and sends the rest to the bank as a **payout**. The bank
statement shows only that net amount, so importing the bank line alone books net sales and hides
the fees. This note sets out how a payout becomes a deposit that shows the gross sales and every
deduction, and how it stays private.

## Privacy

- The feed is **off until the user sets it up** with a key; nothing is fetched before that, and
  only when the user clicks **Get payouts** (no background polling in this change).
- The key is a **restricted, read-only key** the user creates at the processor (Stripe: read access
  to Balance and Payouts). It is kept in the operating system's credential store
  (`secrets.rs`, service "LocalBooks payout feeds"), never in the company file, backups or logs.
- LocalBooks calls only the processor's own API (`https://api.stripe.com`) with that key, and sends
  nothing else anywhere.

## What a payout becomes

A payout is a **Deposit** to the bank account it lands in, dated its arrival date:

| Part | Posts as | Account (chosen at setup) |
|---|---|---|
| Each charge, gross | Credit, a deposit line per charge | Sales (an income or clearing account) |
| Processor fees (all fees in the payout) | Debit, a deduction | Fees (an expense account) |
| Each refund | Debit, a deduction | Refunds (contra income or the sales account) |
| Each dispute (chargeback), and its reversal | Debit (or credit when won back) | Chargebacks |
| Anything else (adjustments, reserves) | Debit or credit by sign | Fees |
| What reaches the bank | Debit | The bank account |

Deposits so far only take additions (payments and "other" lines) and one cash-back amount. This
change adds **deductions** to a deposit (`deposit_deductions`): debits to other accounts that come
out of what the bank gets, like cash back but several and labelled (fee, refund, chargeback,
other). The bank line of the deposit is the payout's net amount, so the existing bank-line
matching (`bank::matching`) matches the downloaded bank line to it with no new code.

The breakdown is worked out from the payout's **balance transactions**: each has a gross `amount`
and a `fee`; summed, `amount − fee` is the payout. The breakdown must add back to the payout to the
cent or the payout is refused (`PAYOUT_MISMATCH`), so a type LocalBooks doesn't know can't make a
deposit that doesn't tie to the bank.

## Flow

1. **Set up** (Banking › Payout Feeds, Admin): paste the restricted key, choose the bank, sales,
   fees, refunds and chargebacks accounts. The key is checked with one call before it is saved.
2. **Get payouts** fetches paid payouts that arrived in the last 90 days (or since the last fetch,
   overlapping a week) and their balance transactions, and keeps each one's breakdown
   (`payouts`). A payout already fetched is not fetched again.
3. Each new payout shows its sales, fees, refunds, chargebacks and net. **Record deposit** posts it
   (through `banking::write_deposit`, so it is an ordinary deposit that can be voided);
   **Ignore** leaves it out (for payouts already entered by hand).
4. Payouts in a currency other than the company's home currency are listed but refused.

## Limits of this change

Stripe only (PayPal and Shopify stay on the T68 row). No automatic schedule; no matching of a charge
to an open invoice (charges post to the sales account; the user applies them to invoices as
needed). Payout reversals (failed payouts) are not fetched.
