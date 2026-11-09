-- 0099_merge_names.sql
-- Merging two customers, vendors, items or accounts (gap T30).
--
-- Renaming one entry to another's name offers to merge them: everything that names the one merged
-- away is pointed at the one kept, and the merged one is made inactive with `merged_into_id` set.
-- Posted journal rows never change. Customers, vendors and items appear on documents and lists, not
-- on journal lines, so their history moves with the documents. An account's posted lines keep it,
-- so merging accounts posts one entry moving its balance into the account kept, and points the
-- lists and settings that name it (items, tax agencies, budgets, subaccounts, unposted estimates
-- and purchase orders) at the account kept.
--
-- `name_merges` is the merge's record, append-only and audited. While a merge is applied (inside
-- its one write transaction) its row has `applying = 1`, and the document triggers below let a row
-- change in exactly one way: a name column moving from the merged entry to the kept one, every
-- other column unchanged. The merge clears `applying` before it commits.

CREATE TABLE name_merges (
    id                  INTEGER PRIMARY KEY,
    kind                TEXT    NOT NULL CHECK (kind IN ('CUSTOMER', 'VENDOR', 'ITEM', 'ACCOUNT')),
    from_id             INTEGER NOT NULL,
    to_id               INTEGER NOT NULL,
    from_name           TEXT    NOT NULL,
    to_name             TEXT    NOT NULL,
    -- An account merge's balance-moving entry; NULL when there was no balance, and for the others.
    journal_entry_id    INTEGER REFERENCES journal_entries (id) ON DELETE RESTRICT,
    applying            INTEGER NOT NULL DEFAULT 1 CHECK (applying IN (0, 1)),
    merged_at           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (from_id <> to_id),
    CHECK (journal_entry_id IS NULL OR kind = 'ACCOUNT')
) STRICT;
CREATE UNIQUE INDEX ux_name_merges_from ON name_merges (kind, from_id);
CREATE INDEX ix_name_merges_applying ON name_merges (kind, from_id, to_id) WHERE applying = 1;

CREATE TRIGGER trg_name_merges_insert
BEFORE INSERT ON name_merges
BEGIN
    SELECT RAISE(ABORT, 'MERGE_ONE_AT_A_TIME: a merge is already being applied')
    WHERE EXISTS (SELECT 1 FROM name_merges WHERE applying = 1);
    SELECT RAISE(ABORT, 'MERGE_STARTS_APPLYING: a merge is recorded as it is applied')
    WHERE NEW.applying <> 1;
END;

CREATE TRIGGER trg_name_merges_update
BEFORE UPDATE ON name_merges
BEGIN
    SELECT RAISE(ABORT, 'MERGE_IMMUTABLE: a merge record cannot be changed')
    WHERE NEW.kind IS NOT OLD.kind OR NEW.from_id IS NOT OLD.from_id OR NEW.to_id IS NOT OLD.to_id
       OR NEW.from_name IS NOT OLD.from_name OR NEW.to_name IS NOT OLD.to_name
       OR NEW.journal_entry_id IS NOT OLD.journal_entry_id OR NEW.merged_at IS NOT OLD.merged_at
       OR NEW.applying > OLD.applying;
END;

CREATE TRIGGER trg_name_merges_no_delete BEFORE DELETE ON name_merges
BEGIN
    SELECT RAISE(ABORT, 'MERGE_IMMUTABLE: a merge record cannot be deleted');
END;

-- The audit trail: one LIST_CHANGE record per merge, sealed with the merge's transaction.
CREATE TRIGGER trg_ledger_audit_name_merge
AFTER INSERT ON name_merges
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.to_id, json_object(
        'list', NEW.kind,
        'merge', json_object('fromId', NEW.from_id, 'fromName', NEW.from_name, 'toId', NEW.to_id, 'toName', NEW.to_name)
    ));
END;

ALTER TABLE customers ADD COLUMN merged_into_id INTEGER REFERENCES customers (id) ON DELETE RESTRICT;
ALTER TABLE vendors ADD COLUMN merged_into_id INTEGER REFERENCES vendors (id) ON DELETE RESTRICT;
ALTER TABLE products_services ADD COLUMN merged_into_id INTEGER REFERENCES products_services (id) ON DELETE RESTRICT;
ALTER TABLE chart_of_accounts ADD COLUMN merged_into_id INTEGER REFERENCES chart_of_accounts (id) ON DELETE RESTRICT;

-- ---------------------------------------------------------------------------------------------
-- Document triggers: each lets a merge repoint a row, and nothing else (generated from the
-- triggers as earlier migrations left them; see the test `merge_exceptions_cover_every_trigger`).
-- ---------------------------------------------------------------------------------------------

DROP TRIGGER trg_inv_posted_header_immutable;
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
       AND NEW.to_be_printed IS OLD.to_be_printed)
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

DROP TRIGGER trg_inv_status_transition;
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
       AND NEW.to_be_printed IS OLD.to_be_printed)
  AND (NOT (
       (OLD.status = 'DRAFT' AND NEW.status IN ('DRAFT', 'UNPAID'))
    OR (OLD.status IN ('UNPAID', 'PARTIAL', 'PAID') AND NEW.status IN ('UNPAID', 'PARTIAL', 'PAID'))
))
BEGIN
    SELECT RAISE(ABORT, 'INV_INVALID_TRANSITION: a posted invoice cannot return to DRAFT');
