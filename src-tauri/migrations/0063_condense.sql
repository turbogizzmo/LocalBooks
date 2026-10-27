-- Condense Data (`condense.rs`): a condensed company file records one CONDENSE event in the ledger
-- audit trail. It names the archive the file was made from, the entries that were summarized, and
-- the summary entry that replaced each month, so `ledger::verify_ledger_audit` can check that every
-- summarized entry's posting snapshot (kept in the trail) adds up to its summary.
--
-- The trail's CHECK constraint gains the new action type. SQLite cannot change a CHECK constraint in
-- place, so the table is rebuilt exactly as in migrations 0045, 0054 and 0059: every row is copied as it is
-- (ids, timestamps, payloads, actors and chain hashes, so the chain still verifies), and the
-- triggers that reference the table are dropped first and recreated unchanged.

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
    action_type         TEXT    NOT NULL CHECK (action_type IN ('POST', 'VOID', 'MIGRATION', 'ACCOUNT_CHANGE', 'FORENSIC_HEAL', 'USER_CHANGE', 'PRICE_CHANGE', 'ACH_FILE', 'LIST_CHANGE', 'CLEANUP', 'CONDENSE')),
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
