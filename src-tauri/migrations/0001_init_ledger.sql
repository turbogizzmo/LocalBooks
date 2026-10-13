-- 0001_init_ledger.sql
-- Core double-entry ledger: account types, chart of accounts, journal entries, journal lines.
--
-- Invariants enforced at the database layer (the Rust engine validates first; these are the backstop):
--   * All tables are STRICT: a REAL (float) can never be stored in a *_cents column.
--   * Every journal line is exactly one of debit or credit, positive, integer cents.
--   * An entry is created as DRAFT, lines are attached, then it is flipped to POSTED.
--     The flip is rejected unless SUM(debits) = SUM(credits) > 0 across at least two lines,
--     and every referenced account is active.
--   * POSTED and VOID entries (and their lines) are immutable. Corrections are made by voiding
--     or by posting a reversing entry, never by editing history.
--   * Subaccounts share their parent's account type, cannot form cycles, and nest at most
--     5 levels deep (QuickBooks' limit).
--
-- Connection requirement: PRAGMA foreign_keys = ON (SQLx's SqliteConnectOptions enables it by default).

-- ---------------------------------------------------------------------------------------------
-- Account types (reference data). Normal balance drives sign conventions for every report.
-- ---------------------------------------------------------------------------------------------
CREATE TABLE account_types (
    code            TEXT    PRIMARY KEY,
    display_name    TEXT    NOT NULL UNIQUE,
    classification  TEXT    NOT NULL CHECK (classification IN ('ASSET', 'LIABILITY', 'EQUITY', 'INCOME', 'EXPENSE')),
    normal_balance  TEXT    NOT NULL CHECK (normal_balance IN ('DEBIT', 'CREDIT')),
    is_balance_sheet INTEGER NOT NULL CHECK (is_balance_sheet IN (0, 1)),
    sort_order      INTEGER NOT NULL UNIQUE
) STRICT;

INSERT INTO account_types (code, display_name, classification, normal_balance, is_balance_sheet, sort_order) VALUES
    ('BANK',                    'Bank',                      'ASSET',     'DEBIT',  1,  10),
    ('ACCOUNTS_RECEIVABLE',     'Accounts Receivable',       'ASSET',     'DEBIT',  1,  20),
    ('OTHER_CURRENT_ASSET',     'Other Current Asset',       'ASSET',     'DEBIT',  1,  30),
    ('FIXED_ASSET',             'Fixed Asset',               'ASSET',     'DEBIT',  1,  40),
    ('OTHER_ASSET',             'Other Asset',               'ASSET',     'DEBIT',  1,  50),
    ('ACCOUNTS_PAYABLE',        'Accounts Payable',          'LIABILITY', 'CREDIT', 1,  60),
    ('CREDIT_CARD',             'Credit Card',               'LIABILITY', 'CREDIT', 1,  70),
    ('OTHER_CURRENT_LIABILITY', 'Other Current Liability',   'LIABILITY', 'CREDIT', 1,  80),
    ('LONG_TERM_LIABILITY',     'Long Term Liability',       'LIABILITY', 'CREDIT', 1,  90),
    ('EQUITY',                  'Equity',                    'EQUITY',    'CREDIT', 1, 100),
    ('INCOME',                  'Income',                    'INCOME',    'CREDIT', 0, 110),
    ('COST_OF_GOODS_SOLD',      'Cost of Goods Sold',        'EXPENSE',   'DEBIT',  0, 120),
    ('EXPENSE',                 'Expense',                   'EXPENSE',   'DEBIT',  0, 130),
    ('OTHER_INCOME',            'Other Income',              'INCOME',    'CREDIT', 0, 140),
    ('OTHER_EXPENSE',           'Other Expense',             'EXPENSE',   'DEBIT',  0, 150);

-- ---------------------------------------------------------------------------------------------
-- Chart of accounts
-- ---------------------------------------------------------------------------------------------
CREATE TABLE chart_of_accounts (
    id              INTEGER PRIMARY KEY,
    account_number  TEXT    UNIQUE CHECK (account_number IS NULL OR length(trim(account_number)) > 0),
    name            TEXT    NOT NULL CHECK (length(trim(name)) > 0 AND instr(name, ':') = 0),
    account_type    TEXT    NOT NULL REFERENCES account_types (code) ON UPDATE RESTRICT ON DELETE RESTRICT,
    parent_id       INTEGER REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    description     TEXT,
    is_active       INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    is_system       INTEGER NOT NULL DEFAULT 0 CHECK (is_system IN (0, 1)),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (parent_id IS NULL OR parent_id <> id)
) STRICT;

-- Sibling names are unique (case-insensitive), mirroring QuickBooks' "Parent:Child" naming.
CREATE UNIQUE INDEX ux_coa_sibling_name ON chart_of_accounts (COALESCE(parent_id, 0), name COLLATE NOCASE);
CREATE INDEX ix_coa_parent ON chart_of_accounts (parent_id);
CREATE INDEX ix_coa_type ON chart_of_accounts (account_type, is_active);

-- Ancestry closure: one row per (account, ancestor-or-self) with distance. Used by the hierarchy
-- triggers below and by the UI to build fully qualified "Parent:Child" names.
CREATE VIEW v_account_ancestry AS
WITH RECURSIVE walk (account_id, ancestor_id, depth) AS (
    SELECT id, id, 0 FROM chart_of_accounts
    UNION ALL
    SELECT w.account_id, a.parent_id, w.depth + 1
    FROM walk w
    JOIN chart_of_accounts a ON a.id = w.ancestor_id
    WHERE a.parent_id IS NOT NULL AND w.depth < 16
)
SELECT account_id, ancestor_id, depth FROM walk;

CREATE VIEW v_chart_of_accounts AS
SELECT
    a.id,
    a.account_number,
    a.name,
    (
        SELECT group_concat(name, ':')
        FROM (
            SELECT p.name
            FROM v_account_ancestry x
            JOIN chart_of_accounts p ON p.id = x.ancestor_id
            WHERE x.account_id = a.id
            ORDER BY x.depth DESC
        )
    ) AS full_name,
    (SELECT MAX(depth) FROM v_account_ancestry x WHERE x.account_id = a.id) AS depth,
    a.account_type,
    t.display_name AS account_type_name,
    t.classification,
    t.normal_balance,
    t.is_balance_sheet,
    t.sort_order AS type_sort_order,
    a.parent_id,
    a.description,
    a.is_active,
    a.is_system,
    a.created_at,
    a.updated_at
FROM chart_of_accounts a
JOIN account_types t ON t.code = a.account_type;

CREATE TRIGGER trg_coa_parent_same_type_ins
BEFORE INSERT ON chart_of_accounts
WHEN NEW.parent_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'COA_PARENT_NOT_FOUND: parent account does not exist')
    WHERE NOT EXISTS (SELECT 1 FROM chart_of_accounts WHERE id = NEW.parent_id);
    SELECT RAISE(ABORT, 'COA_PARENT_TYPE_MISMATCH: subaccount must have the same account type as its parent')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.parent_id) <> NEW.account_type;
    SELECT RAISE(ABORT, 'COA_DEPTH_EXCEEDED: subaccounts may nest at most 5 levels')
    WHERE (SELECT MAX(depth) FROM v_account_ancestry WHERE account_id = NEW.parent_id) >= 4;
