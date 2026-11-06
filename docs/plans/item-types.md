# Item types: Subtotal, Discount, Other Charge, Group (and later Payment and tax items)

People moving from QuickBooks Desktop build invoices out of more than products and services. They
put a **subtotal** under a block of lines, take a **discount** off one line or off that subtotal,
add a **charge** such as shipping or a 3% handling fee, and pick a **group** that drops a set of
lines onto the form at once. This note says how LocalBooks does each, before any code. It is the
plan behind `docs/gaps.md` row T10 and the parity survey's top-15 item #11
(`docs/plans/desktop-parity.md`).

The behaviour described here is what a Desktop user expects, written in our own words from public
product knowledge. Nothing is copied from vendor documentation.

## What LocalBooks has today

- One item list, `products_services`: name, description, rate, income account, taxable. An item is
  a service or a non-inventory part; one with a row in `inventory_items` is a tracked part.
- Sales lines (`invoice_items`, `sales_receipt_lines`, `estimate_lines`) are always item lines:
  quantity × rate = amount, the amount at least one cent (`CHECK`), credited to an income account.
- A **document discount**: one amount or percent in the form's Discount field, taken off the
  subtotal before tax, reducing the taxable amount in proportion, debited to *Discounts Given*
  (`sales_tax::document_discount`, `discount_cents` on each document).
- One sales tax rate per document, on the taxable lines (`sales_tax::document_tax`).

## The rules, in brief

| Type | On the form | Posts | Taxable |
|---|---|---|---|
| **Subtotal** | Sums the lines above it, back to the previous subtotal | nothing | – |
| **Discount** | An amount, or a percent of the line above (a subtotal, a group or an item) | debit to the discount item's account | the item says; a taxable discount lowers the taxable amount |
| **Other Charge** | A fixed amount (quantity × rate), or a percent of the line above | credit to the charge item's account | the item says |
| **Group** | A heading line, then its items with their quantities; may print as one line | the member lines post as items | each member's own flag |

Everything else stays as it is: the document discount field still works and still comes off last,
before tax; inventory, price levels and classes work on the lines that are items.

### Which line a discount or percent charge applies to

"The line above" is the nearest line before it, read this way:

- an item line or a fixed charge: that line's amount;
- a subtotal: the subtotal's amount;
- the last line of a group: the group's total (a group is one block);
- a discount or a percent charge: not allowed (QuickBooks allows it and the result surprises
  people; we refuse it with `ITEMS_DISCOUNT_NEEDS_BASE` and say to put a subtotal above).

A **subtotal** is the sum of every line since the previous subtotal (or the top of the form):
items, charges and discounts. Discounts are negative, so a subtotal below a discount is net of it.

A percent discount or charge is `mul_div_round(base, ppm, 1_000_000)` (half away from zero). The
base must be above zero. A fixed discount may not exceed its base (`ITEMS_DISCOUNT_TOO_LARGE`), and
a discount must leave a positive amount, so no line or block is taken below zero.

### Totals, in order

1. *Lines* = item lines + charges − line discounts (subtotals add nothing).
2. *Taxable* = taxable item lines + taxable charges − taxable line discounts. If a taxable discount
   would take this below zero the form is refused (`ITEMS_TAXABLE_NEGATIVE`).
3. The **document discount** (the Discount field) comes off *Lines*, and off *Taxable* in
   proportion, exactly as now (`sales_tax::taxable_after_discount`).
4. Tax = rate × taxable, rounded once for the document.
5. Total = *Lines* − document discount + tax. It must be above zero.

`documentTotals` in `src/components/SalesTax.jsx` does the same steps on the form, so the total the
user sees is the total that posts. It takes the line layout (below) instead of a flat list of item
amounts; the Rust and JavaScript versions share a table of cases in the tests.

## Storage (migration `0067_item_types.sql`)

### The item list

`products_services` gains two columns (`ALTER TABLE ... ADD COLUMN`, no rebuild):

- `item_type TEXT NOT NULL DEFAULT 'ITEM'`: `ITEM` (service, non-inventory or inventory part, as
  today), `OTHER_CHARGE` or `DISCOUNT`. The allowed values are checked by triggers, not a column
  `CHECK`, so a later migration can add `PAYMENT` and `SALES_TAX` by replacing the triggers instead
  of rebuilding the table.
- `percent_ppm INTEGER`: set for a percent charge or discount (parts per million, 1 to 1,000,000);
  `NULL` means the item is an amount and its rate is the amount. Only `OTHER_CHARGE` and
  `DISCOUNT` items may have one.

Triggers keep the list consistent: only `ITEM` items can be tracked in stock (`inventory_items`)
or given price-level prices; a discount or charge item's account is an income account (the existing
`trg_product_income_account` triggers already require that). A discount's account defaults to
*Discounts Given*, the account the document discount uses. Expense accounts for charges
(reimbursed costs) are later work.

