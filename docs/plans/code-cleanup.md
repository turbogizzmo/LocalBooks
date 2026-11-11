# Code cleanup: Clippy style warnings (A6) and `Cents` in documents (A7)

Both gaps touch most files in `src-tauri/src`, so either one conflicts with every open feature
branch. They land last, after the feature PRs in flight have merged, in small PRs that each merge
the same day they open. This note gives the sizes, measured on `main` at 6a952aa7 (2026-11-09),
and the order.

## Status

- **A6a (mechanical), done** on `main` at c58f7dd8: `cargo clippy --fix`, plus by hand the 8 small
  warnings it skipped (`Some(x).filter` → `bool::then`, a `matches!`, two literal patterns, a doc
  list) and lopdf's deprecated `load_mem_with_password` in two tests. **53** warnings are left, all for
  A6b: 45 `type_complexity`, `enum_variant_names` (custom_reports), `too_many_arguments`
  (inventory), and 6 dead-code items (`bank::payee_rules::suggested_pattern`, `reconcile::start` and
  `report_pdf::to_pdf`, used only by tests; `party_contacts::statement_contact`, used nowhere;
  `CustomerFacts::name`; a Shopify constant). Each dead item is either connected to the screen
  it was written for or kept as test-only with `#[cfg(test)]`. That is a decision, not a mechanical fix.
