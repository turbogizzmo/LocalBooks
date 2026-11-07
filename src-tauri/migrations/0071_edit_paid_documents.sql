-- Editing an invoice or bill that has payments or credits applied (docs/plans/edit-paid-documents.md).
--
-- 1. Payments, bill payments, applied credits and bounced checks may be re-pointed at the document
--    that replaces theirs, and only then: the row is live, `transaction_edits` links the old
--    document's journal entry to the new one's, and the new document is live with the same customer
--    or vendor (and, for invoices, the same currency and exchange rate). Every other column stays
--    immutable, as before. No journal row changes: those rows' entries never name the document.
--
-- 2. The ledger audit trail gains the EDIT action type, one record per replacement listing what was
--    re-linked. SQLite cannot change a CHECK constraint in place, so the table is rebuilt exactly as
--    in migrations 0045, 0054, 0059 and 0063: every row is copied as it is (ids, timestamps,
--    payloads, actors and chain hashes, so the chain still verifies), and the triggers that reference
--    the table are dropped first and recreated unchanged.

DROP TRIGGER trg_ledger_audit_account;
DROP TRIGGER trg_ledger_audit_insert_unsealed;
DROP TRIGGER trg_ledger_audit_no_delete;
DROP TRIGGER trg_ledger_audit_no_update;
DROP TRIGGER trg_ledger_audit_post;
DROP TRIGGER trg_ledger_audit_void;
DROP TRIGGER trg_ledger_audit_customer;
DROP TRIGGER trg_ledger_audit_vendor;
DROP TRIGGER trg_ledger_audit_product;

CREATE TABLE ledger_audit_log_new (
    id                  INTEGER PRIMARY KEY,
    timestamp           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    action_type         TEXT    NOT NULL CHECK (action_type IN ('POST', 'VOID', 'MIGRATION', 'ACCOUNT_CHANGE', 'FORENSIC_HEAL', 'USER_CHANGE', 'PRICE_CHANGE', 'ACH_FILE', 'LIST_CHANGE', 'CLEANUP', 'CONDENSE', 'EDIT')),
    target_entity_id    INTEGER NOT NULL,
    payload_snapshot    TEXT    NOT NULL CHECK (json_valid(payload_snapshot)),
    actor               TEXT,
    prev_hash           TEXT    CHECK (prev_hash IS NULL OR (length(prev_hash) = 64 AND prev_hash NOT GLOB '*[^0-9a-f]*')),
    row_hash            TEXT    CHECK (row_hash IS NULL OR (length(row_hash) = 64 AND row_hash NOT GLOB '*[^0-9a-f]*'))
) STRICT;

INSERT INTO ledger_audit_log_new (id, timestamp, action_type, target_entity_id, payload_snapshot, actor, prev_hash, row_hash)
SELECT id, timestamp, action_type, target_entity_id, payload_snapshot, actor, prev_hash, row_hash FROM ledger_audit_log ORDER BY id;

DROP TABLE ledger_audit_log;
ALTER TABLE ledger_audit_log_new RENAME TO ledger_audit_log;

CREATE INDEX ix_ledger_audit_target ON ledger_audit_log (action_type, target_entity_id);
CREATE INDEX ix_ledger_audit_unsealed ON ledger_audit_log (id) WHERE row_hash IS NULL;

CREATE TRIGGER trg_ledger_audit_account
AFTER UPDATE ON chart_of_accounts
WHEN NEW.name IS NOT OLD.name
  OR NEW.account_number IS NOT OLD.account_number
  OR NEW.account_type IS NOT OLD.account_type
  OR NEW.parent_id IS NOT OLD.parent_id
  OR NEW.is_active IS NOT OLD.is_active
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('ACCOUNT_CHANGE', NEW.id, json_object(
        'before', json_object('accountNumber', OLD.account_number, 'name', OLD.name, 'accountType', OLD.account_type,
                              'parentId', OLD.parent_id, 'isActive', OLD.is_active),
        'after',  json_object('accountNumber', NEW.account_number, 'name', NEW.name, 'accountType', NEW.account_type,
                              'parentId', NEW.parent_id, 'isActive', NEW.is_active)
    ));
END;

CREATE TRIGGER trg_ledger_audit_insert_unsealed BEFORE INSERT ON ledger_audit_log
WHEN NEW.row_hash IS NOT NULL OR NEW.prev_hash IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: audit rows are sealed by the application after they are written');
END;

CREATE TRIGGER trg_ledger_audit_no_delete BEFORE DELETE ON ledger_audit_log
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the ledger audit trail cannot be changed');
END;

CREATE TRIGGER trg_ledger_audit_no_update BEFORE UPDATE ON ledger_audit_log
WHEN NOT (    OLD.row_hash IS NULL
          AND NEW.row_hash IS NOT NULL
          AND NEW.prev_hash IS NOT NULL
          AND NEW.actor IS NOT NULL
          AND (OLD.actor IS NULL OR NEW.actor IS OLD.actor)
          AND NEW.id IS OLD.id
          AND NEW.timestamp IS OLD.timestamp
          AND NEW.action_type IS OLD.action_type
          AND NEW.target_entity_id IS OLD.target_entity_id
          AND NEW.payload_snapshot IS OLD.payload_snapshot)
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the ledger audit trail cannot be changed');
END;

