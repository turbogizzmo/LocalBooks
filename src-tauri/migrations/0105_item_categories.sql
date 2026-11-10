-- 0105_item_categories.sql
-- Item categories (gap T40): a Category List with sub-categories, and a category on each item.
--
-- Categories group items for the item list, the item box on forms and the item reports; they post
-- nothing. An item with no category is "Uncategorized". A category is named once under its parent
-- (case ignored), can be made inactive, and can be deleted only while no item and no sub-category
-- uses it. Preferences › Items & Inventory turns categories off (`item_categories`), which hides
-- them without forgetting them.

CREATE TABLE item_categories (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 60 AND instr(name, ':') = 0),
    parent_id   INTEGER REFERENCES item_categories (id) ON DELETE RESTRICT,
    is_active   INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    CHECK (parent_id IS NULL OR parent_id <> id)
) STRICT;
CREATE UNIQUE INDEX ux_item_categories_name ON item_categories (COALESCE(parent_id, 0), name COLLATE NOCASE);

ALTER TABLE products_services ADD COLUMN category_id INTEGER REFERENCES item_categories (id) ON DELETE RESTRICT;
CREATE INDEX ix_products_category ON products_services (category_id) WHERE category_id IS NOT NULL;

-- A category can't sit under itself, however deep.
CREATE TRIGGER trg_item_category_no_cycle
BEFORE UPDATE OF parent_id ON item_categories
WHEN NEW.parent_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'CATEGORY_CYCLE: a category can''t go under itself or one of its sub-categories')
    WHERE EXISTS (
        WITH RECURSIVE up (id, depth) AS (
            SELECT NEW.parent_id, 0
            UNION ALL SELECT c.parent_id, up.depth + 1 FROM item_categories c JOIN up ON c.id = up.id
            WHERE c.parent_id IS NOT NULL AND up.depth < 64
        )
        SELECT 1 FROM up WHERE id = NEW.id
    );
END;

-- An item's category changing is a list change, in the audit trail like its other fields.
CREATE TRIGGER trg_ledger_audit_product_category
AFTER UPDATE OF category_id ON products_services
WHEN NEW.category_id IS NOT OLD.category_id
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'ITEM',
        'before', json_object('name', OLD.name, 'category', (SELECT name FROM item_categories WHERE id = OLD.category_id)),
        'after',  json_object('name', NEW.name, 'category', (SELECT name FROM item_categories WHERE id = NEW.category_id)),
        'changed', json_array('category')));
END;
