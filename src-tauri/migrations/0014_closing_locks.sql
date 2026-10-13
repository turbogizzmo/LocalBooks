-- Closing date (books lock).
--
-- Each row records one change to the closing date; rows are never updated or deleted, so the table
-- is the full history of when periods were closed and reopened. The most recently set row is in
-- force: entries dated on or before its `lock_date` cannot be posted (which also blocks voiding
-- them, since a reversal carries the original date). A NULL `lock_date` records that the books were
-- reopened. Changing the closing date requires the passcode of the lock in force, when it has one.

CREATE TABLE closing_locks (
    id                  INTEGER PRIMARY KEY,
    lock_date           TEXT    CHECK (lock_date IS NULL OR (
                            lock_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(lock_date) = lock_date)),
    set_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    -- `pbkdf2-sha256$<iterations>$<salt hex>$<hash hex>`, or NULL when no passcode is set.
    hashed_passcode     TEXT    CHECK (hashed_passcode IS NULL OR hashed_passcode GLOB 'pbkdf2-sha256$*')
) STRICT;

CREATE TRIGGER trg_closing_locks_no_update BEFORE UPDATE ON closing_locks
BEGIN
    SELECT RAISE(ABORT, 'CLOSE_HISTORY_IMMUTABLE: closing date history cannot be changed');
END;

CREATE TRIGGER trg_closing_locks_no_delete BEFORE DELETE ON closing_locks
BEGIN
    SELECT RAISE(ABORT, 'CLOSE_HISTORY_IMMUTABLE: closing date history cannot be changed');
END;

-- Backstop for every posting path: the ledger checks first, this guarantees it.
CREATE TRIGGER trg_je_closed_period
BEFORE UPDATE OF status ON journal_entries
WHEN OLD.status = 'DRAFT' AND NEW.status = 'POSTED'
 AND NEW.entry_date <= (SELECT lock_date FROM closing_locks ORDER BY id DESC LIMIT 1)
BEGIN
    SELECT RAISE(ABORT, 'PERIOD_CLOSED: Cannot write transaction: Target period is legally closed.');
END;
