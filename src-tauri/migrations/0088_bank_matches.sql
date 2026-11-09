-- 0088_bank_matches.sql
-- Downloaded bank lines matched to transactions already entered (gap T53).
--
-- A downloaded line that is a check, deposit or payment already in the books is matched to that
-- transaction's journal entry instead of being added again: it posts nothing, and leaves the
-- review queue as EXCLUDED with a live row here saying what it matched. Matching happens on
-- import when exactly one entered transaction agrees (amount, date, check number), or by hand.
--
-- Rows are append-only: unmatching (or voiding the matched transaction) marks the match undone and
-- puts the line back in the queue.

CREATE TABLE bank_matches (
    id                  INTEGER PRIMARY KEY,
    bank_transaction_id INTEGER NOT NULL REFERENCES bank_transactions (id) ON DELETE RESTRICT,
    journal_entry_id    INTEGER NOT NULL REFERENCES journal_entries (id) ON DELETE RESTRICT,
    -- AUTO when the import matched it, MANUAL when the user did.
    origin              TEXT    NOT NULL CHECK (origin IN ('AUTO', 'MANUAL')),
    is_undone           INTEGER NOT NULL DEFAULT 0 CHECK (is_undone IN (0, 1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_bank_matches_line ON bank_matches (bank_transaction_id) WHERE is_undone = 0;
CREATE UNIQUE INDEX ux_bank_matches_entry ON bank_matches (journal_entry_id) WHERE is_undone = 0;

CREATE TRIGGER trg_bank_match_insert
BEFORE INSERT ON bank_matches
BEGIN
    SELECT RAISE(ABORT, 'BANK_MATCH_PREUNDONE: a match is recorded live')
    WHERE NEW.is_undone <> 0;
    SELECT RAISE(ABORT, 'BANK_MATCH_NOT_EXCLUDED: take the bank line out of the review queue before matching it')
    WHERE (SELECT status FROM bank_transactions WHERE id = NEW.bank_transaction_id) IS NOT 'EXCLUDED';
    SELECT RAISE(ABORT, 'BANK_MATCH_BAD_ENTRY: a bank line matches a posted, live transaction in its own account for its amount')
    WHERE NOT EXISTS (
        SELECT 1 FROM bank_transactions t
        JOIN journal_entries e ON e.id = NEW.journal_entry_id AND e.status = 'POSTED' AND e.is_voided = 0 AND e.reverses_entry_id IS NULL
        WHERE t.id = NEW.bank_transaction_id
          AND (SELECT SUM(l.debit_cents - l.credit_cents) FROM journal_lines l
               WHERE l.journal_entry_id = e.id AND l.account_id = t.bank_account_id) = t.amount_cents
    );
    SELECT RAISE(ABORT, 'BANK_MATCH_FROM_FEED: that transaction was added from the bank feed')
    WHERE EXISTS (SELECT 1 FROM bank_transactions WHERE journal_entry_id = NEW.journal_entry_id);
END;

CREATE TRIGGER trg_bank_match_update
BEFORE UPDATE ON bank_matches
BEGIN
    SELECT RAISE(ABORT, 'BANK_MATCH_IMMUTABLE: a match can only be undone')
    WHERE NEW.bank_transaction_id IS NOT OLD.bank_transaction_id OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
       OR NEW.origin IS NOT OLD.origin OR NEW.created_at IS NOT OLD.created_at OR NEW.is_undone < OLD.is_undone;
END;

CREATE TRIGGER trg_bank_match_no_delete BEFORE DELETE ON bank_matches
BEGIN
    SELECT RAISE(ABORT, 'BANK_MATCH_IMMUTABLE: a match can only be undone');
END;

-- Voiding the matched transaction puts the bank line back in the queue.
CREATE TRIGGER trg_bank_match_entry_voided
AFTER UPDATE OF is_voided ON journal_entries
WHEN OLD.is_voided = 0 AND NEW.is_voided = 1
BEGIN
    UPDATE bank_transactions SET status = 'PENDING', reviewed_at = NULL
    WHERE id IN (SELECT bank_transaction_id FROM bank_matches WHERE journal_entry_id = NEW.id AND is_undone = 0);
    UPDATE bank_matches SET is_undone = 1 WHERE journal_entry_id = NEW.id AND is_undone = 0;
END;