**Subtotal is not an item.** It has nothing to set (no account, no rate, nothing taxable), so the
item column offers a fixed **Subtotal** entry instead of making the user create one. Imports that
meet a subtotal item turn it into a subtotal line.

**Groups** have their own tables, since a group has no account or rate of its own:

```sql
CREATE TABLE item_groups (
    id, name (unique, case-insensitive, and not the name of an item), description,
    print_items INTEGER NOT NULL DEFAULT 1,   -- 0: print the group as one line
    is_active, created_at
) STRICT;
CREATE TABLE item_group_members (
    group_id REFERENCES item_groups ON DELETE CASCADE,
    position INTEGER,                         -- order on the form
    product_id REFERENCES products_services ON DELETE RESTRICT,
    quantity_e4 INTEGER CHECK (quantity_e4 > 0),
    PRIMARY KEY (group_id, position)
) STRICT;
```

A member is an `ITEM` or a fixed `OTHER_CHARGE` item; not a discount, a percent charge or another
group (nesting is out of scope, as is a discount inside a group).

### The lines on a form

Item lines and fixed charges stay where they are, in `invoice_items`, `sales_receipt_lines` and
`estimate_lines`, unchanged. Every report, the sales tax report, inventory and classes that read
those tables keep working. A **percent charge** is written there too, as quantity 1 at a rate equal
to its computed amount, with a new nullable `percent_ppm` column recording the percent so the form
and the printout can show "3%" in the rate column.

Subtotals, discounts and group headings cannot go in those tables (their amounts are zero or
negative, which the tables' `CHECK`s rightly forbid, and changing a `CHECK` means rebuilding a table
that a dozen triggers read). They go in one new table per document, laid out like its line table:

```sql
CREATE TABLE invoice_form_lines (
    id INTEGER PRIMARY KEY,
    invoice_id INTEGER NOT NULL REFERENCES invoices (id) ON DELETE CASCADE,
    line_number INTEGER NOT NULL CHECK (line_number > 0),  -- shares numbering with invoice_items
    kind TEXT NOT NULL CHECK (kind IN ('SUBTOTAL', 'DISCOUNT', 'GROUP')),
    description TEXT NOT NULL,
    amount_cents INTEGER NOT NULL,       -- SUBTOTAL: the sum shown (≥ 0); DISCOUNT: < 0; GROUP: the group's total
    product_service_id INTEGER REFERENCES products_services (id),   -- DISCOUNT: the discount item
    percent_ppm INTEGER,                 -- DISCOUNT: when a percent
    account_id INTEGER REFERENCES chart_of_accounts (id),           -- DISCOUNT only
    class_id INTEGER REFERENCES classes (id),                       -- DISCOUNT only
    is_taxable INTEGER NOT NULL DEFAULT 0,                          -- DISCOUNT only
    item_group_id INTEGER REFERENCES item_groups (id),              -- GROUP only
    group_quantity_e4 INTEGER,           -- GROUP: how many of the group
    group_line_count INTEGER,            -- GROUP: the item lines that follow and belong to it
    print_items INTEGER,                 -- GROUP: 0 prints the group as one line
    CHECK (... the columns each kind uses, and no others ...),
    UNIQUE (invoice_id, line_number)
) STRICT;
-- and sales_receipt_form_lines, estimate_form_lines, the same with their parent's key
```

The two tables share one line numbering: the form's first line is 1 whatever kind it is, so the
document reads back in the order it was entered. Rust assigns the numbers in one place and a trigger
refuses a number already used in the other table.

They get the same immutability triggers as their line tables: an invoice's form lines change only
while it is a draft; a posted sales receipt's never; estimate lines as `0020` allows.

The documents' post-check triggers (`trg_inv_post_checks`, `trg_sr_update_rules`, and the estimate
total check) are dropped and recreated with the discount lines in the sum:

```
total = SUM(item lines) + SUM(discount form lines, which are negative) − discount_cents + tax_cents
```

`discount_cents` keeps meaning the document discount only, so nothing that reads it today counts a
line discount twice.

## Input and posting (through `ledger.rs` only)

The forms send their item lines in `lines` as now, plus `formLines`: the subtotals, discounts,
percent charges and group headings, each with `afterLines` (how many item lines come before it).
`formLines` travels in the flattened `SalesDetails`, so every existing caller and test that builds
an `InvoiceInput`, `SalesReceiptInput` or `EstimateInput` with `details: Default::default()` is
unchanged and other branches don't conflict.

A new module, `src-tauri/src/item_types.rs`, lays the form out:

```rust
pub fn lay_out(items: &[ItemAmount], form_lines: &[FormLineInput], code: &str) -> Result<Layout, String>
```

It merges the two lists into line order, computes subtotals, discounts and percent charges by the
rules above, and returns each line's number and amount, the extra item rows the percent charges add,
the discount lines to write and post, and the document's *Lines* and *Taxable*. It is pure, so it
is tested without a database, and every write path (`invoices::write_invoice_carrying`,
`sales::create_sales_receipt`, `estimates::create_estimate`) calls it before anything is written.