END;

CREATE TRIGGER trg_coa_hierarchy_upd
BEFORE UPDATE OF parent_id, account_type ON chart_of_accounts
BEGIN
    SELECT RAISE(ABORT, 'COA_PARENT_TYPE_MISMATCH: subaccount must have the same account type as its parent')
    WHERE NEW.parent_id IS NOT NULL
      AND (SELECT account_type FROM chart_of_accounts WHERE id = NEW.parent_id) <> NEW.account_type;
    SELECT RAISE(ABORT, 'COA_CHILD_TYPE_MISMATCH: change subaccount types before changing the parent type')
    WHERE NEW.account_type <> OLD.account_type
      AND EXISTS (SELECT 1 FROM chart_of_accounts WHERE parent_id = OLD.id AND account_type <> NEW.account_type);
    SELECT RAISE(ABORT, 'COA_CYCLE: an account cannot be moved beneath itself or its own subaccount')
    WHERE NEW.parent_id IS NOT NULL
      AND EXISTS (SELECT 1 FROM v_account_ancestry WHERE account_id = NEW.parent_id AND ancestor_id = OLD.id);
    SELECT RAISE(ABORT, 'COA_DEPTH_EXCEEDED: subaccounts may nest at most 5 levels')
    WHERE NEW.parent_id IS NOT NULL
      AND (SELECT MAX(depth) FROM v_account_ancestry WHERE account_id = NEW.parent_id) + 1
        + (SELECT MAX(depth) FROM v_account_ancestry WHERE ancestor_id = OLD.id) > 4;