END;

DROP TRIGGER trg_inv_void;
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
       AND NEW.to_be_printed IS OLD.to_be_printed)
  AND (NEW.is_voided IS NOT OLD.is_voided)
BEGIN
    SELECT RAISE(ABORT, 'INV_UNVOID: a voided invoice cannot be restored') WHERE OLD.is_voided = 1;
    SELECT RAISE(ABORT, 'INV_VOID_HAS_PAYMENTS: void the payments applied to this invoice first')
    WHERE OLD.amount_paid_cents <> 0;
    SELECT RAISE(ABORT, 'INV_VOID_ENTRY_ACTIVE: the invoice''s journal entry must be voided first')
    WHERE NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
END;

DROP TRIGGER trg_inv_currency_immutable;
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
       AND NEW.to_be_printed IS OLD.to_be_printed)
  AND (OLD.status <> 'DRAFT')
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: the currency of a posted invoice cannot change');
END;

DROP TRIGGER trg_inv_tax_immutable;
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
       AND NEW.to_be_printed IS OLD.to_be_printed)
  AND (OLD.status <> 'DRAFT' AND (NEW.tax_cents IS NOT OLD.tax_cents OR NEW.tax_rate_id IS NOT OLD.tax_rate_id))
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: posted invoices cannot be altered');
END;

DROP TRIGGER trg_inv_discount_immutable;
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
       AND NEW.to_be_printed IS OLD.to_be_printed)
  AND (OLD.status <> 'DRAFT' AND NEW.discount_cents IS NOT OLD.discount_cents)
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: posted invoices cannot be altered');
END;

DROP TRIGGER trg_inv_amount_paid_matches;
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
       AND NEW.to_be_printed IS OLD.to_be_printed)
BEGIN
    SELECT RAISE(ABORT, 'INV_PAYMENT_UNPOSTED: every payment must be posted before it is applied')
    WHERE EXISTS (SELECT 1 FROM invoice_payments WHERE invoice_id = NEW.id AND journal_entry_id IS NULL);
    SELECT RAISE(ABORT, 'INV_PAID_MISMATCH: amount paid must equal the sum of recorded payments and applied credits, less bounced checks')
    WHERE NEW.amount_paid_cents <>
          COALESCE((SELECT SUM(amount_cents) FROM invoice_payments WHERE invoice_id = NEW.id AND is_voided = 0), 0)
        + COALESCE((SELECT SUM(amount_cents) FROM credit_applications WHERE invoice_id = NEW.id AND is_voided = 0), 0)
        - COALESCE((SELECT SUM(amount_cents) FROM bounced_checks WHERE invoice_id = NEW.id AND is_voided = 0), 0);
END;

DROP TRIGGER trg_inv_post_checks;
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
       AND NEW.to_be_printed IS OLD.to_be_printed)
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

DROP TRIGGER trg_uf_matched_receipt_void;
CREATE TRIGGER trg_uf_matched_receipt_void
BEFORE UPDATE OF is_voided ON sales_receipts
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.receipt_number IS OLD.receipt_number
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.receipt_date IS OLD.receipt_date
       AND NEW.deposit_account_id IS OLD.deposit_account_id
       AND NEW.payment_method IS OLD.payment_method
       AND NEW.reference IS OLD.reference
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.tax_rate_id IS OLD.tax_rate_id
       AND NEW.tax_cents IS OLD.tax_cents
       AND NEW.discount_cents IS OLD.discount_cents
       AND NEW.sales_rep_id IS OLD.sales_rep_id
       AND NEW.ship_date IS OLD.ship_date
       AND NEW.ship_method_id IS OLD.ship_method_id
       AND NEW.payment_method_id IS OLD.payment_method_id
       AND NEW.customer_message_id IS OLD.customer_message_id
       AND NEW.bill_to IS OLD.bill_to
       AND NEW.ship_to IS OLD.ship_to
       AND NEW.po_number IS OLD.po_number
       AND NEW.fob IS OLD.fob)
  AND (NEW.is_voided = 1 AND OLD.is_voided = 0)
BEGIN
    SELECT RAISE(ABORT, 'VOID_DEPOSITED: this sales receipt is matched to a deposit; undo the match in Clean Up Books first')
    WHERE EXISTS (SELECT 1 FROM undeposited_fund_matches WHERE sales_receipt_id = NEW.id AND is_voided = 0);
END;

