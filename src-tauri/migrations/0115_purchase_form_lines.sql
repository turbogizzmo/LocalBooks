-- 0115_purchase_form_lines.sql
-- Discounts and charges on the purchase forms (gap T21): bills, checks and expenses, vendor credits
-- and purchase orders. Their split lines are account lines with an amount above zero, so a discount
-- or a percent charge is kept here, beside them, placed after the split line it applies to.
--
-- * products_services.expense_account_id: the account an Other Charge or Discount item posts to on
--   a purchase form (its income account is for sales forms).
-- * purchase_form_lines: one row per discount or charge line, on exactly one document. A discount
--   is taken off the document total and posts the other way from the split lines; a charge is added
--   and posts like them. amount_cents is always positive; a percent line keeps its percent of the
--   split line above.

ALTER TABLE products_services ADD COLUMN expense_account_id INTEGER REFERENCES chart_of_accounts (id) ON DELETE RESTRICT;

CREATE TABLE purchase_form_lines (
    id                  INTEGER PRIMARY KEY,
    bill_id             INTEGER REFERENCES bills (id) ON DELETE CASCADE,
    expense_id          INTEGER REFERENCES expenses (id) ON DELETE CASCADE,
    vendor_credit_id    INTEGER REFERENCES vendor_credits (id) ON DELETE CASCADE,
    purchase_order_id   INTEGER REFERENCES purchase_orders (id) ON DELETE CASCADE,
    -- The split line (numbered from 1) this line follows and applies to.
    after_line          INTEGER NOT NULL CHECK (after_line > 0),
    kind                TEXT    NOT NULL CHECK (kind IN ('DISCOUNT', 'CHARGE')),
    description         TEXT    NOT NULL CHECK (length(description) <= 4000),
    product_service_id  INTEGER REFERENCES products_services (id) ON DELETE RESTRICT,
    percent_ppm         INTEGER CHECK (percent_ppm IS NULL OR percent_ppm BETWEEN 1 AND 1000000),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    account_id          INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    class_id            INTEGER REFERENCES classes (id) ON DELETE RESTRICT,
    CHECK ((bill_id IS NOT NULL) + (expense_id IS NOT NULL) + (vendor_credit_id IS NOT NULL) + (purchase_order_id IS NOT NULL) = 1)
) STRICT;

CREATE UNIQUE INDEX ux_purchase_form_lines_bill ON purchase_form_lines (bill_id, after_line) WHERE bill_id IS NOT NULL;
CREATE UNIQUE INDEX ux_purchase_form_lines_expense ON purchase_form_lines (expense_id, after_line) WHERE expense_id IS NOT NULL;
CREATE UNIQUE INDEX ux_purchase_form_lines_vendor_credit ON purchase_form_lines (vendor_credit_id, after_line) WHERE vendor_credit_id IS NOT NULL;
CREATE UNIQUE INDEX ux_purchase_form_lines_po ON purchase_form_lines (purchase_order_id, after_line) WHERE purchase_order_id IS NOT NULL;
CREATE INDEX ix_purchase_form_lines_account ON purchase_form_lines (account_id);
CREATE INDEX ix_purchase_form_lines_item ON purchase_form_lines (product_service_id);

-- A bill's and an expense's total checks (0099) now count the discounts and charges: the split
-- lines, less discounts, plus charges. The triggers are otherwise as 0099 left them.
DROP TRIGGER trg_bill_post_checks;
CREATE TRIGGER trg_bill_post_checks
BEFORE UPDATE OF status ON bills
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id))
       AND NEW.bill_number IS OLD.bill_number
       AND NEW.bill_date IS OLD.bill_date
       AND NEW.due_date IS OLD.due_date
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.amount_paid_cents IS OLD.amount_paid_cents
       AND NEW.status IS OLD.status
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.created_at IS OLD.created_at
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.terms_id IS OLD.terms_id)
  AND (OLD.status = 'DRAFT' AND NEW.status = 'UNPAID')
