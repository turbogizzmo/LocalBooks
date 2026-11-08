-- 0078_deposit_cash_back.sql
-- Cash back on a deposit (gap T29).
--
-- Part of a deposit can be kept as cash instead of going into the bank, such as petty cash taken
-- when depositing checks. The deposit's `total_cents` stays what was put in (its lines); the bank
-- is debited with that less `cash_back_cents`, and the cash back account with `cash_back_cents`.
-- Imported deposits whose deductions came from the company file keep them in their entry only.

ALTER TABLE deposits ADD COLUMN cash_back_account_id INTEGER REFERENCES chart_of_accounts (id) ON DELETE RESTRICT;
ALTER TABLE deposits ADD COLUMN cash_back_cents INTEGER NOT NULL DEFAULT 0 CHECK (cash_back_cents >= 0);
ALTER TABLE deposits ADD COLUMN cash_back_memo TEXT;