- **A6b and A6c, done** in one PR (#343): the 45 complex types became local `type` aliases next to
  their query, `record_movement` takes a `Movement` struct, `Cleared::Cleared` keeps its name (it is
  the JSON value) with the lint allowed on that enum, and the dead code went: three test-only
  helpers (`suggested_pattern`, `reconcile::start`, `report_pdf::to_pdf`) are `#[cfg(test)]`, and
  `statement_contact`, `CustomerFacts::name` and Shopify's `PAGE` were removed, as nothing read
  them. `lint.yml` and `AGENTS.md` now run `cargo clippy --all-targets --locked -- -D warnings`.

## A6: Clippy warnings

`cargo clippy --all-targets --locked` reports **142** Clippy warnings (the gaps row says 111; new
code since then added the rest) in 57 modules, plus **9** compiler warnings that `-D warnings`
would also fail on.

| Lint | Count | `--fix` handles it | Most in |
|---|---:|---|---|
| `needless_borrow` | 62 | yes | qbw_ledger_import 5, reconcile 5, bank 4, classes 4, qbw_import 4 |
| `type_complexity` | 38 | no | job_reports 4, list_reports 4, condense, inventory, ledger, report_center 2 each |
| `explicit_auto_deref` | 11 | yes | ach, expenses, sales_lists 2 each |
| `manual_is_multiple_of` | 5 | yes | nacha 2, ledger, print_batches, ach |
| `let_unit_value` | 3 | yes | commands |
| `filter_next` | 3 | yes | customer_groups, text_import, harness |
| `unnecessary_sort_by` | 3 | yes | qbw_company 2, sales_tax |
| `doc_lazy_continuation` | 3 | yes | qbw_pages |
| `redundant_guards` | 2 | yes | custom_reports, nacha |
| 12 other lints, 1 each | 12 | 10 of them | `enum_variant_names` (custom_reports) and `too_many_arguments` (inventory) need a hand |

So **102** go with `cargo clippy --fix`, and **40** need a hand:

- 38 `type_complexity`: mostly tuple rows read with `sqlx::query_as` (`Vec<(i64, String, Option<i64>, …)>`).
  These become a named `type Row = (…);` next to the query, as several modules already do, or a
  small `#[derive(sqlx::FromRow)]` struct where the tuple is used more than once. Behavior does not change.
- `enum_variant_names` in custom_reports and `too_many_arguments` in inventory: rename the variants,
  or allow the lint on that one item with a `reason`. The inventory function takes a params struct.

The compiler warnings are 4 `deprecated` (`lopdf` `load_mem_with_password`, in pdf and
report_schedules tests), 4 `dead_code` (one is `CustomerFacts::name`) and 1 `unused_imports`.

### Order

1. **A6a, the mechanical PR:** run `cargo clippy --fix --all-targets --locked`, then the compiler
   warnings. It's one commit, made by the tool and reviewed by reading the diff, not by hand. It
   touches about 50 files, so open it right after a batch merges and merge it the same day.
2. **A6b, by hand:** the 38 `type_complexity` and the 2 named lints, one module group per commit
   (reports, import, inventory and ledger, the rest).
3. **A6c, the gate:** `lint.yml` moves to `cargo clippy --all-targets --locked -- -D warnings`.
   This PR is short: lint.yml, the AGENTS.md command table and the gaps row. From then on a new
   warning fails CI, so the count can't creep back up as it did from 111 to 142.

Each step is re-measured just before it opens, since feature PRs will have added warnings.

## A7: `Cents` in documents and commands

**Status.** Stage 1 (#335): the sales tax payment input. Stage 2a (#346): the invoice inputs
(`InvoiceLineInput.rate_cents`, `InvoiceInput.discount_cents`, `PaidNow.amount_cents`,
`PaymentInput.amount_cents` and `discount_cents`). Each input is read once with `.get()` where it
enters the ledger code; the invoice total and the memorized-invoice summary now use checked
arithmetic. Most of the diff is call sites that build these structs, rewritten from the compiler's
error spans. Stage 2b (#382): the sales receipt and estimate inputs (`SalesLineInput.rate_cents`,
`SalesReceiptInput.discount_cents`, `EstimateInput.discount_cents`), the same way; the receipt and
estimate totals now use checked arithmetic. Stage 2c (#384): `ProductInput.rate_cents` and
`FormLineInput.amount_cents` (a re-edited discount is negated with `checked_neg`). Stage 2d: the
credits inputs (`ApplyCreditInput`, `RefundCreditInput`, `UnappliedPaymentInput` and
`VendorCreditLineInput`, each `amount_cents`). Stages 2e–2h (batch fifteen, #396): bills, checks,
expenses, item receipts, deposits, transfers, bill payments and reconcile. Stage 2i: the purchase
order inputs and views (`PurchaseOrderLineInput.rate_cents` and `amount_cents`,
`PurchaseOrderView.total_cents`, `PurchaseOrderLineView.rate_cents` and `amount_cents`); a bill made
from an order now copies the line amounts as they are. That finishes step 1 for the modules it
lists; 27 `Deserialize` cents fields in other modules (bounced checks, statement charges, mileage,
vouchers, register entry, cleanup and others) wait for step 4. Next: step 2,
the document views of invoices, sales, bills and expenses, then step 3 (reports).

`Cents` (`money.rs`) is used today by `ledger` (journal lines and entry totals), `payroll_import`
and `item_prices`. Everywhere else an amount is a bare `i64` named `*_cents`:

- **373 struct fields** in 65 modules: 325 `i64`, 47 `Option<i64>`, 1 `Vec<Option<i64>>`.
  - 300 are in views sent to the screens (`Serialize`); 128 of those are also read straight from
    SQLite (`sqlx::FromRow`).
  - 53 are in inputs from the screens (`Deserialize`); 17 are internal.
- **3 Tauri command parameters** (reconcile, sales_header, sales_prefs).
- About 2,760 `*_cents` names in non-test code. Most are local variables and arithmetic that
  follows the field's type.

Modules with the most fields: cleanup 24, report_center 23, invoices 21, statements 17, graphs 17,
workflows 15, payout_feeds 13, expenses 11, cash_forecast 11, bank 10, sales 10.

`Cents` is `#[serde(transparent)]` and `#[sqlx(transparent)]`, so changing a field's type changes
neither the JSON nor the schema. The screens and `src/lib/api/` are untouched. The work is in the
Rust that does arithmetic on the field: `a + b` becomes `a.checked_add(b)`, with the overflow
handled where it can happen. That is the point of A7.

### Order

The gaps row's acceptance test is invoices, bills, expenses, payments and reports. They go first,
in this order, one PR each:

1. **Inputs first** (the 53 `Deserialize` fields): an amount the user typed can't reach arithmetic
   unchecked. Invoices, sales receipts, estimates and credit memos (`invoices`, `sales`,
   `estimates`, `credits`, `item_types`), then bills, checks and expenses (`expenses`, `workflows`,
   `item_receipts`), then payments (customer payments in `invoices`, bill payments in `expenses`, deposits and
   transfers in `bank`, `reconcile`).
2. **Document views** of the same modules: the `FromRow` + `Serialize` structs and the code that
   totals them.
3. **Reports** (`report_center`, `reports`, `statements`, `graphs`, `cash_forecast`,
   `custom_reports`, `job_reports`, `list_reports`): mostly read-only sums, which become
   `Cents::checked_sum`.
4. **The rest**, module by module: cleanup, payout_feeds, import modules, etc. Two rules apply here.
   - A field read from a foreign file before it's checked stays `i64` until the check, and is
     converted there.
   - Scaled quantities and rates (`*_e4`, `*_ppm`, rate × 1,000,000) are not `Cents` and stay `i64`.
     The type is what stops them being added to an amount.

Each PR adds `Cents` methods only when a call site needs them (for example `checked_neg` or
`abs`), with a unit test in `money.rs`. It never adds `Add`/`Sub` impls.

### What finishes A7

The gaps row's acceptance test: invoice, bill, expense, payment and report structs use `Cents`, and
the JSON stays a bare integer. That is steps 1–3; step 4 is follow-on and gets its own gaps row
when step 3 lands.

## Sequencing between the two

A6a (mechanical) lands first, since `--fix` output is easiest to redo on a moving `main` and it
shrinks every later diff. A6b and A7 step 1 can then run at the same time, because they touch
different code: A6b is query row types, and A7 is struct fields and arithmetic. A6c (the gate)
goes in once A6b is done. Each A7 PR then has to pass `-D warnings` as well.

Each PR follows `docs/dev/parallel-work.md`: it is 0 commits behind `main` with CI green on that
head, it has no migrations, and it changes no behavior. The tests are the existing suite, plus
`money.rs` unit tests for any new `Cents` method.
