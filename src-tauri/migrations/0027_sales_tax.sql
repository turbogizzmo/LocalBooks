-- Sales tax, as in classic desktop accounting software.
--
-- * A tax rate has a name, a rate in parts per million (8.25% = 82,500), the agency it is paid to
--   (a vendor) and the liability account it accrues in (Sales Tax Payable by default).
-- * Products are taxable or not; customers can have a default rate or be tax exempt.
-- * Invoices, sales receipts, estimates and credit memos record the rate used, which lines were
--   taxable and the tax: round_half_up(taxable subtotal × rate). The tax is credited to the rate's
--   liability account (debited, on a credit memo), and the document total is lines plus tax.
-- * Paying the agency is a sales tax payment: Dr the liability account / Cr the bank account.

INSERT INTO system_preferences (key, value) VALUES ('sales_tax_enabled', 'false');

CREATE TABLE tax_rates (
    id                      INTEGER PRIMARY KEY,
    name                    TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    rate_ppm                INTEGER NOT NULL CHECK (rate_ppm BETWEEN 0 AND 1000000),
    agency_vendor_id        INTEGER REFERENCES vendors (id) ON DELETE RESTRICT,
    liability_account_id    INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    is_active               INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_tax_rates_name ON tax_rates (name COLLATE NOCASE);

ALTER TABLE products_services ADD COLUMN is_taxable INTEGER NOT NULL DEFAULT 1 CHECK (is_taxable IN (0, 1));

ALTER TABLE customers ADD COLUMN tax_rate_id INTEGER REFERENCES tax_rates (id) ON DELETE RESTRICT;
ALTER TABLE customers ADD COLUMN is_tax_exempt INTEGER NOT NULL DEFAULT 0 CHECK (is_tax_exempt IN (0, 1));
ALTER TABLE customers ADD COLUMN tax_exemption TEXT;

ALTER TABLE invoices ADD COLUMN tax_rate_id INTEGER REFERENCES tax_rates (id) ON DELETE RESTRICT;
ALTER TABLE invoices ADD COLUMN tax_cents INTEGER NOT NULL DEFAULT 0 CHECK (tax_cents >= 0);
ALTER TABLE invoice_items ADD COLUMN is_taxable INTEGER NOT NULL DEFAULT 0 CHECK (is_taxable IN (0, 1));

ALTER TABLE sales_receipts ADD COLUMN tax_rate_id INTEGER REFERENCES tax_rates (id) ON DELETE RESTRICT;
ALTER TABLE sales_receipts ADD COLUMN tax_cents INTEGER NOT NULL DEFAULT 0 CHECK (tax_cents >= 0);
ALTER TABLE sales_receipt_lines ADD COLUMN is_taxable INTEGER NOT NULL DEFAULT 0 CHECK (is_taxable IN (0, 1));

ALTER TABLE estimates ADD COLUMN tax_rate_id INTEGER REFERENCES tax_rates (id) ON DELETE RESTRICT;
ALTER TABLE estimates ADD COLUMN tax_cents INTEGER NOT NULL DEFAULT 0 CHECK (tax_cents >= 0);
ALTER TABLE estimate_lines ADD COLUMN is_taxable INTEGER NOT NULL DEFAULT 0 CHECK (is_taxable IN (0, 1));

ALTER TABLE credit_memos ADD COLUMN tax_rate_id INTEGER REFERENCES tax_rates (id) ON DELETE RESTRICT;
ALTER TABLE credit_memos ADD COLUMN tax_cents INTEGER NOT NULL DEFAULT 0 CHECK (tax_cents >= 0);
ALTER TABLE credit_memo_lines ADD COLUMN is_taxable INTEGER NOT NULL DEFAULT 0 CHECK (is_taxable IN (0, 1));

CREATE TABLE sales_tax_payments (
    id                      INTEGER PRIMARY KEY,
    tax_rate_id             INTEGER REFERENCES tax_rates (id) ON DELETE RESTRICT,
    liability_account_id    INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    account_id              INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    payment_date            TEXT    NOT NULL CHECK (date(payment_date) = payment_date),
    amount_cents            INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    reference               TEXT,
    journal_entry_id        INTEGER NOT NULL UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- The tax on a posted document is as fixed as its total.
CREATE TRIGGER trg_inv_tax_immutable
BEFORE UPDATE OF tax_cents, tax_rate_id ON invoices
WHEN OLD.status <> 'DRAFT' AND (NEW.tax_cents IS NOT OLD.tax_cents OR NEW.tax_rate_id IS NOT OLD.tax_rate_id)
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: posted invoices cannot be altered');
END;

CREATE TRIGGER trg_cm_tax_immutable
BEFORE UPDATE OF tax_cents, tax_rate_id ON credit_memos
WHEN OLD.journal_entry_id IS NOT NULL AND (NEW.tax_cents IS NOT OLD.tax_cents OR NEW.tax_rate_id IS NOT OLD.tax_rate_id)
BEGIN
    SELECT RAISE(ABORT, 'CM_IMMUTABLE: posted credit memos cannot be altered; void instead');
END;

-- Document totals now include sales tax (otherwise unchanged from 0011).
DROP TRIGGER trg_inv_post_checks;
CREATE TRIGGER trg_inv_post_checks
BEFORE UPDATE OF status ON invoices
WHEN OLD.status = 'DRAFT' AND NEW.status = 'UNPAID'
BEGIN
    SELECT RAISE(ABORT, 'INV_NO_ITEMS: an invoice requires at least one line item')
    WHERE NOT EXISTS (SELECT 1 FROM invoice_items WHERE invoice_id = NEW.id);
    SELECT RAISE(ABORT, 'INV_TOTAL_MISMATCH: invoice total must equal the sum of its line items plus sales tax')
    WHERE NEW.total_cents <> (SELECT SUM(amount_cents) FROM invoice_items WHERE invoice_id = NEW.id) + NEW.tax_cents;
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
           OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
           OR NEW.created_at IS NOT OLD.created_at);
    SELECT RAISE(ABORT, 'SR_NO_ITEMS: a sales receipt requires at least one line')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM sales_receipt_lines WHERE sales_receipt_id = NEW.id);
    SELECT RAISE(ABORT, 'SR_TOTAL_MISMATCH: receipt total must equal the sum of its lines plus sales tax')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NEW.total_cents <> (SELECT SUM(amount_cents) FROM sales_receipt_lines WHERE sales_receipt_id = NEW.id) + NEW.tax_cents;
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