END;

CREATE TRIGGER trg_coa_system_protect_upd
BEFORE UPDATE ON chart_of_accounts
WHEN OLD.is_system = 1
 AND (NEW.account_type <> OLD.account_type OR NEW.is_active = 0 OR NEW.is_system = 0 OR NEW.parent_id IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'COA_SYSTEM_ACCOUNT: system accounts cannot be retyped, nested, or deactivated');
END;

CREATE TRIGGER trg_coa_system_protect_del
BEFORE DELETE ON chart_of_accounts
WHEN OLD.is_system = 1
BEGIN
    SELECT RAISE(ABORT, 'COA_SYSTEM_ACCOUNT: system accounts cannot be deleted');
END;

CREATE TRIGGER trg_coa_touch_updated_at
AFTER UPDATE ON chart_of_accounts
WHEN NEW.updated_at = OLD.updated_at
BEGIN
    UPDATE chart_of_accounts
    SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE id = NEW.id;
END;

-- System accounts QuickBooks creates automatically; the posting engine resolves them by type + is_system.
INSERT INTO chart_of_accounts (name, account_type, description, is_system) VALUES
    ('Accounts Receivable (A/R)', 'ACCOUNTS_RECEIVABLE',  'Unpaid customer invoices',                         1),
    ('Undeposited Funds',         'OTHER_CURRENT_ASSET',  'Customer payments received but not yet deposited', 1),
    ('Accounts Payable (A/P)',    'ACCOUNTS_PAYABLE',     'Unpaid vendor bills',                              1),
    ('Opening Balance Equity',    'EQUITY',               'Offset for opening balances entered at setup',     1),
    ('Retained Earnings',         'EQUITY',               'Accumulated net income from prior fiscal years',   1);

-- ---------------------------------------------------------------------------------------------
-- Journal entries (headers)
-- ---------------------------------------------------------------------------------------------
CREATE TABLE journal_entries (
    id                  INTEGER PRIMARY KEY,
    entry_number        INTEGER NOT NULL UNIQUE CHECK (entry_number > 0),
    entry_date          TEXT    NOT NULL CHECK (
                            entry_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(entry_date) = entry_date
                        ),
    memo                TEXT,
    source_type         TEXT    NOT NULL DEFAULT 'JOURNAL' CHECK (source_type IN (
                            'JOURNAL', 'INVOICE', 'RECEIVE_PAYMENT', 'SALES_RECEIPT', 'CREDIT_MEMO',
                            'BILL', 'BILL_PAYMENT', 'VENDOR_CREDIT', 'CHECK', 'EXPENSE',
                            'DEPOSIT', 'TRANSFER', 'OPENING_BALANCE'
                        )),
    source_id           INTEGER,
    reverses_entry_id   INTEGER UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    is_adjusting        INTEGER NOT NULL DEFAULT 0 CHECK (is_adjusting IN (0, 1)),
    status              TEXT    NOT NULL DEFAULT 'DRAFT' CHECK (status IN ('DRAFT', 'POSTED', 'VOID')),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    posted_at           TEXT,
    voided_at           TEXT,
    CHECK ((source_type = 'JOURNAL') = (source_id IS NULL)),
    CHECK ((status = 'DRAFT') = (posted_at IS NULL)),
    CHECK ((status = 'VOID') = (voided_at IS NOT NULL)),
    CHECK (reverses_entry_id IS NULL OR reverses_entry_id <> id)
) STRICT;

