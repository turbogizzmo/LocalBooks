-- Class tracking: a second way to slice income and expenses (by department, location, line of
-- business...), independent of the chart of accounts.
--
-- Classes form their own list with "Parent:Child" subclasses, like accounts. Any journal line may
-- carry one class; the business documents that post lines (invoices, sales receipts, bills,
-- expenses and checks, estimates) keep the class chosen on each line so it is posted with it.
-- A posted line's class is as immutable as its amount (the existing journal_lines triggers already
-- refuse any change to a posted line), and a void's reversing entry carries the same classes, so
-- Profit & Loss by Class nets voided transactions to zero in every column.
--
-- Classes are never required. Lines without one are reported as "Unclassified".

CREATE TABLE classes (
    id              INTEGER PRIMARY KEY,
    name            TEXT    NOT NULL CHECK (length(trim(name)) > 0 AND instr(name, ':') = 0),
    parent_id       INTEGER REFERENCES classes (id) ON DELETE RESTRICT,
    is_active       INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (parent_id IS NULL OR parent_id <> id)
) STRICT;

CREATE UNIQUE INDEX ux_classes_sibling_name ON classes (COALESCE(parent_id, 0), name COLLATE NOCASE);
CREATE INDEX ix_classes_parent ON classes (parent_id);

CREATE VIEW v_class_ancestry AS
WITH RECURSIVE walk (class_id, ancestor_id, depth) AS (
    SELECT id, id, 0 FROM classes
    UNION ALL
    SELECT w.class_id, c.parent_id, w.depth + 1
    FROM walk w
    JOIN classes c ON c.id = w.ancestor_id
    WHERE c.parent_id IS NOT NULL AND w.depth < 16
)
SELECT class_id, ancestor_id, depth FROM walk;

CREATE VIEW v_classes AS
SELECT
    c.id,
    c.name,
    (
        SELECT group_concat(name, ':')
        FROM (
            SELECT p.name
            FROM v_class_ancestry x
            JOIN classes p ON p.id = x.ancestor_id
            WHERE x.class_id = c.id
            ORDER BY x.depth DESC
        )
    ) AS full_name,
    (SELECT MAX(depth) FROM v_class_ancestry x WHERE x.class_id = c.id) AS depth,
    c.parent_id,
    c.is_active,
    c.created_at
FROM classes c;

CREATE TRIGGER trg_classes_hierarchy_ins
BEFORE INSERT ON classes
WHEN NEW.parent_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'CLASS_PARENT_NOT_FOUND: the parent class does not exist')
    WHERE NOT EXISTS (SELECT 1 FROM classes WHERE id = NEW.parent_id);
    SELECT RAISE(ABORT, 'CLASS_DEPTH_EXCEEDED: subclasses may nest at most 5 levels')
    WHERE (SELECT MAX(depth) FROM v_class_ancestry WHERE class_id = NEW.parent_id) >= 4;
END;

CREATE TRIGGER trg_classes_hierarchy_upd
BEFORE UPDATE OF parent_id ON classes
WHEN NEW.parent_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'CLASS_CYCLE: a class cannot be moved beneath itself or its own subclass')
    WHERE EXISTS (SELECT 1 FROM v_class_ancestry WHERE class_id = NEW.parent_id AND ancestor_id = OLD.id);
    SELECT RAISE(ABORT, 'CLASS_DEPTH_EXCEEDED: subclasses may nest at most 5 levels')
    WHERE (SELECT MAX(depth) FROM v_class_ancestry WHERE class_id = NEW.parent_id) + 1
        + (SELECT MAX(depth) FROM v_class_ancestry WHERE ancestor_id = OLD.id) > 4;
END;

-- ---------------------------------------------------------------------------------------------
-- The class on each line
-- ---------------------------------------------------------------------------------------------
ALTER TABLE journal_lines ADD COLUMN class_id INTEGER REFERENCES classes (id) ON DELETE RESTRICT;
CREATE INDEX ix_jl_class ON journal_lines (class_id) WHERE class_id IS NOT NULL;

ALTER TABLE invoice_items       ADD COLUMN class_id INTEGER REFERENCES classes (id) ON DELETE RESTRICT;
ALTER TABLE sales_receipt_lines ADD COLUMN class_id INTEGER REFERENCES classes (id) ON DELETE RESTRICT;
ALTER TABLE estimate_lines      ADD COLUMN class_id INTEGER REFERENCES classes (id) ON DELETE RESTRICT;
ALTER TABLE bill_lines          ADD COLUMN class_id INTEGER REFERENCES classes (id) ON DELETE RESTRICT;
ALTER TABLE expense_lines       ADD COLUMN class_id INTEGER REFERENCES classes (id) ON DELETE RESTRICT;

-- Reporting view: unchanged apart from the new class_id column.
DROP VIEW v_posted_lines;
CREATE VIEW v_posted_lines AS
SELECT
    l.id               AS line_id,
    e.id               AS journal_entry_id,
    e.entry_number,
    e.entry_date,
    e.source_type,
    e.source_id,
    e.is_adjusting,
    l.line_number,
    l.account_id,
    l.debit_cents,
    l.credit_cents,
    COALESCE(l.memo, e.memo) AS memo,
    l.class_id
FROM journal_lines l
JOIN journal_entries e ON e.id = l.journal_entry_id
WHERE e.status = 'POSTED';

-- ---------------------------------------------------------------------------------------------
-- Audit snapshot
-- ---------------------------------------------------------------------------------------------
-- A line's class is part of what was posted, so it joins the snapshot. It is added only to lines
-- that have one: every entry posted before classes existed keeps a byte-identical snapshot, so
-- `verify_ledger_audit` still matches the POST rows recorded for them.
DROP TRIGGER trg_ledger_audit_post;
DROP VIEW v_entry_snapshot;
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
                       CASE WHEN l.class_id IS NULL
                           THEN json_object(
                               'lineNumber',         l.line_number,
                               'accountId',          l.account_id,
                               'debitCents',         l.debit_cents,
                               'creditCents',        l.credit_cents,
                               'memo',               l.memo,
                               'foreignDebitCents',  l.foreign_debit_cents,
                               'foreignCreditCents', l.foreign_credit_cents
                           )
                           ELSE json_set(json_object(
                               'lineNumber',         l.line_number,
                               'accountId',          l.account_id,
                               'debitCents',         l.debit_cents,
                               'creditCents',        l.credit_cents,
                               'memo',               l.memo,
                               'foreignDebitCents',  l.foreign_debit_cents,
                               'foreignCreditCents', l.foreign_credit_cents
                           ), '$.classId', l.class_id)
                       END ORDER BY l.line_number)
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
