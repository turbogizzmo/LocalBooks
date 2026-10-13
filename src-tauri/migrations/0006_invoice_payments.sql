-- 0006_invoice_payments.sql
-- Partial payments: invoices gain amount_paid_cents and a PARTIAL status; each payment becomes a row
-- in invoice_payments with its own journal entry (Dr deposit account / Cr Accounts Receivable).
--
-- SQLite cannot alter CHECK constraints in place, so `invoices` is rebuilt using the documented
-- 12-step procedure. That procedure requires foreign keys OFF while the old table is dropped (otherwise
-- the drop would cascade into invoice_items); db::migrate runs every migration on a connection with
-- enforcement off and re-verifies the whole database with foreign_key_check afterwards. This migration
-- additionally aborts on any dangling reference before it commits.

-- Triggers on invoice_items reference `invoices`; they must not exist while the table is swapped.
DROP TRIGGER trg_inv_items_insert_draft_only;
DROP TRIGGER trg_inv_items_update_draft_only;
DROP TRIGGER trg_inv_items_delete_draft_only;

CREATE TABLE invoices_new (
    id                  INTEGER PRIMARY KEY,
    invoice_number      INTEGER NOT NULL UNIQUE CHECK (invoice_number > 0),
    customer_name       TEXT    NOT NULL CHECK (length(trim(customer_name)) > 0),
    issue_date          TEXT    NOT NULL CHECK (
                            issue_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(issue_date) = issue_date
                        ),
    due_date            TEXT    NOT NULL CHECK (
                            due_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(due_date) = due_date
                        ),
    total_cents         INTEGER NOT NULL CHECK (total_cents > 0),
    amount_paid_cents   INTEGER NOT NULL DEFAULT 0 CHECK (amount_paid_cents >= 0),
    status              TEXT    NOT NULL DEFAULT 'DRAFT' CHECK (status IN ('DRAFT', 'UNPAID', 'PARTIAL', 'PAID')),
    journal_entry_id    INTEGER UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (due_date >= issue_date),
    CHECK (amount_paid_cents <= total_cents),
    CHECK ((status = 'DRAFT') = (journal_entry_id IS NULL)),
    CHECK (status NOT IN ('DRAFT', 'UNPAID') OR amount_paid_cents = 0),
    CHECK ((status = 'PARTIAL') = (amount_paid_cents > 0 AND amount_paid_cents < total_cents)),
    CHECK ((status = 'PAID') = (amount_paid_cents = total_cents))
) STRICT;

INSERT INTO invoices_new
    (id, invoice_number, customer_name, issue_date, due_date, total_cents, amount_paid_cents, status,
     journal_entry_id, created_at)
SELECT id, invoice_number, customer_name, issue_date, due_date, total_cents,
       CASE WHEN status = 'PAID' THEN total_cents ELSE 0 END,
       status, journal_entry_id, created_at
FROM invoices;

