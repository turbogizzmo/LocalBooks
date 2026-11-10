# Sales orders (gaps row T69)

A customer orders goods or work today that you'll ship and bill later, perhaps in several parts.
LocalBooks has estimates (a price offered) and invoices (a sale posted), but nothing in between, so
there is no record of what customers have ordered and not yet received, nothing to invoice from
piece by piece, and no way to see what stock is already promised. This note sets out sales orders,
how they turn into invoices, the reports on them, and backorders.

## What a sales order is

A sales order is a **non-posting** document, like an estimate or a purchase order. Saving one
writes no journal entry: receivables, income, sales tax and stock stay as they are. Only the
invoices made from it post, through the normal invoice path (`invoices::write_invoice_carrying`),
so every ledger rule (closing date, balancing, the audit trail, inventory cost) applies unchanged.

It holds:

- **Header:** its own number sequence (next free number, with the same duplicate-number warning
  invoices have), the customer or job, the order date, an expected ship date, P.O. No., rep, ship
  via and FOB, Bill To / Ship To (the same `sales_lists` details estimates and invoices carry, as a
  new `SalesDoc::SalesOrder`), the sales tax rate, memo and customer message.
- **Lines:** item, description, quantity ordered (`quantity_e4`), rate, amount, income account,
  class, taxable, and two quantities the system keeps: **invoiced** and **closed**. A line can also
  be a subtotal or other form line, as on invoices.
- **Status,** worked out from the lines rather than typed: **Open** (nothing invoiced), **Partly
  invoiced**, or **Closed** (every line fully invoiced or closed). Closing an order by hand closes
  the rest of every line ("the customer cancelled the rest"); reopening it undoes that.

Amounts are integer cents and quantities × 10,000, as everywhere else. A sales order is in the
company's home currency; a foreign-currency customer's order is refused (`SO_CURRENCY`) until
foreign-currency sales orders are a row of their own.

## Schema (one migration, number claimed in the PR that builds it)

- `sales_orders`: the header, `STRICT`, with `is_closed` for an order closed by hand.
- `sales_order_lines`: the lines, with `invoiced_qty_e4` and `closed_qty_e4`, and a check that
  `invoiced_qty_e4 + closed_qty_e4 <= quantity_e4`.
- `sales_order_invoiced`: one row per invoice line made from an order line (order line, invoice,
  invoice line number, quantity), so each invoice traces back to its order and each order shows its
  invoices.
- Triggers keep `invoiced_qty_e4` honest: a link row adds its quantity, and **voiding the
  invoice's journal entry** takes it back off (an `AFTER UPDATE OF is_voided ON journal_entries`
  trigger, so every void path does it). A voided invoice's quantities become open again.
- Condense keeps sales orders (they hold no ledger rows). Link rows to invoices that a condense
  replaces are dropped; `invoiced_qty_e4` stays as it was, which is why it is stored and not summed.

## From sales order to invoice

**Create Invoice** on an order (or **Sales › Create Invoices** with a customer that has open orders,
which offers them) asks: **all that is left**, or **selected quantities**, where each line shows
ordered, invoiced, left, and a **To invoice** box. The invoice form then opens with those lines,
their rates, tax rate and details. The user can change rates, add lines that aren't on the order
(freight, a discount), and remove lines before posting.

Posting writes the invoice and the link rows **in one write transaction** (`begin_write` …
`commit_write`), as `estimates::invoice_from_estimate` does today, so an order is never marked
invoiced for an invoice that didn't post. Checks, inside that transaction:

- a line can't be invoiced beyond what is left on it (`SO_OVER_INVOICED`); to ship more, edit the
  order first, so the order always says what was agreed;
- a closed order, or a closed line, can't be invoiced (`SO_CLOSED`);
- the invoice's customer must be the order's (or one of its jobs).

**Partial invoicing** is just this, repeated: each invoice takes some of what is left, and the order
stays Partly invoiced until nothing is.