DROP TRIGGER trg_sr_update_rules;
CREATE TRIGGER trg_sr_update_rules
BEFORE UPDATE ON sales_receipts
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.receipt_number IS OLD.receipt_number
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.receipt_date IS OLD.receipt_date
       AND NEW.deposit_account_id IS OLD.deposit_account_id
       AND NEW.payment_method IS OLD.payment_method
       AND NEW.reference IS OLD.reference
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.tax_rate_id IS OLD.tax_rate_id
       AND NEW.tax_cents IS OLD.tax_cents
       AND NEW.discount_cents IS OLD.discount_cents
       AND NEW.sales_rep_id IS OLD.sales_rep_id
       AND NEW.ship_date IS OLD.ship_date
       AND NEW.ship_method_id IS OLD.ship_method_id
       AND NEW.payment_method_id IS OLD.payment_method_id
       AND NEW.customer_message_id IS OLD.customer_message_id
       AND NEW.bill_to IS OLD.bill_to
       AND NEW.ship_to IS OLD.ship_to
       AND NEW.po_number IS OLD.po_number
       AND NEW.fob IS OLD.fob)
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
    SELECT RAISE(ABORT, 'SR_TOTAL_MISMATCH: receipt total must equal its lines less discounts plus sales tax')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NEW.total_cents <> (SELECT SUM(amount_cents) FROM sales_receipt_lines WHERE sales_receipt_id = NEW.id)
                           + (SELECT COALESCE(SUM(amount_cents), 0) FROM sales_receipt_form_lines WHERE sales_receipt_id = NEW.id AND kind = 'DISCOUNT')
                           - NEW.discount_cents + NEW.tax_cents;
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

DROP TRIGGER trg_est_status_rules;
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
       AND NEW.fob IS OLD.fob)
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

DROP TRIGGER trg_credit_memo_immutable;
CREATE TRIGGER trg_credit_memo_immutable
BEFORE UPDATE ON credit_memos
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.credit_number IS OLD.credit_number
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.credit_date IS OLD.credit_date
       AND NEW.memo IS OLD.memo
       AND NEW.refund_account_id IS OLD.refund_account_id
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.created_at IS OLD.created_at
       AND NEW.tax_rate_id IS OLD.tax_rate_id
       AND NEW.tax_cents IS OLD.tax_cents
       AND NEW.bill_to IS OLD.bill_to
       AND NEW.ship_to IS OLD.ship_to
       AND NEW.po_number IS OLD.po_number
       AND NEW.fob IS OLD.fob)
  AND (OLD.journal_entry_id IS NOT NULL
 AND (   NEW.credit_number IS NOT OLD.credit_number OR NEW.customer_id IS NOT OLD.customer_id
      OR NEW.credit_date IS NOT OLD.credit_date OR NEW.total_cents IS NOT OLD.total_cents
      OR NEW.refund_account_id IS NOT OLD.refund_account_id OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
      OR (OLD.is_voided = 1 AND NEW.is_voided = 0)))
BEGIN
    SELECT RAISE(ABORT, 'CM_IMMUTABLE: posted credit memos cannot be altered; void instead');
END;

DROP TRIGGER trg_cm_tax_immutable;
CREATE TRIGGER trg_cm_tax_immutable
BEFORE UPDATE OF tax_cents, tax_rate_id ON credit_memos
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.credit_number IS OLD.credit_number
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.credit_date IS OLD.credit_date
       AND NEW.memo IS OLD.memo
       AND NEW.refund_account_id IS OLD.refund_account_id
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.created_at IS OLD.created_at
       AND NEW.tax_rate_id IS OLD.tax_rate_id
       AND NEW.tax_cents IS OLD.tax_cents
       AND NEW.bill_to IS OLD.bill_to
       AND NEW.ship_to IS OLD.ship_to
       AND NEW.po_number IS OLD.po_number
       AND NEW.fob IS OLD.fob)
  AND (OLD.journal_entry_id IS NOT NULL AND (NEW.tax_cents IS NOT OLD.tax_cents OR NEW.tax_rate_id IS NOT OLD.tax_rate_id))
BEGIN
    SELECT RAISE(ABORT, 'CM_IMMUTABLE: posted credit memos cannot be altered; void instead');
END;

DROP TRIGGER trg_bounce_update_rules;
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

DROP TRIGGER trg_dep_lines_update_rules;
CREATE TRIGGER trg_dep_lines_update_rules
BEFORE UPDATE ON deposit_lines
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.deposit_id IS OLD.deposit_id
       AND NEW.line_number IS OLD.line_number
       AND NEW.invoice_payment_id IS OLD.invoice_payment_id
       AND NEW.sales_receipt_id IS OLD.sales_receipt_id
       AND NEW.account_id IS OLD.account_id
       AND NEW.amount_cents IS OLD.amount_cents
       AND NEW.memo IS OLD.memo
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.class_id IS OLD.class_id
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id)))
BEGIN
    SELECT RAISE(ABORT, 'DEP_IMMUTABLE: deposit lines can only be marked void along with their deposit')
    WHERE NEW.deposit_id IS NOT OLD.deposit_id OR NEW.line_number IS NOT OLD.line_number
       OR NEW.invoice_payment_id IS NOT OLD.invoice_payment_id OR NEW.sales_receipt_id IS NOT OLD.sales_receipt_id
       OR NEW.account_id IS NOT OLD.account_id OR NEW.amount_cents IS NOT OLD.amount_cents OR NEW.memo IS NOT OLD.memo
       OR (OLD.is_voided = 1 AND NEW.is_voided = 0)
       OR (NEW.is_voided = 1 AND (SELECT is_voided FROM deposits WHERE id = OLD.deposit_id) = 0);
