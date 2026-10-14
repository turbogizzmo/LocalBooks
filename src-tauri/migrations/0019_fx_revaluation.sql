-- Period-end revaluation of open foreign-currency receivables.
--
-- A revaluation restates each open foreign invoice's receivable at a period-end rate. The difference
-- is posted on the revaluation date to Unrealized Exchange Gain or Loss and reversed the next day, so
-- the books keep each invoice at its booked rate and the realized gain or loss is still worked out
-- correctly when the customer pays.

INSERT INTO chart_of_accounts (account_number, name, account_type, description, is_system)
SELECT CASE WHEN EXISTS (SELECT 1 FROM chart_of_accounts WHERE account_number = '8110') THEN NULL ELSE '8110' END,
       'Unrealized Exchange Gain or Loss', 'OTHER_EXPENSE',
       'Period-end revaluation of open foreign-currency balances; reversed the next day', 1
WHERE NOT EXISTS (SELECT 1 FROM chart_of_accounts WHERE parent_id IS NULL AND name = 'Unrealized Exchange Gain or Loss' COLLATE NOCASE);

CREATE TABLE fx_revaluations (
    id                  INTEGER PRIMARY KEY,
    as_of               TEXT    NOT NULL CHECK (date(as_of) = as_of),
    -- {"EUR": 1100000, ...}: home units per foreign unit, scaled by 1,000,000.
    rates               TEXT    NOT NULL CHECK (json_valid(rates)),
    net_change_cents    INTEGER NOT NULL CHECK (net_change_cents <> 0),
    journal_entry_id    INTEGER NOT NULL UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    reversal_entry_id   INTEGER NOT NULL UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TRIGGER trg_fx_revaluations_no_update BEFORE UPDATE ON fx_revaluations
BEGIN
    SELECT RAISE(ABORT, 'FX_REVAL_IMMUTABLE: a revaluation cannot be changed; void it instead');
END;

CREATE TRIGGER trg_fx_revaluations_no_delete BEFORE DELETE ON fx_revaluations
BEGIN
    SELECT RAISE(ABORT, 'FX_REVAL_IMMUTABLE: a revaluation cannot be changed; void it instead');
END;
