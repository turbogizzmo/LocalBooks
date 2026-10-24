-- Freeze legacy rows that used the pre-0008 status='VOID' path.
CREATE TABLE legacy_status_voids (
    journal_entry_id    INTEGER PRIMARY KEY REFERENCES journal_entries (id) ON DELETE RESTRICT
) STRICT;

INSERT INTO legacy_status_voids (journal_entry_id)
SELECT id
FROM journal_entries
WHERE status = 'VOID';

-- Status now only controls posting from DRAFT. Voiding must use is_voided + reversal.
DROP TRIGGER trg_je_status_transition;
CREATE TRIGGER trg_je_status_transition
BEFORE UPDATE OF status ON journal_entries
WHEN NOT (
       (OLD.status = 'DRAFT'  AND NEW.status IN ('DRAFT', 'POSTED'))
    OR (OLD.status = 'POSTED' AND NEW.status = 'POSTED')
    OR (OLD.status = 'VOID'   AND NEW.status = 'VOID')
)
BEGIN
    SELECT RAISE(ABORT, 'JE_INVALID_TRANSITION: allowed transitions are DRAFT->POSTED only');
END;