Posting stays in the ledger. The writers build one `JournalEntry` as now and post it with
`ledger::post_entry_in_tx`:

- item lines and charges: credits to their income accounts (by account and class, as now);
- each discount line: a **debit** to its account, with its class;
- the document discount and the tax: as now;
- A/R (invoice) or the deposit account (sales receipt): the total.

Groups and subtotals add no journal lines. Estimates post nothing.

`trg_je_post_must_balance` and `ledger::validate_entry` check the result as for any entry; nothing
in either is relaxed.

## How it meets the rest of LocalBooks

- **Sales tax.** Each discount or charge line is taxable or not as its item says (the user can
  change it on the line, as for items). One rate per document still; the combined rates, per-agency
  split and tax items of gap R11 are not part of this. The sales tax liability report counts
  taxable and non-taxable discount lines on the right side.
- **Sales Tax Item and Tax Group.** Not built here. In Desktop these are items placed on the form;
  in LocalBooks the tax is the document's rate (`tax_rates`), and R11 will add combined rates
  (a group of component rates, each with its agency) to that table. When R11 lands, a *Tax Group*
  is a combined rate, not an item, and a *Sales Tax Item* is a rate. The `item_type` triggers leave
  room for a `SALES_TAX` item if imports ever need one; this note recommends against it.
- **Price levels.** They set the rate of item lines and fixed charges, including the members a group
  drops on the form (each member priced for the customer). They never change a discount, a percent
  charge or a subtotal. `price_level_items` refuses discount items.
- **Inventory.** Group members that are tracked parts relieve stock and post cost of goods sold
  through `inventory::cost_of_sale`, as if entered one by one. Discounts don't change cost.
- **Classes.** A discount line has its own class (defaulting to the line above's); the form lines
  tables are added to `classes::CLASS_USERS`.
- **Printing.** Subtotal lines print in bold with the amount; discount lines print the item's
  description, "10%" or the amount in the rate column, and a negative amount; a percent charge
  prints its percent. A group printed as one line shows the group's description, quantity and
  total, and its members are left off the printout (they stay on screen and in reports).
- **Editing and copying.** `invoice_detail`, `estimate_detail` and the sales receipt detail return
  form lines with the item lines, merged by line number, and the forms load them back the same way.
  Editing a posted invoice (void and re-post, `edits::replace_invoice`) and turning an estimate into
  an invoice carry them over. A group already on a form doesn't change when the group's definition
  changes.
- **Reports by item.** Sales by Product/Service lists discount items as their own rows (negative),
  and charges as items (they are in the item tables). Group members appear as themselves; a group
  is not a report row, as in Desktop.
- **Import from QuickBooks files.** `qbw_documents.rs` reads an invoice's lines with their item and
  ledger amount. The full import (`qbw_full_import::form`) already turns lines that post the other
  way into the document discount, and posts the file's own journal lines, so imported books stay
  exact. With this work, lines whose item is a known discount item become discount lines on that
  account instead, and other-charge items (`item_type` 4, already decoded) come in as
  `OTHER_CHARGE` items. The item-type codes for subtotal, discount and group items are not decoded
  yet; until they are, imports keep today's behaviour. IIF and QIF imports are unchanged.
- **Recurring invoices, finance charges, statements.** Recurring templates store the input JSON,
  which now may hold `formLines`; finance charges and statements don't add form lines.
- **Credit memos and purchase forms.** Not in this step. The item column offers the new kinds only on
  invoices, sales receipts and estimates.

## Out of scope (later work)

- **Payment item** (a payment taken on the invoice itself). It mixes a receipt into an invoice; in
  LocalBooks a payment is its own record linked to the invoice, so it is better done as "Receive a
  payment when saving" on the invoice form, writing an `invoice_payments` row in the same
  transaction. A later row in `docs/gaps.md`.
- **Sales Tax Item and Tax Group**: with R11, as above.
- Discounts and charges on credit memos, bills, checks and purchase orders.
- Expense accounts on other charges (reimbursable costs) and charges that are a percent of the
  whole form rather than the line above.
- Groups inside groups, and discounts inside groups.
- Decoding the subtotal, discount and group item types in company files.

## Building it

One commit per type, on sales forms (invoices, sales receipts, estimates), in this order:

1. **Subtotal**: migration `0067` (all of the storage above, so the schema lands once), `item_types.rs`,
   form-lines tables written and read back, the grid's Subtotal entry, printing.
2. **Discount**: discount items, discount lines posting, totals in Rust and `documentTotals`.
3. **Other Charge**: charge items (amount or percent), percent charges laid out and written.
4. **Group**: groups in the item list, expanding onto the form, print as one line.

Each commit adds Rust tests that the documents post balanced and that their total equals the posted
entry's A/R (or deposit) line, and that the JavaScript totals agree with the Rust ones on the same
cases.
