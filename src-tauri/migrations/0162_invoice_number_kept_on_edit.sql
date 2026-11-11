-- 0162_invoice_number_kept_on_edit.sql
-- Editing a posted invoice voids it and posts the corrected one. Until now the corrected invoice took
-- the next number, because the voided original kept its number and invoice numbers were UNIQUE. From
-- here on the corrected invoice keeps the original's number: a number is unique among invoices that
-- an edit has not replaced (`is_replaced = 0`), enforced by a partial unique index. The replaced
-- original keeps its number too, voided, for the audit trail; `transaction_edits` links the two.
--
-- SQLite can't drop a column's UNIQUE in place, so `invoices` is rebuilt with the documented 12-step
-- procedure, as 0006 did: db::migrate runs migrations with foreign keys off and checks the whole
-- database with foreign_key_check afterwards. The table, its indexes and triggers, and the views and
-- other tables' triggers that name it are recreated exactly as they were, apart from the two changes.
-- Invoices replaced by an edit before this migration are marked replaced; their numbers don't change.

-- Views and other tables' triggers that name `invoices` must not exist while it is swapped.
DROP VIEW v_entry_party;
DROP VIEW v_invoice_balances;
DROP VIEW v_invoice_replacements;
DROP TRIGGER trg_inv_items_insert_draft_only;
DROP TRIGGER trg_inv_items_delete_draft_only;
DROP TRIGGER trg_pay_insert_checks;
DROP TRIGGER trg_inv_form_lines_insert_draft_only;
DROP TRIGGER trg_inv_form_lines_delete_draft_only;
DROP TRIGGER trg_pay_update_rules;
DROP TRIGGER trg_credit_applications_once;
DROP TRIGGER trg_bounced_receipt_insert_checks;
DROP TRIGGER trg_est_status_rules;
DROP TRIGGER trg_bounce_update_rules;
DROP TRIGGER trg_inv_items_update_draft_only;
DROP TRIGGER trg_inv_form_lines_update_draft_only;

CREATE TABLE invoices_new (
    id                  INTEGER PRIMARY KEY,
    invoice_number      INTEGER NOT NULL CHECK (invoice_number > 0),
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
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')), customer_id INTEGER REFERENCES customers (id) ON DELETE RESTRICT, is_voided INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1)), currency_code TEXT CHECK (currency_code IS NULL OR currency_code GLOB '[A-Z][A-Z][A-Z]'), exchange_rate_e6 INTEGER
    CHECK ((exchange_rate_e6 IS NULL) = (currency_code IS NULL) AND (exchange_rate_e6 IS NULL OR exchange_rate_e6 > 0)), home_total_cents INTEGER
    CHECK ((home_total_cents IS NULL) = (currency_code IS NULL) AND (home_total_cents IS NULL OR home_total_cents > 0)), tax_rate_id INTEGER REFERENCES tax_rates (id) ON DELETE RESTRICT, tax_cents INTEGER NOT NULL DEFAULT 0 CHECK (tax_cents >= 0), discount_cents INTEGER NOT NULL DEFAULT 0 CHECK (discount_cents >= 0), sales_rep_id   INTEGER REFERENCES sales_reps (id) ON DELETE RESTRICT, ship_date      TEXT CHECK (ship_date IS NULL OR date(ship_date) = ship_date), ship_method_id INTEGER REFERENCES ship_methods (id) ON DELETE RESTRICT, terms_id INTEGER REFERENCES terms (id) ON DELETE RESTRICT, customer_message_id INTEGER REFERENCES profile_entries (id) ON DELETE RESTRICT, bill_to   TEXT, ship_to   TEXT, po_number TEXT CHECK (po_number IS NULL OR length(po_number) <= 25), fob       TEXT CHECK (fob IS NULL OR length(fob) <= 25), to_be_printed INTEGER NOT NULL DEFAULT 0 CHECK (to_be_printed IN (0, 1)), form_style_id INTEGER REFERENCES form_styles (id) ON DELETE SET NULL,
    -- 1 once an edit has replaced this invoice: the corrected one carries its number from then on.
    is_replaced INTEGER NOT NULL DEFAULT 0 CHECK (is_replaced IN (0, 1)),
    CHECK (due_date >= issue_date),
    CHECK (amount_paid_cents <= total_cents),
    CHECK ((status = 'DRAFT') = (journal_entry_id IS NULL)),
    CHECK (status NOT IN ('DRAFT', 'UNPAID') OR amount_paid_cents = 0),
    CHECK ((status = 'PARTIAL') = (amount_paid_cents > 0 AND amount_paid_cents < total_cents)),
    CHECK ((status = 'PAID') = (amount_paid_cents = total_cents))
) STRICT;

INSERT INTO invoices_new (id, invoice_number, customer_name, issue_date, due_date, total_cents, amount_paid_cents, status, journal_entry_id, created_at, customer_id, is_voided, currency_code, exchange_rate_e6, home_total_cents, tax_rate_id, tax_cents, discount_cents, sales_rep_id, ship_date, ship_method_id, terms_id, customer_message_id, bill_to, ship_to, po_number, fob, to_be_printed, form_style_id, is_replaced)
SELECT id, invoice_number, customer_name, issue_date, due_date, total_cents, amount_paid_cents, status, journal_entry_id, created_at, customer_id, is_voided, currency_code, exchange_rate_e6, home_total_cents, tax_rate_id, tax_cents, discount_cents, sales_rep_id, ship_date, ship_method_id, terms_id, customer_message_id, bill_to, ship_to, po_number, fob, to_be_printed, form_style_id,
       CASE WHEN EXISTS (SELECT 1 FROM transaction_edits te WHERE te.old_entry_id = invoices.journal_entry_id) THEN 1 ELSE 0 END