CREATE TABLE invoice_payments (
    id                  INTEGER PRIMARY KEY,
    invoice_id          INTEGER NOT NULL REFERENCES invoices (id) ON DELETE RESTRICT,
    payment_date        TEXT    NOT NULL CHECK (
                            payment_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(payment_date) = payment_date
                        ),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    payment_method      TEXT    NOT NULL CHECK (payment_method IN ('CASH', 'CHECK', 'CREDIT_CARD', 'ACH', 'WIRE', 'OTHER')),
    reference           TEXT,
    deposit_account_id  INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    journal_entry_id    INTEGER UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- Carry existing full payments over. The payment id reuses the invoice id because the posted
-- RECEIVE_PAYMENT entries already carry the invoice id as their (immutable) source_id.
INSERT INTO invoice_payments (id, invoice_id, payment_date, amount_cents, payment_method, deposit_account_id, journal_entry_id)
SELECT i.id, i.id, i.paid_date, i.total_cents, 'OTHER',
       (SELECT l.account_id FROM journal_lines l
        WHERE l.journal_entry_id = i.payment_journal_entry_id AND l.debit_cents > 0
        ORDER BY l.line_number LIMIT 1),
       i.payment_journal_entry_id
FROM invoices i
WHERE i.status = 'PAID';

DROP TABLE invoices;
ALTER TABLE invoices_new RENAME TO invoices;

CREATE INDEX ix_invoices_status_due ON invoices (status, due_date);
CREATE INDEX ix_invoices_customer ON invoices (customer_name COLLATE NOCASE);
CREATE INDEX ix_invoice_payments_invoice ON invoice_payments (invoice_id, payment_date);

-- ---------------------------------------------------------------------------------------------
-- Invoice triggers (recreated for the new table)
-- ---------------------------------------------------------------------------------------------
CREATE TRIGGER trg_inv_insert_as_draft
BEFORE INSERT ON invoices
WHEN NEW.status <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_INSERT_NOT_DRAFT: invoices must be inserted as DRAFT, then posted');
END;

CREATE TRIGGER trg_inv_status_transition
BEFORE UPDATE OF status ON invoices
WHEN NOT (
       (OLD.status = 'DRAFT'   AND NEW.status IN ('DRAFT', 'UNPAID'))
    OR (OLD.status = 'UNPAID'  AND NEW.status IN ('UNPAID', 'PARTIAL', 'PAID'))
    OR (OLD.status = 'PARTIAL' AND NEW.status IN ('PARTIAL', 'PAID'))
    OR (OLD.status = 'PAID'    AND NEW.status = 'PAID')
)
BEGIN
    SELECT RAISE(ABORT, 'INV_INVALID_TRANSITION: allowed transitions are DRAFT->UNPAID->PARTIAL->PAID');
END;

CREATE TRIGGER trg_inv_post_checks
BEFORE UPDATE OF status ON invoices
WHEN OLD.status = 'DRAFT' AND NEW.status = 'UNPAID'
BEGIN
    SELECT RAISE(ABORT, 'INV_NO_ITEMS: an invoice requires at least one line item')
    WHERE NOT EXISTS (SELECT 1 FROM invoice_items WHERE invoice_id = NEW.id);
    SELECT RAISE(ABORT, 'INV_TOTAL_MISMATCH: invoice total must equal the sum of its line items')
    WHERE NEW.total_cents <> (SELECT SUM(amount_cents) FROM invoice_items WHERE invoice_id = NEW.id);
    SELECT RAISE(ABORT, 'INV_JOURNAL_MISSING: invoice must reference its POSTED journal entry')
    WHERE NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED'
          AND source_type = 'INVOICE' AND source_id = NEW.id
    );
END;

-- amount_paid_cents must always equal the sum of the invoice's posted payments.
CREATE TRIGGER trg_inv_amount_paid_matches
BEFORE UPDATE OF amount_paid_cents ON invoices
BEGIN
    SELECT RAISE(ABORT, 'INV_PAYMENT_UNPOSTED: every payment must be posted before it is applied')
    WHERE EXISTS (SELECT 1 FROM invoice_payments WHERE invoice_id = NEW.id AND journal_entry_id IS NULL);
    SELECT RAISE(ABORT, 'INV_PAID_MISMATCH: amount paid must equal the sum of recorded payments')
    WHERE NEW.amount_paid_cents <> COALESCE((SELECT SUM(amount_cents) FROM invoice_payments WHERE invoice_id = NEW.id), 0);
END;

CREATE TRIGGER trg_inv_posted_header_immutable
BEFORE UPDATE ON invoices
WHEN OLD.status <> 'DRAFT'
 AND (   NEW.invoice_number   IS NOT OLD.invoice_number
      OR NEW.customer_name    IS NOT OLD.customer_name
      OR NEW.issue_date       IS NOT OLD.issue_date
      OR NEW.due_date         IS NOT OLD.due_date
      OR NEW.total_cents      IS NOT OLD.total_cents
      OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
      OR NEW.created_at       IS NOT OLD.created_at)
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: posted invoices cannot be altered');
END;

