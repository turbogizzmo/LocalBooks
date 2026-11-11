-- Closing Date Exceptions (`list_reports/parity.rs`) lists entries entered after their date was
-- closed. It compared the entry's created_at with the close's set_at, both millisecond clock times;
-- on Windows the clock moves in steps of about 15 ms, so an entry posted just after a close (or a
-- close just after an entry) can carry the same time and the order is lost. Writes are serialized
-- (`ledger::begin_write`), so the order is exact in journal entry ids: each change to the closing
-- date now records the highest entry id when it was made, and an entry is after that close exactly
-- when its id is higher.
ALTER TABLE closing_locks ADD COLUMN last_entry_id INTEGER;

-- Earlier changes: the highest entry made on or before their time, as the report read them before.
-- The history is otherwise never changed; its guard is lifted for this one fill and put back as it was.
DROP TRIGGER trg_closing_locks_no_update;
UPDATE closing_locks
SET last_entry_id = COALESCE((SELECT MAX(e.id) FROM journal_entries e WHERE e.created_at <= closing_locks.set_at), 0);
CREATE TRIGGER trg_closing_locks_no_update BEFORE UPDATE ON closing_locks
BEGIN
    SELECT RAISE(ABORT, 'CLOSE_HISTORY_IMMUTABLE: closing date history cannot be changed');
END;
