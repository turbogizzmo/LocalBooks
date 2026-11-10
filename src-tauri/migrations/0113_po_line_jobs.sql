-- 0113_po_line_jobs.sql
-- Gap T9: a purchase order line can name the customer or job it is for, as bill lines can, so
-- Open Purchase Orders by Job can group the open orders. The job carries over to the bill made
-- from the order. NULL: no customer or job.

ALTER TABLE purchase_order_lines ADD COLUMN customer_id INTEGER REFERENCES customers (id) ON DELETE RESTRICT;
CREATE INDEX ix_purchase_order_lines_customer ON purchase_order_lines (customer_id) WHERE customer_id IS NOT NULL;
