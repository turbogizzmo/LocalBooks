-- The day of the month a monthly, quarterly or yearly schedule falls on (`workflows.rs`,
-- `advance`). A schedule from the 31st lands on the last day of shorter months and goes back to
-- the 31st after them (Jan 31, Feb 28, Mar 31), instead of staying on the 28th.
ALTER TABLE recurring_templates ADD COLUMN anchor_day INTEGER CHECK (anchor_day IS NULL OR anchor_day BETWEEN 1 AND 31);

-- Existing schedules: the day of their first run (before any clamping), or of their next date.
UPDATE recurring_templates
SET anchor_day = CAST(strftime('%d', COALESCE((SELECT MIN(r.run_date) FROM recurring_runs r WHERE r.template_id = recurring_templates.id), next_date)) AS INTEGER);
