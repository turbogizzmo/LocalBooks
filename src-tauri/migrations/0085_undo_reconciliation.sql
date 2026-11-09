-- 0085_undo_reconciliation.sql
-- Undo Last Reconciliation (T54). Design: docs/plans/undo-reconciliation.md.
--
-- A locked reconciliation is never edited or deleted. Undoing it adds an append-only row to
-- `reconciliation_undos`; only then may its lines lose `reconciled_at` and `reconciliation_id`.
-- The undone reconciliation then no longer counts as the account's last one.

CREATE TABLE reconciliation_undos (
    id                  INTEGER PRIMARY KEY,
    reconciliation_id   INTEGER NOT NULL UNIQUE REFERENCES reconciliations (id) ON DELETE RESTRICT,
    undone_at           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    -- The lines it released and their total on the account's normal side, as they stood.
    released_count      INTEGER NOT NULL CHECK (released_count >= 0),
    released_total_cents INTEGER NOT NULL,
    reason              TEXT    CHECK (reason IS NULL OR length(reason) BETWEEN 1 AND 500)
) STRICT;

-- Only the account's last reconciliation not already undone, while none is in progress.
CREATE TRIGGER trg_rec_undo_checks
BEFORE INSERT ON reconciliation_undos
BEGIN
    SELECT RAISE(ABORT, 'REC_UNDO_NOT_LOCKED: only a completed reconciliation can be undone')
    WHERE (SELECT is_locked FROM reconciliations WHERE id = NEW.reconciliation_id) IS NOT 1;
    SELECT RAISE(ABORT, 'REC_UNDO_NOT_LAST: only the last reconciliation of an account can be undone')
    WHERE EXISTS (
        SELECT 1 FROM reconciliations later, reconciliations r
        WHERE r.id = NEW.reconciliation_id AND later.account_id = r.account_id AND later.is_locked = 1
          AND later.statement_end_date > r.statement_end_date
          AND NOT EXISTS (SELECT 1 FROM reconciliation_undos u WHERE u.reconciliation_id = later.id)
    );
    SELECT RAISE(ABORT, 'REC_UNDO_IN_PROGRESS: finish or cancel the reconciliation in progress first')
    WHERE EXISTS (
        SELECT 1 FROM reconciliations o, reconciliations r
        WHERE r.id = NEW.reconciliation_id AND o.account_id = r.account_id AND o.is_locked = 0
    );
END;

CREATE TRIGGER trg_rec_undo_no_update BEFORE UPDATE ON reconciliation_undos
BEGIN
    SELECT RAISE(ABORT, 'REC_UNDO_IMMUTABLE: an undone reconciliation stays undone');
END;
CREATE TRIGGER trg_rec_undo_no_delete BEFORE DELETE ON reconciliation_undos
BEGIN
    SELECT RAISE(ABORT, 'REC_UNDO_IMMUTABLE: an undone reconciliation stays undone');
END;

-- Line rules from 0005, with one exception: a line of an undone reconciliation may be released
-- (both columns cleared together).
DROP TRIGGER trg_jl_reconcile_rules;
CREATE TRIGGER trg_jl_reconcile_rules
BEFORE UPDATE OF reconciliation_id, reconciled_at ON journal_lines
BEGIN
    SELECT RAISE(ABORT, 'REC_LOCKED: this line belongs to a completed reconciliation')
    WHERE OLD.reconciled_at IS NOT NULL
      AND NOT (NEW.reconciliation_id IS NULL AND NEW.reconciled_at IS NULL
               AND EXISTS (SELECT 1 FROM reconciliation_undos u WHERE u.reconciliation_id = OLD.reconciliation_id));
    SELECT RAISE(ABORT, 'REC_NOT_POSTED: only posted transactions can be cleared')
    WHERE NEW.reconciliation_id IS NOT NULL
      AND (SELECT status FROM journal_entries WHERE id = NEW.journal_entry_id) <> 'POSTED';
    SELECT RAISE(ABORT, 'REC_WRONG_ACCOUNT: line does not belong to the account being reconciled')
    WHERE NEW.reconciliation_id IS NOT NULL
      AND (SELECT account_id FROM reconciliations WHERE id = NEW.reconciliation_id) <> NEW.account_id;
    SELECT RAISE(ABORT, 'REC_LOCKED: this reconciliation is already completed')
    WHERE NEW.reconciliation_id IS NOT NULL
      AND (SELECT is_locked FROM reconciliations WHERE id = NEW.reconciliation_id) = 1;
    SELECT RAISE(ABORT, 'REC_AFTER_STATEMENT: transaction is dated after the statement end date')
    WHERE NEW.reconciliation_id IS NOT NULL
      AND (SELECT entry_date FROM journal_entries WHERE id = NEW.journal_entry_id)
        > (SELECT statement_end_date FROM reconciliations WHERE id = NEW.reconciliation_id);
