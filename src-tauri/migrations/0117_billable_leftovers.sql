-- 0117_billable_leftovers.sql
-- Billable expenses and time (T22).
--
-- 1. An expense account can name the income account its reimbursements go to, when Preferences ›
--    Time & Expenses counts reimbursed expenses as income (as the desktop product maps one per
--    expense account). Accounts without one use the income account chosen in Preferences.
-- 2. Billable time can be invoiced from Sales › Bill Costs. `billed_time` records which invoice
--    billed each time activity, as `billed_costs` does for costs: rows are only inserted, a voided
--    invoice frees its time to be billed again, and an edited invoice carries its links over.

ALTER TABLE chart_of_accounts ADD COLUMN reimbursement_income_account_id INTEGER REFERENCES chart_of_accounts (id) ON DELETE SET NULL;

CREATE TABLE billed_time (
    id                  INTEGER PRIMARY KEY,
    time_activity_id    INTEGER NOT NULL REFERENCES time_activities (id) ON DELETE RESTRICT,
    invoice_id          INTEGER NOT NULL REFERENCES invoices (id) ON DELETE RESTRICT,
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 0 AND 100000000000000),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX ix_billed_time_activity ON billed_time (time_activity_id);
CREATE INDEX ix_billed_time_invoice ON billed_time (invoice_id);

-- A billing record is never changed (Condense keeps the table whole, so it keeps its invoice too).
CREATE TRIGGER trg_billed_time_no_update BEFORE UPDATE ON billed_time
BEGIN
    SELECT RAISE(ABORT, 'TIME_BILLED_IMMUTABLE: a billing record cannot be changed');
END;
