-- 0003_invoices.sql
-- Invoices post to the ledger as: Dr Accounts Receivable / Cr income account(s).
-- Receiving payment posts: Dr deposit account / Cr Accounts Receivable.
--
-- Lifecycle: DRAFT -> UNPAID (posted) -> PAID. Items are frozen once the invoice leaves DRAFT.

CREATE TABLE invoices (
    id                          INTEGER PRIMARY KEY,
    invoice_number              INTEGER NOT NULL UNIQUE CHECK (invoice_number > 0),
    customer_name               TEXT    NOT NULL CHECK (length(trim(customer_name)) > 0),
    issue_date                  TEXT    NOT NULL CHECK (
                                    issue_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                                    AND date(issue_date) = issue_date
                                ),
    due_date                    TEXT    NOT NULL CHECK (
                                    due_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                                    AND date(due_date) = due_date
                                ),
    total_cents                 INTEGER NOT NULL CHECK (total_cents > 0),
    status                      TEXT    NOT NULL DEFAULT 'DRAFT' CHECK (status IN ('DRAFT', 'UNPAID', 'PAID')),
    journal_entry_id            INTEGER UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    payment_journal_entry_id    INTEGER UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    paid_date                   TEXT    CHECK (paid_date IS NULL OR date(paid_date) = paid_date),
    created_at                  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (due_date >= issue_date),
    CHECK ((status = 'DRAFT') = (journal_entry_id IS NULL)),
    CHECK ((status = 'PAID') = (payment_journal_entry_id IS NOT NULL)),
    CHECK ((status = 'PAID') = (paid_date IS NOT NULL))
) STRICT;

CREATE INDEX ix_invoices_status_due ON invoices (status, due_date);
CREATE INDEX ix_invoices_customer ON invoices (customer_name COLLATE NOCASE);

CREATE TABLE invoice_items (
    id                  INTEGER PRIMARY KEY,
    invoice_id          INTEGER NOT NULL REFERENCES invoices (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    description         TEXT    NOT NULL,
    quantity            INTEGER NOT NULL CHECK (quantity BETWEEN 1 AND 1000000000),
    rate_cents          INTEGER NOT NULL CHECK (rate_cents BETWEEN 1 AND 100000000000000),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    income_account_id   INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    CHECK (amount_cents = quantity * rate_cents),
    UNIQUE (invoice_id, line_number)
) STRICT;

CREATE INDEX ix_invoice_items_account ON invoice_items (income_account_id);

CREATE TRIGGER trg_inv_insert_as_draft
BEFORE INSERT ON invoices
WHEN NEW.status <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_INSERT_NOT_DRAFT: invoices must be inserted as DRAFT, then posted');
END;

CREATE TRIGGER trg_inv_status_transition
BEFORE UPDATE OF status ON invoices
WHEN NOT (
       (OLD.status = 'DRAFT'  AND NEW.status IN ('DRAFT', 'UNPAID'))
    OR (OLD.status = 'UNPAID' AND NEW.status IN ('UNPAID', 'PAID'))
    OR (OLD.status = 'PAID'   AND NEW.status = 'PAID')
)
BEGIN
    SELECT RAISE(ABORT, 'INV_INVALID_TRANSITION: allowed transitions are DRAFT->UNPAID and UNPAID->PAID');
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

CREATE TRIGGER trg_inv_paid_checks
BEFORE UPDATE OF status ON invoices
WHEN OLD.status = 'UNPAID' AND NEW.status = 'PAID'
BEGIN
    SELECT RAISE(ABORT, 'INV_PAYMENT_MISSING: paid invoice must reference its POSTED payment entry')
    WHERE NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.payment_journal_entry_id AND status = 'POSTED'
          AND source_type = 'RECEIVE_PAYMENT' AND source_id = NEW.id
    );
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
      OR (OLD.status = 'PAID' AND (   NEW.payment_journal_entry_id IS NOT OLD.payment_journal_entry_id
                                   OR NEW.paid_date IS NOT OLD.paid_date)))
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