FROM invoices;

DROP TABLE invoices;
ALTER TABLE invoices_new RENAME TO invoices;

-- A number belongs to one invoice that no edit has replaced.
CREATE UNIQUE INDEX ux_invoices_number_current ON invoices (invoice_number) WHERE is_replaced = 0;

CREATE INDEX ix_invoices_status_due ON invoices (status, due_date);
CREATE INDEX ix_invoices_customer ON invoices (customer_name COLLATE NOCASE);
CREATE INDEX ix_invoices_customer_id ON invoices (customer_id);
CREATE INDEX ix_invoices_sales_rep       ON invoices (sales_rep_id) WHERE sales_rep_id IS NOT NULL;
CREATE INDEX ix_invoices_to_be_printed ON invoices (invoice_number) WHERE to_be_printed = 1;

CREATE TRIGGER trg_inv_insert_as_draft
BEFORE INSERT ON invoices
WHEN NEW.status <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_INSERT_NOT_DRAFT: invoices must be inserted as DRAFT, then posted');
END;

CREATE TRIGGER trg_inv_no_delete_posted
BEFORE DELETE ON invoices
WHEN OLD.status <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: posted invoices cannot be deleted');
END;

CREATE TRIGGER trg_inv_posted_header_immutable
BEFORE UPDATE ON invoices
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.invoice_number IS OLD.invoice_number
       AND (NEW.customer_name IS OLD.customer_name OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_name = OLD.customer_name AND m.to_name = NEW.customer_name))
       AND NEW.issue_date IS OLD.issue_date
       AND NEW.due_date IS OLD.due_date
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.amount_paid_cents IS OLD.amount_paid_cents
       AND NEW.status IS OLD.status
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.currency_code IS OLD.currency_code
       AND NEW.exchange_rate_e6 IS OLD.exchange_rate_e6
       AND NEW.home_total_cents IS OLD.home_total_cents
       AND NEW.tax_rate_id IS OLD.tax_rate_id
       AND NEW.tax_cents IS OLD.tax_cents
       AND NEW.discount_cents IS OLD.discount_cents
       AND NEW.sales_rep_id IS OLD.sales_rep_id
       AND NEW.ship_date IS OLD.ship_date
       AND NEW.ship_method_id IS OLD.ship_method_id
       AND NEW.terms_id IS OLD.terms_id
       AND NEW.customer_message_id IS OLD.customer_message_id
       AND NEW.bill_to IS OLD.bill_to
       AND NEW.ship_to IS OLD.ship_to
       AND NEW.po_number IS OLD.po_number
       AND NEW.fob IS OLD.fob
       AND NEW.to_be_printed IS OLD.to_be_printed
       AND NEW.form_style_id IS OLD.form_style_id)
  AND (OLD.status <> 'DRAFT'
 AND (   NEW.invoice_number   IS NOT OLD.invoice_number
      OR NEW.customer_name    IS NOT OLD.customer_name
      OR NEW.issue_date       IS NOT OLD.issue_date
      OR NEW.due_date         IS NOT OLD.due_date
      OR NEW.total_cents      IS NOT OLD.total_cents
      OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
      OR NEW.created_at       IS NOT OLD.created_at))
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: posted invoices cannot be altered');
END;

CREATE TRIGGER trg_inv_status_transition
BEFORE UPDATE OF status ON invoices
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.invoice_number IS OLD.invoice_number
       AND (NEW.customer_name IS OLD.customer_name OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_name = OLD.customer_name AND m.to_name = NEW.customer_name))
       AND NEW.issue_date IS OLD.issue_date
       AND NEW.due_date IS OLD.due_date
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.amount_paid_cents IS OLD.amount_paid_cents
       AND NEW.status IS OLD.status
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.currency_code IS OLD.currency_code
       AND NEW.exchange_rate_e6 IS OLD.exchange_rate_e6
       AND NEW.home_total_cents IS OLD.home_total_cents
       AND NEW.tax_rate_id IS OLD.tax_rate_id
       AND NEW.tax_cents IS OLD.tax_cents
       AND NEW.discount_cents IS OLD.discount_cents
       AND NEW.sales_rep_id IS OLD.sales_rep_id
       AND NEW.ship_date IS OLD.ship_date
       AND NEW.ship_method_id IS OLD.ship_method_id
       AND NEW.terms_id IS OLD.terms_id
       AND NEW.customer_message_id IS OLD.customer_message_id
       AND NEW.bill_to IS OLD.bill_to
       AND NEW.ship_to IS OLD.ship_to
       AND NEW.po_number IS OLD.po_number
       AND NEW.fob IS OLD.fob
       AND NEW.to_be_printed IS OLD.to_be_printed
       AND NEW.form_style_id IS OLD.form_style_id)
  AND (NOT (
       (OLD.status = 'DRAFT' AND NEW.status IN ('DRAFT', 'UNPAID'))
    OR (OLD.status IN ('UNPAID', 'PARTIAL', 'PAID') AND NEW.status IN ('UNPAID', 'PARTIAL', 'PAID'))
))
BEGIN
    SELECT RAISE(ABORT, 'INV_INVALID_TRANSITION: a posted invoice cannot return to DRAFT');
