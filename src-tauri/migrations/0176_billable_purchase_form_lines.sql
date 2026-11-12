-- 0176_billable_purchase_form_lines.sql
-- Gap T84: a purchase form's discount and charge lines (purchase_form_lines, 0115) can name a
-- customer or job and, on bills, checks and expenses, be marked billable, so freight on a bill can
-- be billed on to the job from Sales › Bill Costs like any split line.
--
-- * purchase_form_lines.customer_id / is_billable: as on bill_lines and expense_lines. A line is
--   billable only with a customer.
-- * billed_costs: rebuilt with a third source column, purchase_form_line_id, so one table still
--   records which invoice billed each cost. Exactly one of the three source columns is set. The
--   rows are copied unchanged, and the table keeps its indexes and its insert-only triggers.

ALTER TABLE purchase_form_lines ADD COLUMN customer_id INTEGER REFERENCES customers (id) ON DELETE RESTRICT;
ALTER TABLE purchase_form_lines ADD COLUMN is_billable INTEGER NOT NULL DEFAULT 0 CHECK (is_billable IN (0, 1) AND (is_billable = 0 OR customer_id IS NOT NULL));
CREATE INDEX ix_purchase_form_lines_customer ON purchase_form_lines (customer_id) WHERE customer_id IS NOT NULL;

CREATE TABLE billed_costs_new (
    id                      INTEGER PRIMARY KEY,
    expense_line_id         INTEGER REFERENCES expense_lines (id) ON DELETE RESTRICT,
    bill_line_id            INTEGER REFERENCES bill_lines (id) ON DELETE RESTRICT,
    invoice_id              INTEGER NOT NULL REFERENCES invoices (id) ON DELETE RESTRICT,
    amount_cents            INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    markup_cents            INTEGER NOT NULL DEFAULT 0 CHECK (markup_cents BETWEEN 0 AND 100000000000000),
    created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    purchase_form_line_id   INTEGER REFERENCES purchase_form_lines (id) ON DELETE RESTRICT,
    CHECK ((expense_line_id IS NOT NULL) + (bill_line_id IS NOT NULL) + (purchase_form_line_id IS NOT NULL) = 1)
) STRICT;

INSERT INTO billed_costs_new (id, expense_line_id, bill_line_id, invoice_id, amount_cents, markup_cents, created_at)
SELECT id, expense_line_id, bill_line_id, invoice_id, amount_cents, markup_cents, created_at FROM billed_costs;

DROP TABLE billed_costs;
ALTER TABLE billed_costs_new RENAME TO billed_costs;

CREATE INDEX ix_billed_costs_expense_line ON billed_costs (expense_line_id) WHERE expense_line_id IS NOT NULL;
CREATE INDEX ix_billed_costs_bill_line    ON billed_costs (bill_line_id) WHERE bill_line_id IS NOT NULL;
CREATE INDEX ix_billed_costs_form_line    ON billed_costs (purchase_form_line_id) WHERE purchase_form_line_id IS NOT NULL;
CREATE INDEX ix_billed_costs_invoice      ON billed_costs (invoice_id);

CREATE TRIGGER trg_billed_costs_immutable BEFORE UPDATE ON billed_costs
BEGIN
    SELECT RAISE(ABORT, 'BILLED_COST_IMMUTABLE: a billed cost cannot be changed; void its invoice instead');
END;

CREATE TRIGGER trg_billed_costs_no_delete BEFORE DELETE ON billed_costs
BEGIN
    SELECT RAISE(ABORT, 'BILLED_COST_IMMUTABLE: a billed cost cannot be removed; void its invoice instead');
END;
