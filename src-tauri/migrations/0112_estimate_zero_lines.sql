-- 0112_estimate_zero_lines.sql
-- Preferences T3 (sales batch): an estimate line may be priced at nothing (an included extra), so
-- Sales & Customers › "Don't print zero-amount lines when an estimate becomes an invoice" has lines
-- to leave off. estimate_lines is rebuilt with rate_cents and amount_cents allowed to be 0 (the
-- estimate's total must still be above zero, checked in estimates.rs). Its triggers, and the
-- triggers on other tables that read it, are recreated exactly as they were.


DROP TRIGGER IF EXISTS trg_est_form_lines_frozen_upd;
DROP TRIGGER IF EXISTS trg_est_form_lines_number;
DROP TRIGGER IF EXISTS trg_est_lines_converted_no_delete;
DROP TRIGGER IF EXISTS trg_est_lines_form_number;
DROP TRIGGER IF EXISTS trg_est_lines_frozen_upd;

CREATE TABLE estimate_lines_new (
    id                  INTEGER PRIMARY KEY,
    estimate_id         INTEGER NOT NULL REFERENCES estimates (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    product_service_id  INTEGER REFERENCES products_services (id) ON DELETE RESTRICT,
    description         TEXT    NOT NULL,
    quantity_e4         INTEGER NOT NULL CHECK (quantity_e4 BETWEEN 1 AND 10000000000000),
    rate_cents          INTEGER NOT NULL CHECK (rate_cents BETWEEN 0 AND 100000000000000),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 0 AND 100000000000000),
    income_account_id   INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT, class_id INTEGER REFERENCES classes (id) ON DELETE RESTRICT, is_taxable INTEGER NOT NULL DEFAULT 0 CHECK (is_taxable IN (0, 1)), percent_ppm INTEGER CHECK (percent_ppm IS NULL OR percent_ppm BETWEEN 1 AND 1000000),
    CHECK (amount_cents = (quantity_e4 * rate_cents + 5000) / 10000),
    UNIQUE (estimate_id, line_number)
) STRICT;
INSERT INTO estimate_lines_new (id, estimate_id, line_number, product_service_id, description, quantity_e4, rate_cents, amount_cents, income_account_id, class_id, is_taxable, percent_ppm) SELECT id, estimate_id, line_number, product_service_id, description, quantity_e4, rate_cents, amount_cents, income_account_id, class_id, is_taxable, percent_ppm FROM estimate_lines;
DROP TABLE estimate_lines;
ALTER TABLE estimate_lines_new RENAME TO estimate_lines;

CREATE TRIGGER trg_est_form_lines_frozen_upd
BEFORE UPDATE ON estimate_form_lines
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.estimate_id IS OLD.estimate_id
       AND NEW.line_number IS OLD.line_number
       AND NEW.kind IS OLD.kind
       AND NEW.description IS OLD.description
       AND NEW.amount_cents IS OLD.amount_cents
       AND (NEW.product_service_id IS OLD.product_service_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'ITEM' AND m.from_id = OLD.product_service_id AND m.to_id = NEW.product_service_id))
       AND NEW.percent_ppm IS OLD.percent_ppm
       AND (NEW.account_id IS OLD.account_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'ACCOUNT' AND m.from_id = OLD.account_id AND m.to_id = NEW.account_id))
       AND NEW.class_id IS OLD.class_id
       AND NEW.is_taxable IS OLD.is_taxable
       AND NEW.item_group_id IS OLD.item_group_id
       AND NEW.group_quantity_e4 IS OLD.group_quantity_e4
       AND NEW.group_line_count IS OLD.group_line_count
       AND NEW.print_items IS OLD.print_items)
BEGIN
    SELECT RAISE(ABORT, 'EST_IMMUTABLE: estimate lines cannot be changed after saving');
END;

CREATE TRIGGER trg_est_form_lines_number
BEFORE INSERT ON estimate_form_lines
WHEN EXISTS (SELECT 1 FROM estimate_lines WHERE estimate_id = NEW.estimate_id AND line_number = NEW.line_number)
BEGIN
    SELECT RAISE(ABORT, 'ITEMS_LINE_NUMBER: that line number is already an item line');
END;

CREATE TRIGGER trg_est_lines_converted_no_delete
BEFORE DELETE ON estimate_lines
WHEN (SELECT status FROM estimates WHERE id = OLD.estimate_id) = 'CONVERTED'
BEGIN
    SELECT RAISE(ABORT, 'EST_CONVERTED: a converted estimate''s lines are kept as a record and cannot be deleted');
END;

CREATE TRIGGER trg_est_lines_form_number
BEFORE INSERT ON estimate_lines
WHEN EXISTS (SELECT 1 FROM estimate_form_lines WHERE estimate_id = NEW.estimate_id AND line_number = NEW.line_number)
BEGIN
    SELECT RAISE(ABORT, 'ITEMS_LINE_NUMBER: that line number is already a form line');
END;

CREATE TRIGGER trg_est_lines_frozen_upd
BEFORE UPDATE ON estimate_lines
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.estimate_id IS OLD.estimate_id
       AND NEW.line_number IS OLD.line_number
       AND (NEW.product_service_id IS OLD.product_service_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'ITEM' AND m.from_id = OLD.product_service_id AND m.to_id = NEW.product_service_id))
       AND NEW.description IS OLD.description
       AND NEW.quantity_e4 IS OLD.quantity_e4
       AND NEW.rate_cents IS OLD.rate_cents
       AND NEW.amount_cents IS OLD.amount_cents
       AND (NEW.income_account_id IS OLD.income_account_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'ACCOUNT' AND m.from_id = OLD.income_account_id AND m.to_id = NEW.income_account_id))
       AND NEW.class_id IS OLD.class_id
       AND NEW.is_taxable IS OLD.is_taxable
       AND NEW.percent_ppm IS OLD.percent_ppm)
BEGIN
    SELECT RAISE(ABORT, 'EST_IMMUTABLE: estimate lines cannot be changed after saving');
END;