END;

CREATE TRIGGER trg_inv_void
BEFORE UPDATE OF is_voided ON invoices
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.invoice_number IS OLD.invoice_number
       AND (NEW.customer_name IS OLD.customer_name OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_name = OLD.customer_name AND m.to_name = NEW.customer_name))
       AND NEW.issue_date IS OLD.issue_date
       AND NEW.due_date IS OLD.due_date
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.amount_paid_cents IS OLD.amount_paid_cents
       AND NEW.status IS OLD.status
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.currency_code IS OLD.currency_code
       AND NEW.exchange_rate_e6 IS OLD.exchange_rate_e6
       AND NEW.home_total_cents IS OLD.home_total_cents
       AND NEW.tax_rate_id IS OLD.tax_rate_id
       AND NEW.tax_cents IS OLD.tax_cents
       AND NEW.discount_cents IS OLD.discount_cents
       AND NEW.sales_rep_id IS OLD.sales_rep_id
       AND NEW.ship_date IS OLD.ship_date
       AND NEW.ship_method_id IS OLD.ship_method_id
       AND NEW.terms_id IS OLD.terms_id
       AND NEW.customer_message_id IS OLD.customer_message_id
       AND NEW.bill_to IS OLD.bill_to
       AND NEW.ship_to IS OLD.ship_to
       AND NEW.po_number IS OLD.po_number
       AND NEW.fob IS OLD.fob
       AND NEW.to_be_printed IS OLD.to_be_printed
       AND NEW.form_style_id IS OLD.form_style_id)
  AND (NEW.is_voided IS NOT OLD.is_voided)
BEGIN
    SELECT RAISE(ABORT, 'INV_UNVOID: a voided invoice cannot be restored') WHERE OLD.is_voided = 1;
    SELECT RAISE(ABORT, 'INV_VOID_HAS_PAYMENTS: void the payments applied to this invoice first')
    WHERE OLD.amount_paid_cents <> 0;
    SELECT RAISE(ABORT, 'INV_VOID_ENTRY_ACTIVE: the invoice''s journal entry must be voided first')
    WHERE NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
END;

CREATE TRIGGER trg_inv_currency_immutable
BEFORE UPDATE OF currency_code, exchange_rate_e6, home_total_cents ON invoices
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.invoice_number IS OLD.invoice_number
       AND (NEW.customer_name IS OLD.customer_name OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_name = OLD.customer_name AND m.to_name = NEW.customer_name))
       AND NEW.issue_date IS OLD.issue_date
       AND NEW.due_date IS OLD.due_date
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.amount_paid_cents IS OLD.amount_paid_cents
       AND NEW.status IS OLD.status
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.currency_code IS OLD.currency_code
       AND NEW.exchange_rate_e6 IS OLD.exchange_rate_e6
       AND NEW.home_total_cents IS OLD.home_total_cents
       AND NEW.tax_rate_id IS OLD.tax_rate_id
       AND NEW.tax_cents IS OLD.tax_cents
       AND NEW.discount_cents IS OLD.discount_cents
       AND NEW.sales_rep_id IS OLD.sales_rep_id
       AND NEW.ship_date IS OLD.ship_date
       AND NEW.ship_method_id IS OLD.ship_method_id
       AND NEW.terms_id IS OLD.terms_id
       AND NEW.customer_message_id IS OLD.customer_message_id
       AND NEW.bill_to IS OLD.bill_to
       AND NEW.ship_to IS OLD.ship_to
       AND NEW.po_number IS OLD.po_number
       AND NEW.fob IS OLD.fob
       AND NEW.to_be_printed IS OLD.to_be_printed
       AND NEW.form_style_id IS OLD.form_style_id)
  AND (OLD.status <> 'DRAFT')
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: the currency of a posted invoice cannot change');
END;

CREATE TRIGGER trg_inv_tax_immutable
BEFORE UPDATE OF tax_cents, tax_rate_id ON invoices
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.invoice_number IS OLD.invoice_number
       AND (NEW.customer_name IS OLD.customer_name OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_name = OLD.customer_name AND m.to_name = NEW.customer_name))
       AND NEW.issue_date IS OLD.issue_date
       AND NEW.due_date IS OLD.due_date
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.amount_paid_cents IS OLD.amount_paid_cents
       AND NEW.status IS OLD.status
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.currency_code IS OLD.currency_code
       AND NEW.exchange_rate_e6 IS OLD.exchange_rate_e6
       AND NEW.home_total_cents IS OLD.home_total_cents
       AND NEW.tax_rate_id IS OLD.tax_rate_id
       AND NEW.tax_cents IS OLD.tax_cents
       AND NEW.discount_cents IS OLD.discount_cents
       AND NEW.sales_rep_id IS OLD.sales_rep_id
       AND NEW.ship_date IS OLD.ship_date
       AND NEW.ship_method_id IS OLD.ship_method_id
       AND NEW.terms_id IS OLD.terms_id
       AND NEW.customer_message_id IS OLD.customer_message_id
       AND NEW.bill_to IS OLD.bill_to
       AND NEW.ship_to IS OLD.ship_to
       AND NEW.po_number IS OLD.po_number
       AND NEW.fob IS OLD.fob
       AND NEW.to_be_printed IS OLD.to_be_printed
       AND NEW.form_style_id IS OLD.form_style_id)
  AND (OLD.status <> 'DRAFT' AND (NEW.tax_cents IS NOT OLD.tax_cents OR NEW.tax_rate_id IS NOT OLD.tax_rate_id))
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: posted invoices cannot be altered');
END;

