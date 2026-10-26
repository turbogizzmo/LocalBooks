-- Pay bills by ACH file (see `ach.rs` and `nacha.rs`).
--
-- Vendors may have bank details: a routing number, an account number and whether it is checking or
-- savings, and whether ACH pays them as a business (CCD) or a person (PPD). Like tax IDs (migration
-- 0044), the routing and account numbers are stored encrypted with the file's data key once users
-- are on, each next to a masked copy (`*****6789`), and are only shown or changed by users who may
-- see sensitive data. A bank account used to pay by ACH has its originator details: what the bank
-- calls the immediate destination and origin, and the company name and ID it shows to vendors.
--
-- Paying by ACH file records the bill payments as usual, marked kind ACH, and an `ach_files` row with
-- one `ach_entries` row per vendor: the credit the file sends, its trace number, and a copy of the
-- vendor's bank details as they were (encrypted like the vendor's). The file is written from these
-- rows, so saving it again gives the same file and never records another payment. Each file written
-- is an ACH_FILE event in the ledger audit trail, without any account number.

CREATE TABLE vendor_bank_accounts (
    vendor_id           INTEGER PRIMARY KEY REFERENCES vendors (id) ON DELETE RESTRICT,
    -- Plain in a file without users; encrypted ('lb1:…') once users are on.
    routing_number      TEXT    NOT NULL,
    routing_masked      TEXT    NOT NULL,
    account_number      TEXT    NOT NULL,
    account_masked      TEXT    NOT NULL,
    account_type        TEXT    NOT NULL CHECK (account_type IN ('CHECKING', 'SAVINGS')),
    sec_code            TEXT    NOT NULL DEFAULT 'CCD' CHECK (sec_code IN ('CCD', 'PPD')),
    updated_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE ach_originators (
    account_id                  INTEGER PRIMARY KEY REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    immediate_destination       TEXT    NOT NULL CHECK (length(immediate_destination) = 9 AND immediate_destination NOT GLOB '*[^0-9]*'),
    immediate_destination_name  TEXT    NOT NULL,
    immediate_origin            TEXT    NOT NULL CHECK (length(immediate_origin) IN (9, 10)),
    immediate_origin_name       TEXT    NOT NULL,
    company_name                TEXT    NOT NULL CHECK (length(company_name) BETWEEN 1 AND 16),
    company_id                  TEXT    NOT NULL CHECK (length(company_id) BETWEEN 1 AND 10),
    entry_description           TEXT    NOT NULL DEFAULT 'VENDOR PAY' CHECK (length(entry_description) BETWEEN 1 AND 10),
    updated_at                  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE ach_files (
    id                          INTEGER PRIMARY KEY,
    -- Chosen by the Pay Bills screen for one click of Pay: sending it again returns this file
    -- instead of paying the bills a second time.
    request_id                  TEXT    NOT NULL UNIQUE CHECK (length(request_id) BETWEEN 8 AND 64),
    payment_account_id          INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    payment_date                TEXT    NOT NULL CHECK (date(payment_date) = payment_date),
    effective_date              TEXT    NOT NULL CHECK (date(effective_date) = effective_date),
    -- The file header's creation date and time (local) and modifier, fixed so the file can be rewritten exactly.
    created_on                  TEXT    NOT NULL CHECK (date(created_on) = created_on),
    created_time                TEXT    NOT NULL CHECK (length(created_time) = 4 AND created_time NOT GLOB '*[^0-9]*'),
    file_id_modifier            TEXT    NOT NULL CHECK (length(file_id_modifier) = 1 AND file_id_modifier GLOB '[A-Z0-9]'),
    -- The originator details as they were when the file was made.
    immediate_destination       TEXT    NOT NULL,
    immediate_destination_name  TEXT    NOT NULL,
    immediate_origin            TEXT    NOT NULL,
    immediate_origin_name       TEXT    NOT NULL,
    company_name                TEXT    NOT NULL,
    company_id                  TEXT    NOT NULL,
    entry_description           TEXT    NOT NULL,
    total_cents                 INTEGER NOT NULL CHECK (total_cents > 0),
    created_at                  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (immediate_origin, created_on, file_id_modifier)
) STRICT;

CREATE TABLE ach_entries (
    id                  INTEGER PRIMARY KEY,
    ach_file_id         INTEGER NOT NULL REFERENCES ach_files (id) ON DELETE RESTRICT,
    vendor_id           INTEGER NOT NULL REFERENCES vendors (id) ON DELETE RESTRICT,
    sec_code            TEXT    NOT NULL CHECK (sec_code IN ('CCD', 'PPD')),
    account_type        TEXT    NOT NULL CHECK (account_type IN ('CHECKING', 'SAVINGS')),
    -- Copies of the vendor's bank details, plain or encrypted as theirs are.
    routing_number      TEXT    NOT NULL,
    account_number      TEXT    NOT NULL,
    account_masked      TEXT    NOT NULL,
    receiver_name       TEXT    NOT NULL,
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 9999999999),
    trace_number        TEXT    NOT NULL UNIQUE CHECK (length(trace_number) = 15 AND trace_number NOT GLOB '*[^0-9]*'),
    addenda             TEXT,
    UNIQUE (ach_file_id, vendor_id)
) STRICT;
CREATE INDEX ix_ach_entries_vendor ON ach_entries (vendor_id);

-- Which bill payments each entry pays: every one belongs to exactly one entry.
CREATE TABLE ach_entry_payments (
    bill_payment_id     INTEGER PRIMARY KEY REFERENCES bill_payments (id) ON DELETE RESTRICT,
    ach_entry_id        INTEGER NOT NULL REFERENCES ach_entries (id) ON DELETE RESTRICT
) STRICT;
CREATE INDEX ix_ach_entry_payments_entry ON ach_entry_payments (ach_entry_id);

CREATE TRIGGER trg_ach_files_immutable BEFORE UPDATE ON ach_files
BEGIN
    SELECT RAISE(ABORT, 'ACH_FILE_IMMUTABLE: an ACH file cannot be changed; void its payments instead');
END;
CREATE TRIGGER trg_ach_files_no_delete BEFORE DELETE ON ach_files
BEGIN
    SELECT RAISE(ABORT, 'ACH_FILE_IMMUTABLE: an ACH file cannot be changed; void its payments instead');
END;
-- Only the stored bank details may change, when they are encrypted (users turned on).
CREATE TRIGGER trg_ach_entries_immutable BEFORE UPDATE ON ach_entries
WHEN NEW.id IS NOT OLD.id OR NEW.ach_file_id IS NOT OLD.ach_file_id OR NEW.vendor_id IS NOT OLD.vendor_id
  OR NEW.sec_code IS NOT OLD.sec_code OR NEW.account_type IS NOT OLD.account_type OR NEW.account_masked IS NOT OLD.account_masked
  OR NEW.receiver_name IS NOT OLD.receiver_name OR NEW.amount_cents IS NOT OLD.amount_cents
  OR NEW.trace_number IS NOT OLD.trace_number OR NEW.addenda IS NOT OLD.addenda
BEGIN
    SELECT RAISE(ABORT, 'ACH_FILE_IMMUTABLE: an ACH file cannot be changed; void its payments instead');
END;
CREATE TRIGGER trg_ach_entries_no_delete BEFORE DELETE ON ach_entries
BEGIN
    SELECT RAISE(ABORT, 'ACH_FILE_IMMUTABLE: an ACH file cannot be changed; void its payments instead');
END;
CREATE TRIGGER trg_ach_entry_payments_immutable BEFORE UPDATE ON ach_entry_payments
BEGIN
    SELECT RAISE(ABORT, 'ACH_FILE_IMMUTABLE: an ACH file cannot be changed; void its payments instead');
END;
CREATE TRIGGER trg_ach_entry_payments_no_delete BEFORE DELETE ON ach_entry_payments
BEGIN
    SELECT RAISE(ABORT, 'ACH_FILE_IMMUTABLE: an ACH file cannot be changed; void its payments instead');
END;

-- How a bill payment was made: STANDARD (check, transfer, card: as before) or by ACH file.
ALTER TABLE bill_payments ADD COLUMN kind TEXT NOT NULL DEFAULT 'STANDARD' CHECK (kind IN ('STANDARD', 'ACH'));

CREATE TRIGGER trg_billpay_kind_immutable BEFORE UPDATE OF kind ON bill_payments
WHEN NEW.kind IS NOT OLD.kind
BEGIN
    SELECT RAISE(ABORT, 'BILL_PAYMENT_IMMUTABLE: recorded payments cannot be changed');
END;

-- ACH_FILE events: the ledger audit trail's CHECK constraint gains the new action type. SQLite cannot
-- change a CHECK constraint in place, so the table is rebuilt exactly as in migration 0045: every row
-- is copied as it is (ids, timestamps, payloads, actors and chain hashes, so the chain still
-- verifies), and the triggers that reference the table are dropped first and recreated unchanged.
-- PRICE_CHANGE (migration 0049) stays allowed.


DROP TRIGGER trg_ledger_audit_account;
DROP TRIGGER trg_ledger_audit_insert_unsealed;
DROP TRIGGER trg_ledger_audit_no_delete;
DROP TRIGGER trg_ledger_audit_no_update;
DROP TRIGGER trg_ledger_audit_post;
DROP TRIGGER trg_ledger_audit_void;

CREATE TABLE ledger_audit_log_new (
    id                  INTEGER PRIMARY KEY,
    timestamp           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    action_type         TEXT    NOT NULL CHECK (action_type IN ('POST', 'VOID', 'MIGRATION', 'ACCOUNT_CHANGE', 'FORENSIC_HEAL', 'USER_CHANGE', 'PRICE_CHANGE', 'ACH_FILE')),
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
