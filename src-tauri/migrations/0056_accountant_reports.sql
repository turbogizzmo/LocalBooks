-- 0056_accountant_reports.sql
-- Accountant & Taxes reports.
--
-- 1. Tax lines: each account may name the income tax return line its amounts go to (a code from
--    `accountant_reports::TAX_LINES`, or the text a QuickBooks import carried when it matches none).
-- 2. Reconciliation snapshots: when a reconciliation is completed, the cleared lines are copied as
--    they stood, so the Reconciliation Discrepancy report can show anything changed, voided or
--    deleted afterwards. Reconciliations completed before this migration have no snapshot; the
--    report checks those by their totals only.
--
-- (Adjusting journal entries need no change: `journal_entries.is_adjusting` exists since 0001.)

ALTER TABLE chart_of_accounts ADD COLUMN tax_line TEXT
    CHECK (tax_line IS NULL OR length(trim(tax_line)) BETWEEN 1 AND 200);

CREATE TABLE reconciliation_snapshots (
    reconciliation_id   INTEGER PRIMARY KEY REFERENCES reconciliations (id) ON DELETE RESTRICT,
    cleared_count       INTEGER NOT NULL,
    -- On the account's normal side, as `reconciliations` balances are.
    cleared_total_cents INTEGER NOT NULL,
    recorded_at         TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- No foreign key to journal_lines: a snapshot must outlive the line it describes.
CREATE TABLE reconciliation_snapshot_lines (
    reconciliation_id   INTEGER NOT NULL REFERENCES reconciliation_snapshots (reconciliation_id) ON DELETE RESTRICT,
    journal_line_id     INTEGER NOT NULL,
    journal_entry_id    INTEGER NOT NULL,
    entry_number        INTEGER NOT NULL,
    entry_date          TEXT    NOT NULL,
    source_type         TEXT    NOT NULL,
    account_id          INTEGER NOT NULL,
    debit_cents         INTEGER NOT NULL,
    credit_cents        INTEGER NOT NULL,
    -- Effect on the account's normal-side balance.
    amount_cents        INTEGER NOT NULL,
    memo                TEXT,
    PRIMARY KEY (reconciliation_id, journal_line_id)
) STRICT;

CREATE TRIGGER trg_rec_snapshot
AFTER UPDATE OF is_locked ON reconciliations
WHEN OLD.is_locked = 0 AND NEW.is_locked = 1
BEGIN
    INSERT INTO reconciliation_snapshots (reconciliation_id, cleared_count, cleared_total_cents)
    SELECT NEW.id, COUNT(*),
           COALESCE(SUM(CASE t.normal_balance WHEN 'DEBIT' THEN l.debit_cents - l.credit_cents ELSE l.credit_cents - l.debit_cents END), 0)
    FROM journal_lines l
    JOIN chart_of_accounts a ON a.id = l.account_id
    JOIN account_types t ON t.code = a.account_type
    WHERE l.reconciliation_id = NEW.id;

    INSERT INTO reconciliation_snapshot_lines
        (reconciliation_id, journal_line_id, journal_entry_id, entry_number, entry_date, source_type, account_id,
         debit_cents, credit_cents, amount_cents, memo)
    SELECT NEW.id, l.id, e.id, e.entry_number, e.entry_date,
           CASE WHEN e.source_type = 'EXPENSE' AND (SELECT is_credit FROM expenses WHERE id = e.source_id) = 1 THEN 'CARD_CREDIT' ELSE e.source_type END,
           l.account_id, l.debit_cents, l.credit_cents,
           CASE t.normal_balance WHEN 'DEBIT' THEN l.debit_cents - l.credit_cents ELSE l.credit_cents - l.debit_cents END,
           COALESCE(l.memo, e.memo)
    FROM journal_lines l
    JOIN journal_entries e ON e.id = l.journal_entry_id
    JOIN chart_of_accounts a ON a.id = l.account_id
    JOIN account_types t ON t.code = a.account_type
    WHERE l.reconciliation_id = NEW.id;
END;

CREATE TRIGGER trg_rec_snapshot_no_update BEFORE UPDATE ON reconciliation_snapshots
BEGIN
    SELECT RAISE(ABORT, 'REC_SNAPSHOT_IMMUTABLE: a reconciliation snapshot cannot be changed');
END;
CREATE TRIGGER trg_rec_snapshot_no_delete BEFORE DELETE ON reconciliation_snapshots
BEGIN
    SELECT RAISE(ABORT, 'REC_SNAPSHOT_IMMUTABLE: a reconciliation snapshot cannot be changed');
END;
CREATE TRIGGER trg_rec_snapshot_lines_no_update BEFORE UPDATE ON reconciliation_snapshot_lines
BEGIN
    SELECT RAISE(ABORT, 'REC_SNAPSHOT_IMMUTABLE: a reconciliation snapshot cannot be changed');
END;
CREATE TRIGGER trg_rec_snapshot_lines_no_delete BEFORE DELETE ON reconciliation_snapshot_lines
BEGIN
    SELECT RAISE(ABORT, 'REC_SNAPSHOT_IMMUTABLE: a reconciliation snapshot cannot be changed');
END;

-- The tax line a QuickBooks import read for each source account, copied to the account it maps to.
ALTER TABLE ledger_import_accounts ADD COLUMN tax_line TEXT;