CREATE TRIGGER trg_inv_discount_immutable
BEFORE UPDATE OF discount_cents ON invoices
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.invoice_number IS OLD.invoice_number
       AND (NEW.customer_name IS OLD.customer_name OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_name = OLD.customer_name AND m.to_name = NEW.customer_name))
       AND NEW.issue_date IS OLD.issue_date
       AND NEW.due_date IS OLD.due_date
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.amount_paid_cents IS OLD.amount_paid_cents
       AND NEW.status IS OLD.status
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.currency_code IS OLD.currency_code
       AND NEW.exchange_rate_e6 IS OLD.exchange_rate_e6
       AND NEW.home_total_cents IS OLD.home_total_cents
       AND NEW.tax_rate_id IS OLD.tax_rate_id
       AND NEW.tax_cents IS OLD.tax_cents
       AND NEW.discount_cents IS OLD.discount_cents
       AND NEW.sales_rep_id IS OLD.sales_rep_id
       AND NEW.ship_date IS OLD.ship_date
       AND NEW.ship_method_id IS OLD.ship_method_id
       AND NEW.terms_id IS OLD.terms_id
       AND NEW.customer_message_id IS OLD.customer_message_id
       AND NEW.bill_to IS OLD.bill_to
       AND NEW.ship_to IS OLD.ship_to
       AND NEW.po_number IS OLD.po_number
       AND NEW.fob IS OLD.fob
       AND NEW.to_be_printed IS OLD.to_be_printed
       AND NEW.form_style_id IS OLD.form_style_id)
  AND (OLD.status <> 'DRAFT' AND NEW.discount_cents IS NOT OLD.discount_cents)
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: posted invoices cannot be altered');
END;

CREATE TRIGGER trg_inv_amount_paid_matches
BEFORE UPDATE OF amount_paid_cents ON invoices
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.invoice_number IS OLD.invoice_number
       AND (NEW.customer_name IS OLD.customer_name OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_name = OLD.customer_name AND m.to_name = NEW.customer_name))
       AND NEW.issue_date IS OLD.issue_date
       AND NEW.due_date IS OLD.due_date
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.amount_paid_cents IS OLD.amount_paid_cents
       AND NEW.status IS OLD.status
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.currency_code IS OLD.currency_code
       AND NEW.exchange_rate_e6 IS OLD.exchange_rate_e6
       AND NEW.home_total_cents IS OLD.home_total_cents
       AND NEW.tax_rate_id IS OLD.tax_rate_id
       AND NEW.tax_cents IS OLD.tax_cents
       AND NEW.discount_cents IS OLD.discount_cents
       AND NEW.sales_rep_id IS OLD.sales_rep_id
       AND NEW.ship_date IS OLD.ship_date
       AND NEW.ship_method_id IS OLD.ship_method_id
       AND NEW.terms_id IS OLD.terms_id
       AND NEW.customer_message_id IS OLD.customer_message_id
       AND NEW.bill_to IS OLD.bill_to
       AND NEW.ship_to IS OLD.ship_to
       AND NEW.po_number IS OLD.po_number
       AND NEW.fob IS OLD.fob
       AND NEW.to_be_printed IS OLD.to_be_printed
       AND NEW.form_style_id IS OLD.form_style_id)
BEGIN
    SELECT RAISE(ABORT, 'INV_PAYMENT_UNPOSTED: every payment must be posted before it is applied')
    WHERE EXISTS (SELECT 1 FROM invoice_payments WHERE invoice_id = NEW.id AND journal_entry_id IS NULL);
    SELECT RAISE(ABORT, 'INV_PAID_MISMATCH: amount paid must equal the sum of recorded payments and applied credits, less bounced checks')
    WHERE NEW.amount_paid_cents <>
          COALESCE((SELECT SUM(amount_cents) FROM invoice_payments WHERE invoice_id = NEW.id AND is_voided = 0), 0)
        + COALESCE((SELECT SUM(amount_cents) FROM credit_applications WHERE invoice_id = NEW.id AND is_voided = 0), 0)
        - COALESCE((SELECT SUM(amount_cents) FROM bounced_checks WHERE invoice_id = NEW.id AND is_voided = 0), 0);
END;

