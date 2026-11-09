-- 0094_payment_reminders.sql
-- Payment reminders by customer group (gap T35): every so many days, a group's invoices overdue by
-- at least `payment_min_days_overdue` days come up in Reminders to review; sending them queues
-- each invoice for Email later and starts the wait for the next round.

ALTER TABLE customer_groups ADD COLUMN payment_every_days INTEGER
    CHECK (payment_every_days IS NULL OR payment_every_days BETWEEN 1 AND 366);
ALTER TABLE customer_groups ADD COLUMN payment_min_days_overdue INTEGER NOT NULL DEFAULT 1
    CHECK (payment_min_days_overdue BETWEEN 0 AND 365);
ALTER TABLE customer_groups ADD COLUMN payment_last_sent TEXT
    CHECK (payment_last_sent IS NULL OR date(payment_last_sent) IS payment_last_sent);