END;

DROP TRIGGER trg_dep_lines_name_fixed;
CREATE TRIGGER trg_dep_lines_name_fixed
BEFORE UPDATE OF customer_id, vendor_id ON deposit_lines
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.deposit_id IS OLD.deposit_id
       AND NEW.line_number IS OLD.line_number
       AND NEW.invoice_payment_id IS OLD.invoice_payment_id
       AND NEW.sales_receipt_id IS OLD.sales_receipt_id
       AND NEW.account_id IS OLD.account_id
       AND NEW.amount_cents IS OLD.amount_cents
       AND NEW.memo IS OLD.memo
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.class_id IS OLD.class_id
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id)))
  AND (NEW.customer_id IS NOT OLD.customer_id OR NEW.vendor_id IS NOT OLD.vendor_id)
BEGIN
    SELECT RAISE(ABORT, 'DEP_IMMUTABLE: deposit lines can only be marked void along with their deposit');
END;

DROP TRIGGER trg_bill_lines_update_draft_only;
CREATE TRIGGER trg_bill_lines_update_draft_only
BEFORE UPDATE ON bill_lines
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.bill_id IS OLD.bill_id
       AND NEW.line_number IS OLD.line_number
       AND NEW.memo IS OLD.memo
       AND NEW.account_id IS OLD.account_id
       AND NEW.amount_cents IS OLD.amount_cents
       AND NEW.quantity_e4 IS OLD.quantity_e4
       AND NEW.rate_cents IS OLD.rate_cents
       AND (NEW.product_service_id IS OLD.product_service_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'ITEM' AND m.from_id = OLD.product_service_id AND m.to_id = NEW.product_service_id))
       AND NEW.class_id IS OLD.class_id
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.is_billable IS OLD.is_billable)
  AND ((SELECT status FROM bills WHERE id = OLD.bill_id) <> 'DRAFT'
  OR (SELECT status FROM bills WHERE id = NEW.bill_id) <> 'DRAFT')
BEGIN
    SELECT RAISE(ABORT, 'BILL_IMMUTABLE: lines can only be changed on a DRAFT bill');
END;

DROP TRIGGER trg_exp_lines_update_unposted;
CREATE TRIGGER trg_exp_lines_update_unposted
BEFORE UPDATE ON expense_lines
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.expense_id IS OLD.expense_id
       AND NEW.line_number IS OLD.line_number
       AND NEW.account_id IS OLD.account_id
       AND NEW.amount_cents IS OLD.amount_cents
       AND NEW.memo IS OLD.memo
       AND NEW.class_id IS OLD.class_id
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id))
       AND NEW.is_billable IS OLD.is_billable)
  AND ((SELECT journal_entry_id FROM expenses WHERE id = OLD.expense_id) IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'EXP_IMMUTABLE: lines of a posted expense cannot be changed');
END;

DROP TRIGGER trg_vendor_credit_lines_immutable_upd;
CREATE TRIGGER trg_vendor_credit_lines_immutable_upd
BEFORE UPDATE ON vendor_credit_lines
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.vendor_credit_id IS OLD.vendor_credit_id
       AND NEW.line_number IS OLD.line_number
       AND NEW.account_id IS OLD.account_id
       AND NEW.amount_cents IS OLD.amount_cents
       AND NEW.memo IS OLD.memo
       AND NEW.class_id IS OLD.class_id
       AND (NEW.customer_id IS OLD.customer_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'CUSTOMER' AND m.from_id = OLD.customer_id AND m.to_id = NEW.customer_id)))
  AND ((SELECT journal_entry_id FROM vendor_credits WHERE id = OLD.vendor_credit_id) IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'VC_IMMUTABLE: lines of a posted vendor credit cannot change');
END;

DROP TRIGGER trg_bill_post_checks;
CREATE TRIGGER trg_bill_post_checks
BEFORE UPDATE OF status ON bills
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id))
       AND NEW.bill_number IS OLD.bill_number
       AND NEW.bill_date IS OLD.bill_date
       AND NEW.due_date IS OLD.due_date
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.amount_paid_cents IS OLD.amount_paid_cents
       AND NEW.status IS OLD.status
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.terms_id IS OLD.terms_id)
  AND (OLD.status = 'DRAFT' AND NEW.status = 'UNPAID')
BEGIN
    SELECT RAISE(ABORT, 'BILL_NO_ITEMS: a bill requires at least one line')
    WHERE NOT EXISTS (SELECT 1 FROM bill_lines WHERE bill_id = NEW.id);
    SELECT RAISE(ABORT, 'BILL_TOTAL_MISMATCH: bill total must equal the sum of its lines')
    WHERE NEW.total_cents <> (SELECT SUM(amount_cents) FROM bill_lines WHERE bill_id = NEW.id);
    SELECT RAISE(ABORT, 'BILL_JOURNAL_MISSING: bill must reference its POSTED journal entry')
    WHERE NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = 'BILL' AND source_id = NEW.id
    );
END;

