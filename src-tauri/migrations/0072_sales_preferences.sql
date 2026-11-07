-- Preferences batch 4 (Sales & Customers, Jobs & Estimates, Send Forms).
--
-- A customer's preferred way to send forms (new customers take the company default from
-- Preferences › Send Forms), a job's status (its labels can be renamed in Preferences › Jobs &
-- Estimates), and the F.O.B. point printed on sales forms (new forms take the company default).

ALTER TABLE customers ADD COLUMN preferred_delivery TEXT
    CHECK (preferred_delivery IS NULL OR preferred_delivery IN ('EMAIL', 'PRINT', 'NONE'));

ALTER TABLE customers ADD COLUMN job_status TEXT
    CHECK (job_status IS NULL OR job_status IN ('PENDING', 'AWARDED', 'IN_PROGRESS', 'CLOSED', 'NOT_AWARDED'));

ALTER TABLE invoices       ADD COLUMN fob TEXT CHECK (fob IS NULL OR length(fob) BETWEEN 1 AND 30);
ALTER TABLE sales_receipts ADD COLUMN fob TEXT CHECK (fob IS NULL OR length(fob) BETWEEN 1 AND 30);
ALTER TABLE estimates      ADD COLUMN fob TEXT CHECK (fob IS NULL OR length(fob) BETWEEN 1 AND 30);

-- Estimates may carry zero-amount lines (an included extra, a note priced at nothing), as the
-- desktop products allow; Preferences › Sales & Customers can leave them off the invoice. Only the
-- rate and amount checks change: SQLite rebuilds the table to change a CHECK. Invoice lines still
-- need an amount.
CREATE TABLE estimate_lines_new (
    id                  INTEGER PRIMARY KEY,
    estimate_id         INTEGER NOT NULL REFERENCES estimates (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    product_service_id  INTEGER REFERENCES products_services (id) ON DELETE RESTRICT,
    description         TEXT    NOT NULL,
    quantity_e4         INTEGER NOT NULL CHECK (quantity_e4 BETWEEN 1 AND 10000000000000),
    rate_cents          INTEGER NOT NULL CHECK (rate_cents BETWEEN 0 AND 100000000000000),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 0 AND 100000000000000),
    income_account_id   INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    class_id            INTEGER REFERENCES classes (id) ON DELETE RESTRICT,
    is_taxable          INTEGER NOT NULL DEFAULT 0 CHECK (is_taxable IN (0, 1)),
    CHECK (amount_cents = (quantity_e4 * rate_cents + 5000) / 10000),
    UNIQUE (estimate_id, line_number)
) STRICT;
INSERT INTO estimate_lines_new
    (id, estimate_id, line_number, product_service_id, description, quantity_e4, rate_cents, amount_cents, income_account_id, class_id, is_taxable)
SELECT id, estimate_id, line_number, product_service_id, description, quantity_e4, rate_cents, amount_cents, income_account_id, class_id, is_taxable
FROM estimate_lines;
DROP TABLE estimate_lines;
ALTER TABLE estimate_lines_new RENAME TO estimate_lines;

CREATE TRIGGER trg_est_lines_frozen_upd
BEFORE UPDATE ON estimate_lines
BEGIN
    SELECT RAISE(ABORT, 'EST_IMMUTABLE: estimate lines cannot be changed after saving');
END;

CREATE TRIGGER trg_est_lines_converted_no_delete
BEFORE DELETE ON estimate_lines
WHEN (SELECT status FROM estimates WHERE id = OLD.estimate_id) = 'CONVERTED'
BEGIN
    SELECT RAISE(ABORT, 'EST_CONVERTED: a converted estimate''s lines are kept as a record and cannot be deleted');
END;

-- A new customer, however it is added (the Customer Center, typed on a form, an import), takes
-- the Send Forms default unless it was given its own.
CREATE TRIGGER trg_customer_default_delivery
AFTER INSERT ON customers
WHEN NEW.preferred_delivery IS NULL
BEGIN
    UPDATE customers
    SET preferred_delivery = (SELECT CASE value WHEN 'email' THEN 'EMAIL' WHEN 'print' THEN 'PRINT' END
                              FROM system_preferences WHERE key = 'default_delivery_method')
    WHERE id = NEW.id;
END;
