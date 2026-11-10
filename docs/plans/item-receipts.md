# Item receipts (receive items before the bill)

People moving from desktop accounting software receive stock against a purchase order before the
vendor's bill arrives: the goods are on the shelf and can be sold, but there is nothing to pay
yet. When the bill comes, they turn the receipt into a bill. This note is the design for that.
It was split out of T8 as T12 (`docs/gaps.md`) because it needs decisions the rest of T8 didn't.

## What we have

- **Purchase orders** (`workflows.rs`, migration 0028) don't post. A PO turns into a bill in one
  step, all lines at once; there is no record of what has been received line by line.
- **Inventory** (`inventory.rs`) is moving average cost. Stock comes in on a bill line for a
  tracked product (Dr the item's Inventory Asset), goes out on invoices and sales receipts (Dr
  COGS / Cr Inventory Asset), and every movement is a row in `stock_ledger_entries` tied to the
  journal entry that carries it, so voiding the entry moves the stock back. Stock may be sold
  before it is bought; the cost is trued up when the purchase arrives.
- **Bills** post Dr expense or Inventory Asset / Cr A/P, and A/P aging, Pay Bills, vendor
  balances and the 1099 summary all read `bills`.

## How it is usually modelled

The common desktop behaviour: an item receipt posts **Dr Inventory Asset / Cr Accounts Payable**
straight away, but the receipt is not a bill. It is left out of Pay Bills and shown apart in A/P
reports, and it carries the received quantities against the PO lines. Entering the bill for the
received items turns the receipt into a bill in place, at the bill's prices.

The alternative most accounting texts give is a **clearing (accrual) account**: the receipt posts
Dr Inventory Asset / Cr "Items Received Not Billed" (a current liability); the bill later posts Dr
that account / Cr A/P. A/P then only ever holds real bills.

## Decisions needed

1. **Where the receipt's credit goes.** A/P (familiar to people who used desktop software, and
   their A/P total includes received-not-billed goods) or a clearing liability (A/P aging, Pay Bills
   and the vendor balance stay as they are, and nothing reading `bills` changes). *Recommended:
   the clearing account*, created as a system account like Undeposited Funds, with the A/P Aging
   report showing an "Items received, not billed" line underneath so the total people expect is
   still one glance away.
2. **Price differences.** The bill can differ from the receipt (a price change, freight, a short
   shipment). With moving average cost, the difference on units still in stock goes to Inventory
   Asset; on units already sold, to COGS, the same true-up `inventory.rs` already does for stock
   sold before it was bought. *Recommended:* reuse that path, so the bill posts Dr clearing (receipt
   amount) ± Inventory/COGS difference / Cr A/P (bill amount).
3. **Turning a receipt into a bill.** Replace the receipt (void it and post the bill, as edits do)
   or post the bill alongside it and close the receipt. Voiding would move stock out and back in,
   re-costing later sales. *Recommended:* post the bill alongside (Dr clearing / Cr A/P), link it to
   the receipt, and mark the receipt billed. Stock doesn't move twice.
4. **Partial receiving.** Each receipt line points at a PO line and carries a quantity; a PO line is
   closed when received quantity ≥ ordered. Over-receiving is allowed with a warning. This needs a
   `received_quantity_e4` view over receipt lines, not a stored counter.
5. **Voiding a receipt** that has been billed is refused (void the bill first); an unbilled one
   voids like any document and moves its stock back, refused if later sales would go negative under
   the rules `inventory::reverse_for_entry` already applies.
6. **Non-stock lines** on a receipt (freight, a service) are allowed and post to their expense
   account against the same clearing account.

## Shape of the work (for the PR that builds it)

- Migration: `item_receipts`, `item_receipt_lines` (with `purchase_order_line_id`), a `bill_id`
  link, the clearing system account, triggers in the house style (immutable once posted, void only
  through its entry). A new `EntrySource` needs a `journal_entries.source_type` value, which means
  rebuilding that table's CHECK; to avoid that, post as `JOURNAL` with the receipt row holding the
  entry id, as credit refunds and bounced checks do.
- `item_receipts.rs` posting through `ledger::post_entry_in_tx` and stock through `inventory.rs`.
- Screens: Vendors › Receive Items (pick a vendor's open PO, enter quantities received) and Vendors
  › Enter Bill for Received Items (pick a receipt, bill form prefilled).
- Reports: Open Purchase Orders shows received quantities; A/P Aging shows the received-not-billed
  line; Inventory Valuation is unchanged.
- Tests: balanced and voidable postings, partial receipts closing a PO, price differences with and
  without units sold in between, refusals.

## What was built (T12)

Each open decision above took its recommended default:

1. **The clearing account.** Receipts credit **Items Received Not Billed**, a system current
   liability created on first use. A/P, Pay Bills and vendor balances don't change, and A/P Aging
   Summary shows "Items received, not billed" under its total (`item_receipts::received_not_billed`).
2. **Price differences** go through `inventory::plan_price_difference`. For tracked stock, the share
   for units still on hand goes to Inventory Asset (a `COST_TRUE_UP` movement on the bill's entry, so
   voiding the bill reverses it) and the share for units sold goes to COGS. For other lines the
   difference goes to the line's own account.
3. **The bill is posted alongside the receipt** and linked to it in `item_receipt_bills`. A receipt
   counts as billed while that bill's entry stands. Voiding the bill reopens the receipt, and a
   receipt's bill can't be edited (`EDIT_RECEIPT_BILL`), because an edit would bring its stock in a
   second time.
4. **Partial receiving.** Receipt lines point at PO lines. What was received is a query over live
   receipts, not a stored counter. The order closes once every line has been received in full, and
   reopens if a receipt is voided. Receiving more than was ordered is allowed after a warning on the
   screen. An order with receipts is billed through them (`PO_HAS_RECEIPTS`).
5. **Voiding** a receipt goes through its journal entry (`ledger::void_entry` calls
   `item_receipts::before_void`). It is refused while a live bill uses the receipt
   (`VOID_RECEIPT_BILLED`). Otherwise the receipt's stock moves back under the usual stock rules,
   which allow stock below zero when some of it was already sold, just as voiding a bill does.
6. **Non-stock lines** (freight, a service) post to their expense account against the same clearing
   account.

The receipt's entry is a `JOURNAL` entry named by `item_receipts.journal_entry_id`, and the
journal-entry editor refuses it. Migration 0118. The screen receives against open purchase orders;
the engine also takes receipts without one, but no screen offers that yet.