CREATE INDEX ix_je_date ON journal_entries (entry_date, id);
CREATE INDEX ix_je_status_date ON journal_entries (status, entry_date);
-- A source document (invoice, bill, ...) has at most one live ledger posting at a time.
CREATE UNIQUE INDEX ux_je_live_source ON journal_entries (source_type, source_id)
    WHERE source_id IS NOT NULL AND status <> 'VOID';

-- ---------------------------------------------------------------------------------------------
-- Journal lines (multi-split debits and credits)
-- ---------------------------------------------------------------------------------------------
CREATE TABLE journal_lines (
    id                  INTEGER PRIMARY KEY,
    journal_entry_id    INTEGER NOT NULL REFERENCES journal_entries (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    account_id          INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    debit_cents         INTEGER NOT NULL DEFAULT 0 CHECK (debit_cents  BETWEEN 0 AND 100000000000000),
    credit_cents        INTEGER NOT NULL DEFAULT 0 CHECK (credit_cents BETWEEN 0 AND 100000000000000),
    memo                TEXT,
    CHECK ((debit_cents > 0) + (credit_cents > 0) = 1),
    UNIQUE (journal_entry_id, line_number)
) STRICT;

CREATE INDEX ix_jl_account ON journal_lines (account_id, journal_entry_id);

-- ---------------------------------------------------------------------------------------------
-- Ledger integrity triggers
-- ---------------------------------------------------------------------------------------------
CREATE TRIGGER trg_je_insert_as_draft
BEFORE INSERT ON journal_entries
WHEN NEW.status <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'JE_INSERT_NOT_DRAFT: journal entries must be inserted as DRAFT, then posted');
END;

CREATE TRIGGER trg_je_status_transition
BEFORE UPDATE OF status ON journal_entries
WHEN NOT (
       (OLD.status = 'DRAFT'  AND NEW.status IN ('DRAFT', 'POSTED'))
    OR (OLD.status = 'POSTED' AND NEW.status IN ('POSTED', 'VOID'))
    OR (OLD.status = 'VOID'   AND NEW.status = 'VOID')
)
BEGIN
    SELECT RAISE(ABORT, 'JE_INVALID_TRANSITION: allowed transitions are DRAFT->POSTED and POSTED->VOID');
END;

CREATE TRIGGER trg_je_post_must_balance
BEFORE UPDATE OF status ON journal_entries
WHEN OLD.status = 'DRAFT' AND NEW.status = 'POSTED'
BEGIN
    SELECT RAISE(ABORT, 'JE_TOO_FEW_LINES: a journal entry requires at least two lines')
    WHERE (SELECT COUNT(*) FROM journal_lines WHERE journal_entry_id = NEW.id) < 2;
    SELECT RAISE(ABORT, 'JE_UNBALANCED: total debits must equal total credits')
    WHERE (SELECT SUM(debit_cents) - SUM(credit_cents) FROM journal_lines WHERE journal_entry_id = NEW.id) <> 0;
    SELECT RAISE(ABORT, 'JE_INACTIVE_ACCOUNT: cannot post to an inactive account')
    WHERE EXISTS (
        SELECT 1
        FROM journal_lines l
        JOIN chart_of_accounts a ON a.id = l.account_id
        WHERE l.journal_entry_id = NEW.id AND a.is_active = 0
    );
    SELECT RAISE(ABORT, 'JE_REVERSAL_TARGET_NOT_POSTED: only a POSTED entry can be reversed')
    WHERE NEW.reverses_entry_id IS NOT NULL
      AND (SELECT status FROM journal_entries WHERE id = NEW.reverses_entry_id) <> 'POSTED';
