-- Item types on sales forms: subtotals, discounts, other charges and groups.
-- The design is docs/plans/item-types.md.
--
-- * products_services.item_type: ITEM (a service or part, as before), OTHER_CHARGE or DISCOUNT.
--   Allowed values are checked by triggers, so a later migration can add types by replacing them.
--   percent_ppm makes a charge or discount a percentage of the line above (parts per million);
--   otherwise the item's rate is its amount.
-- * item_groups / item_group_members: a named set of items, with quantities, that a sales form
--   expands into its lines. A group may print as one line.
-- * percent_ppm on the item line tables: a percent charge is an ordinary positive line at
--   quantity 1; the column keeps its percent for the form and the printout.
-- * <document>_form_lines: subtotal, discount and group heading lines, which have no place in the
--   item line tables (zero or negative amounts). They share line numbering with the item lines.
--   Discount lines are negative and count in the document total.

-- ---------------------------------------------------------------------------------------------
-- The item list
-- ---------------------------------------------------------------------------------------------
ALTER TABLE products_services ADD COLUMN item_type TEXT NOT NULL DEFAULT 'ITEM';
ALTER TABLE products_services ADD COLUMN percent_ppm INTEGER;

CREATE TRIGGER trg_product_item_type
BEFORE INSERT ON products_services
BEGIN
    SELECT RAISE(ABORT, 'ITEM_BAD_TYPE: an item is an ITEM, OTHER_CHARGE or DISCOUNT')
    WHERE NEW.item_type NOT IN ('ITEM', 'OTHER_CHARGE', 'DISCOUNT');
    SELECT RAISE(ABORT, 'ITEM_BAD_PERCENT: only a charge or a discount can be a percentage, from 0.0001% to 100%')
    WHERE NEW.percent_ppm IS NOT NULL
      AND (NEW.item_type = 'ITEM' OR typeof(NEW.percent_ppm) <> 'integer' OR NEW.percent_ppm NOT BETWEEN 1 AND 1000000);
    SELECT RAISE(ABORT, 'ITEM_GROUP_NAME: a group already has that name')
    WHERE EXISTS (SELECT 1 FROM item_groups WHERE name = NEW.name COLLATE NOCASE);
END;

CREATE TRIGGER trg_product_item_type_upd
BEFORE UPDATE OF item_type, percent_ppm, name ON products_services
BEGIN
    SELECT RAISE(ABORT, 'ITEM_BAD_TYPE: an item is an ITEM, OTHER_CHARGE or DISCOUNT')
    WHERE NEW.item_type NOT IN ('ITEM', 'OTHER_CHARGE', 'DISCOUNT');
    SELECT RAISE(ABORT, 'ITEM_BAD_PERCENT: only a charge or a discount can be a percentage, from 0.0001% to 100%')
    WHERE NEW.percent_ppm IS NOT NULL
      AND (NEW.item_type = 'ITEM' OR typeof(NEW.percent_ppm) <> 'integer' OR NEW.percent_ppm NOT BETWEEN 1 AND 1000000);
    SELECT RAISE(ABORT, 'ITEM_TYPE_TRACKED: an item tracked in stock stays an ITEM')
    WHERE NEW.item_type <> 'ITEM' AND EXISTS (SELECT 1 FROM inventory_items WHERE product_id = NEW.id);
    SELECT RAISE(ABORT, 'ITEM_TYPE_IN_GROUP: an item in a group is an ITEM or a fixed charge')
    WHERE (NEW.item_type = 'DISCOUNT' OR NEW.percent_ppm IS NOT NULL)
      AND EXISTS (SELECT 1 FROM item_group_members WHERE product_id = NEW.id);
    SELECT RAISE(ABORT, 'ITEM_GROUP_NAME: a group already has that name')
    WHERE NEW.name IS NOT OLD.name AND EXISTS (SELECT 1 FROM item_groups WHERE name = NEW.name COLLATE NOCASE);
END;