CREATE TRIGGER trg_ledger_audit_post
AFTER UPDATE OF status ON journal_entries
WHEN OLD.status = 'DRAFT' AND NEW.status = 'POSTED'
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    SELECT 'POST', NEW.id, snapshot FROM v_entry_snapshot WHERE entry_id = NEW.id;
END;

CREATE TRIGGER trg_ledger_audit_void
AFTER UPDATE OF is_voided ON journal_entries
WHEN OLD.is_voided = 0 AND NEW.is_voided = 1
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    SELECT 'VOID', NEW.id, json_object(
        'entryId',             NEW.id,
        'entryNumber',         NEW.entry_number,
        'reversalEntryId',     r.id,
        'reversalEntryNumber', r.entry_number,
        'reversalEntryDate',   r.entry_date,
        'amountCents',         (SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = NEW.id)
    )
    FROM journal_entries r WHERE r.reverses_entry_id = NEW.id;
END;

CREATE TRIGGER trg_ledger_audit_customer
AFTER UPDATE ON customers
WHEN NEW.name IS NOT OLD.name OR NEW.is_active IS NOT OLD.is_active OR NEW.email IS NOT OLD.email
  OR NEW.company_name IS NOT OLD.company_name OR NEW.phone IS NOT OLD.phone OR NEW.billing_address IS NOT OLD.billing_address
  OR NEW.terms_days IS NOT OLD.terms_days OR NEW.notes IS NOT OLD.notes OR NEW.parent_id IS NOT OLD.parent_id
  OR NEW.tax_rate_id IS NOT OLD.tax_rate_id OR NEW.is_tax_exempt IS NOT OLD.is_tax_exempt
  OR NEW.sales_rep_id IS NOT OLD.sales_rep_id
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'CUSTOMER',
        'before', json_object('name', OLD.name, 'isActive', OLD.is_active),
        'after',  json_object('name', NEW.name, 'isActive', NEW.is_active),
        'changed', (SELECT json_group_array(f) FROM (
            SELECT 'email' AS f WHERE NEW.email IS NOT OLD.email
            UNION ALL SELECT 'company name' WHERE NEW.company_name IS NOT OLD.company_name
            UNION ALL SELECT 'phone' WHERE NEW.phone IS NOT OLD.phone
            UNION ALL SELECT 'billing address' WHERE NEW.billing_address IS NOT OLD.billing_address
            UNION ALL SELECT 'terms' WHERE NEW.terms_days IS NOT OLD.terms_days
            UNION ALL SELECT 'notes' WHERE NEW.notes IS NOT OLD.notes
            UNION ALL SELECT 'parent customer' WHERE NEW.parent_id IS NOT OLD.parent_id
            UNION ALL SELECT 'sales tax' WHERE NEW.tax_rate_id IS NOT OLD.tax_rate_id OR NEW.is_tax_exempt IS NOT OLD.is_tax_exempt
            UNION ALL SELECT 'sales rep' WHERE NEW.sales_rep_id IS NOT OLD.sales_rep_id))
    ));
END;

CREATE TRIGGER trg_ledger_audit_vendor
AFTER UPDATE ON vendors
WHEN NEW.name IS NOT OLD.name OR NEW.is_active IS NOT OLD.is_active OR NEW.email IS NOT OLD.email
  OR NEW.company_name IS NOT OLD.company_name OR NEW.phone IS NOT OLD.phone OR NEW.address IS NOT OLD.address
  OR NEW.terms_days IS NOT OLD.terms_days OR NEW.notes IS NOT OLD.notes OR NEW.is_1099 IS NOT OLD.is_1099
  OR NEW.tax_id_masked IS NOT OLD.tax_id_masked
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'VENDOR',
        'before', json_object('name', OLD.name, 'isActive', OLD.is_active),
        'after',  json_object('name', NEW.name, 'isActive', NEW.is_active),
        'changed', (SELECT json_group_array(f) FROM (
            SELECT 'email' AS f WHERE NEW.email IS NOT OLD.email
            UNION ALL SELECT 'company name' WHERE NEW.company_name IS NOT OLD.company_name
            UNION ALL SELECT 'phone' WHERE NEW.phone IS NOT OLD.phone
            UNION ALL SELECT 'address' WHERE NEW.address IS NOT OLD.address
            UNION ALL SELECT 'terms' WHERE NEW.terms_days IS NOT OLD.terms_days
            UNION ALL SELECT 'notes' WHERE NEW.notes IS NOT OLD.notes
            UNION ALL SELECT '1099' WHERE NEW.is_1099 IS NOT OLD.is_1099
            UNION ALL SELECT 'tax ID' WHERE NEW.tax_id_masked IS NOT OLD.tax_id_masked))
    ));
END;

