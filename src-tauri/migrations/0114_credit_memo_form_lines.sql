-- 0114_credit_memo_form_lines.sql
-- Gap T21: credit memos (and refund receipts) take the same non-item lines as invoices: subtotals,
-- discounts, percent charges and groups (`item_types.rs`, docs/plans/item-types.md). As on the
-- other sales forms, a percent charge is written to the item table with its percent, and the other
-- kinds to a form-lines table of their own. Lines of a posted credit memo never change.

ALTER TABLE credit_memo_lines ADD COLUMN percent_ppm INTEGER CHECK (percent_ppm IS NULL OR percent_ppm BETWEEN 1 AND 1000000);

CREATE TABLE credit_memo_form_lines (
    id                  INTEGER PRIMARY KEY,
    credit_memo_id      INTEGER NOT NULL REFERENCES credit_memos (id) ON DELETE CASCADE,
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
    UNIQUE (credit_memo_id, line_number)
) STRICT;
CREATE INDEX ix_credit_memo_form_lines_account ON credit_memo_form_lines (account_id);

CREATE TRIGGER trg_cm_form_lines_number
BEFORE INSERT ON credit_memo_form_lines
WHEN EXISTS (SELECT 1 FROM credit_memo_lines WHERE credit_memo_id = NEW.credit_memo_id AND line_number = NEW.line_number)
BEGIN
    SELECT RAISE(ABORT, 'ITEMS_LINE_NUMBER: that line number is already an item line');
END;

CREATE TRIGGER trg_cm_form_lines_insert_unposted
BEFORE INSERT ON credit_memo_form_lines
WHEN (SELECT journal_entry_id FROM credit_memos WHERE id = NEW.credit_memo_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'CM_IMMUTABLE: lines of a posted credit memo cannot change');
END;

CREATE TRIGGER trg_cm_form_lines_delete_unposted
BEFORE DELETE ON credit_memo_form_lines
WHEN (SELECT journal_entry_id FROM credit_memos WHERE id = OLD.credit_memo_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'CM_IMMUTABLE: lines of a posted credit memo cannot change');
END;

-- As on the other form-line tables, a name merge being applied may point a line at the kept item.
CREATE TRIGGER trg_cm_form_lines_update_unposted
BEFORE UPDATE ON credit_memo_form_lines
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.credit_memo_id IS OLD.credit_memo_id
       AND NEW.line_number IS OLD.line_number
       AND NEW.kind IS OLD.kind
       AND NEW.description IS OLD.description
       AND NEW.amount_cents IS OLD.amount_cents
       AND (NEW.product_service_id IS OLD.product_service_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'ITEM' AND m.from_id = OLD.product_service_id AND m.to_id = NEW.product_service_id))
       AND NEW.percent_ppm IS OLD.percent_ppm
       AND NEW.account_id IS OLD.account_id
       AND NEW.class_id IS OLD.class_id
       AND NEW.is_taxable IS OLD.is_taxable
       AND NEW.item_group_id IS OLD.item_group_id
       AND NEW.group_quantity_e4 IS OLD.group_quantity_e4
       AND NEW.group_line_count IS OLD.group_line_count
       AND NEW.print_items IS OLD.print_items)
  AND ((SELECT journal_entry_id FROM credit_memos WHERE id = OLD.credit_memo_id) IS NOT NULL)
BEGIN
    SELECT RAISE(ABORT, 'CM_IMMUTABLE: lines of a posted credit memo cannot change');
END;
