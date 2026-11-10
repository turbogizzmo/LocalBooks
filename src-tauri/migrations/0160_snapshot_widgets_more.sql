-- 0160_snapshot_widgets_more.sql
-- Company Snapshot, the rest of gap T61: five more widget kinds (a note, open to-dos, where income
-- comes from, where the money goes, and this period against the same one a year earlier), and the
-- note widget's text. The kind list is a CHECK on a STRICT table, so the table is rebuilt with the
-- same rows and positions.

CREATE TABLE snapshot_widgets_new (
    id       INTEGER PRIMARY KEY,
    kind     TEXT    NOT NULL CHECK (kind IN ('ACCOUNT_BALANCES', 'WHO_OWES', 'VENDORS_TO_PAY', 'INCOME_EXPENSE_TREND', 'TOP_CUSTOMERS', 'TOP_ITEMS', 'TOP_VENDORS',
                                               'NOTES', 'TODOS', 'INCOME_BREAKDOWN', 'EXPENSE_BREAKDOWN', 'YEAR_OVER_YEAR')),
    period   TEXT    NOT NULL CHECK (period IN ('TODAY', 'THIS_MONTH', 'LAST_MONTH', 'THIS_QUARTER', 'LAST_QUARTER', 'THIS_FISCAL_YEAR', 'LAST_FISCAL_YEAR', 'LAST_12_MONTHS')),
    position INTEGER NOT NULL,
    -- The note widget's text (blank for the others).
    note     TEXT    CHECK (note IS NULL OR length(note) <= 4000)
) STRICT;

INSERT INTO snapshot_widgets_new (id, kind, period, position) SELECT id, kind, period, position FROM snapshot_widgets;
DROP TABLE snapshot_widgets;
ALTER TABLE snapshot_widgets_new RENAME TO snapshot_widgets;
