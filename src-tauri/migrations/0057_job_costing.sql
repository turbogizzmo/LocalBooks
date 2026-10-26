-- Job costing: a customer or job on each line of a bill, expense (check, card charge or card
-- credit) and vendor credit, so a cost can be charged to the job it was bought for. The job
-- costing reports (`job_reports.rs`) read it; it never changes how a document posts.

ALTER TABLE bill_lines          ADD COLUMN customer_id INTEGER REFERENCES customers (id) ON DELETE RESTRICT;
ALTER TABLE expense_lines       ADD COLUMN customer_id INTEGER REFERENCES customers (id) ON DELETE RESTRICT;
ALTER TABLE vendor_credit_lines ADD COLUMN customer_id INTEGER REFERENCES customers (id) ON DELETE RESTRICT;

CREATE INDEX ix_bill_lines_customer          ON bill_lines (customer_id)          WHERE customer_id IS NOT NULL;
CREATE INDEX ix_expense_lines_customer       ON expense_lines (customer_id)       WHERE customer_id IS NOT NULL;
CREATE INDEX ix_vendor_credit_lines_customer ON vendor_credit_lines (customer_id) WHERE customer_id IS NOT NULL;