CREATE TRIGGER trg_ledger_audit_product
AFTER UPDATE ON products_services
WHEN NEW.name IS NOT OLD.name OR NEW.is_active IS NOT OLD.is_active OR NEW.description IS NOT OLD.description
  OR NEW.rate_cents IS NOT OLD.rate_cents OR NEW.income_account_id IS NOT OLD.income_account_id
  OR NEW.is_taxable IS NOT OLD.is_taxable
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'ITEM',
        'before', json_object('name', OLD.name, 'isActive', OLD.is_active, 'rateCents', OLD.rate_cents),
        'after',  json_object('name', NEW.name, 'isActive', NEW.is_active, 'rateCents', NEW.rate_cents),
        'changed', (SELECT json_group_array(f) FROM (
            SELECT 'description' AS f WHERE NEW.description IS NOT OLD.description
            UNION ALL SELECT 'price' WHERE NEW.rate_cents IS NOT OLD.rate_cents
            UNION ALL SELECT 'income account' WHERE NEW.income_account_id IS NOT OLD.income_account_id
            UNION ALL SELECT 'taxable' WHERE NEW.is_taxable IS NOT OLD.is_taxable))
    ));
END;

-- ---------------------------------------------------------------------------------------------
-- Re-linking applied amounts to a replacement document
-- ---------------------------------------------------------------------------------------------
-- An invoice that replaces another in a recorded edit, for the same customer and currency.
CREATE VIEW v_invoice_replacements AS
SELECT o.id AS old_invoice_id, n.id AS new_invoice_id
FROM transaction_edits te
JOIN invoices o ON o.journal_entry_id = te.old_entry_id
JOIN invoices n ON n.journal_entry_id = te.new_entry_id
WHERE n.is_voided = 0
  AND n.customer_id IS o.customer_id
  AND n.currency_code IS o.currency_code
  AND n.exchange_rate_e6 IS o.exchange_rate_e6;

CREATE VIEW v_bill_replacements AS
SELECT o.id AS old_bill_id, n.id AS new_bill_id
FROM transaction_edits te
JOIN bills o ON o.journal_entry_id = te.old_entry_id
JOIN bills n ON n.journal_entry_id = te.new_entry_id
WHERE n.is_voided = 0 AND n.vendor_id IS o.vendor_id;

DROP TRIGGER trg_pay_update_rules;
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

DROP TRIGGER trg_billpay_update_rules;
CREATE TRIGGER trg_billpay_update_rules
BEFORE UPDATE ON bill_payments
BEGIN
    SELECT RAISE(ABORT, 'BILL_PAYMENT_IMMUTABLE: recorded payments cannot be changed')
    WHERE (NEW.bill_id IS NOT OLD.bill_id AND NOT (
               OLD.is_voided = 0 AND NEW.is_voided = 0
               AND EXISTS (SELECT 1 FROM v_bill_replacements
                           WHERE old_bill_id = OLD.bill_id AND new_bill_id = NEW.bill_id)))
       OR NEW.payment_date IS NOT OLD.payment_date
       OR NEW.amount_cents IS NOT OLD.amount_cents
       OR NEW.payment_account_id IS NOT OLD.payment_account_id
       OR NEW.reference IS NOT OLD.reference
       OR NEW.created_at IS NOT OLD.created_at
       OR (OLD.journal_entry_id IS NOT NULL AND NEW.journal_entry_id IS NOT OLD.journal_entry_id)
       OR NEW.journal_entry_id IS NULL;
    SELECT RAISE(ABORT, 'BILL_PAYMENT_ENTRY_MISMATCH: payment must reference its POSTED journal entry')
    WHERE OLD.journal_entry_id IS NULL AND NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = 'BILL_PAYMENT' AND source_id = NEW.id
    );
    SELECT RAISE(ABORT, 'BILL_PAYMENT_UNVOID: a voided payment cannot be restored')
    WHERE OLD.is_voided = 1 AND NEW.is_voided = 0;
    SELECT RAISE(ABORT, 'BILL_PAYMENT_VOID_ENTRY_ACTIVE: the payment''s journal entry must be voided first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
END;

-- An application only ever goes from live to removed, or follows its document to a replacement.
DROP TRIGGER trg_credit_applications_once;
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

DROP TRIGGER trg_vendor_credit_applications_once;
CREATE TRIGGER trg_vendor_credit_applications_once
BEFORE UPDATE ON vendor_credit_applications
WHEN NEW.is_voided < OLD.is_voided OR NEW.amount_cents IS NOT OLD.amount_cents
  OR NEW.vendor_credit_id IS NOT OLD.vendor_credit_id
  OR (NEW.bill_id IS NOT OLD.bill_id AND NOT (
          OLD.is_voided = 0 AND NEW.is_voided = 0
          AND EXISTS (SELECT 1 FROM v_bill_replacements
                      WHERE old_bill_id = OLD.bill_id AND new_bill_id = NEW.bill_id)))
BEGIN
    SELECT RAISE(ABORT, 'VC_APPLICATION_IMMUTABLE: a credit application can only be removed');
END;

DROP TRIGGER trg_bounce_update_rules;
CREATE TRIGGER trg_bounce_update_rules
BEFORE UPDATE ON bounced_checks
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