DROP TRIGGER trg_bill_posted_header_immutable;
CREATE TRIGGER trg_bill_posted_header_immutable
BEFORE UPDATE ON bills
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id))
       AND NEW.bill_number IS OLD.bill_number
       AND NEW.bill_date IS OLD.bill_date
       AND NEW.due_date IS OLD.due_date
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.amount_paid_cents IS OLD.amount_paid_cents
       AND NEW.status IS OLD.status
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.terms_id IS OLD.terms_id)
  AND (OLD.status <> 'DRAFT'
 AND (   NEW.vendor_id        IS NOT OLD.vendor_id
      OR NEW.bill_number      IS NOT OLD.bill_number
      OR NEW.bill_date        IS NOT OLD.bill_date
      OR NEW.due_date         IS NOT OLD.due_date
      OR NEW.memo             IS NOT OLD.memo
      OR NEW.total_cents      IS NOT OLD.total_cents
      OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
      OR NEW.created_at       IS NOT OLD.created_at))
BEGIN
    SELECT RAISE(ABORT, 'BILL_IMMUTABLE: posted bills cannot be altered');
END;

DROP TRIGGER trg_bill_status_transition;
CREATE TRIGGER trg_bill_status_transition
BEFORE UPDATE OF status ON bills
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id))
       AND NEW.bill_number IS OLD.bill_number
       AND NEW.bill_date IS OLD.bill_date
       AND NEW.due_date IS OLD.due_date
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.amount_paid_cents IS OLD.amount_paid_cents
       AND NEW.status IS OLD.status
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.terms_id IS OLD.terms_id)
  AND (NOT (
       (OLD.status = 'DRAFT' AND NEW.status IN ('DRAFT', 'UNPAID'))
    OR (OLD.status IN ('UNPAID', 'PARTIAL', 'PAID') AND NEW.status IN ('UNPAID', 'PARTIAL', 'PAID'))
))
BEGIN
    SELECT RAISE(ABORT, 'BILL_INVALID_TRANSITION: a posted bill cannot return to DRAFT');
END;

DROP TRIGGER trg_bill_void;
CREATE TRIGGER trg_bill_void
BEFORE UPDATE OF is_voided ON bills
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id))
       AND NEW.bill_number IS OLD.bill_number
       AND NEW.bill_date IS OLD.bill_date
       AND NEW.due_date IS OLD.due_date
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.amount_paid_cents IS OLD.amount_paid_cents
       AND NEW.status IS OLD.status
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.terms_id IS OLD.terms_id)
  AND (NEW.is_voided IS NOT OLD.is_voided)
BEGIN
    SELECT RAISE(ABORT, 'BILL_UNVOID: a voided bill cannot be restored') WHERE OLD.is_voided = 1;
    SELECT RAISE(ABORT, 'BILL_VOID_HAS_PAYMENTS: void the payments made on this bill first')
    WHERE OLD.amount_paid_cents <> 0;
    SELECT RAISE(ABORT, 'BILL_VOID_ENTRY_ACTIVE: the bill''s journal entry must be voided first')
    WHERE NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
END;

DROP TRIGGER trg_bill_amount_paid_matches;
CREATE TRIGGER trg_bill_amount_paid_matches
BEFORE UPDATE OF amount_paid_cents ON bills
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id))
       AND NEW.bill_number IS OLD.bill_number
       AND NEW.bill_date IS OLD.bill_date
       AND NEW.due_date IS OLD.due_date
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.amount_paid_cents IS OLD.amount_paid_cents
       AND NEW.status IS OLD.status
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.terms_id IS OLD.terms_id)
BEGIN
    SELECT RAISE(ABORT, 'BILL_PAYMENT_UNPOSTED: every payment must be posted before it is applied')
    WHERE EXISTS (SELECT 1 FROM bill_payments WHERE bill_id = NEW.id AND journal_entry_id IS NULL);
    SELECT RAISE(ABORT, 'BILL_PAID_MISMATCH: amount paid must equal the sum of recorded payments and applied credits')
    WHERE NEW.amount_paid_cents <>
          COALESCE((SELECT SUM(amount_cents) FROM bill_payments WHERE bill_id = NEW.id AND is_voided = 0), 0)
        + COALESCE((SELECT SUM(amount_cents) FROM vendor_credit_applications WHERE bill_id = NEW.id AND is_voided = 0), 0);
END;

DROP TRIGGER trg_exp_currency_immutable;
CREATE TRIGGER trg_exp_currency_immutable
BEFORE UPDATE OF currency_code, exchange_rate_e6, home_total_cents ON expenses
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.kind IS OLD.kind
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id))
       AND NEW.payment_account_id IS OLD.payment_account_id
       AND NEW.expense_date IS OLD.expense_date
       AND NEW.check_number IS OLD.check_number
       AND NEW.payment_method IS OLD.payment_method
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.created_at IS OLD.created_at
       AND NEW.currency_code IS OLD.currency_code
       AND NEW.exchange_rate_e6 IS OLD.exchange_rate_e6
       AND NEW.home_total_cents IS OLD.home_total_cents
       AND NEW.is_credit IS OLD.is_credit
       AND NEW.payment_method_id IS OLD.payment_method_id
       AND NEW.to_be_printed IS OLD.to_be_printed)
  AND (OLD.journal_entry_id IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'EXP_IMMUTABLE: posted expenses cannot be changed');
