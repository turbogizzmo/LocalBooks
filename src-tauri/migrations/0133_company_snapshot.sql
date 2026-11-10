-- T61: the Company Snapshot page (`company_snapshot.rs`): which widgets it shows, in what order,
-- and each one's period. Kept in the company file, so everyone opening it sees the same page.
CREATE TABLE snapshot_widgets (
    id       INTEGER PRIMARY KEY,
    kind     TEXT    NOT NULL CHECK (kind IN ('ACCOUNT_BALANCES', 'WHO_OWES', 'VENDORS_TO_PAY', 'INCOME_EXPENSE_TREND', 'TOP_CUSTOMERS', 'TOP_ITEMS', 'TOP_VENDORS')),
    period   TEXT    NOT NULL CHECK (period IN ('TODAY', 'THIS_MONTH', 'LAST_MONTH', 'THIS_QUARTER', 'LAST_QUARTER', 'THIS_FISCAL_YEAR', 'LAST_FISCAL_YEAR', 'LAST_12_MONTHS')),
    position INTEGER NOT NULL
) STRICT;

-- A page to start from; the user adds, removes and moves widgets from there.
INSERT INTO snapshot_widgets (kind, period, position) VALUES
    ('ACCOUNT_BALANCES', 'TODAY', 1),
    ('WHO_OWES', 'TODAY', 2),
    ('VENDORS_TO_PAY', 'TODAY', 3),
    ('INCOME_EXPENSE_TREND', 'LAST_12_MONTHS', 4),
    ('TOP_CUSTOMERS', 'THIS_FISCAL_YEAR', 5);
