# Payout feeds from payment processors (gaps row T68)

A payment processor (Stripe first; PayPal and Shopify Payments later) collects card sales, takes
its fees, pays back refunds and disputes, and sends the rest to the bank as a **payout**. The bank
statement shows only that net amount, so importing the bank line alone books net sales and hides
the fees. This note sets out how a payout becomes a deposit that shows the gross sales and every
deduction, and how it stays private.

## Privacy

- The feed is **off until the user sets it up** with a key; nothing is fetched before that, and
  only when the user clicks **Get payouts**, or on the schedule an Admin picks for that feed
  (below).
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

## PayPal

PayPal has no payout object. Its balance collects payments, fees, refunds and chargebacks, and a
**withdrawal** to the bank (event codes T0400–T0403) takes money out, so each withdrawal is a payout
here, made of every balance-affecting transaction after the withdrawal before it. They are read
with PayPal's Transaction Search API (`GET /v1/reporting/transactions`, at most 31 days per
request, paged), with a token from the app's client id and secret (OAuth client credentials, the
Transaction Search read scope). Amounts are decimal strings, read to cents without floats.

| Event code | Becomes |
|---|---|
| T00xx with money in | a sale, gross; its fee is a processor fee |
| T01xx, T1108, T1109 | processor fees (or fees given back) |
| T1107, T1115, T1120 | refunds |
| T1106, T1201 (and T1202, T1205 back) | chargebacks |
| anything else (holds and releases, conversions, payments sent) | other, by sign |

Only transactions in the withdrawal's currency count: a payment received in another currency comes in as what its conversion added (an adjustment), never as cents of the wrong currency. The breakdown ties only when a withdrawal took the whole balance; one that took part of it, or whose
withdrawal before it is outside the history read (the fetch reads a month further back for this), is
listed as not kept, with the reason. The client id and secret are kept together in the credential
store; `payout_feeds.account_ref` says whether they are live or sandbox credentials.

## Shopify Payments

A store's custom app with only the `read_shopify_payments_payouts` and
`read_shopify_payments_accounts` scopes has an Admin API access token (kept in the credential
store; the store's `….myshopify.com` address is `payout_feeds.account_ref`). LocalBooks speaks the
GraphQL Admin API (version 2026-10; the REST API is legacy):

- `shopifyPaymentsAccount.payouts`, newest first, until one was issued before the fetch window;
  only `PAID` payouts to the bank (`DEPOSIT`) are kept.
- `shopifyPaymentsAccount.balanceTransactions`, newest first, back to two months before the oldest
  payout wanted, grouped by `associatedPayout`; test transactions are left out.

| Balance transaction type | Becomes |
|---|---|
| CHARGE | a sale, gross; its `fee` is a processor fee |
| REFUND, REFUND_FAILURE, REFUND_ADJUSTMENT | refunds |
| DISPUTE_WITHDRAWAL, DISPUTE_REVERSAL | chargebacks |
| CHARGEBACK_FEE, CHARGEBACK_FEE_REFUND, STRIPE_FEE | processor fees |
| TRANSFER… | the payout itself, skipped |
| anything else (reserves, holds, adjustments, credits) | other, by sign |

Each transaction's `amount − fee` is what it added; they must add up to the payout's `net`.

## Fetching on a schedule

Each feed is fetched only by hand until an Admin picks an interval: every 1, 6, 12 or 24 hours
(`payout_feeds.auto_fetch_hours`, migration 0157). While LocalBooks is open, a task checks every 15
minutes (`commands::run_due_payout_fetches`) and fetches each feed whose interval has passed since
its last fetch or its last automatic try, so a failing feed waits for the next interval too. It runs
only when the company file is open and someone who may change the books is signed in (or the file
has no users), never in an archive: the same rule as clicking Get payouts. It only fetches: each
payout still waits on the screen to be recorded. What the last automatic fetch found, or why it
failed, is kept on the feed (`last_auto_result`) and shown on the screen; it never holds a key.

## Limits of this change

Stripe first; PayPal and Shopify Payments came next (above). No matching of a charge
to an open invoice (charges post to the sales account; the user applies them to invoices as
needed). Payout reversals (failed payouts) are not fetched.
