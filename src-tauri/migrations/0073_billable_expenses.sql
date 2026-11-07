-- Preferences batch 5 (tax and time): billable expenses and the vendor's account number.

-- The account number a vendor knows this company by. Preferences › Checking can print it in the
-- memo of a check written to that vendor.
ALTER TABLE vendors ADD COLUMN account_number TEXT CHECK (account_number IS NULL OR length(account_number) <= 40);

-- A cost on an expense, check or bill line for a customer or job that is to be invoiced to them.
-- The flag is written with the line; posted lines never change.
ALTER TABLE expense_lines ADD COLUMN is_billable INTEGER NOT NULL DEFAULT 0 CHECK (is_billable IN (0, 1));
ALTER TABLE bill_lines    ADD COLUMN is_billable INTEGER NOT NULL DEFAULT 0 CHECK (is_billable IN (0, 1));

-- Which invoice billed a cost, with the amount and the markup put on it. Rows are only inserted:
-- a cost whose invoice is voided can be billed again, and an edited invoice or expense gets a new
-- row pointing at its replacement.
CREATE TABLE billed_costs (
    id               INTEGER PRIMARY KEY,
    expense_line_id  INTEGER REFERENCES expense_lines (id) ON DELETE RESTRICT,
    bill_line_id     INTEGER REFERENCES bill_lines (id) ON DELETE RESTRICT,
    invoice_id       INTEGER NOT NULL REFERENCES invoices (id) ON DELETE RESTRICT,
    amount_cents     INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    markup_cents     INTEGER NOT NULL DEFAULT 0 CHECK (markup_cents BETWEEN 0 AND 100000000000000),
    created_at       TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK ((expense_line_id IS NULL) <> (bill_line_id IS NULL))
) STRICT;

CREATE INDEX ix_billed_costs_expense_line ON billed_costs (expense_line_id) WHERE expense_line_id IS NOT NULL;
CREATE INDEX ix_billed_costs_bill_line    ON billed_costs (bill_line_id) WHERE bill_line_id IS NOT NULL;
CREATE INDEX ix_billed_costs_invoice      ON billed_costs (invoice_id);

CREATE TRIGGER trg_billed_costs_immutable BEFORE UPDATE ON billed_costs
BEGIN
    SELECT RAISE(ABORT, 'BILLED_COST_IMMUTABLE: a billed cost cannot be changed; void its invoice instead');
END;

CREATE TRIGGER trg_billed_costs_no_delete BEFORE DELETE ON billed_costs
BEGIN
    SELECT RAISE(ABORT, 'BILLED_COST_IMMUTABLE: a billed cost cannot be removed; void its invoice instead');
END;