CREATE TRIGGER trg_inv_post_checks
BEFORE UPDATE OF status ON invoices
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.invoice_number IS OLD.invoice_number
       AND (NEW.customer_name IS OLD.customer_name OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_name = OLD.customer_name AND m.to_name = NEW.customer_name))
       AND NEW.issue_date IS OLD.issue_date
       AND NEW.due_date IS OLD.due_date
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.amount_paid_cents IS OLD.amount_paid_cents
       AND NEW.status IS OLD.status
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.currency_code IS OLD.currency_code
       AND NEW.exchange_rate_e6 IS OLD.exchange_rate_e6
       AND NEW.home_total_cents IS OLD.home_total_cents
       AND NEW.tax_rate_id IS OLD.tax_rate_id
       AND NEW.tax_cents IS OLD.tax_cents
       AND NEW.discount_cents IS OLD.discount_cents
       AND NEW.sales_rep_id IS OLD.sales_rep_id
       AND NEW.ship_date IS OLD.ship_date
       AND NEW.ship_method_id IS OLD.ship_method_id
       AND NEW.terms_id IS OLD.terms_id
       AND NEW.customer_message_id IS OLD.customer_message_id
       AND NEW.bill_to IS OLD.bill_to
       AND NEW.ship_to IS OLD.ship_to
       AND NEW.po_number IS OLD.po_number
       AND NEW.fob IS OLD.fob
       AND NEW.to_be_printed IS OLD.to_be_printed
       AND NEW.form_style_id IS OLD.form_style_id)
  AND (OLD.status = 'DRAFT' AND NEW.status = 'UNPAID')
BEGIN
    SELECT RAISE(ABORT, 'INV_NO_ITEMS: an invoice requires at least one line item')
    WHERE NOT EXISTS (SELECT 1 FROM invoice_items WHERE invoice_id = NEW.id);
    SELECT RAISE(ABORT, 'INV_TOTAL_MISMATCH: invoice total must equal its lines less discounts plus sales tax')
    WHERE NEW.total_cents <> (SELECT SUM(amount_cents) FROM invoice_items WHERE invoice_id = NEW.id)
                           + (SELECT COALESCE(SUM(amount_cents), 0) FROM invoice_form_lines WHERE invoice_id = NEW.id AND kind = 'DISCOUNT')
                           - NEW.discount_cents + NEW.tax_cents;
    SELECT RAISE(ABORT, 'INV_JOURNAL_MISSING: invoice must reference its POSTED journal entry')
    WHERE NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED'
          AND source_type = 'INVOICE' AND source_id = NEW.id
    );
END;

CREATE TRIGGER trg_sales_order_invoice_voided AFTER UPDATE OF is_voided ON invoices
WHEN OLD.is_voided = 0 AND NEW.is_voided = 1
BEGIN
    UPDATE sales_order_lines
    SET invoiced_qty_e4 = invoiced_qty_e4 - (SELECT SUM(k.quantity_e4) FROM sales_order_invoiced k
                                             WHERE k.sales_order_line_id = sales_order_lines.id AND k.invoice_id = NEW.id AND k.is_reversed = 0)
    WHERE id IN (SELECT sales_order_line_id FROM sales_order_invoiced WHERE invoice_id = NEW.id AND is_reversed = 0);
    UPDATE sales_order_invoiced SET is_reversed = 1 WHERE invoice_id = NEW.id AND is_reversed = 0;
END;

-- Only an invoice whose entry is voided can be marked replaced (an edit marks it before the corrected
-- invoice is written; the invoice row itself is flagged void once its payments have moved), and
-- the mark is never taken off.
CREATE TRIGGER trg_inv_replaced_rules
BEFORE UPDATE OF is_replaced ON invoices
WHEN NEW.is_replaced IS NOT OLD.is_replaced
 AND (OLD.is_replaced = 1
      OR NOT EXISTS (SELECT 1 FROM journal_entries e WHERE e.id = OLD.journal_entry_id AND e.is_voided = 1))
BEGIN
    SELECT RAISE(ABORT, 'INV_REPLACED: only an invoice whose entry is voided can be marked replaced, and the mark stays');
END;

