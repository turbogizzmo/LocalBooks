-- 0011_quantities_currency.sql
-- Runs with foreign keys off (see db::migrate), as SQLite's table-rebuild procedure requires.
--
-- 1. Fractional quantities: line quantities are stored as integers scaled by 10,000
--    (1.5 → 15000). Line amount = round_half_up(quantity_e4 × rate_cents / 10,000), enforced by CHECK.
-- 2. Multi-currency: journal entries, invoices, payments and expenses can carry a foreign currency
--    and an exchange rate (home units per 1 foreign unit, scaled by 1,000,000). Journal lines always
--    hold home-currency amounts, so every report stays in one currency; foreign amounts are kept
--    alongside for display. Realized differences on payments post to "Exchange Gain or Loss".

-- =============================================================================================
-- Line tables with scaled quantities (rebuilt: their CHECK constraints change)
-- =============================================================================================
-- Triggers on other tables that read these line tables must not exist while a table is swapped.
DROP TRIGGER trg_inv_post_checks;
DROP TRIGGER trg_sr_update_rules;

CREATE TABLE invoice_items_new (
    id                  INTEGER PRIMARY KEY,
    invoice_id          INTEGER NOT NULL REFERENCES invoices (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    description         TEXT    NOT NULL,
    quantity_e4         INTEGER NOT NULL CHECK (quantity_e4 BETWEEN 1 AND 10000000000000),
    rate_cents          INTEGER NOT NULL CHECK (rate_cents BETWEEN 1 AND 100000000000000),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    income_account_id   INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    CHECK (amount_cents = (quantity_e4 * rate_cents + 5000) / 10000),
    UNIQUE (invoice_id, line_number)
) STRICT;
INSERT INTO invoice_items_new (id, invoice_id, line_number, description, quantity_e4, rate_cents, amount_cents, income_account_id)
SELECT id, invoice_id, line_number, description, quantity * 10000, rate_cents, amount_cents, income_account_id FROM invoice_items;
DROP TABLE invoice_items;
ALTER TABLE invoice_items_new RENAME TO invoice_items;
CREATE INDEX ix_invoice_items_account ON invoice_items (income_account_id);

CREATE TRIGGER trg_inv_items_insert_draft_only
BEFORE INSERT ON invoice_items
WHEN (SELECT status FROM invoices WHERE id = NEW.invoice_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: items can only be added to a DRAFT invoice');
END;

CREATE TRIGGER trg_inv_items_update_draft_only
BEFORE UPDATE ON invoice_items
WHEN (SELECT status FROM invoices WHERE id = OLD.invoice_id) <> 'DRAFT'
  OR (SELECT status FROM invoices WHERE id = NEW.invoice_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: items can only be changed on a DRAFT invoice');
END;

CREATE TRIGGER trg_inv_items_delete_draft_only
BEFORE DELETE ON invoice_items
WHEN (SELECT status FROM invoices WHERE id = OLD.invoice_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: items can only be removed from a DRAFT invoice');
END;

CREATE TABLE sales_receipt_lines_new (
    id                  INTEGER PRIMARY KEY,
    sales_receipt_id    INTEGER NOT NULL REFERENCES sales_receipts (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    product_service_id  INTEGER REFERENCES products_services (id) ON DELETE RESTRICT,
    description         TEXT    NOT NULL,
    quantity_e4         INTEGER NOT NULL CHECK (quantity_e4 BETWEEN 1 AND 10000000000000),
    rate_cents          INTEGER NOT NULL CHECK (rate_cents BETWEEN 1 AND 100000000000000),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    income_account_id   INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    CHECK (amount_cents = (quantity_e4 * rate_cents + 5000) / 10000),
    UNIQUE (sales_receipt_id, line_number)
) STRICT;
INSERT INTO sales_receipt_lines_new
    (id, sales_receipt_id, line_number, product_service_id, description, quantity_e4, rate_cents, amount_cents, income_account_id)
SELECT id, sales_receipt_id, line_number, product_service_id, description, quantity * 10000, rate_cents, amount_cents, income_account_id
FROM sales_receipt_lines;
DROP TABLE sales_receipt_lines;
ALTER TABLE sales_receipt_lines_new RENAME TO sales_receipt_lines;

CREATE TRIGGER trg_sr_lines_insert_unposted
BEFORE INSERT ON sales_receipt_lines
WHEN (SELECT journal_entry_id FROM sales_receipts WHERE id = NEW.sales_receipt_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'SR_IMMUTABLE: lines cannot be added to a posted sales receipt');
END;

CREATE TRIGGER trg_sr_lines_update_unposted
BEFORE UPDATE ON sales_receipt_lines
WHEN (SELECT journal_entry_id FROM sales_receipts WHERE id = OLD.sales_receipt_id) IS NOT NULL
  OR (SELECT journal_entry_id FROM sales_receipts WHERE id = NEW.sales_receipt_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'SR_IMMUTABLE: lines of a posted sales receipt cannot be changed');
END;

CREATE TRIGGER trg_sr_lines_delete_unposted
BEFORE DELETE ON sales_receipt_lines
WHEN (SELECT journal_entry_id FROM sales_receipts WHERE id = OLD.sales_receipt_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'SR_IMMUTABLE: lines of a posted sales receipt cannot be removed');
END;

CREATE TABLE estimate_lines_new (
    id                  INTEGER PRIMARY KEY,
    estimate_id         INTEGER NOT NULL REFERENCES estimates (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    product_service_id  INTEGER REFERENCES products_services (id) ON DELETE RESTRICT,
    description         TEXT    NOT NULL,
    quantity_e4         INTEGER NOT NULL CHECK (quantity_e4 BETWEEN 1 AND 10000000000000),
    rate_cents          INTEGER NOT NULL CHECK (rate_cents BETWEEN 1 AND 100000000000000),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    income_account_id   INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    CHECK (amount_cents = (quantity_e4 * rate_cents + 5000) / 10000),
    UNIQUE (estimate_id, line_number)
) STRICT;
INSERT INTO estimate_lines_new
    (id, estimate_id, line_number, product_service_id, description, quantity_e4, rate_cents, amount_cents, income_account_id)
SELECT id, estimate_id, line_number, product_service_id, description, quantity * 10000, rate_cents, amount_cents, income_account_id
FROM estimate_lines;
DROP TABLE estimate_lines;
ALTER TABLE estimate_lines_new RENAME TO estimate_lines;

CREATE TRIGGER trg_est_lines_frozen_upd
BEFORE UPDATE ON estimate_lines
BEGIN
    SELECT RAISE(ABORT, 'EST_IMMUTABLE: estimate lines cannot be changed after saving');
END;

-- Recreate the cross-table triggers dropped above (unchanged from 0006 and 0008).
CREATE TRIGGER trg_inv_post_checks
BEFORE UPDATE OF status ON invoices
WHEN OLD.status = 'DRAFT' AND NEW.status = 'UNPAID'
BEGIN
    SELECT RAISE(ABORT, 'INV_NO_ITEMS: an invoice requires at least one line item')
    WHERE NOT EXISTS (SELECT 1 FROM invoice_items WHERE invoice_id = NEW.id);
    SELECT RAISE(ABORT, 'INV_TOTAL_MISMATCH: invoice total must equal the sum of its line items')
    WHERE NEW.total_cents <> (SELECT SUM(amount_cents) FROM invoice_items WHERE invoice_id = NEW.id);
    SELECT RAISE(ABORT, 'INV_JOURNAL_MISSING: invoice must reference its POSTED journal entry')
    WHERE NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED'
          AND source_type = 'INVOICE' AND source_id = NEW.id
    );
END;

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
           OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
           OR NEW.created_at IS NOT OLD.created_at);
    SELECT RAISE(ABORT, 'SR_NO_ITEMS: a sales receipt requires at least one line')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM sales_receipt_lines WHERE sales_receipt_id = NEW.id);
    SELECT RAISE(ABORT, 'SR_TOTAL_MISMATCH: receipt total must equal the sum of its lines')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NEW.total_cents <> (SELECT SUM(amount_cents) FROM sales_receipt_lines WHERE sales_receipt_id = NEW.id);
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

-- Bill split rows may optionally be priced as quantity × rate.
ALTER TABLE bill_lines ADD COLUMN quantity_e4 INTEGER
    CHECK (quantity_e4 IS NULL OR quantity_e4 BETWEEN 1 AND 10000000000000);
ALTER TABLE bill_lines ADD COLUMN rate_cents INTEGER
    CHECK ((rate_cents IS NULL) = (quantity_e4 IS NULL)
           AND (rate_cents IS NULL OR (rate_cents BETWEEN 1 AND 100000000000000
                                       AND amount_cents = (quantity_e4 * rate_cents + 5000) / 10000)));

-- =============================================================================================
-- Multi-currency
-- =============================================================================================
INSERT INTO system_preferences (key, value) VALUES ('home_currency_code', 'USD');

-- Journal entries: lines always hold home-currency amounts; these describe the original currency.
ALTER TABLE journal_entries ADD COLUMN foreign_currency_code TEXT
    CHECK (foreign_currency_code IS NULL OR foreign_currency_code GLOB '[A-Z][A-Z][A-Z]');
ALTER TABLE journal_entries ADD COLUMN exchange_rate_e6 INTEGER
    CHECK ((exchange_rate_e6 IS NULL) = (foreign_currency_code IS NULL)
           AND (exchange_rate_e6 IS NULL OR exchange_rate_e6 BETWEEN 1 AND 1000000000000));
ALTER TABLE journal_lines ADD COLUMN foreign_debit_cents INTEGER CHECK (foreign_debit_cents IS NULL OR foreign_debit_cents >= 0);
ALTER TABLE journal_lines ADD COLUMN foreign_credit_cents INTEGER CHECK (foreign_credit_cents IS NULL OR foreign_credit_cents >= 0);

CREATE TRIGGER trg_je_currency_immutable
BEFORE UPDATE OF foreign_currency_code, exchange_rate_e6 ON journal_entries
WHEN OLD.status <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'JE_IMMUTABLE: the currency of a posted entry cannot change');
END;

CREATE TRIGGER trg_jl_foreign_immutable
BEFORE UPDATE OF foreign_debit_cents, foreign_credit_cents ON journal_lines
WHEN (SELECT status FROM journal_entries WHERE id = OLD.journal_entry_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'JE_IMMUTABLE: lines can only be changed on a DRAFT entry');
END;

-- Invoices: totals and payments stay in the invoice's currency; home_total_cents is the A/R booked.
ALTER TABLE invoices ADD COLUMN currency_code TEXT CHECK (currency_code IS NULL OR currency_code GLOB '[A-Z][A-Z][A-Z]');
ALTER TABLE invoices ADD COLUMN exchange_rate_e6 INTEGER
    CHECK ((exchange_rate_e6 IS NULL) = (currency_code IS NULL) AND (exchange_rate_e6 IS NULL OR exchange_rate_e6 > 0));
ALTER TABLE invoices ADD COLUMN home_total_cents INTEGER
    CHECK ((home_total_cents IS NULL) = (currency_code IS NULL) AND (home_total_cents IS NULL OR home_total_cents > 0));

CREATE TRIGGER trg_inv_currency_immutable
BEFORE UPDATE OF currency_code, exchange_rate_e6, home_total_cents ON invoices
WHEN OLD.status <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'INV_IMMUTABLE: the currency of a posted invoice cannot change');
END;

-- Payments on foreign invoices: amount_cents is in the invoice currency; home_amount_cents is the
-- cash received, ar_relief_cents the A/R relieved; the difference is the realized FX gain/loss.
ALTER TABLE invoice_payments ADD COLUMN exchange_rate_e6 INTEGER CHECK (exchange_rate_e6 IS NULL OR exchange_rate_e6 > 0);
ALTER TABLE invoice_payments ADD COLUMN home_amount_cents INTEGER
    CHECK ((home_amount_cents IS NULL) = (exchange_rate_e6 IS NULL) AND (home_amount_cents IS NULL OR home_amount_cents > 0));
ALTER TABLE invoice_payments ADD COLUMN ar_relief_cents INTEGER
    CHECK ((ar_relief_cents IS NULL) = (exchange_rate_e6 IS NULL) AND (ar_relief_cents IS NULL OR ar_relief_cents >= 0));

CREATE TRIGGER trg_pay_currency_immutable
BEFORE UPDATE OF exchange_rate_e6, home_amount_cents, ar_relief_cents ON invoice_payments
BEGIN
    SELECT RAISE(ABORT, 'INV_PAYMENT_IMMUTABLE: recorded payments cannot be changed');
END;

-- Expenses & checks.
ALTER TABLE expenses ADD COLUMN currency_code TEXT CHECK (currency_code IS NULL OR currency_code GLOB '[A-Z][A-Z][A-Z]');
ALTER TABLE expenses ADD COLUMN exchange_rate_e6 INTEGER
    CHECK ((exchange_rate_e6 IS NULL) = (currency_code IS NULL) AND (exchange_rate_e6 IS NULL OR exchange_rate_e6 > 0));
ALTER TABLE expenses ADD COLUMN home_total_cents INTEGER
    CHECK ((home_total_cents IS NULL) = (currency_code IS NULL) AND (home_total_cents IS NULL OR home_total_cents > 0));

CREATE TRIGGER trg_exp_currency_immutable
BEFORE UPDATE OF currency_code, exchange_rate_e6, home_total_cents ON expenses
WHEN OLD.journal_entry_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'EXP_IMMUTABLE: posted expenses cannot be changed');
END;

