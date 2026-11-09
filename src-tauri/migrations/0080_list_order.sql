-- 0080_list_order.sql
-- Lists in a custom order (gap T65): the chart of accounts, items and customers can be dragged
-- into an order that is kept. `list_order` is the row's place among its siblings (0 first); NULL
-- means the list's usual order (after any rows that have a place). Only list_order.rs sets it.

ALTER TABLE chart_of_accounts ADD COLUMN list_order INTEGER CHECK (list_order IS NULL OR list_order >= 0);
ALTER TABLE products_services ADD COLUMN list_order INTEGER CHECK (list_order IS NULL OR list_order >= 0);
ALTER TABLE customers ADD COLUMN list_order INTEGER CHECK (list_order IS NULL OR list_order >= 0);
