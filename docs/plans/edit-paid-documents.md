# Editing an invoice or bill that has payments or credits applied

Status: built (gap T20). Migration 0074.

## The problem

Since 0.11.0 a posted invoice, bill or expense opens for editing, and saving replaces it: the
original's journal entry is voided (a reversing entry on its date) and the edited version is posted,
in one write transaction, linked in `transaction_edits` (`src-tauri/src/edits.rs`). Voiding refused
any invoice or bill with payments or credits applied, so the edit refused them too, and the screens
showed "void its payments first". People moving from QuickBooks Desktop expect to change a part-paid
invoice (a wrong rate, a forgotten line) and keep the payment where it is; QuickBooks shows a warning
and lets them.

## What a payment is tied to

A received payment, a bill payment, an applied credit memo or vendor credit, and a bounced check all
point at their document by id (`invoice_payments.invoice_id`, `bill_payments.bill_id`,
`credit_applications.invoice_id`, `vendor_credit_applications.bill_id`, `bounced_checks.invoice_id`).
Their journal entries do not: a payment's entry debits the bank (or Undeposited Funds) and credits
A/R, and never names the invoice's entry. So the general ledger does not care which invoice a payment
is applied to; only the open balances (`amount_paid_cents`, `v_invoice_balances`, the aging reports)
do.

That means a replacement can keep every payment applied by pointing those rows at the new document.
No posted journal entry changes: the payment's entry, its deposit and any reconciliation that cleared
them are untouched.

## The edit itself

In one write transaction (`begin_write` … `commit_write`, so the audit rows are sealed before
commit), `edits::replace_invoice` / `replace_bill`:

1. checks the original is live (not voided, posted);
2. voids the original's journal entry with `ledger::void_for_replacement`: a reversing entry on the
   original's date, with every voiding rule of `void_in_tx` except "has payments or credits" and
   "has a bounced check": reconciled or ticked lines, closed period, reclassified lines, inventory
   already used. The document itself is not marked void yet. Voiding first means stock the original
   sold is back before the edited version sells it;
3. posts the edited document (a new invoice or bill, a new number for invoices, as today);
4. records the edit in `transaction_edits` (original entry → new entry) and moves attachments;
5. if anything is applied, checks the edit may keep it (same customer or vendor, same currency and
   rate, a total that still covers it; below);
6. calls `ledger::relink_applied`, which points every live payment, applied credit and live bounced
   check at the new document, recomputes `amount_paid_cents` and status on both documents (the
   original's drops to zero) and then marks the original document void;
7. writes one `EDIT` record in the ledger audit trail.

Any error rolls the whole transaction back, so a refused edit changes nothing.

Voided payments and removed credit applications stay on the original: they are history of that
version and count toward nothing.

### Why the ledger rules still hold

- Only `ledger.rs` writes journal rows: the replacement posts through `post_entry_in_tx` and voids
  through `void_for_replacement` (the same code as `void_in_tx`); re-linking writes no journal rows at all, and lives in `ledger.rs` next to
  `recompute_paid`.
- Posted entries are never edited: the original is reversed, not changed; payments' entries are not
  touched.
- Debits equal credits: every entry involved goes through `validate_entry` and the balance trigger.
- A/R and A/P still agree with the aging reports: the GL A/R balance is (new invoice total) −
  (payments), and the aging's open balance is (new invoice total) − (re-linked payments). The same
  holds for bills and A/P. The tests check both after a part-paid edit.

### The schema guards

Payment and application rows are immutable by trigger: their document id could never change. Migration
0074 recreates those triggers with one exception, so a re-link is possible only as part of a recorded
edit:

- the row is live (not voided);
- a `transaction_edits` row links the old document's journal entry to the new document's;
- the new document is live and has the same customer (or vendor); for invoices, also the same
  currency and exchange rate.

Every other column stays immutable, exactly as before.

## When the new total is less than what's applied: blocked

Saving is refused with `EDIT_BELOW_PAID` ("this invoice has $X in payments and credits applied; the
edited total $Y is less"), and nothing changes.

Why not leave the excess as a customer or vendor credit, as QuickBooks does? In LocalBooks a payment
always belongs to one invoice (`invoice_payments.invoice_id` is NOT NULL, the amount is immutable, and
there is no "unapplied payment" record). Leaving an excess would mean either splitting a recorded
payment, which changes a posted amount, or inventing a credit memo the person did not enter, with its
own journal entry and income reversal. Both are worse than a clear message. The person can void or
remove the payment or credit that is too much (or issue a refund or credit memo deliberately), then
make the edit. If LocalBooks later gains unapplied customer payments and vendor prepayments, this
rule can relax to "leave the excess as a credit".

## Still refused

| Case | Rule | Why |
|---|---|---|
| Voided document | `EDIT_VOIDED` | A voided document has nothing to replace. |
| A line of the document's own entry is reconciled or ticked | `VOID_RECONCILED` / `VOID_CLEARED` | Reversing it would change a reconciled balance. |
| Document dated in a closed period, or the edit dated in one | closing-date check | The reversal is dated on the original's date. |
| Lines moved by a class reclassification | `VOID_RECLASSIFIED` | Unchanged voiding rule. |
| New total below what's applied | `EDIT_BELOW_PAID` | See above. |
| Payments applied and the customer or vendor changed | `EDIT_PAID_PARTY` | The payments came from that customer; they can't follow to someone else. Remove them first. |
| Payments applied to a foreign-currency invoice and its currency or rate changed | `EDIT_PAID_CURRENCY` | Each payment's A/R relief was computed at the invoice's rate; a new rate would leave A/R and the aging apart. |

### Payments in a deposit, even a reconciled one: allowed

A payment that was deposited (or matched to a deposit in Clean Up Books), even in a deposit that is
part of a completed bank reconciliation, stays where it is. The edit reverses and reposts only the
invoice's own entry (A/R, income, tax, discount); the payment's entry and the deposit's entry, which
are the ones on the bank register, are not touched, so no reconciled balance moves. The same is true
for bill payments that cleared the bank or went out in an ACH file.

A bounced check recorded against the invoice moves with its payment, so the invoice stays reopened by
the bounce.

## Audit trail

For every replacement, three things are recorded, all inside the edit's transaction:

- The usual hash-chained `POST` record for the new entry and `VOID` record for the original (written
  by the triggers).
- The `transaction_edits` row linking the original entry to its replacement.
- A new `EDIT` record in the ledger audit trail (action type added by migration 0074), written for
  every replacement; expenses, checks and journal entries simply list nothing re-linked. Its
  payload names the document kind, the old and new document ids and entry ids, and every row that
  was re-linked: kind (`PAYMENT`, `CREDIT`, `BILL_PAYMENT`, `VENDOR_CREDIT`, `BOUNCED_CHECK`), its
  id and amount. It is sealed into the chain like every other
  record, so "which invoice was this payment applied to before" can always be answered.

## Screens

Invoices and Bills open a paid document for editing. A banner over the form says, for example,
"This invoice has $250.00 in payments or credits applied. Saving will keep them applied to the new
version." and saving asks once more with the same sentence. Expenses and checks have no applied
payments and are unchanged.
