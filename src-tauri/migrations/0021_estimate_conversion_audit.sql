-- Converting an estimate keeps it as a permanent record of what was quoted.
--
-- The status for an estimate that became an invoice is now CONVERTED (it was CLOSED), and the link
-- column is converted_to_invoice_id (it was invoice_id). A converted estimate and its lines can no
-- longer be changed or deleted, so the original quote can always be compared with the invoice.
--
-- SQLite cannot change a CHECK constraint in place, so the table is rebuilt; rows keep their ids.

DROP TRIGGER trg_est_status_rules;

CREATE TABLE estimates_new (
    id                      INTEGER PRIMARY KEY,
    estimate_number         INTEGER NOT NULL UNIQUE CHECK (estimate_number > 0),
    customer_id             INTEGER NOT NULL REFERENCES customers (id) ON DELETE RESTRICT,
    estimate_date           TEXT    NOT NULL CHECK (
                                estimate_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                                AND date(estimate_date) = estimate_date
                            ),
    expiration_date         TEXT    CHECK (expiration_date IS NULL OR date(expiration_date) = expiration_date),
    status                  TEXT    NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'ACCEPTED', 'REJECTED', 'CONVERTED')),
    memo                    TEXT,
    total_cents             INTEGER NOT NULL CHECK (total_cents BETWEEN 1 AND 100000000000000),
    converted_to_invoice_id INTEGER UNIQUE REFERENCES invoices (id) ON DELETE RESTRICT,
    created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (expiration_date IS NULL OR expiration_date >= estimate_date),
    CHECK ((status = 'CONVERTED') = (converted_to_invoice_id IS NOT NULL))
) STRICT;

INSERT INTO estimates_new
    (id, estimate_number, customer_id, estimate_date, expiration_date, status, memo, total_cents, converted_to_invoice_id, created_at)
SELECT id, estimate_number, customer_id, estimate_date, expiration_date,
       CASE status WHEN 'CLOSED' THEN 'CONVERTED' ELSE status END,
       memo, total_cents, invoice_id, created_at
FROM estimates ORDER BY id;

DROP TABLE estimates;
ALTER TABLE estimates_new RENAME TO estimates;

CREATE TRIGGER trg_est_status_rules
BEFORE UPDATE ON estimates
BEGIN
    SELECT RAISE(ABORT, 'EST_CONVERTED: a converted estimate is kept as it was quoted and cannot be changed')
    WHERE OLD.status = 'CONVERTED';
    SELECT RAISE(ABORT, 'EST_IMMUTABLE: only the status of an estimate can change after it is saved')
    WHERE NEW.estimate_number IS NOT OLD.estimate_number OR NEW.customer_id IS NOT OLD.customer_id
       OR NEW.estimate_date IS NOT OLD.estimate_date OR NEW.expiration_date IS NOT OLD.expiration_date
       OR NEW.total_cents IS NOT OLD.total_cents OR NEW.memo IS NOT OLD.memo OR NEW.created_at IS NOT OLD.created_at;
    SELECT RAISE(ABORT, 'EST_INVOICE_MISMATCH: an estimate converts only into a posted invoice for the same customer')
    WHERE NEW.status = 'CONVERTED' AND NOT EXISTS (
        SELECT 1 FROM invoices
        WHERE id = NEW.converted_to_invoice_id AND status <> 'DRAFT' AND customer_id = NEW.customer_id
    );
END;

CREATE TRIGGER trg_est_converted_no_delete
BEFORE DELETE ON estimates
WHEN OLD.status = 'CONVERTED'
BEGIN
    SELECT RAISE(ABORT, 'EST_CONVERTED: a converted estimate is kept as a record and cannot be deleted');
END;

CREATE TRIGGER trg_est_lines_converted_no_delete
BEFORE DELETE ON estimate_lines
WHEN (SELECT status FROM estimates WHERE id = OLD.estimate_id) = 'CONVERTED'
BEGIN
    SELECT RAISE(ABORT, 'EST_CONVERTED: a converted estimate''s lines are kept as a record and cannot be deleted');
END;