**Editing an order** after part of it is invoiced: a line's quantity can't go below what is already
invoiced, and an invoiced line can't be removed. An order with nothing invoiced can be deleted (it
never posted); after that it can only be closed.

**Editing an invoice** that came from an order keeps its link quantities. To change how much of the
order it took, void it and invoice again. The edit screen says so.

**From an estimate:** an accepted estimate gets **Create Sales Order** next to Create Invoice, which
copies its lines and marks it converted (`converted_to_sales_order_id`), with the same refusals as
converting to an invoice.

## Backorders and inventory

Nothing in stock moves on a sales order: stock leaves, and cost of goods sold posts, on the invoice,
as today. What changes is what LocalBooks **shows**:

- **On sales order** for an item: the quantity left on open order lines.
- **Available** = on hand − on sales order.
- **Backordered** for an order line: the part of what is left that available stock can't cover.

Stock Status and the item's information panel gain On Sales Order and Available columns. The sales
order form warns (and doesn't block) when a line asks for more than is available, like the
credit-limit warning, and marks such lines **Backordered**. When a bill or adjustment brings stock
in, Open Sales Orders by Item shows which backorders can now ship. The reorder-point check keeps
using on hand; whether it should use available is a separate question for the reorder report.

## Reports

All run, export and print like other reports, and tie out to the order lines.

- **Open Sales Orders by Customer:** each open line by customer and order: date, number, item,
  ordered, invoiced, left, backordered, and the open amount (left × rate), with customer subtotals.
- **Open Sales Orders by Item:** the same lines grouped by item, with on hand, on sales order and
  available for each item, so you see what can ship.
- **Pending Sales** (the T9 report): the sales agreed and not yet invoiced, which in LocalBooks are
  the open sales orders' remaining amounts, by customer, with the total. The desktop programs also
  let an invoice be marked "pending" (saved without posting); LocalBooks doesn't add that second
  kind of unposted sale, since a sales order covers the same need and keeps every invoice a posted
  one. If testers ask for pending invoices, they get a row of their own.

The reports go in through `ReportBody` once #276 is in main (it moves the report switch there).

## Screens and the rest

- **Sales › Create Sales Orders** (the form, using the invoice form's layout and keyboard flow),
  **Sales › Sales Orders** (the list, with status filter), and the Customer Center's transactions.
- **Print, PDF and email** through the forms engine, with a Sales Order template in Custom Forms.
- **Preferences › Sales:** sales orders on or off (off hides the menu entries; existing orders stay),
  and their duplicate-number warning, the entry `docs/plans/preferences.md` held back for this.
- **Permissions:** `list_`/`get_` commands are Read; saving, closing, deleting and invoicing are
  Write. Nothing is Admin.
- **Help, guide (`docs/guide/sales-orders.md`), README row and CHANGELOG** with the build.

## Building it

1. This note (its own PR).
2. Core: the migration; `sales_orders.rs` (save, list, detail, close and reopen, delete, invoice
   from an order with partial quantities, the void trigger); commands; the form and list; Open Sales
   Orders by Customer and by Item and Pending Sales; On Sales Order and Available in Stock Status
   and the information panel; Help, guide, README, CHANGELOG.
3. Printing and email, Create Sales Order from an estimate, and the preference.

## Tests the build must pass

- An order posts nothing: no journal rows, balances and stock unchanged.
- Invoice part of an order, then the rest: quantities and status follow; the invoices post as any
  invoice, with cost of goods sold for stock items.
- Over-invoicing, invoicing a closed order or line, and another customer's order are refused, and a
  refused invoice changes nothing.
- Voiding an invoice made from an order reopens its quantities.
- Editing a partly invoiced order can't drop a line below what is invoiced.
- On sales order, available and backordered for an item with stock short; the open-orders reports
  and Pending Sales add up to the order lines' open amounts.
- Condense keeps open orders and their invoiced quantities.