-- Home-currency position of every invoice (the A/R view used by aging and customer balances).
CREATE VIEW v_invoice_balances AS
SELECT
    i.id AS invoice_id,
    COALESCE(i.home_total_cents, i.total_cents) AS home_total_cents,
    COALESCE(i.home_total_cents, i.total_cents)
      - COALESCE((SELECT SUM(COALESCE(p.ar_relief_cents, p.amount_cents))
                  FROM invoice_payments p WHERE p.invoice_id = i.id AND p.is_voided = 0), 0) AS home_open_cents
FROM invoices i;

-- Undeposited Funds holds home-currency amounts, so a deposited foreign payment uses its home amount.
DROP TRIGGER trg_dep_lines_insert_checks;
CREATE TRIGGER trg_dep_lines_insert_checks
BEFORE INSERT ON deposit_lines
BEGIN
    SELECT RAISE(ABORT, 'DEP_IMMUTABLE: lines can only be added before the deposit is posted')
    WHERE (SELECT journal_entry_id FROM deposits WHERE id = NEW.deposit_id) IS NOT NULL OR NEW.is_voided <> 0;
    SELECT RAISE(ABORT, 'DEP_BAD_ACCOUNT: a deposit line cannot use the account being deposited into, A/R or A/P')
    WHERE NEW.account_id = (SELECT bank_account_id FROM deposits WHERE id = NEW.deposit_id)
       OR (SELECT account_type FROM chart_of_accounts WHERE id = NEW.account_id) IN ('ACCOUNTS_RECEIVABLE', 'ACCOUNTS_PAYABLE');
    SELECT RAISE(ABORT, 'DEP_PAYMENT_MISMATCH: a deposited payment must come out of Undeposited Funds for its full amount')
    WHERE NEW.invoice_payment_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM invoice_payments p
        WHERE p.id = NEW.invoice_payment_id AND p.is_voided = 0 AND p.journal_entry_id IS NOT NULL
          AND p.deposit_account_id = NEW.account_id AND COALESCE(p.home_amount_cents, p.amount_cents) = NEW.amount_cents
          AND (SELECT is_system FROM chart_of_accounts WHERE id = p.deposit_account_id) = 1
    );
    SELECT RAISE(ABORT, 'DEP_RECEIPT_MISMATCH: a deposited sales receipt must come out of Undeposited Funds for its full amount')
    WHERE NEW.sales_receipt_id IS NOT NULL AND NOT EXISTS (
        SELECT 1 FROM sales_receipts r
        WHERE r.id = NEW.sales_receipt_id AND r.is_voided = 0 AND r.journal_entry_id IS NOT NULL
          AND r.deposit_account_id = NEW.account_id AND r.total_cents = NEW.amount_cents
          AND (SELECT is_system FROM chart_of_accounts WHERE id = r.deposit_account_id) = 1
    );
END;

-- System account for realized foreign exchange differences (QuickBooks: "Exchange Gain or Loss").
INSERT INTO chart_of_accounts (account_number, name, account_type, description, is_system)
SELECT CASE WHEN EXISTS (SELECT 1 FROM chart_of_accounts WHERE account_number = '8100') THEN NULL ELSE '8100' END,
       'Exchange Gain or Loss', 'OTHER_EXPENSE', 'Realized gains (credits) and losses (debits) on foreign currency payments', 1
WHERE NOT EXISTS (SELECT 1 FROM chart_of_accounts WHERE parent_id IS NULL AND name = 'Exchange Gain or Loss' COLLATE NOCASE);