BEGIN
    SELECT RAISE(ABORT, 'BILL_NO_ITEMS: a bill requires at least one line')
    WHERE NOT EXISTS (SELECT 1 FROM bill_lines WHERE bill_id = NEW.id);
    SELECT RAISE(ABORT, 'BILL_TOTAL_MISMATCH: bill total must equal its lines less discounts plus charges')
    WHERE NEW.total_cents <> (SELECT SUM(amount_cents) FROM bill_lines WHERE bill_id = NEW.id)
                           + COALESCE((SELECT SUM(CASE kind WHEN 'CHARGE' THEN amount_cents ELSE -amount_cents END) FROM purchase_form_lines WHERE bill_id = NEW.id), 0);
    SELECT RAISE(ABORT, 'BILL_JOURNAL_MISSING: bill must reference its POSTED journal entry')
    WHERE NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = 'BILL' AND source_id = NEW.id
    );
END;

DROP TRIGGER trg_exp_update_rules;
CREATE TRIGGER trg_exp_update_rules
BEFORE UPDATE ON expenses
WHEN NOT (EXISTS (SELECT 1 FROM name_merges WHERE applying = 1)
       AND NEW.id IS OLD.id
       AND NEW.kind IS OLD.kind
       AND (NEW.vendor_id IS OLD.vendor_id OR EXISTS (SELECT 1 FROM name_merges m WHERE m.applying = 1 AND m.kind = 'VENDOR' AND m.from_id = OLD.vendor_id AND m.to_id = NEW.vendor_id))
       AND NEW.payment_account_id IS OLD.payment_account_id
       AND NEW.expense_date IS OLD.expense_date
       AND NEW.check_number IS OLD.check_number
       AND NEW.payment_method IS OLD.payment_method
       AND NEW.memo IS OLD.memo
       AND NEW.total_cents IS OLD.total_cents
       AND NEW.journal_entry_id IS OLD.journal_entry_id
       AND NEW.is_voided IS OLD.is_voided
       AND NEW.created_at IS OLD.created_at
       AND NEW.currency_code IS OLD.currency_code
       AND NEW.exchange_rate_e6 IS OLD.exchange_rate_e6
       AND NEW.home_total_cents IS OLD.home_total_cents
       AND NEW.is_credit IS OLD.is_credit
       AND NEW.payment_method_id IS OLD.payment_method_id
       AND NEW.to_be_printed IS OLD.to_be_printed)
BEGIN
    SELECT RAISE(ABORT, 'EXP_IMMUTABLE: posted expenses cannot be changed')
    WHERE OLD.journal_entry_id IS NOT NULL
      AND (   NEW.kind IS NOT OLD.kind OR NEW.vendor_id IS NOT OLD.vendor_id
           OR NEW.payment_account_id IS NOT OLD.payment_account_id OR NEW.expense_date IS NOT OLD.expense_date
           OR (NEW.check_number IS NOT OLD.check_number
               AND NOT (OLD.check_number IS NULL AND OLD.to_be_printed = 1 AND NEW.check_number IS NOT NULL))
           OR NEW.payment_method IS NOT OLD.payment_method
           OR NEW.memo IS NOT OLD.memo OR NEW.total_cents IS NOT OLD.total_cents
           OR NEW.journal_entry_id IS NOT OLD.journal_entry_id OR NEW.created_at IS NOT OLD.created_at);
    SELECT RAISE(ABORT, 'EXP_TOTAL_MISMATCH: expense total must equal its split lines less discounts plus charges')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NEW.total_cents IS NOT (SELECT SUM(amount_cents) FROM expense_lines WHERE expense_id = NEW.id)
                              + COALESCE((SELECT SUM(CASE kind WHEN 'CHARGE' THEN amount_cents ELSE -amount_cents END) FROM purchase_form_lines WHERE expense_id = NEW.id), 0);
    SELECT RAISE(ABORT, 'EXP_JOURNAL_MISSING: expense must reference its POSTED journal entry')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NOT EXISTS (
          SELECT 1 FROM journal_entries
          WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = NEW.kind AND source_id = NEW.id
      );
    SELECT RAISE(ABORT, 'EXP_UNVOID: a voided expense cannot be restored')
    WHERE OLD.is_voided = 1 AND NEW.is_voided = 0;
    SELECT RAISE(ABORT, 'EXP_VOID_ENTRY_ACTIVE: the expense''s journal entry must be voided first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
END;