CREATE TRIGGER trg_inventory_item_is_item
BEFORE INSERT ON inventory_items
WHEN (SELECT item_type FROM products_services WHERE id = NEW.product_id) <> 'ITEM'
BEGIN
    SELECT RAISE(ABORT, 'STOCK_NOT_AN_ITEM: only an ITEM can be tracked in stock, not a charge or discount');
END;

CREATE TRIGGER trg_price_level_item_not_discount
BEFORE INSERT ON price_level_items
WHEN (SELECT item_type FROM products_services WHERE id = NEW.product_id) = 'DISCOUNT'
BEGIN
    SELECT RAISE(ABORT, 'PRICE_LEVEL_DISCOUNT: price levels do not apply to discounts');
END;

-- ---------------------------------------------------------------------------------------------
-- Groups
-- ---------------------------------------------------------------------------------------------
CREATE TABLE item_groups (
    id              INTEGER PRIMARY KEY,
    name            TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    description     TEXT,
    -- 0: the printed form shows the group as one line (description, quantity, total).
    print_items     INTEGER NOT NULL DEFAULT 1 CHECK (print_items IN (0, 1)),
    is_active       INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_item_groups_name ON item_groups (name COLLATE NOCASE);

CREATE TRIGGER trg_item_group_name
BEFORE INSERT ON item_groups
WHEN EXISTS (SELECT 1 FROM products_services WHERE name = NEW.name COLLATE NOCASE)
BEGIN
    SELECT RAISE(ABORT, 'ITEM_GROUP_NAME: an item already has that name');
END;

CREATE TRIGGER trg_item_group_name_upd
BEFORE UPDATE OF name ON item_groups
WHEN EXISTS (SELECT 1 FROM products_services WHERE name = NEW.name COLLATE NOCASE)
BEGIN
    SELECT RAISE(ABORT, 'ITEM_GROUP_NAME: an item already has that name');
END;

CREATE TABLE item_group_members (
    group_id        INTEGER NOT NULL REFERENCES item_groups (id) ON DELETE CASCADE,
    position        INTEGER NOT NULL CHECK (position > 0),
    product_id      INTEGER NOT NULL REFERENCES products_services (id) ON DELETE RESTRICT,
    quantity_e4     INTEGER NOT NULL CHECK (quantity_e4 BETWEEN 1 AND 10000000000000),
    PRIMARY KEY (group_id, position)
) STRICT;
CREATE INDEX ix_item_group_members_product ON item_group_members (product_id);

CREATE TRIGGER trg_item_group_member_type
BEFORE INSERT ON item_group_members
WHEN (SELECT item_type = 'DISCOUNT' OR percent_ppm IS NOT NULL FROM products_services WHERE id = NEW.product_id)
BEGIN
    SELECT RAISE(ABORT, 'ITEM_TYPE_IN_GROUP: a group holds items and fixed charges, not discounts or percent charges');
END;

-- ---------------------------------------------------------------------------------------------
-- Percent charges on the item line tables
-- ---------------------------------------------------------------------------------------------
ALTER TABLE invoice_items       ADD COLUMN percent_ppm INTEGER CHECK (percent_ppm IS NULL OR percent_ppm BETWEEN 1 AND 1000000);
ALTER TABLE sales_receipt_lines ADD COLUMN percent_ppm INTEGER CHECK (percent_ppm IS NULL OR percent_ppm BETWEEN 1 AND 1000000);
ALTER TABLE estimate_lines      ADD COLUMN percent_ppm INTEGER CHECK (percent_ppm IS NULL OR percent_ppm BETWEEN 1 AND 1000000);

-- ---------------------------------------------------------------------------------------------
-- Form lines: subtotals, discounts and group headings
-- ---------------------------------------------------------------------------------------------
-- Each kind uses its own columns and leaves the others NULL (or 0):
--   SUBTOTAL  amount_cents = the sum of the lines since the previous subtotal (>= 0)
--   DISCOUNT  amount_cents < 0, debited to account_id; product_service_id is the discount item,
--             percent_ppm set when it is a percentage of the line above
--   GROUP     amount_cents = the group's total; group_line_count item lines follow and belong to it
CREATE TABLE invoice_form_lines (
    id                  INTEGER PRIMARY KEY,
    invoice_id          INTEGER NOT NULL REFERENCES invoices (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    kind                TEXT    NOT NULL CHECK (kind IN ('SUBTOTAL', 'DISCOUNT', 'GROUP')),
    description         TEXT    NOT NULL,
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN -100000000000000 AND 100000000000000),
    product_service_id  INTEGER REFERENCES products_services (id) ON DELETE RESTRICT,
    percent_ppm         INTEGER CHECK (percent_ppm IS NULL OR percent_ppm BETWEEN 1 AND 1000000),
    account_id          INTEGER REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    class_id            INTEGER REFERENCES classes (id) ON DELETE RESTRICT,
    is_taxable          INTEGER NOT NULL DEFAULT 0 CHECK (is_taxable IN (0, 1)),
    item_group_id       INTEGER REFERENCES item_groups (id) ON DELETE RESTRICT,
    group_quantity_e4   INTEGER CHECK (group_quantity_e4 IS NULL OR group_quantity_e4 > 0),
    group_line_count    INTEGER CHECK (group_line_count IS NULL OR group_line_count > 0),
    print_items         INTEGER CHECK (print_items IS NULL OR print_items IN (0, 1)),
    CHECK (kind <> 'SUBTOTAL' OR (amount_cents >= 0 AND product_service_id IS NULL AND percent_ppm IS NULL AND account_id IS NULL
                                  AND class_id IS NULL AND is_taxable = 0 AND item_group_id IS NULL AND group_line_count IS NULL)),
    CHECK (kind <> 'DISCOUNT' OR (amount_cents < 0 AND account_id IS NOT NULL AND item_group_id IS NULL AND group_line_count IS NULL)),
    CHECK (kind <> 'GROUP' OR (amount_cents >= 0 AND group_line_count IS NOT NULL AND print_items IS NOT NULL AND product_service_id IS NULL
                               AND percent_ppm IS NULL AND account_id IS NULL AND class_id IS NULL AND is_taxable = 0)),
    UNIQUE (invoice_id, line_number)
) STRICT;
CREATE INDEX ix_invoice_form_lines_account ON invoice_form_lines (account_id);

CREATE TABLE sales_receipt_form_lines (
    id                  INTEGER PRIMARY KEY,
    sales_receipt_id    INTEGER NOT NULL REFERENCES sales_receipts (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    kind                TEXT    NOT NULL CHECK (kind IN ('SUBTOTAL', 'DISCOUNT', 'GROUP')),
    description         TEXT    NOT NULL,
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN -100000000000000 AND 100000000000000),
    product_service_id  INTEGER REFERENCES products_services (id) ON DELETE RESTRICT,
    percent_ppm         INTEGER CHECK (percent_ppm IS NULL OR percent_ppm BETWEEN 1 AND 1000000),
    account_id          INTEGER REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    class_id            INTEGER REFERENCES classes (id) ON DELETE RESTRICT,
    is_taxable          INTEGER NOT NULL DEFAULT 0 CHECK (is_taxable IN (0, 1)),
    item_group_id       INTEGER REFERENCES item_groups (id) ON DELETE RESTRICT,
    group_quantity_e4   INTEGER CHECK (group_quantity_e4 IS NULL OR group_quantity_e4 > 0),
    group_line_count    INTEGER CHECK (group_line_count IS NULL OR group_line_count > 0),
    print_items         INTEGER CHECK (print_items IS NULL OR print_items IN (0, 1)),
    CHECK (kind <> 'SUBTOTAL' OR (amount_cents >= 0 AND product_service_id IS NULL AND percent_ppm IS NULL AND account_id IS NULL
                                  AND class_id IS NULL AND is_taxable = 0 AND item_group_id IS NULL AND group_line_count IS NULL)),
    CHECK (kind <> 'DISCOUNT' OR (amount_cents < 0 AND account_id IS NOT NULL AND item_group_id IS NULL AND group_line_count IS NULL)),
    CHECK (kind <> 'GROUP' OR (amount_cents >= 0 AND group_line_count IS NOT NULL AND print_items IS NOT NULL AND product_service_id IS NULL
                               AND percent_ppm IS NULL AND account_id IS NULL AND class_id IS NULL AND is_taxable = 0)),
    UNIQUE (sales_receipt_id, line_number)
) STRICT;
CREATE INDEX ix_sales_receipt_form_lines_account ON sales_receipt_form_lines (account_id);

CREATE TABLE estimate_form_lines (
    id                  INTEGER PRIMARY KEY,
    estimate_id         INTEGER NOT NULL REFERENCES estimates (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    kind                TEXT    NOT NULL CHECK (kind IN ('SUBTOTAL', 'DISCOUNT', 'GROUP')),
    description         TEXT    NOT NULL,
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN -100000000000000 AND 100000000000000),
    product_service_id  INTEGER REFERENCES products_services (id) ON DELETE RESTRICT,
    percent_ppm         INTEGER CHECK (percent_ppm IS NULL OR percent_ppm BETWEEN 1 AND 1000000),
    account_id          INTEGER REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    class_id            INTEGER REFERENCES classes (id) ON DELETE RESTRICT,
    is_taxable          INTEGER NOT NULL DEFAULT 0 CHECK (is_taxable IN (0, 1)),
    item_group_id       INTEGER REFERENCES item_groups (id) ON DELETE RESTRICT,
    group_quantity_e4   INTEGER CHECK (group_quantity_e4 IS NULL OR group_quantity_e4 > 0),
    group_line_count    INTEGER CHECK (group_line_count IS NULL OR group_line_count > 0),
    print_items         INTEGER CHECK (print_items IS NULL OR print_items IN (0, 1)),
    CHECK (kind <> 'SUBTOTAL' OR (amount_cents >= 0 AND product_service_id IS NULL AND percent_ppm IS NULL AND account_id IS NULL
                                  AND class_id IS NULL AND is_taxable = 0 AND item_group_id IS NULL AND group_line_count IS NULL)),
    CHECK (kind <> 'DISCOUNT' OR (amount_cents < 0 AND account_id IS NOT NULL AND item_group_id IS NULL AND group_line_count IS NULL)),
    CHECK (kind <> 'GROUP' OR (amount_cents >= 0 AND group_line_count IS NOT NULL AND print_items IS NOT NULL AND product_service_id IS NULL
                               AND percent_ppm IS NULL AND account_id IS NULL AND class_id IS NULL AND is_taxable = 0)),
    UNIQUE (estimate_id, line_number)
) STRICT;

-- Form lines and item lines share one numbering.
CREATE TRIGGER trg_inv_form_lines_number
BEFORE INSERT ON invoice_form_lines
WHEN EXISTS (SELECT 1 FROM invoice_items WHERE invoice_id = NEW.invoice_id AND line_number = NEW.line_number)
BEGIN
    SELECT RAISE(ABORT, 'ITEMS_LINE_NUMBER: that line number is already an item line');
END;
CREATE TRIGGER trg_inv_items_form_number
BEFORE INSERT ON invoice_items
WHEN EXISTS (SELECT 1 FROM invoice_form_lines WHERE invoice_id = NEW.invoice_id AND line_number = NEW.line_number)
BEGIN
    SELECT RAISE(ABORT, 'ITEMS_LINE_NUMBER: that line number is already a form line');
END;
CREATE TRIGGER trg_sr_form_lines_number
BEFORE INSERT ON sales_receipt_form_lines
WHEN EXISTS (SELECT 1 FROM sales_receipt_lines WHERE sales_receipt_id = NEW.sales_receipt_id AND line_number = NEW.line_number)
BEGIN
    SELECT RAISE(ABORT, 'ITEMS_LINE_NUMBER: that line number is already an item line');
END;
CREATE TRIGGER trg_sr_lines_form_number
BEFORE INSERT ON sales_receipt_lines
WHEN EXISTS (SELECT 1 FROM sales_receipt_form_lines WHERE sales_receipt_id = NEW.sales_receipt_id AND line_number = NEW.line_number)
BEGIN
    SELECT RAISE(ABORT, 'ITEMS_LINE_NUMBER: that line number is already a form line');
END;
CREATE TRIGGER trg_est_form_lines_number
BEFORE INSERT ON estimate_form_lines
WHEN EXISTS (SELECT 1 FROM estimate_lines WHERE estimate_id = NEW.estimate_id AND line_number = NEW.line_number)
BEGIN
    SELECT RAISE(ABORT, 'ITEMS_LINE_NUMBER: that line number is already an item line');
END;
CREATE TRIGGER trg_est_lines_form_number
BEFORE INSERT ON estimate_lines
WHEN EXISTS (SELECT 1 FROM estimate_form_lines WHERE estimate_id = NEW.estimate_id AND line_number = NEW.line_number)
BEGIN
    SELECT RAISE(ABORT, 'ITEMS_LINE_NUMBER: that line number is already a form line');
END;

-- Immutability, as for each document's item lines.
CREATE TRIGGER trg_inv_form_lines_insert_draft_only
BEFORE INSERT ON invoice_form_lines
WHEN (SELECT status FROM invoices WHERE id = NEW.invoice_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: lines can only be added to a DRAFT invoice');
END;
CREATE TRIGGER trg_inv_form_lines_update_draft_only
BEFORE UPDATE ON invoice_form_lines
WHEN (SELECT status FROM invoices WHERE id = OLD.invoice_id) <> 'DRAFT'
  OR (SELECT status FROM invoices WHERE id = NEW.invoice_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: lines can only be changed on a DRAFT invoice');
END;
CREATE TRIGGER trg_inv_form_lines_delete_draft_only
BEFORE DELETE ON invoice_form_lines
WHEN (SELECT status FROM invoices WHERE id = OLD.invoice_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: lines can only be removed from a DRAFT invoice');
END;

CREATE TRIGGER trg_sr_form_lines_insert_unposted
BEFORE INSERT ON sales_receipt_form_lines
WHEN (SELECT journal_entry_id FROM sales_receipts WHERE id = NEW.sales_receipt_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'SR_IMMUTABLE: lines cannot be added to a posted sales receipt');
END;
CREATE TRIGGER trg_sr_form_lines_update_unposted
BEFORE UPDATE ON sales_receipt_form_lines
WHEN (SELECT journal_entry_id FROM sales_receipts WHERE id = OLD.sales_receipt_id) IS NOT NULL
  OR (SELECT journal_entry_id FROM sales_receipts WHERE id = NEW.sales_receipt_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'SR_IMMUTABLE: lines of a posted sales receipt cannot be changed');
END;
CREATE TRIGGER trg_sr_form_lines_delete_unposted
BEFORE DELETE ON sales_receipt_form_lines
WHEN (SELECT journal_entry_id FROM sales_receipts WHERE id = OLD.sales_receipt_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'SR_IMMUTABLE: lines of a posted sales receipt cannot be removed');
END;

CREATE TRIGGER trg_est_form_lines_frozen_upd
BEFORE UPDATE ON estimate_form_lines
BEGIN
    SELECT RAISE(ABORT, 'EST_IMMUTABLE: estimate lines cannot be changed after saving');
END;
CREATE TRIGGER trg_est_form_lines_converted_no_delete
BEFORE DELETE ON estimate_form_lines
WHEN (SELECT status FROM estimates WHERE id = OLD.estimate_id) = 'CONVERTED'
BEGIN
    SELECT RAISE(ABORT, 'EST_CONVERTED: a converted estimate''s lines are kept as a record and cannot be deleted');
END;

-- ---------------------------------------------------------------------------------------------
-- Document totals count discount lines (otherwise unchanged from 0028)
-- ---------------------------------------------------------------------------------------------
DROP TRIGGER trg_inv_post_checks;
CREATE TRIGGER trg_inv_post_checks
BEFORE UPDATE OF status ON invoices
WHEN OLD.status = 'DRAFT' AND NEW.status = 'UNPAID'
BEGIN
    SELECT RAISE(ABORT, 'INV_NO_ITEMS: an invoice requires at least one line item')
    WHERE NOT EXISTS (SELECT 1 FROM invoice_items WHERE invoice_id = NEW.id);
    SELECT RAISE(ABORT, 'INV_TOTAL_MISMATCH: invoice total must equal its lines less discounts plus sales tax')
    WHERE NEW.total_cents <> (SELECT SUM(amount_cents) FROM invoice_items WHERE invoice_id = NEW.id)
                           + (SELECT COALESCE(SUM(amount_cents), 0) FROM invoice_form_lines WHERE invoice_id = NEW.id AND kind = 'DISCOUNT')
                           - NEW.discount_cents + NEW.tax_cents;
    SELECT RAISE(ABORT, 'INV_JOURNAL_MISSING: invoice must reference its POSTED journal entry')
    WHERE NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED'
          AND source_type = 'INVOICE' AND source_id = NEW.id
    );
END;

DROP TRIGGER trg_sr_update_rules;
CREATE TRIGGER trg_sr_update_rules
BEFORE UPDATE ON sales_receipts
BEGIN
    SELECT RAISE(ABORT, 'SR_IMMUTABLE: posted sales receipts cannot be changed')
    WHERE OLD.journal_entry_id IS NOT NULL
      AND (   NEW.receipt_number IS NOT OLD.receipt_number
           OR NEW.customer_id IS NOT OLD.customer_id
           OR NEW.receipt_date IS NOT OLD.receipt_date
           OR NEW.deposit_account_id IS NOT OLD.deposit_account_id
           OR NEW.payment_method IS NOT OLD.payment_method
           OR NEW.reference IS NOT OLD.reference
           OR NEW.memo IS NOT OLD.memo
           OR NEW.total_cents IS NOT OLD.total_cents
           OR NEW.tax_cents IS NOT OLD.tax_cents
           OR NEW.tax_rate_id IS NOT OLD.tax_rate_id
           OR NEW.discount_cents IS NOT OLD.discount_cents
           OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
           OR NEW.created_at IS NOT OLD.created_at);
    SELECT RAISE(ABORT, 'SR_NO_ITEMS: a sales receipt requires at least one line')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM sales_receipt_lines WHERE sales_receipt_id = NEW.id);
    SELECT RAISE(ABORT, 'SR_TOTAL_MISMATCH: receipt total must equal its lines less discounts plus sales tax')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NEW.total_cents <> (SELECT SUM(amount_cents) FROM sales_receipt_lines WHERE sales_receipt_id = NEW.id)
                           + (SELECT COALESCE(SUM(amount_cents), 0) FROM sales_receipt_form_lines WHERE sales_receipt_id = NEW.id AND kind = 'DISCOUNT')
                           - NEW.discount_cents + NEW.tax_cents;
    SELECT RAISE(ABORT, 'SR_JOURNAL_MISSING: receipt must reference its POSTED journal entry')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NOT EXISTS (
          SELECT 1 FROM journal_entries
          WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = 'SALES_RECEIPT' AND source_id = NEW.id
      );
    SELECT RAISE(ABORT, 'SR_UNVOID: a voided sales receipt cannot be restored')
    WHERE OLD.is_voided = 1 AND NEW.is_voided = 0;
    SELECT RAISE(ABORT, 'SR_VOID_ENTRY_ACTIVE: the receipt''s journal entry must be voided first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
    SELECT RAISE(ABORT, 'SR_DEPOSITED: void the bank deposit that includes this sales receipt first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND EXISTS (SELECT 1 FROM deposit_lines WHERE sales_receipt_id = OLD.id AND is_voided = 0);
END;