CREATE TRIGGER trg_inv_items_insert_draft_only
BEFORE INSERT ON invoice_items
WHEN (SELECT status FROM invoices WHERE id = NEW.invoice_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: items can only be added to a DRAFT invoice');
END;

CREATE TRIGGER trg_inv_items_delete_draft_only
BEFORE DELETE ON invoice_items
WHEN (SELECT status FROM invoices WHERE id = OLD.invoice_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: items can only be removed from a DRAFT invoice');
END;

CREATE TRIGGER trg_pay_insert_checks
BEFORE INSERT ON invoice_payments
BEGIN
    SELECT RAISE(ABORT, 'INV_NOT_OPEN: payments can only be applied to open invoices')
    WHERE (SELECT status FROM invoices WHERE id = NEW.invoice_id) NOT IN ('UNPAID', 'PARTIAL')
       OR (SELECT is_voided FROM invoices WHERE id = NEW.invoice_id) = 1;
    SELECT RAISE(ABORT, 'INV_OVERPAYMENT: payment exceeds the open balance')
    WHERE NEW.amount_cents > (SELECT total_cents - amount_paid_cents FROM invoices WHERE id = NEW.invoice_id);
    SELECT RAISE(ABORT, 'INV_BAD_DEPOSIT_ACCOUNT: deposit to a Bank or Undeposited Funds account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.deposit_account_id) NOT IN ('BANK', 'OTHER_CURRENT_ASSET');
    SELECT RAISE(ABORT, 'INV_PAYMENT_PREPOSTED: insert the payment first, then attach its journal entry')
    WHERE NEW.journal_entry_id IS NOT NULL OR NEW.is_voided <> 0;
END;

CREATE TRIGGER trg_inv_form_lines_insert_draft_only
BEFORE INSERT ON invoice_form_lines
WHEN (SELECT status FROM invoices WHERE id = NEW.invoice_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: lines can only be added to a DRAFT invoice');
END;

CREATE TRIGGER trg_inv_form_lines_delete_draft_only
BEFORE DELETE ON invoice_form_lines
WHEN (SELECT status FROM invoices WHERE id = OLD.invoice_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: lines can only be removed from a DRAFT invoice');
END;

CREATE TRIGGER trg_pay_update_rules
BEFORE UPDATE ON invoice_payments
BEGIN
    SELECT RAISE(ABORT, 'INV_PAYMENT_IMMUTABLE: recorded payments cannot be changed')
    WHERE (NEW.invoice_id IS NOT OLD.invoice_id AND NOT (
               OLD.is_voided = 0 AND NEW.is_voided = 0
               AND EXISTS (SELECT 1 FROM v_invoice_replacements
                           WHERE old_invoice_id = OLD.invoice_id AND new_invoice_id = NEW.invoice_id)))
       OR NEW.payment_date IS NOT OLD.payment_date
       OR NEW.amount_cents IS NOT OLD.amount_cents
       OR NEW.payment_method IS NOT OLD.payment_method
       OR NEW.reference IS NOT OLD.reference
       OR NEW.deposit_account_id IS NOT OLD.deposit_account_id
       OR NEW.created_at IS NOT OLD.created_at
       OR (OLD.journal_entry_id IS NOT NULL AND NEW.journal_entry_id IS NOT OLD.journal_entry_id)
       OR NEW.journal_entry_id IS NULL;
    SELECT RAISE(ABORT, 'INV_PAYMENT_ENTRY_MISMATCH: payment must reference its POSTED journal entry')
    WHERE OLD.journal_entry_id IS NULL AND NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = 'RECEIVE_PAYMENT' AND source_id = NEW.id
    );
    SELECT RAISE(ABORT, 'INV_PAYMENT_UNVOID: a voided payment cannot be restored')
    WHERE OLD.is_voided = 1 AND NEW.is_voided = 0;
    SELECT RAISE(ABORT, 'INV_PAYMENT_VOID_ENTRY_ACTIVE: the payment''s journal entry must be voided first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
    SELECT RAISE(ABORT, 'INV_PAYMENT_DEPOSITED: void the bank deposit that includes this payment first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND EXISTS (SELECT 1 FROM deposit_lines WHERE invoice_payment_id = OLD.id AND is_voided = 0);
END;

CREATE TRIGGER trg_credit_applications_once
BEFORE UPDATE ON credit_applications
WHEN NEW.is_voided < OLD.is_voided OR NEW.amount_cents IS NOT OLD.amount_cents
  OR NEW.credit_memo_id IS NOT OLD.credit_memo_id
  OR (NEW.invoice_id IS NOT OLD.invoice_id AND NOT (
          OLD.is_voided = 0 AND NEW.is_voided = 0
          AND EXISTS (SELECT 1 FROM v_invoice_replacements
                      WHERE old_invoice_id = OLD.invoice_id AND new_invoice_id = NEW.invoice_id)))
BEGIN
    SELECT RAISE(ABORT, 'CM_APPLICATION_IMMUTABLE: a credit application can only be removed');
END;

CREATE TRIGGER trg_bounced_receipt_insert_checks
BEFORE INSERT ON bounced_sales_receipts
BEGIN
    SELECT RAISE(ABORT, 'BOUNCE_PREVOIDED: a bounced check is recorded live')
    WHERE NEW.is_voided <> 0;
    SELECT RAISE(ABORT, 'BOUNCE_JOURNAL_MISSING: a bounced check must reference its POSTED journal entry')
    WHERE NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = 'JOURNAL' AND is_voided = 0
    );
    SELECT RAISE(ABORT, 'BOUNCE_PAYMENT_VOIDED: a voided sales receipt cannot bounce')
    WHERE (SELECT is_voided FROM sales_receipts WHERE id = NEW.sales_receipt_id) <> 0;
    SELECT RAISE(ABORT, 'BOUNCE_WRONG_AMOUNT: a sales receipt bounces for its full amount')
    WHERE NEW.amount_cents IS NOT (SELECT total_cents FROM sales_receipts WHERE id = NEW.sales_receipt_id);
    SELECT RAISE(ABORT, 'BOUNCE_WRONG_CUSTOMER: the charge-back goes to the sales receipt''s customer')
    WHERE NEW.customer_id IS NOT (SELECT customer_id FROM sales_receipts WHERE id = NEW.sales_receipt_id)
       OR NEW.customer_id IS NOT (SELECT customer_id FROM invoices WHERE id = NEW.charge_invoice_id);
    SELECT RAISE(ABORT, 'BOUNCE_BAD_BANK: a bounced check comes out of a Bank account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.bank_account_id) IS NOT 'BANK';
END;

CREATE TRIGGER trg_est_status_rules
BEFORE UPDATE ON estimates
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.estimate_number IS OLD.estimate_number
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.estimate_date IS OLD.estimate_date
       AND NEW.expiration_date IS OLD.expiration_date
       AND NEW.status IS OLD.status
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.converted_to_invoice_id IS OLD.converted_to_invoice_id
       AND NEW.created_at IS OLD.created_at
       AND NEW.tax_rate_id IS OLD.tax_rate_id
       AND NEW.tax_cents IS OLD.tax_cents
       AND NEW.discount_cents IS OLD.discount_cents
       AND NEW.sales_rep_id IS OLD.sales_rep_id
       AND NEW.ship_date IS OLD.ship_date
       AND NEW.ship_method_id IS OLD.ship_method_id
       AND NEW.customer_message_id IS OLD.customer_message_id
       AND NEW.bill_to IS OLD.bill_to
       AND NEW.ship_to IS OLD.ship_to
       AND NEW.po_number IS OLD.po_number
       AND NEW.fob IS OLD.fob
       AND NEW.form_style_id IS OLD.form_style_id)
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

CREATE TRIGGER trg_bounce_update_rules
BEFORE UPDATE ON bounced_checks
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.invoice_payment_id IS OLD.invoice_payment_id
       AND NEW.deposit_line_id IS OLD.deposit_line_id
       AND NEW.invoice_id IS OLD.invoice_id
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.bank_account_id IS OLD.bank_account_id
       AND NEW.bounce_date IS OLD.bounce_date
       AND NEW.amount_cents IS OLD.amount_cents
       AND NEW.bank_fee_cents IS OLD.bank_fee_cents
       AND NEW.bank_fee_account_id IS OLD.bank_fee_account_id
       AND NEW.fee_invoice_id IS OLD.fee_invoice_id
       AND NEW.memo IS OLD.memo
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.created_at IS OLD.created_at)
BEGIN
    SELECT RAISE(ABORT, 'BOUNCE_IMMUTABLE: a recorded bounced check cannot be changed; void it instead')
    WHERE NEW.invoice_payment_id IS NOT OLD.invoice_payment_id OR NEW.deposit_line_id IS NOT OLD.deposit_line_id
       OR (NEW.invoice_id IS NOT OLD.invoice_id AND NOT (
               OLD.is_voided = 0 AND NEW.is_voided = 0
               AND NEW.invoice_id IS (SELECT invoice_id FROM invoice_payments WHERE id = NEW.invoice_payment_id)
               AND EXISTS (SELECT 1 FROM v_invoice_replacements
                           WHERE old_invoice_id = OLD.invoice_id AND new_invoice_id = NEW.invoice_id)))
       OR NEW.customer_id IS NOT OLD.customer_id
       OR NEW.bank_account_id IS NOT OLD.bank_account_id OR NEW.bounce_date IS NOT OLD.bounce_date
       OR NEW.amount_cents IS NOT OLD.amount_cents OR NEW.bank_fee_cents IS NOT OLD.bank_fee_cents
       OR NEW.bank_fee_account_id IS NOT OLD.bank_fee_account_id OR NEW.fee_invoice_id IS NOT OLD.fee_invoice_id
       OR NEW.memo IS NOT OLD.memo OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
       OR NEW.created_at IS NOT OLD.created_at;
    SELECT RAISE(ABORT, 'BOUNCE_UNVOID: a voided bounced check cannot be restored')
    WHERE OLD.is_voided = 1 AND NEW.is_voided = 0;
    SELECT RAISE(ABORT, 'BOUNCE_VOID_ENTRY_ACTIVE: the bounced check''s journal entry must be voided first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
END;

CREATE TRIGGER trg_inv_items_update_draft_only
BEFORE UPDATE ON invoice_items
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.invoice_id IS OLD.invoice_id
       AND NEW.line_number IS OLD.line_number
       AND NEW.description IS OLD.description
       AND NEW.quantity_e4 IS OLD.quantity_e4
       AND NEW.rate_cents IS OLD.rate_cents
       AND NEW.amount_cents IS OLD.amount_cents
       AND NEW.income_account_id IS OLD.income_account_id
       AND (NEW.product_service_id IS OLD.product_service_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'ITEM' AND m.from_id = OLD.product_service_id AND m.to_id = NEW.product_service_id))
       AND NEW.class_id IS OLD.class_id
       AND NEW.is_taxable IS OLD.is_taxable
       AND NEW.percent_ppm IS OLD.percent_ppm
       AND NEW.service_date IS OLD.service_date)
  AND ((SELECT status FROM invoices WHERE id = OLD.invoice_id) <> 'DRAFT'
  OR (SELECT status FROM invoices WHERE id = NEW.invoice_id) <> 'DRAFT')
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: items can only be changed on a DRAFT invoice');
END;

CREATE TRIGGER trg_inv_form_lines_update_draft_only
BEFORE UPDATE ON invoice_form_lines
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.invoice_id IS OLD.invoice_id
       AND NEW.line_number IS OLD.line_number
       AND NEW.kind IS OLD.kind
       AND NEW.description IS OLD.description
       AND NEW.amount_cents IS OLD.amount_cents
       AND (NEW.product_service_id IS OLD.product_service_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'ITEM' AND m.from_id = OLD.product_service_id AND m.to_id = NEW.product_service_id))
       AND NEW.percent_ppm IS OLD.percent_ppm
       AND NEW.account_id IS OLD.account_id
       AND NEW.class_id IS OLD.class_id
       AND NEW.is_taxable IS OLD.is_taxable
       AND NEW.item_group_id IS OLD.item_group_id
       AND NEW.group_quantity_e4 IS OLD.group_quantity_e4
       AND NEW.group_line_count IS OLD.group_line_count
       AND NEW.print_items IS OLD.print_items)
  AND ((SELECT status FROM invoices WHERE id = OLD.invoice_id) <> 'DRAFT'
  OR (SELECT status FROM invoices WHERE id = NEW.invoice_id) <> 'DRAFT')
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: lines can only be changed on a DRAFT invoice');
END;

CREATE VIEW v_invoice_replacements AS
SELECT o.id AS old_invoice_id, n.id AS new_invoice_id
FROM transaction_edits te
JOIN invoices o ON o.journal_entry_id = te.old_entry_id
JOIN invoices n ON n.journal_entry_id = te.new_entry_id
WHERE n.is_voided = 0
  AND n.customer_id IS o.customer_id
  AND n.currency_code IS o.currency_code
  AND n.exchange_rate_e6 IS o.exchange_rate_e6;

CREATE VIEW v_invoice_balances AS
SELECT
    i.id AS invoice_id,
    COALESCE(i.home_total_cents, i.total_cents) AS home_total_cents,
    COALESCE(i.home_total_cents, i.total_cents)
      - COALESCE((SELECT SUM(COALESCE(p.ar_relief_cents, p.amount_cents))
                  FROM invoice_payments p WHERE p.invoice_id = i.id AND p.is_voided = 0), 0)
      - COALESCE((SELECT SUM(a.amount_cents) FROM credit_applications a WHERE a.invoice_id = i.id AND a.is_voided = 0), 0)
      + COALESCE((SELECT SUM(COALESCE(p.ar_relief_cents, b.amount_cents)) FROM bounced_checks b
                  LEFT JOIN invoice_payments p ON p.id = b.invoice_payment_id
                  WHERE b.invoice_id = i.id AND b.is_voided = 0), 0)
      AS home_open_cents
FROM invoices i;

CREATE VIEW v_entry_party AS
SELECT e.id AS journal_entry_id,
       CASE x.source_type
           WHEN 'INVOICE'         THEN (SELECT c.name FROM invoices d JOIN customers c ON c.id = d.customer_id WHERE d.id = x.source_id)
           WHEN 'RECEIVE_PAYMENT' THEN (SELECT c.name FROM invoice_payments p JOIN invoices d ON d.id = p.invoice_id JOIN customers c ON c.id = d.customer_id WHERE p.id = x.source_id)
           WHEN 'SALES_RECEIPT'   THEN (SELECT c.name FROM sales_receipts d JOIN customers c ON c.id = d.customer_id WHERE d.id = x.source_id)
           WHEN 'CREDIT_MEMO'     THEN (SELECT c.name FROM credit_memos d JOIN customers c ON c.id = d.customer_id WHERE d.id = x.source_id)
           WHEN 'BILL'            THEN (SELECT v.name FROM bills d JOIN vendors v ON v.id = d.vendor_id WHERE d.id = x.source_id)
           WHEN 'BILL_PAYMENT'    THEN (SELECT v.name FROM bill_payments p JOIN bills d ON d.id = p.bill_id JOIN vendors v ON v.id = d.vendor_id WHERE p.id = x.source_id)
           WHEN 'VENDOR_CREDIT'   THEN (SELECT v.name FROM vendor_credits d JOIN vendors v ON v.id = d.vendor_id WHERE d.id = x.source_id)
           WHEN 'EXPENSE'         THEN (SELECT COALESCE(v.name, o2.name) FROM expenses d LEFT JOIN vendors v ON v.id = d.vendor_id
                                        LEFT JOIN other_names o2 ON o2.id = d.other_name_id WHERE d.id = x.source_id)
           WHEN 'CHECK'           THEN (SELECT COALESCE(v.name, o2.name) FROM expenses d LEFT JOIN vendors v ON v.id = d.vendor_id
                                        LEFT JOIN other_names o2 ON o2.id = d.other_name_id WHERE d.id = x.source_id)
           WHEN 'DEPOSIT'         THEN (SELECT COALESCE(c.name, v.name) FROM v_deposit_party d
                                        LEFT JOIN customers c ON c.id = d.customer_id LEFT JOIN vendors v ON v.id = d.vendor_id
                                        WHERE d.deposit_id = x.source_id)
           WHEN 'JOURNAL'         THEN COALESCE(
               (SELECT c.name FROM credit_refunds r JOIN credit_memos d ON d.id = r.credit_memo_id JOIN customers c ON c.id = d.customer_id WHERE r.journal_entry_id = x.id),
               (SELECT v.name FROM vendor_credit_refunds r JOIN vendor_credits d ON d.id = r.vendor_credit_id JOIN vendors v ON v.id = d.vendor_id WHERE r.journal_entry_id = x.id),
               (SELECT c.name FROM bounced_checks b JOIN customers c ON c.id = b.customer_id WHERE b.journal_entry_id = x.id))
       END AS party_name
FROM journal_entries e
LEFT JOIN journal_entries o ON o.id = e.reverses_entry_id
JOIN journal_entries x ON x.id = COALESCE(o.id, e.id);
