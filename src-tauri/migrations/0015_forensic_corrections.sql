-- Reviewed forensic corrections: adds the FORENSIC_HEAL event to the ledger audit trail.
--
-- SQLite cannot change a CHECK constraint in place, so the table is rebuilt. Every row is copied
-- exactly (same ids, timestamps and payloads). Triggers that reference the table are dropped first
-- (the bundled SQLite refuses the rename otherwise) and recreated unchanged afterwards.

DROP TRIGGER trg_ledger_audit_post;
DROP TRIGGER trg_ledger_audit_void;
DROP TRIGGER trg_ledger_audit_account;
DROP TRIGGER trg_ledger_audit_no_update;
DROP TRIGGER trg_ledger_audit_no_delete;

CREATE TABLE ledger_audit_log_new (
    id                  INTEGER PRIMARY KEY,
    timestamp           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    action_type         TEXT    NOT NULL CHECK (action_type IN ('POST', 'VOID', 'MIGRATION', 'ACCOUNT_CHANGE', 'FORENSIC_HEAL')),
    target_entity_id    INTEGER NOT NULL,
    payload_snapshot    TEXT    NOT NULL CHECK (json_valid(payload_snapshot))
) STRICT;

INSERT INTO ledger_audit_log_new (id, timestamp, action_type, target_entity_id, payload_snapshot)
SELECT id, timestamp, action_type, target_entity_id, payload_snapshot FROM ledger_audit_log ORDER BY id;

DROP TABLE ledger_audit_log;
ALTER TABLE ledger_audit_log_new RENAME TO ledger_audit_log;
CREATE INDEX ix_ledger_audit_target ON ledger_audit_log (action_type, target_entity_id);

CREATE TRIGGER trg_ledger_audit_no_update BEFORE UPDATE ON ledger_audit_log
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the ledger audit trail cannot be changed');
END;

CREATE TRIGGER trg_ledger_audit_no_delete BEFORE DELETE ON ledger_audit_log
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
