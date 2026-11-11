-- The statement period an emailed statement covered (its end date, the statement date), so Customer
-- Statements marks a customer's statement sent for that period only (`email_log.rs`). Other forms,
-- and statements sent before this, have none.
ALTER TABLE form_email_log ADD COLUMN period_end TEXT CHECK (period_end IS NULL OR date(period_end) = period_end);