END;

DROP TRIGGER trg_exp_credit_immutable;
CREATE TRIGGER trg_exp_credit_immutable
BEFORE UPDATE OF is_credit ON expenses
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.kind IS OLD.kind
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id))
       AND NEW.payment_account_id IS OLD.payment_account_id
       AND NEW.expense_date IS OLD.expense_date
       AND NEW.check_number IS OLD.check_number
       AND NEW.payment_method IS OLD.payment_method
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.created_at IS OLD.created_at
       AND NEW.currency_code IS OLD.currency_code
       AND NEW.exchange_rate_e6 IS OLD.exchange_rate_e6
       AND NEW.home_total_cents IS OLD.home_total_cents
       AND NEW.is_credit IS OLD.is_credit
       AND NEW.payment_method_id IS OLD.payment_method_id
       AND NEW.to_be_printed IS OLD.to_be_printed)
  AND (NEW.is_credit IS NOT OLD.is_credit)
BEGIN
    SELECT RAISE(ABORT, 'EXP_IMMUTABLE: an expense cannot become a card credit, or a card credit an expense');
END;

DROP TRIGGER trg_exp_posting_direction;
CREATE TRIGGER trg_exp_posting_direction
BEFORE UPDATE OF journal_entry_id ON expenses
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.kind IS OLD.kind
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id))
       AND NEW.payment_account_id IS OLD.payment_account_id
       AND NEW.expense_date IS OLD.expense_date
       AND NEW.check_number IS OLD.check_number
       AND NEW.payment_method IS OLD.payment_method
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.created_at IS OLD.created_at
       AND NEW.currency_code IS OLD.currency_code
       AND NEW.exchange_rate_e6 IS OLD.exchange_rate_e6
       AND NEW.home_total_cents IS OLD.home_total_cents
       AND NEW.is_credit IS OLD.is_credit
       AND NEW.payment_method_id IS OLD.payment_method_id
       AND NEW.to_be_printed IS OLD.to_be_printed)
  AND (OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'EXP_WRONG_DIRECTION: a card credit must debit the card account and credit its split accounts')
    WHERE NEW.is_credit = 1
      AND (   EXISTS (SELECT 1 FROM journal_lines WHERE journal_entry_id = NEW.journal_entry_id
                      AND account_id = NEW.payment_account_id AND credit_cents > 0)
           OR NOT EXISTS (SELECT 1 FROM journal_lines WHERE journal_entry_id = NEW.journal_entry_id
                          AND account_id = NEW.payment_account_id AND debit_cents > 0));
    SELECT RAISE(ABORT, 'EXP_WRONG_DIRECTION: an expense must credit the account it was paid from')
    WHERE NEW.is_credit = 0
      AND (   EXISTS (SELECT 1 FROM journal_lines WHERE journal_entry_id = NEW.journal_entry_id
                      AND account_id = NEW.payment_account_id AND debit_cents > 0)
           OR NOT EXISTS (SELECT 1 FROM journal_lines WHERE journal_entry_id = NEW.journal_entry_id
                          AND account_id = NEW.payment_account_id AND credit_cents > 0));
END;

DROP TRIGGER trg_exp_print_queue_update;
CREATE TRIGGER trg_exp_print_queue_update
BEFORE UPDATE OF to_be_printed ON expenses
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.kind IS OLD.kind
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id))
       AND NEW.payment_account_id IS OLD.payment_account_id
       AND NEW.expense_date IS OLD.expense_date
       AND NEW.check_number IS OLD.check_number
       AND NEW.payment_method IS OLD.payment_method
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.created_at IS OLD.created_at
       AND NEW.currency_code IS OLD.currency_code
       AND NEW.exchange_rate_e6 IS OLD.exchange_rate_e6
       AND NEW.home_total_cents IS OLD.home_total_cents
       AND NEW.is_credit IS OLD.is_credit
       AND NEW.payment_method_id IS OLD.payment_method_id
       AND NEW.to_be_printed IS OLD.to_be_printed)
  AND (NEW.to_be_printed = 1 AND OLD.to_be_printed = 0)
BEGIN
    SELECT RAISE(ABORT, 'PRINT_NOT_A_CHECK: only checks can be printed later')
    WHERE NEW.kind <> 'CHECK';
    SELECT RAISE(ABORT, 'PRINT_HAS_NUMBER: this check already has a number; print it from its row instead')
    WHERE NEW.check_number IS NOT NULL;
    SELECT RAISE(ABORT, 'PRINT_VOIDED: a voided check cannot be printed')
    WHERE NEW.is_voided = 1;
END;

DROP TRIGGER trg_exp_update_rules;
CREATE TRIGGER trg_exp_update_rules
BEFORE UPDATE ON expenses
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.kind IS OLD.kind
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id))
       AND NEW.payment_account_id IS OLD.payment_account_id
       AND NEW.expense_date IS OLD.expense_date
       AND NEW.check_number IS OLD.check_number
       AND NEW.payment_method IS OLD.payment_method
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.created_at IS OLD.created_at
       AND NEW.currency_code IS OLD.currency_code
       AND NEW.exchange_rate_e6 IS OLD.exchange_rate_e6
       AND NEW.home_total_cents IS OLD.home_total_cents
       AND NEW.is_credit IS OLD.is_credit
       AND NEW.payment_method_id IS OLD.payment_method_id
       AND NEW.to_be_printed IS OLD.to_be_printed)
