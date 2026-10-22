-- Class tracking, completed to match classic desktop accounting software.
--
-- * Preferences: `class_tracking` turns the feature on (off by default, as in QuickBooks; on
--   already for a file that has classes), `class_prompt` warns before saving a transaction with
--   income or expense lines that have no class, and `class_assign` chooses between one class per
--   line and one class for the whole transaction.
-- * Deposit lines, bank-feed lines and time activities can carry a class too. A deposit's
--   "other funds" lines are often income; a bank-feed line keeps the class it was approved with;
--   a time activity records which class the work belongs to.

INSERT INTO system_preferences (key, value) VALUES
    ('class_tracking', CASE WHEN EXISTS (SELECT 1 FROM classes) THEN 'true' ELSE 'false' END),
    ('class_prompt', 'false'),
    ('class_assign', 'line');

ALTER TABLE deposit_lines     ADD COLUMN class_id INTEGER REFERENCES classes (id) ON DELETE RESTRICT;
ALTER TABLE bank_transactions ADD COLUMN class_id INTEGER REFERENCES classes (id) ON DELETE RESTRICT;
ALTER TABLE time_activities   ADD COLUMN class_id INTEGER REFERENCES classes (id) ON DELETE RESTRICT;

-- Reclassifying: a posted line's class cannot change, so moving it to another class posts a
-- reclassifying journal entry on the line's own date (the same account, out of the old class and
-- into the new one). Each move is recorded here, so a line's current class is the `to_class_id`
-- of its latest move whose entry is still live, and a line can be moved again without doubling up.
CREATE TABLE class_reclassifications (
    id                  INTEGER PRIMARY KEY,
    line_id             INTEGER NOT NULL REFERENCES journal_lines (id) ON DELETE RESTRICT,
    from_class_id       INTEGER REFERENCES classes (id) ON DELETE RESTRICT,
    to_class_id         INTEGER REFERENCES classes (id) ON DELETE RESTRICT,
    journal_entry_id    INTEGER NOT NULL REFERENCES journal_entries (id) ON DELETE RESTRICT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (from_class_id IS NOT to_class_id)
) STRICT;

CREATE INDEX ix_class_reclass_line ON class_reclassifications (line_id, id);
CREATE INDEX ix_class_reclass_entry ON class_reclassifications (journal_entry_id);

CREATE TRIGGER trg_class_reclass_no_update BEFORE UPDATE ON class_reclassifications
BEGIN
    SELECT RAISE(ABORT, 'RECLASS_IMMUTABLE: reclassifications are history; void the reclassifying entry instead');
END;

CREATE TRIGGER trg_class_reclass_no_delete BEFORE DELETE ON class_reclassifications
BEGIN
    SELECT RAISE(ABORT, 'RECLASS_IMMUTABLE: reclassifications are history; void the reclassifying entry instead');
END;

-- Each posted line with its current class, after any live reclassification.
CREATE VIEW v_line_effective_class AS
SELECT
    l.id AS line_id,
    COALESCE(
        (SELECT r.to_class_id FROM class_reclassifications r
         JOIN journal_entries re ON re.id = r.journal_entry_id
         WHERE r.line_id = l.id AND re.status = 'POSTED' AND re.is_voided = 0
         ORDER BY r.id DESC LIMIT 1),
        CASE WHEN EXISTS (SELECT 1 FROM class_reclassifications r
                          JOIN journal_entries re ON re.id = r.journal_entry_id
                          WHERE r.line_id = l.id AND re.status = 'POSTED' AND re.is_voided = 0)
             THEN NULL ELSE l.class_id END
    ) AS class_id
FROM journal_lines l;