END;

-- "After the last completed reconciliation" leaves out undone ones (0005's checks otherwise).
DROP TRIGGER trg_rec_insert_checks;
CREATE TRIGGER trg_rec_insert_checks
BEFORE INSERT ON reconciliations
BEGIN
    SELECT RAISE(ABORT, 'REC_INSERT_LOCKED: reconciliations must start unlocked')
    WHERE NEW.is_locked <> 0;
    SELECT RAISE(ABORT, 'REC_ACCOUNT_TYPE: reconcile bank, credit card, asset or liability accounts only')
    WHERE (SELECT a.account_type FROM chart_of_accounts a WHERE a.id = NEW.account_id)
          NOT IN ('BANK', 'CREDIT_CARD', 'OTHER_CURRENT_ASSET', 'FIXED_ASSET', 'OTHER_ASSET',
                  'OTHER_CURRENT_LIABILITY', 'LONG_TERM_LIABILITY')
       OR (SELECT is_system FROM chart_of_accounts WHERE id = NEW.account_id) = 1;
    SELECT RAISE(ABORT, 'REC_DATE_NOT_AFTER_LAST: statement must end after the last completed reconciliation')
    WHERE NEW.statement_end_date <= (
        SELECT MAX(r.statement_end_date) FROM reconciliations r
        WHERE r.account_id = NEW.account_id AND r.is_locked = 1
          AND NOT EXISTS (SELECT 1 FROM reconciliation_undos u WHERE u.reconciliation_id = r.id)
    );
END;

DROP TRIGGER trg_rec_update_checks;
CREATE TRIGGER trg_rec_update_checks
BEFORE UPDATE ON reconciliations
BEGIN
    SELECT RAISE(ABORT, 'REC_LOCKED: completed reconciliations cannot be changed')
    WHERE OLD.is_locked = 1;
    SELECT RAISE(ABORT, 'REC_IMMUTABLE: account and beginning balance are fixed when a reconciliation starts')
    WHERE NEW.account_id <> OLD.account_id OR NEW.beginning_balance_cents <> OLD.beginning_balance_cents;
    SELECT RAISE(ABORT, 'REC_DATE_NOT_AFTER_LAST: statement must end after the last completed reconciliation')
    WHERE NEW.statement_end_date <> OLD.statement_end_date
      AND NEW.statement_end_date <= (
          SELECT MAX(r.statement_end_date) FROM reconciliations r
          WHERE r.account_id = NEW.account_id AND r.is_locked = 1
            AND NOT EXISTS (SELECT 1 FROM reconciliation_undos u WHERE u.reconciliation_id = r.id)
      );
    SELECT RAISE(ABORT, 'REC_AFTER_STATEMENT: cleared transactions are dated after the new statement end date')
    WHERE EXISTS (
        SELECT 1 FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
        WHERE l.reconciliation_id = OLD.id AND e.entry_date > NEW.statement_end_date
    );
END;

-- The Audit Log (audit_log, 0008) gains UNDO_RECONCILIATION. Rebuilt with its index and the
-- triggers that write to it or guard it, unchanged otherwise.
DROP TRIGGER trg_audit_post;
DROP TRIGGER trg_audit_void;
DROP TRIGGER trg_audit_reconcile;
DROP TRIGGER trg_audit_immutable_upd;
DROP TRIGGER trg_audit_immutable_del;

CREATE TABLE audit_log_new (
    id                  INTEGER PRIMARY KEY,
    occurred_at         TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    action              TEXT    NOT NULL CHECK (action IN ('POST', 'VOID', 'LOCK_RECONCILIATION', 'UNDO_RECONCILIATION')),
    journal_entry_id    INTEGER REFERENCES journal_entries (id) ON DELETE RESTRICT,
    reconciliation_id   INTEGER REFERENCES reconciliations (id) ON DELETE RESTRICT,
    source_type         TEXT,
    source_id           INTEGER,
    amount_cents        INTEGER,
    detail              TEXT    NOT NULL
) STRICT;
INSERT INTO audit_log_new (id, occurred_at, action, journal_entry_id, reconciliation_id, source_type, source_id, amount_cents, detail)
SELECT id, occurred_at, action, journal_entry_id, reconciliation_id, source_type, source_id, amount_cents, detail FROM audit_log;
DROP TABLE audit_log;
ALTER TABLE audit_log_new RENAME TO audit_log;
CREATE INDEX ix_audit_log_time ON audit_log (occurred_at);

CREATE TRIGGER trg_audit_post
AFTER UPDATE OF status ON journal_entries
WHEN OLD.status = 'DRAFT' AND NEW.status = 'POSTED'
BEGIN
    INSERT INTO audit_log (action, journal_entry_id, source_type, source_id, amount_cents, detail)
    VALUES ('POST', NEW.id, NEW.source_type, NEW.source_id,
            (SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = NEW.id),
            'Posted entry #' || NEW.entry_number || ' dated ' || NEW.entry_date || COALESCE(': ' || NEW.memo, ''));
END;

CREATE TRIGGER trg_audit_void
AFTER UPDATE OF is_voided ON journal_entries
WHEN OLD.is_voided = 0 AND NEW.is_voided = 1
BEGIN
    INSERT INTO audit_log (action, journal_entry_id, source_type, source_id, amount_cents, detail)
    VALUES ('VOID', NEW.id, NEW.source_type, NEW.source_id,
            (SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = NEW.id),
            'Voided entry #' || NEW.entry_number || ' (reversed by entry #' ||
            (SELECT entry_number FROM journal_entries WHERE reverses_entry_id = NEW.id) || ')' ||
            COALESCE(': ' || NEW.memo, ''));
END;

CREATE TRIGGER trg_audit_reconcile
AFTER UPDATE OF is_locked ON reconciliations
WHEN OLD.is_locked = 0 AND NEW.is_locked = 1
BEGIN
    INSERT INTO audit_log (action, reconciliation_id, amount_cents, detail)
    VALUES ('LOCK_RECONCILIATION', NEW.id, NEW.statement_ending_balance_cents,
            'Reconciled ' || (SELECT name FROM chart_of_accounts WHERE id = NEW.account_id) ||
            ' through ' || NEW.statement_end_date);
END;

CREATE TRIGGER trg_audit_undo_reconcile
AFTER INSERT ON reconciliation_undos
BEGIN
    INSERT INTO audit_log (action, reconciliation_id, amount_cents, detail)
    SELECT 'UNDO_RECONCILIATION', r.id, NEW.released_total_cents,
           'Undid the reconciliation of ' || a.name || ' through ' || r.statement_end_date ||
           ' (' || NEW.released_count || ' transaction' || CASE NEW.released_count WHEN 1 THEN '' ELSE 's' END || ' uncleared)' ||
           COALESCE(': ' || NEW.reason, '')
    FROM reconciliations r JOIN chart_of_accounts a ON a.id = r.account_id
    WHERE r.id = NEW.reconciliation_id;
END;

CREATE TRIGGER trg_audit_immutable_upd BEFORE UPDATE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the audit log cannot be changed');
END;
CREATE TRIGGER trg_audit_immutable_del BEFORE DELETE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the audit log cannot be changed');
END;
