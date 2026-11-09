-- 0092_item_reorder_vendor.sql
-- An item's reorder point (quantity × 10,000, as stock quantities are) and preferred vendor, kept
-- on the product or service record (T56 shows them on the item's information panel; T34 will
-- remind when stock falls to the reorder point). Both optional. A preferred vendor that is
-- deleted (only possible when it has no transactions) clears the choice.

ALTER TABLE products_services ADD COLUMN reorder_point_e4 INTEGER
    CHECK (reorder_point_e4 IS NULL OR reorder_point_e4 BETWEEN 0 AND 10000000000000);
ALTER TABLE products_services ADD COLUMN preferred_vendor_id INTEGER REFERENCES vendors (id) ON DELETE SET NULL;
