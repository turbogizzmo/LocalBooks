-- Forensic ledger audit trail.
--
-- `ledger_audit_log` stores a complete JSON snapshot of every journal entry (header plus every line)
-- at the moment it is posted, a link from every voided entry to the reversing entry that corrected
-- it, every schema migration applied to this file, and every change to an account's identity. Rows
-- are written by triggers (and, for migrations, by `db::migrate`) and can never be updated or
-- deleted. `ledger::verify_ledger_audit` rebuilds each posted entry from the live tables through
-- `v_entry_snapshot` and compares it with its posting snapshot, so any drift is detectable even if
-- the protecting triggers were bypassed.

CREATE TABLE ledger_audit_log (
    id                  INTEGER PRIMARY KEY,
    timestamp           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    action_type         TEXT    NOT NULL CHECK (action_type IN ('POST', 'VOID', 'MIGRATION', 'ACCOUNT_CHANGE')),
    target_entity_id    INTEGER NOT NULL,
    payload_snapshot    TEXT    NOT NULL CHECK (json_valid(payload_snapshot))
) STRICT;

CREATE INDEX ix_ledger_audit_target ON ledger_audit_log (action_type, target_entity_id);

CREATE TRIGGER trg_ledger_audit_no_update BEFORE UPDATE ON ledger_audit_log
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the ledger audit trail cannot be changed');
END;

CREATE TRIGGER trg_ledger_audit_no_delete BEFORE DELETE ON ledger_audit_log
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the ledger audit trail cannot be changed');
END;

-- The canonical snapshot of a journal entry: every field that is fixed once the entry is posted.
-- (`status` and `is_voided` are excluded: voiding is recorded as its own VOID event.)
CREATE VIEW v_entry_snapshot AS
SELECT
    e.id AS entry_id,
    json_object(
        'entryId',             e.id,
        'entryNumber',         e.entry_number,
        'entryDate',           e.entry_date,
        'memo',                e.memo,
        'sourceType',          e.source_type,
        'sourceId',            e.source_id,
        'reversesEntryId',     e.reverses_entry_id,
        'isAdjusting',         e.is_adjusting,
        'postedAt',            e.posted_at,
        'foreignCurrencyCode', e.foreign_currency_code,
        'exchangeRateE6',      e.exchange_rate_e6,
        'totalDebitCents',     (SELECT COALESCE(SUM(debit_cents), 0)  FROM journal_lines WHERE journal_entry_id = e.id),
        'totalCreditCents',    (SELECT COALESCE(SUM(credit_cents), 0) FROM journal_lines WHERE journal_entry_id = e.id),
        'lines', (
            SELECT json_group_array(
                       json_object(
                           'lineNumber',         l.line_number,
                           'accountId',          l.account_id,
                           'debitCents',         l.debit_cents,
                           'creditCents',        l.credit_cents,
                           'memo',               l.memo,
                           'foreignDebitCents',  l.foreign_debit_cents,
                           'foreignCreditCents', l.foreign_credit_cents
                       ) ORDER BY l.line_number)
            FROM journal_lines l WHERE l.journal_entry_id = e.id
        )
    ) AS snapshot
FROM journal_entries e;

CREATE TRIGGER trg_ledger_audit_post
AFTER UPDATE OF status ON journal_entries
WHEN OLD.status = 'DRAFT' AND NEW.status = 'POSTED'
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    SELECT 'POST', NEW.id, snapshot FROM v_entry_snapshot WHERE entry_id = NEW.id;
END;

-- A void is corrected by a fresh reversing entry (already posted, with its own POST snapshot);
-- this row links the original to that correction.
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

-- Account identity changes are recorded with before/after values.
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

-- Gap 1: a posted entry's memo was the one header field that could still be edited in place.
DROP TRIGGER trg_je_posted_header_immutable;
CREATE TRIGGER trg_je_posted_header_immutable
BEFORE UPDATE ON journal_entries
WHEN OLD.status <> 'DRAFT'
 AND (   NEW.entry_number      IS NOT OLD.entry_number
      OR NEW.entry_date        IS NOT OLD.entry_date
      OR NEW.memo              IS NOT OLD.memo
      OR NEW.source_type       IS NOT OLD.source_type
      OR NEW.source_id         IS NOT OLD.source_id
      OR NEW.reverses_entry_id IS NOT OLD.reverses_entry_id
      OR NEW.is_adjusting      IS NOT OLD.is_adjusting
      OR NEW.posted_at         IS NOT OLD.posted_at
      OR NEW.created_at        IS NOT OLD.created_at
      OR (OLD.status = 'VOID' AND NEW.voided_at IS NOT OLD.voided_at))
BEGIN
    SELECT RAISE(ABORT, 'JE_IMMUTABLE: posted or voided entries cannot be altered; void or reverse instead');
END;

-- Gap 2: changing an account's type would silently move its posted history between statements
-- (e.g. a bank account's balance reappearing as an expense).
CREATE TRIGGER trg_coa_type_locked_by_history
BEFORE UPDATE OF account_type ON chart_of_accounts
WHEN NEW.account_type <> OLD.account_type
 AND EXISTS (SELECT 1 FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
             WHERE l.account_id = OLD.id AND e.status <> 'DRAFT')
BEGIN
    SELECT RAISE(ABORT, 'COA_TYPE_LOCKED: this account has posted transactions, so its type cannot change; create a new account and move the balance with a journal entry');
END;

-- Backfill: snapshot everything posted before the trail existed. Posted rows are immutable, so the
-- snapshot taken now is the snapshot that would have been taken at posting time.
INSERT INTO ledger_audit_log (timestamp, action_type, target_entity_id, payload_snapshot)
SELECT e.posted_at, 'POST', e.id, json_set(s.snapshot, '$.backfilled', json('true'))
FROM journal_entries e JOIN v_entry_snapshot s ON s.entry_id = e.id
WHERE e.status <> 'DRAFT'
ORDER BY e.id;

INSERT INTO ledger_audit_log (timestamp, action_type, target_entity_id, payload_snapshot)
SELECT r.posted_at, 'VOID', o.id, json_object(
    'entryId',             o.id,
    'entryNumber',         o.entry_number,
    'reversalEntryId',     r.id,
    'reversalEntryNumber', r.entry_number,
    'reversalEntryDate',   r.entry_date,
    'amountCents',         (SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = o.id),
    'backfilled',          json('true')
)
FROM journal_entries o JOIN journal_entries r ON r.reverses_entry_id = o.id
WHERE o.is_voided = 1
ORDER BY o.id;