BEGIN
    SELECT RAISE(ABORT, 'EXP_IMMUTABLE: posted expenses cannot be changed')
    WHERE OLD.journal_entry_id IS NOT NULL
      AND (   NEW.kind IS NOT OLD.kind OR NEW.vendor_id IS NOT OLD.vendor_id
           OR NEW.payment_account_id IS NOT OLD.payment_account_id OR NEW.expense_date IS NOT OLD.expense_date
           OR (NEW.check_number IS NOT OLD.check_number
               AND NOT (OLD.check_number IS NULL AND OLD.to_be_printed = 1 AND NEW.check_number IS NOT NULL))
           OR NEW.payment_method IS NOT OLD.payment_method
           OR NEW.memo IS NOT OLD.memo OR NEW.total_cents IS NOT OLD.total_cents
           OR NEW.journal_entry_id IS NOT OLD.journal_entry_id OR NEW.created_at IS NOT OLD.created_at);
    SELECT RAISE(ABORT, 'EXP_TOTAL_MISMATCH: expense total must equal the sum of its split lines')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NEW.total_cents IS NOT (SELECT SUM(amount_cents) FROM expense_lines WHERE expense_id = NEW.id);
    SELECT RAISE(ABORT, 'EXP_JOURNAL_MISSING: expense must reference its POSTED journal entry')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NOT EXISTS (
          SELECT 1 FROM journal_entries
          WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = NEW.kind AND source_id = NEW.id
      );
    SELECT RAISE(ABORT, 'EXP_UNVOID: a voided expense cannot be restored')
    WHERE OLD.is_voided = 1 AND NEW.is_voided = 0;
    SELECT RAISE(ABORT, 'EXP_VOID_ENTRY_ACTIVE: the expense''s journal entry must be voided first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
END;

DROP TRIGGER trg_vendor_credit_immutable;
CREATE TRIGGER trg_vendor_credit_immutable
BEFORE UPDATE ON vendor_credits
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id))
       AND NEW.reference IS OLD.reference
       AND NEW.credit_date IS OLD.credit_date
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.created_at IS OLD.created_at)
  AND (OLD.journal_entry_id IS NOT NULL
 AND (   NEW.vendor_id IS NOT OLD.vendor_id OR NEW.credit_date IS NOT OLD.credit_date
      OR NEW.total_cents IS NOT OLD.total_cents OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
      OR (OLD.is_voided = 1 AND NEW.is_voided = 0)))
BEGIN
    SELECT RAISE(ABORT, 'VC_IMMUTABLE: posted vendor credits cannot be altered; void instead');
END;

DROP TRIGGER trg_ach_entries_immutable;
CREATE TRIGGER trg_ach_entries_immutable BEFORE UPDATE ON ach_entries
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.ach_file_id IS OLD.ach_file_id
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id))
       AND NEW.sec_code IS OLD.sec_code
       AND NEW.account_type IS OLD.account_type
       AND NEW.routing_number IS OLD.routing_number
       AND NEW.account_number IS OLD.account_number
       AND NEW.account_masked IS OLD.account_masked
       AND NEW.receiver_name IS OLD.receiver_name
       AND NEW.amount_cents IS OLD.amount_cents
       AND NEW.trace_number IS OLD.trace_number
       AND NEW.addenda IS OLD.addenda)
  AND (NEW.id IS NOT OLD.id OR NEW.ach_file_id IS NOT OLD.ach_file_id OR NEW.vendor_id IS NOT OLD.vendor_id
  OR NEW.sec_code IS NOT OLD.sec_code OR NEW.account_type IS NOT OLD.account_type OR NEW.account_masked IS NOT OLD.account_masked
  OR NEW.receiver_name IS NOT OLD.receiver_name OR NEW.amount_cents IS NOT OLD.amount_cents
  OR NEW.trace_number IS NOT OLD.trace_number OR NEW.addenda IS NOT OLD.addenda)
BEGIN
    SELECT RAISE(ABORT, 'ACH_FILE_IMMUTABLE: an ACH file cannot be changed; void its payments instead');
END;

DROP TRIGGER trg_inv_items_update_draft_only;
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
       AND NEW.percent_ppm IS OLD.percent_ppm)
  AND ((SELECT status FROM invoices WHERE id = OLD.invoice_id) <> 'DRAFT'
  OR (SELECT status FROM invoices WHERE id = NEW.invoice_id) <> 'DRAFT')
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: items can only be changed on a DRAFT invoice');
END;