CREATE TRIGGER trg_inv_no_delete_posted
BEFORE DELETE ON invoices
WHEN OLD.status <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: posted invoices cannot be deleted');
END;

CREATE TRIGGER trg_inv_items_insert_draft_only
BEFORE INSERT ON invoice_items
WHEN (SELECT status FROM invoices WHERE id = NEW.invoice_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: items can only be added to a DRAFT invoice');
END;

CREATE TRIGGER trg_inv_items_update_draft_only
BEFORE UPDATE ON invoice_items
WHEN (SELECT status FROM invoices WHERE id = OLD.invoice_id) <> 'DRAFT'
  OR (SELECT status FROM invoices WHERE id = NEW.invoice_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: items can only be changed on a DRAFT invoice');
END;

CREATE TRIGGER trg_inv_items_delete_draft_only
BEFORE DELETE ON invoice_items
WHEN (SELECT status FROM invoices WHERE id = OLD.invoice_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: items can only be removed from a DRAFT invoice');
END;

-- ---------------------------------------------------------------------------------------------
-- Payment triggers
-- ---------------------------------------------------------------------------------------------
CREATE TRIGGER trg_pay_insert_checks
BEFORE INSERT ON invoice_payments
BEGIN
    SELECT RAISE(ABORT, 'INV_NOT_OPEN: payments can only be applied to open invoices')
    WHERE (SELECT status FROM invoices WHERE id = NEW.invoice_id) NOT IN ('UNPAID', 'PARTIAL');
    SELECT RAISE(ABORT, 'INV_OVERPAYMENT: payment exceeds the open balance')
    WHERE NEW.amount_cents > (SELECT total_cents - amount_paid_cents FROM invoices WHERE id = NEW.invoice_id);
    SELECT RAISE(ABORT, 'INV_PAYMENT_BEFORE_ISSUE: payment date is earlier than the invoice date')
    WHERE NEW.payment_date < (SELECT issue_date FROM invoices WHERE id = NEW.invoice_id);
    SELECT RAISE(ABORT, 'INV_BAD_DEPOSIT_ACCOUNT: deposit to a Bank or Undeposited Funds account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.deposit_account_id) NOT IN ('BANK', 'OTHER_CURRENT_ASSET');
    SELECT RAISE(ABORT, 'INV_PAYMENT_PREPOSTED: insert the payment first, then attach its journal entry')
    WHERE NEW.journal_entry_id IS NOT NULL;
END;

CREATE TRIGGER trg_pay_attach_entry_only
BEFORE UPDATE ON invoice_payments
BEGIN
    SELECT RAISE(ABORT, 'INV_PAYMENT_IMMUTABLE: recorded payments cannot be changed')
    WHERE OLD.journal_entry_id IS NOT NULL
       OR NEW.journal_entry_id IS NULL
       OR NEW.invoice_id IS NOT OLD.invoice_id
       OR NEW.payment_date IS NOT OLD.payment_date
       OR NEW.amount_cents IS NOT OLD.amount_cents
       OR NEW.payment_method IS NOT OLD.payment_method
       OR NEW.reference IS NOT OLD.reference
       OR NEW.deposit_account_id IS NOT OLD.deposit_account_id;
    SELECT RAISE(ABORT, 'INV_PAYMENT_ENTRY_MISMATCH: payment must reference its POSTED journal entry')
    WHERE NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED'
          AND source_type = 'RECEIVE_PAYMENT' AND source_id = NEW.id
    );
END;

CREATE TRIGGER trg_pay_no_delete
BEFORE DELETE ON invoice_payments
BEGIN
    SELECT RAISE(ABORT, 'INV_PAYMENT_IMMUTABLE: recorded payments cannot be deleted');
END;

-- Abort (rolling everything back) if the rebuild left any dangling reference.
CREATE TEMP TABLE _migration_fk_guard (violations INTEGER NOT NULL CHECK (violations = 0));
INSERT INTO _migration_fk_guard SELECT COUNT(*) FROM pragma_foreign_key_check;
DROP TABLE _migration_fk_guard;
