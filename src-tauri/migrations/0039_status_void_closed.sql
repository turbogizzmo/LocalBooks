-- Closes the legacy status-void path (issue #45).
--
-- Since 0008 a void keeps the entry POSTED, sets is_voided = 1 and posts a reversing entry; every
-- void guard (closing date, audit trail, verifier) hangs off that model. The original 0001 rule still
-- let a POSTED entry's status become VOID, which dropped it from v_posted_lines (and so from every
-- balance) with no reversal, no audit row, inside a closed period, and without the verifier noticing.
--
-- Status now only ever moves DRAFT -> POSTED. Entries voided the old way before this migration are
-- recorded in legacy_status_voids, so the verifier can tell them apart from any later tampering.
-- (voided_at can only be set together with status VOID, so this also freezes voided_at.)

CREATE TABLE legacy_status_voids (
    entry_id    INTEGER PRIMARY KEY REFERENCES journal_entries (id) ON DELETE RESTRICT
) STRICT;

INSERT INTO legacy_status_voids (entry_id) SELECT id FROM journal_entries WHERE status = 'VOID' ORDER BY id;

CREATE TRIGGER trg_legacy_status_voids_no_insert BEFORE INSERT ON legacy_status_voids
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the list of legacy voids cannot be changed');
END;

CREATE TRIGGER trg_legacy_status_voids_no_update BEFORE UPDATE ON legacy_status_voids
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the list of legacy voids cannot be changed');
END;

CREATE TRIGGER trg_legacy_status_voids_no_delete BEFORE DELETE ON legacy_status_voids
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the list of legacy voids cannot be changed');
END;

DROP TRIGGER trg_je_status_transition;
CREATE TRIGGER trg_je_status_transition
BEFORE UPDATE OF status ON journal_entries
WHEN NEW.status IS NOT OLD.status AND NOT (OLD.status = 'DRAFT' AND NEW.status = 'POSTED')
BEGIN
    SELECT RAISE(ABORT, 'JE_INVALID_TRANSITION: the only allowed transition is DRAFT->POSTED; void a posted entry with a reversing entry');
END;
