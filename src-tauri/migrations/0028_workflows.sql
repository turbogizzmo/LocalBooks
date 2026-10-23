-- Workflow features from classic desktop accounting software.
--
-- * transaction_edits: an edited transaction is voided and re-posted in one step; this links the
--   original entry to its replacement.
-- * Recurring (memorized) transactions: a saved journal entry, expense or check, bill or invoice
--   with a schedule. Entering one posts it through the normal path, dated on the schedule.
-- * Purchase orders: what was ordered from a vendor; they post nothing until turned into a bill.
-- * Jobs: a customer can have jobs (sub-customers). A job is a customer whose name is
--   "Customer:Job" and whose parent_id is the customer, so every list and report shows it in full.
-- * Budgets: an amount per income or expense account per month of a fiscal year.
-- * Attachments: files kept in the company file and attached to a transaction.
-- * Discounts on invoices, sales receipts and estimates: an amount taken off the subtotal before
--   tax (the taxable amount is reduced in proportion), debited to a Discounts Given income account.

CREATE TABLE transaction_edits (
    id              INTEGER PRIMARY KEY,
    old_entry_id    INTEGER NOT NULL UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    new_entry_id    INTEGER NOT NULL UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    edited_at       TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE recurring_templates (
    id              INTEGER PRIMARY KEY,
    name            TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    kind            TEXT    NOT NULL CHECK (kind IN ('JOURNAL', 'EXPENSE', 'BILL', 'INVOICE')),
    payload         TEXT    NOT NULL CHECK (json_valid(payload)),
    frequency       TEXT    NOT NULL CHECK (frequency IN ('WEEKLY', 'MONTHLY', 'QUARTERLY', 'YEARLY')),
    every           INTEGER NOT NULL DEFAULT 1 CHECK (every BETWEEN 1 AND 52),
    next_date       TEXT    NOT NULL CHECK (date(next_date) = next_date),
    end_date        TEXT    CHECK (end_date IS NULL OR date(end_date) = end_date),
    is_active       INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_recurring_name ON recurring_templates (name COLLATE NOCASE);

CREATE TABLE recurring_runs (
    id              INTEGER PRIMARY KEY,
    template_id     INTEGER NOT NULL REFERENCES recurring_templates (id) ON DELETE CASCADE,
    run_date        TEXT    NOT NULL CHECK (date(run_date) = run_date),
    journal_entry_id INTEGER NOT NULL REFERENCES journal_entries (id) ON DELETE RESTRICT,
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (template_id, run_date)
) STRICT;

CREATE TABLE purchase_orders (
    id                  INTEGER PRIMARY KEY,
    po_number           INTEGER NOT NULL UNIQUE CHECK (po_number > 0),
    vendor_id           INTEGER NOT NULL REFERENCES vendors (id) ON DELETE RESTRICT,
    po_date             TEXT    NOT NULL CHECK (date(po_date) = po_date),
    expected_date       TEXT    CHECK (expected_date IS NULL OR date(expected_date) = expected_date),
    memo                TEXT,
    total_cents         INTEGER NOT NULL CHECK (total_cents BETWEEN 1 AND 100000000000000),
    status              TEXT    NOT NULL DEFAULT 'OPEN' CHECK (status IN ('OPEN', 'CLOSED', 'BILLED')),
    bill_id             INTEGER REFERENCES bills (id) ON DELETE RESTRICT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK ((status = 'BILLED') = (bill_id IS NOT NULL))
) STRICT;

CREATE TABLE purchase_order_lines (
    id                  INTEGER PRIMARY KEY,
    purchase_order_id   INTEGER NOT NULL REFERENCES purchase_orders (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    account_id          INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    product_service_id  INTEGER REFERENCES products_services (id) ON DELETE RESTRICT,
    description         TEXT,
    quantity_e4         INTEGER CHECK (quantity_e4 IS NULL OR quantity_e4 > 0),
    rate_cents          INTEGER CHECK (rate_cents IS NULL OR rate_cents > 0),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    class_id            INTEGER REFERENCES classes (id) ON DELETE RESTRICT,
    UNIQUE (purchase_order_id, line_number)
) STRICT;

ALTER TABLE customers ADD COLUMN parent_id INTEGER REFERENCES customers (id) ON DELETE RESTRICT;
CREATE INDEX ix_customers_parent ON customers (parent_id);

CREATE TABLE budgets (
    id              INTEGER PRIMARY KEY,
    name            TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    -- First day of the fiscal year the budget covers.
    fiscal_year_start TEXT  NOT NULL CHECK (date(fiscal_year_start) = fiscal_year_start AND substr(fiscal_year_start, 9, 2) = '01'),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_budgets_name ON budgets (name COLLATE NOCASE);

CREATE TABLE budget_amounts (
    budget_id       INTEGER NOT NULL REFERENCES budgets (id) ON DELETE CASCADE,
    account_id      INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    -- 1 = the fiscal year's first month.
    period          INTEGER NOT NULL CHECK (period BETWEEN 1 AND 12),
    amount_cents    INTEGER NOT NULL CHECK (amount_cents BETWEEN -100000000000000 AND 100000000000000),
    PRIMARY KEY (budget_id, account_id, period)
) STRICT;

CREATE TABLE attachments (
    id                  INTEGER PRIMARY KEY,
    journal_entry_id    INTEGER NOT NULL REFERENCES journal_entries (id) ON DELETE RESTRICT,
    file_name           TEXT    NOT NULL CHECK (length(file_name) BETWEEN 1 AND 255),
    media_type          TEXT    NOT NULL,
    size_bytes          INTEGER NOT NULL CHECK (size_bytes BETWEEN 1 AND 26214400),
    content             BLOB    NOT NULL,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX ix_attachments_entry ON attachments (journal_entry_id);

ALTER TABLE invoices ADD COLUMN discount_cents INTEGER NOT NULL DEFAULT 0 CHECK (discount_cents >= 0);
ALTER TABLE sales_receipts ADD COLUMN discount_cents INTEGER NOT NULL DEFAULT 0 CHECK (discount_cents >= 0);
ALTER TABLE estimates ADD COLUMN discount_cents INTEGER NOT NULL DEFAULT 0 CHECK (discount_cents >= 0);

CREATE TRIGGER trg_inv_discount_immutable
BEFORE UPDATE OF discount_cents ON invoices
WHEN OLD.status <> 'DRAFT' AND NEW.discount_cents IS NOT OLD.discount_cents
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: posted invoices cannot be altered');
END;

-- Document totals now take the discount off (otherwise unchanged from 0027).
DROP TRIGGER trg_inv_post_checks;
CREATE TRIGGER trg_inv_post_checks
BEFORE UPDATE OF status ON invoices
WHEN OLD.status = 'DRAFT' AND NEW.status = 'UNPAID'
BEGIN
    SELECT RAISE(ABORT, 'INV_NO_ITEMS: an invoice requires at least one line item')
    WHERE NOT EXISTS (SELECT 1 FROM invoice_items WHERE invoice_id = NEW.id);
    SELECT RAISE(ABORT, 'INV_TOTAL_MISMATCH: invoice total must equal its line items less discount plus sales tax')
    WHERE NEW.total_cents <> (SELECT SUM(amount_cents) FROM invoice_items WHERE invoice_id = NEW.id) - NEW.discount_cents + NEW.tax_cents;
    SELECT RAISE(ABORT, 'INV_JOURNAL_MISSING: invoice must reference its POSTED journal entry')
    WHERE NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED'
          AND source_type = 'INVOICE' AND source_id = NEW.id
    );
END;

DROP TRIGGER trg_sr_update_rules;
CREATE TRIGGER trg_sr_update_rules
BEFORE UPDATE ON sales_receipts
BEGIN
    SELECT RAISE(ABORT, 'SR_IMMUTABLE: posted sales receipts cannot be changed')
    WHERE OLD.journal_entry_id IS NOT NULL
      AND (   NEW.receipt_number IS NOT OLD.receipt_number
           OR NEW.customer_id IS NOT OLD.customer_id
           OR NEW.receipt_date IS NOT OLD.receipt_date
           OR NEW.deposit_account_id IS NOT OLD.deposit_account_id
           OR NEW.payment_method IS NOT OLD.payment_method
           OR NEW.reference IS NOT OLD.reference
           OR NEW.memo IS NOT OLD.memo
           OR NEW.total_cents IS NOT OLD.total_cents
           OR NEW.tax_cents IS NOT OLD.tax_cents
           OR NEW.tax_rate_id IS NOT OLD.tax_rate_id
           OR NEW.discount_cents IS NOT OLD.discount_cents
           OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
           OR NEW.created_at IS NOT OLD.created_at);
    SELECT RAISE(ABORT, 'SR_NO_ITEMS: a sales receipt requires at least one line')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM sales_receipt_lines WHERE sales_receipt_id = NEW.id);
    SELECT RAISE(ABORT, 'SR_TOTAL_MISMATCH: receipt total must equal its lines less discount plus sales tax')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NEW.total_cents <> (SELECT SUM(amount_cents) FROM sales_receipt_lines WHERE sales_receipt_id = NEW.id) - NEW.discount_cents + NEW.tax_cents;
    SELECT RAISE(ABORT, 'SR_JOURNAL_MISSING: receipt must reference its POSTED journal entry')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NOT EXISTS (
          SELECT 1 FROM journal_entries
          WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = 'SALES_RECEIPT' AND source_id = NEW.id
      );
    SELECT RAISE(ABORT, 'SR_UNVOID: a voided sales receipt cannot be restored')
    WHERE OLD.is_voided = 1 AND NEW.is_voided = 0;
    SELECT RAISE(ABORT, 'SR_VOID_ENTRY_ACTIVE: the receipt''s journal entry must be voided first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
    SELECT RAISE(ABORT, 'SR_DEPOSITED: void the bank deposit that includes this sales receipt first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND EXISTS (SELECT 1 FROM deposit_lines WHERE sales_receipt_id = OLD.id AND is_voided = 0);
END;
