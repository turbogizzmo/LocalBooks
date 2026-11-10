-- 0108_item_purchase_details.sql
-- The purchase side of an item (gap T43): the description purchase forms use, and what it costs
-- to buy. Both optional; the cost is in cents. Imports carry them through staging. A change to
-- either is a list change in the audit trail.

ALTER TABLE products_services ADD COLUMN purchase_description TEXT CHECK (purchase_description IS NULL OR length(purchase_description) <= 4000);
ALTER TABLE products_services ADD COLUMN purchase_cost_cents INTEGER CHECK (purchase_cost_cents IS NULL OR purchase_cost_cents BETWEEN 0 AND 100000000000000);

ALTER TABLE ledger_import_items ADD COLUMN purchase_description TEXT;
ALTER TABLE ledger_import_items ADD COLUMN purchase_cost_cents INTEGER;

CREATE TRIGGER trg_ledger_audit_item_purchase
AFTER UPDATE OF purchase_description, purchase_cost_cents ON products_services
WHEN NEW.purchase_description IS NOT OLD.purchase_description OR NEW.purchase_cost_cents IS NOT OLD.purchase_cost_cents
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'ITEM',
        'before', json_object('name', OLD.name, 'purchaseCostCents', OLD.purchase_cost_cents),
        'after',  json_object('name', NEW.name, 'purchaseCostCents', NEW.purchase_cost_cents),
        'changed', (SELECT json_group_array(f) FROM (
            SELECT 'purchase description' AS f WHERE NEW.purchase_description IS NOT OLD.purchase_description
            UNION ALL SELECT 'cost' WHERE NEW.purchase_cost_cents IS NOT OLD.purchase_cost_cents))));
END;
