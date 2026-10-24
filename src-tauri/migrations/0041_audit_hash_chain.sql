-- Tamper-evident ledger audit trail (issue #46).
--
-- Each ledger_audit_log row is sealed into a SHA-256 hash chain by `audit_chain::seal` as part of the
-- transaction that wrote it: row_hash covers the row's fields, its actor and the previous row's hash.
-- The newest sealed row is then recorded outside the company file (the operating system's credential
-- store), keyed by `ledger_chain.file_id`. Rewriting, removing or reordering sealed rows breaks the
-- chain or no longer matches that record, even if every protecting trigger was dropped first.
--
-- Rows can still never be deleted or changed, except that an unsealed row may be sealed once.

CREATE TABLE ledger_chain (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    file_id     TEXT    NOT NULL CHECK (length(file_id) = 32)
) STRICT;

INSERT INTO ledger_chain (id, file_id) VALUES (1, lower(hex(randomblob(16))));

CREATE TRIGGER trg_ledger_chain_no_update BEFORE UPDATE ON ledger_chain
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the audit chain identity cannot be changed');
END;

CREATE TRIGGER trg_ledger_chain_no_delete BEFORE DELETE ON ledger_chain
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the audit chain identity cannot be changed');
END;

ALTER TABLE ledger_audit_log ADD COLUMN actor TEXT;
ALTER TABLE ledger_audit_log ADD COLUMN prev_hash TEXT CHECK (prev_hash IS NULL OR (length(prev_hash) = 64 AND prev_hash NOT GLOB '*[^0-9a-f]*'));
ALTER TABLE ledger_audit_log ADD COLUMN row_hash TEXT CHECK (row_hash IS NULL OR (length(row_hash) = 64 AND row_hash NOT GLOB '*[^0-9a-f]*'));

-- Rows written before the chain existed were not attributed to anyone.
DROP TRIGGER trg_ledger_audit_no_update;
UPDATE ledger_audit_log SET actor = 'unrecorded (before audit chain)';

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

-- New rows always start unsealed: the chain is computed by the application, never supplied.
CREATE TRIGGER trg_ledger_audit_insert_unsealed BEFORE INSERT ON ledger_audit_log
WHEN NEW.row_hash IS NOT NULL OR NEW.prev_hash IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: audit rows are sealed by the application after they are written');
END;