DROP TRIGGER trg_sr_lines_update_unposted;
CREATE TRIGGER trg_sr_lines_update_unposted
BEFORE UPDATE ON sales_receipt_lines
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.sales_receipt_id IS OLD.sales_receipt_id
       AND NEW.line_number IS OLD.line_number
       AND (NEW.product_service_id IS OLD.product_service_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'ITEM' AND m.from_id = OLD.product_service_id AND m.to_id = NEW.product_service_id))
       AND NEW.description IS OLD.description
       AND NEW.quantity_e4 IS OLD.quantity_e4
       AND NEW.rate_cents IS OLD.rate_cents
       AND NEW.amount_cents IS OLD.amount_cents
       AND NEW.income_account_id IS OLD.income_account_id
       AND NEW.class_id IS OLD.class_id
       AND NEW.is_taxable IS OLD.is_taxable
       AND NEW.percent_ppm IS OLD.percent_ppm)
  AND ((SELECT journal_entry_id FROM sales_receipts WHERE id = OLD.sales_receipt_id) IS NOT NULL
  OR (SELECT journal_entry_id FROM sales_receipts WHERE id = NEW.sales_receipt_id) IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'SR_IMMUTABLE: lines of a posted sales receipt cannot be changed');
END;

DROP TRIGGER trg_est_lines_frozen_upd;
CREATE TRIGGER trg_est_lines_frozen_upd
BEFORE UPDATE ON estimate_lines
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.estimate_id IS OLD.estimate_id
       AND NEW.line_number IS OLD.line_number
       AND (NEW.product_service_id IS OLD.product_service_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'ITEM' AND m.from_id = OLD.product_service_id AND m.to_id = NEW.product_service_id))
       AND NEW.description IS OLD.description
       AND NEW.quantity_e4 IS OLD.quantity_e4
       AND NEW.rate_cents IS OLD.rate_cents
       AND NEW.amount_cents IS OLD.amount_cents
       AND (NEW.income_account_id IS OLD.income_account_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'ACCOUNT' AND m.from_id = OLD.income_account_id AND m.to_id = NEW.income_account_id))
       AND NEW.class_id IS OLD.class_id
       AND NEW.is_taxable IS OLD.is_taxable
       AND NEW.percent_ppm IS OLD.percent_ppm)
BEGIN
    SELECT RAISE(ABORT, 'EST_IMMUTABLE: estimate lines cannot be changed after saving');
END;

DROP TRIGGER trg_credit_memo_lines_immutable_upd;
CREATE TRIGGER trg_credit_memo_lines_immutable_upd
BEFORE UPDATE ON credit_memo_lines
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.credit_memo_id IS OLD.credit_memo_id
       AND NEW.line_number IS OLD.line_number
       AND (NEW.product_service_id IS OLD.product_service_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'ITEM' AND m.from_id = OLD.product_service_id AND m.to_id = NEW.product_service_id))
       AND NEW.description IS OLD.description
       AND NEW.quantity_e4 IS OLD.quantity_e4
       AND NEW.rate_cents IS OLD.rate_cents
       AND NEW.amount_cents IS OLD.amount_cents
       AND NEW.income_account_id IS OLD.income_account_id
       AND NEW.class_id IS OLD.class_id
       AND NEW.is_taxable IS OLD.is_taxable)
  AND ((SELECT journal_entry_id FROM credit_memos WHERE id = OLD.credit_memo_id) IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'CM_IMMUTABLE: lines of a posted credit memo cannot change');
END;

DROP TRIGGER trg_inv_form_lines_update_draft_only;
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

DROP TRIGGER trg_sr_form_lines_update_unposted;
CREATE TRIGGER trg_sr_form_lines_update_unposted
BEFORE UPDATE ON sales_receipt_form_lines
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.sales_receipt_id IS OLD.sales_receipt_id
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
  AND ((SELECT journal_entry_id FROM sales_receipts WHERE id = OLD.sales_receipt_id) IS NOT NULL
  OR (SELECT journal_entry_id FROM sales_receipts WHERE id = NEW.sales_receipt_id) IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'SR_IMMUTABLE: lines of a posted sales receipt cannot be changed');
END;

DROP TRIGGER trg_est_form_lines_frozen_upd;
CREATE TRIGGER trg_est_form_lines_frozen_upd
BEFORE UPDATE ON estimate_form_lines
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.estimate_id IS OLD.estimate_id
       AND NEW.line_number IS OLD.line_number
       AND NEW.kind IS OLD.kind
       AND NEW.description IS OLD.description
       AND NEW.amount_cents IS OLD.amount_cents
       AND (NEW.product_service_id IS OLD.product_service_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'ITEM' AND m.from_id = OLD.product_service_id AND m.to_id = NEW.product_service_id))
       AND NEW.percent_ppm IS OLD.percent_ppm
       AND (NEW.account_id IS OLD.account_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'ACCOUNT' AND m.from_id = OLD.account_id AND m.to_id = NEW.account_id))
       AND NEW.class_id IS OLD.class_id
       AND NEW.is_taxable IS OLD.is_taxable
       AND NEW.item_group_id IS OLD.item_group_id
       AND NEW.group_quantity_e4 IS OLD.group_quantity_e4
       AND NEW.group_line_count IS OLD.group_line_count
       AND NEW.print_items IS OLD.print_items)
BEGIN
    SELECT RAISE(ABORT, 'EST_IMMUTABLE: estimate lines cannot be changed after saving');
END;

