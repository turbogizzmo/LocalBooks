-- 0005_reconciliation.sql
-- Statement reconciliation for bank, credit card and other balance-sheet accounts.
--
-- Workflow: start a reconciliation (statement end date + ending balance) -> tick lines as cleared
-- (journal_lines.reconciliation_id is set) -> lock once the cleared balance equals the statement
-- ending balance, which stamps journal_lines.reconciled_at. Reconciled lines are frozen and their
-- entries can no longer be voided.
--
-- Balances here are expressed on the account's normal side: a positive bank balance is money in
-- the bank, a positive credit card balance is money owed.

CREATE TABLE reconciliations (
    id                              INTEGER PRIMARY KEY,
    account_id                      INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    statement_end_date              TEXT    NOT NULL CHECK (
                                        statement_end_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                                        AND date(statement_end_date) = statement_end_date
                                    ),
    statement_ending_balance_cents  INTEGER NOT NULL CHECK (abs(statement_ending_balance_cents) <= 1000000000000000),
    beginning_balance_cents         INTEGER NOT NULL,
    is_locked                       INTEGER NOT NULL DEFAULT 0 CHECK (is_locked IN (0, 1)),
    created_at                      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    locked_at                       TEXT,
    CHECK ((is_locked = 1) = (locked_at IS NOT NULL))
) STRICT;

-- At most one reconciliation in progress per account.
CREATE UNIQUE INDEX ux_rec_one_open ON reconciliations (account_id) WHERE is_locked = 0;
CREATE INDEX ix_rec_account_date ON reconciliations (account_id, statement_end_date);

ALTER TABLE journal_lines ADD COLUMN reconciliation_id INTEGER REFERENCES reconciliations (id) ON DELETE SET NULL;
ALTER TABLE journal_lines ADD COLUMN reconciled_at TEXT CHECK (reconciled_at IS NULL OR reconciliation_id IS NOT NULL);

CREATE INDEX ix_jl_reconciliation ON journal_lines (reconciliation_id);
CREATE INDEX ix_jl_account_unreconciled ON journal_lines (account_id) WHERE reconciled_at IS NULL;

-- Posted lines stay immutable for every financial column; only the reconciliation columns may change.
DROP TRIGGER trg_jl_update_draft_only;
CREATE TRIGGER trg_jl_update_draft_only
BEFORE UPDATE OF journal_entry_id, line_number, account_id, debit_cents, credit_cents, memo ON journal_lines
WHEN (SELECT status FROM journal_entries WHERE id = OLD.journal_entry_id) <> 'DRAFT'
  OR (SELECT status FROM journal_entries WHERE id = NEW.journal_entry_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'JE_IMMUTABLE: lines can only be changed on a DRAFT entry');
END;

CREATE TRIGGER trg_jl_reconcile_rules
BEFORE UPDATE OF reconciliation_id, reconciled_at ON journal_lines
BEGIN
    SELECT RAISE(ABORT, 'REC_LOCKED: this line belongs to a completed reconciliation')
    WHERE OLD.reconciled_at IS NOT NULL;
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
        SELECT MAX(statement_end_date) FROM reconciliations WHERE account_id = NEW.account_id AND is_locked = 1
    );
END;

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
          SELECT MAX(statement_end_date) FROM reconciliations WHERE account_id = NEW.account_id AND is_locked = 1
      );
    SELECT RAISE(ABORT, 'REC_AFTER_STATEMENT: cleared transactions are dated after the new statement end date')
    WHERE EXISTS (
        SELECT 1 FROM journal_lines l JOIN journal_entries e ON e.id = l.journal_entry_id
        WHERE l.reconciliation_id = OLD.id AND e.entry_date > NEW.statement_end_date
    );
END;

-- Locking requires the cleared balance to equal the statement and every cleared line to be stamped.
CREATE TRIGGER trg_rec_lock_balanced
BEFORE UPDATE OF is_locked ON reconciliations
WHEN OLD.is_locked = 0 AND NEW.is_locked = 1
BEGIN
    SELECT RAISE(ABORT, 'REC_NOT_BALANCED: cleared balance does not equal the statement ending balance')
    WHERE NEW.beginning_balance_cents + COALESCE((
        SELECT CASE t.normal_balance
                   WHEN 'DEBIT' THEN SUM(l.debit_cents) - SUM(l.credit_cents)
                   ELSE SUM(l.credit_cents) - SUM(l.debit_cents)
               END
        FROM journal_lines l
        JOIN chart_of_accounts a ON a.id = l.account_id
        JOIN account_types t ON t.code = a.account_type
        WHERE l.reconciliation_id = NEW.id
    ), 0) <> NEW.statement_ending_balance_cents;
    SELECT RAISE(ABORT, 'REC_UNSTAMPED: every cleared line must be stamped before locking')
    WHERE EXISTS (SELECT 1 FROM journal_lines WHERE reconciliation_id = NEW.id AND reconciled_at IS NULL);
END;

CREATE TRIGGER trg_rec_no_delete_locked
BEFORE DELETE ON reconciliations
WHEN OLD.is_locked = 1
BEGIN
    SELECT RAISE(ABORT, 'REC_LOCKED: completed reconciliations cannot be deleted');
END;

-- An entry with cleared or reconciled lines cannot be voided out from under the reconciliation.
CREATE TRIGGER trg_je_void_reconciled
BEFORE UPDATE OF status ON journal_entries
WHEN NEW.status = 'VOID' AND OLD.status <> 'VOID'
 AND EXISTS (SELECT 1 FROM journal_lines WHERE journal_entry_id = NEW.id AND reconciliation_id IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'REC_LOCKED: this entry has cleared or reconciled lines and cannot be voided');
END;