END;

CREATE TRIGGER trg_je_posted_header_immutable
BEFORE UPDATE ON journal_entries
WHEN OLD.status <> 'DRAFT'
 AND (   NEW.entry_number      IS NOT OLD.entry_number
      OR NEW.entry_date        IS NOT OLD.entry_date
      OR NEW.source_type       IS NOT OLD.source_type
      OR NEW.source_id         IS NOT OLD.source_id
      OR NEW.reverses_entry_id IS NOT OLD.reverses_entry_id
      OR NEW.is_adjusting      IS NOT OLD.is_adjusting
      OR NEW.posted_at         IS NOT OLD.posted_at
      OR NEW.created_at        IS NOT OLD.created_at
      OR (OLD.status = 'VOID' AND NEW.voided_at IS NOT OLD.voided_at))
BEGIN
    SELECT RAISE(ABORT, 'JE_IMMUTABLE: posted or voided entries cannot be altered; void or reverse instead');
END;

CREATE TRIGGER trg_je_no_delete_posted
BEFORE DELETE ON journal_entries
WHEN OLD.status <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'JE_IMMUTABLE: posted or voided entries cannot be deleted; void or reverse instead');
END;

CREATE TRIGGER trg_jl_insert_draft_only
BEFORE INSERT ON journal_lines
WHEN (SELECT status FROM journal_entries WHERE id = NEW.journal_entry_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'JE_IMMUTABLE: lines can only be added to a DRAFT entry');
END;

CREATE TRIGGER trg_jl_update_draft_only
BEFORE UPDATE ON journal_lines
WHEN (SELECT status FROM journal_entries WHERE id = OLD.journal_entry_id) <> 'DRAFT'
  OR (SELECT status FROM journal_entries WHERE id = NEW.journal_entry_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'JE_IMMUTABLE: lines can only be changed on a DRAFT entry');
END;

CREATE TRIGGER trg_jl_delete_draft_only
BEFORE DELETE ON journal_lines
WHEN (SELECT status FROM journal_entries WHERE id = OLD.journal_entry_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'JE_IMMUTABLE: lines can only be removed from a DRAFT entry');
END;

-- ---------------------------------------------------------------------------------------------
-- Reporting views (POSTED entries only; VOID and DRAFT never touch balances)
-- ---------------------------------------------------------------------------------------------
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
    COALESCE(l.memo, e.memo) AS memo
FROM journal_lines l
JOIN journal_entries e ON e.id = l.journal_entry_id
WHERE e.status = 'POSTED';

-- balance_cents is signed by the account type's normal balance: positive = normal side.
CREATE VIEW v_account_balances AS
SELECT
    a.id                          AS account_id,
    a.account_number,
    a.name,
    a.account_type,
    t.classification,
    t.normal_balance,
    COALESCE(s.total_debit_cents, 0)  AS total_debit_cents,
    COALESCE(s.total_credit_cents, 0) AS total_credit_cents,
    CASE t.normal_balance
        WHEN 'DEBIT'  THEN COALESCE(s.total_debit_cents, 0)  - COALESCE(s.total_credit_cents, 0)
        ELSE               COALESCE(s.total_credit_cents, 0) - COALESCE(s.total_debit_cents, 0)
    END                           AS balance_cents
FROM chart_of_accounts a
JOIN account_types t ON t.code = a.account_type
LEFT JOIN (
    SELECT account_id, SUM(debit_cents) AS total_debit_cents, SUM(credit_cents) AS total_credit_cents
    FROM v_posted_lines
    GROUP BY account_id
) s ON s.account_id = a.id;
