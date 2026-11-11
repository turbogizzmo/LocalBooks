-- T70: sales tax adjustments (`sales_tax/adjustments.rs`). An adjustment raises or lowers what is
-- owed on one rate (not a combined rate): it posts to that rate's liability account against an
-- income or expense account the user chooses, and the sales tax return lists it under the rate's
-- agency for its date. Voiding its journal entry takes it off the return.
CREATE TABLE sales_tax_adjustments (
    id                   INTEGER PRIMARY KEY,
    tax_rate_id          INTEGER NOT NULL REFERENCES tax_rates (id) ON DELETE RESTRICT,
    adjustment_date      TEXT    NOT NULL CHECK (date(adjustment_date) = adjustment_date),
    -- Positive raises what is owed (Cr the liability); negative lowers it (Dr the liability).
    amount_cents         INTEGER NOT NULL CHECK (amount_cents <> 0 AND amount_cents BETWEEN -100000000000000 AND 100000000000000),
    liability_account_id INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    account_id           INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    memo                 TEXT,
    journal_entry_id     INTEGER NOT NULL UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    created_at           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TRIGGER trg_sta_immutable
BEFORE UPDATE ON sales_tax_adjustments
BEGIN
    SELECT RAISE(ABORT, 'TAX_ADJUSTMENT_IMMUTABLE: a posted sales tax adjustment cannot be altered; void it instead');
END;
