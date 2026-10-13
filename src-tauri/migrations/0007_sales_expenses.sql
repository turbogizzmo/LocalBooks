-- 0007_sales_expenses.sql
-- Sales: customers, products & services, and sales receipts (cash sales: Dr deposit / Cr income).
-- Expenses: vendors, bills (Dr expense / Cr Accounts Payable) and bill payments (Dr A/P / Cr bank).
--
-- Documents follow the invoice pattern: rows are written first, the journal entry is posted with the
-- document as its source, then the entry is attached. Attached documents are immutable.

-- ---------------------------------------------------------------------------------------------
-- Customers
-- ---------------------------------------------------------------------------------------------
CREATE TABLE customers (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    email       TEXT    CHECK (email IS NULL OR email LIKE '%_@_%._%'),
    is_active   INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_customers_name ON customers (name COLLATE NOCASE);

INSERT INTO customers (name)
SELECT MIN(customer_name) FROM invoices GROUP BY customer_name COLLATE NOCASE;

ALTER TABLE invoices ADD COLUMN customer_id INTEGER REFERENCES customers (id) ON DELETE RESTRICT;
UPDATE invoices SET customer_id = (SELECT c.id FROM customers c WHERE c.name = invoices.customer_name COLLATE NOCASE);
CREATE INDEX ix_invoices_customer_id ON invoices (customer_id);

-- ---------------------------------------------------------------------------------------------
-- Products & services
-- ---------------------------------------------------------------------------------------------
CREATE TABLE products_services (
    id                  INTEGER PRIMARY KEY,
    name                TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    description         TEXT,
    rate_cents          INTEGER NOT NULL CHECK (rate_cents BETWEEN 0 AND 100000000000000),
    income_account_id   INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    is_active           INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_products_name ON products_services (name COLLATE NOCASE);

CREATE TRIGGER trg_product_income_account
BEFORE INSERT ON products_services
WHEN (SELECT t.classification FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type
      WHERE a.id = NEW.income_account_id) IS NOT 'INCOME'
BEGIN
    SELECT RAISE(ABORT, 'PRODUCT_NOT_INCOME_ACCOUNT: products and services must post to an Income account');
END;

CREATE TRIGGER trg_product_income_account_upd
BEFORE UPDATE OF income_account_id ON products_services
WHEN (SELECT t.classification FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type
      WHERE a.id = NEW.income_account_id) IS NOT 'INCOME'
BEGIN
    SELECT RAISE(ABORT, 'PRODUCT_NOT_INCOME_ACCOUNT: products and services must post to an Income account');
END;

-- ---------------------------------------------------------------------------------------------
-- Sales receipts
-- ---------------------------------------------------------------------------------------------
CREATE TABLE sales_receipts (
    id                  INTEGER PRIMARY KEY,
    receipt_number      INTEGER NOT NULL UNIQUE CHECK (receipt_number > 0),
    customer_id         INTEGER REFERENCES customers (id) ON DELETE RESTRICT,
    receipt_date        TEXT    NOT NULL CHECK (
                            receipt_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(receipt_date) = receipt_date
                        ),
    deposit_account_id  INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    payment_method      TEXT    NOT NULL CHECK (payment_method IN ('CASH', 'CHECK', 'CREDIT_CARD', 'ACH', 'WIRE', 'OTHER')),
    reference           TEXT,
    memo                TEXT,
    total_cents         INTEGER NOT NULL CHECK (total_cents BETWEEN 1 AND 100000000000000),
    journal_entry_id    INTEGER UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX ix_sales_receipts_date ON sales_receipts (receipt_date);

CREATE TABLE sales_receipt_lines (
    id                  INTEGER PRIMARY KEY,
    sales_receipt_id    INTEGER NOT NULL REFERENCES sales_receipts (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    product_service_id  INTEGER REFERENCES products_services (id) ON DELETE RESTRICT,
    description         TEXT    NOT NULL,
    quantity            INTEGER NOT NULL CHECK (quantity BETWEEN 1 AND 1000000000),
    rate_cents          INTEGER NOT NULL CHECK (rate_cents BETWEEN 1 AND 100000000000000),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    income_account_id   INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    CHECK (amount_cents = quantity * rate_cents),
    UNIQUE (sales_receipt_id, line_number)
) STRICT;

CREATE TRIGGER trg_sr_insert_checks
BEFORE INSERT ON sales_receipts
BEGIN
    SELECT RAISE(ABORT, 'SR_PREPOSTED: insert the receipt first, then attach its journal entry')
    WHERE NEW.journal_entry_id IS NOT NULL;
    SELECT RAISE(ABORT, 'SR_BAD_DEPOSIT_ACCOUNT: deposit to a Bank or Undeposited Funds account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.deposit_account_id) NOT IN ('BANK', 'OTHER_CURRENT_ASSET');
END;

CREATE TRIGGER trg_sr_attach_entry
BEFORE UPDATE ON sales_receipts
BEGIN
    SELECT RAISE(ABORT, 'SR_IMMUTABLE: posted sales receipts cannot be changed')
    WHERE OLD.journal_entry_id IS NOT NULL;
    SELECT RAISE(ABORT, 'SR_NO_ITEMS: a sales receipt requires at least one line')
    WHERE NEW.journal_entry_id IS NOT NULL
      AND NOT EXISTS (SELECT 1 FROM sales_receipt_lines WHERE sales_receipt_id = NEW.id);
    SELECT RAISE(ABORT, 'SR_TOTAL_MISMATCH: receipt total must equal the sum of its lines')
    WHERE NEW.journal_entry_id IS NOT NULL
      AND NEW.total_cents <> (SELECT SUM(amount_cents) FROM sales_receipt_lines WHERE sales_receipt_id = NEW.id);
    SELECT RAISE(ABORT, 'SR_JOURNAL_MISSING: receipt must reference its POSTED journal entry')
    WHERE NEW.journal_entry_id IS NOT NULL
      AND NOT EXISTS (
          SELECT 1 FROM journal_entries
          WHERE id = NEW.journal_entry_id AND status = 'POSTED'
            AND source_type = 'SALES_RECEIPT' AND source_id = NEW.id
      );
END;

CREATE TRIGGER trg_sr_no_delete_posted
BEFORE DELETE ON sales_receipts
WHEN OLD.journal_entry_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'SR_IMMUTABLE: posted sales receipts cannot be deleted');
END;

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

-- ---------------------------------------------------------------------------------------------
-- Vendors
-- ---------------------------------------------------------------------------------------------
CREATE TABLE vendors (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    email       TEXT    CHECK (email IS NULL OR email LIKE '%_@_%._%'),
    is_active   INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_vendors_name ON vendors (name COLLATE NOCASE);

-- ---------------------------------------------------------------------------------------------
-- Bills
-- ---------------------------------------------------------------------------------------------
CREATE TABLE bills (
    id                  INTEGER PRIMARY KEY,
    vendor_id           INTEGER NOT NULL REFERENCES vendors (id) ON DELETE RESTRICT,
    bill_number         TEXT,
    bill_date           TEXT    NOT NULL CHECK (
                            bill_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(bill_date) = bill_date
                        ),
    due_date            TEXT    NOT NULL CHECK (
                            due_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(due_date) = due_date
                        ),
    memo                TEXT,
    total_cents         INTEGER NOT NULL CHECK (total_cents BETWEEN 1 AND 100000000000000),
    amount_paid_cents   INTEGER NOT NULL DEFAULT 0 CHECK (amount_paid_cents >= 0),
    status              TEXT    NOT NULL DEFAULT 'DRAFT' CHECK (status IN ('DRAFT', 'UNPAID', 'PARTIAL', 'PAID')),
    journal_entry_id    INTEGER UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (due_date >= bill_date),
    CHECK (amount_paid_cents <= total_cents),
    CHECK ((status = 'DRAFT') = (journal_entry_id IS NULL)),
    CHECK (status NOT IN ('DRAFT', 'UNPAID') OR amount_paid_cents = 0),
    CHECK ((status = 'PARTIAL') = (amount_paid_cents > 0 AND amount_paid_cents < total_cents)),
    CHECK ((status = 'PAID') = (amount_paid_cents = total_cents))
) STRICT;
CREATE INDEX ix_bills_status_due ON bills (status, due_date);
CREATE INDEX ix_bills_vendor ON bills (vendor_id);

CREATE TABLE bill_lines (
    id                  INTEGER PRIMARY KEY,
    bill_id             INTEGER NOT NULL REFERENCES bills (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    description         TEXT    NOT NULL,
    expense_account_id  INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    UNIQUE (bill_id, line_number)
) STRICT;
CREATE INDEX ix_bill_lines_account ON bill_lines (expense_account_id);

CREATE TABLE bill_payments (
    id                  INTEGER PRIMARY KEY,
    bill_id             INTEGER NOT NULL REFERENCES bills (id) ON DELETE RESTRICT,
    payment_date        TEXT    NOT NULL CHECK (
                            payment_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(payment_date) = payment_date
                        ),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    payment_account_id  INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    reference           TEXT,
    journal_entry_id    INTEGER UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX ix_bill_payments_bill ON bill_payments (bill_id, payment_date);

CREATE TRIGGER trg_bill_insert_as_draft
BEFORE INSERT ON bills
WHEN NEW.status <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'BILL_INSERT_NOT_DRAFT: bills must be inserted as DRAFT, then posted');
END;

CREATE TRIGGER trg_bill_status_transition
BEFORE UPDATE OF status ON bills
WHEN NOT (
       (OLD.status = 'DRAFT'   AND NEW.status IN ('DRAFT', 'UNPAID'))
    OR (OLD.status = 'UNPAID'  AND NEW.status IN ('UNPAID', 'PARTIAL', 'PAID'))
    OR (OLD.status = 'PARTIAL' AND NEW.status IN ('PARTIAL', 'PAID'))
    OR (OLD.status = 'PAID'    AND NEW.status = 'PAID')
)
BEGIN
    SELECT RAISE(ABORT, 'BILL_INVALID_TRANSITION: allowed transitions are DRAFT->UNPAID->PARTIAL->PAID');
END;

CREATE TRIGGER trg_bill_post_checks
BEFORE UPDATE OF status ON bills
WHEN OLD.status = 'DRAFT' AND NEW.status = 'UNPAID'
BEGIN
    SELECT RAISE(ABORT, 'BILL_NO_ITEMS: a bill requires at least one line')
    WHERE NOT EXISTS (SELECT 1 FROM bill_lines WHERE bill_id = NEW.id);
    SELECT RAISE(ABORT, 'BILL_TOTAL_MISMATCH: bill total must equal the sum of its lines')
    WHERE NEW.total_cents <> (SELECT SUM(amount_cents) FROM bill_lines WHERE bill_id = NEW.id);
    SELECT RAISE(ABORT, 'BILL_JOURNAL_MISSING: bill must reference its POSTED journal entry')
    WHERE NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = 'BILL' AND source_id = NEW.id
    );
END;

CREATE TRIGGER trg_bill_amount_paid_matches
BEFORE UPDATE OF amount_paid_cents ON bills
BEGIN
    SELECT RAISE(ABORT, 'BILL_PAYMENT_UNPOSTED: every payment must be posted before it is applied')
    WHERE EXISTS (SELECT 1 FROM bill_payments WHERE bill_id = NEW.id AND journal_entry_id IS NULL);
    SELECT RAISE(ABORT, 'BILL_PAID_MISMATCH: amount paid must equal the sum of recorded payments')
    WHERE NEW.amount_paid_cents <> COALESCE((SELECT SUM(amount_cents) FROM bill_payments WHERE bill_id = NEW.id), 0);
END;

CREATE TRIGGER trg_bill_posted_header_immutable
BEFORE UPDATE ON bills
WHEN OLD.status <> 'DRAFT'
 AND (   NEW.vendor_id        IS NOT OLD.vendor_id
      OR NEW.bill_number      IS NOT OLD.bill_number
      OR NEW.bill_date        IS NOT OLD.bill_date
      OR NEW.due_date         IS NOT OLD.due_date
      OR NEW.memo             IS NOT OLD.memo
      OR NEW.total_cents      IS NOT OLD.total_cents
      OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
      OR NEW.created_at       IS NOT OLD.created_at)
BEGIN
    SELECT RAISE(ABORT, 'BILL_IMMUTABLE: posted bills cannot be altered');
END;

CREATE TRIGGER trg_bill_no_delete_posted
BEFORE DELETE ON bills
WHEN OLD.status <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'BILL_IMMUTABLE: posted bills cannot be deleted');
END;

CREATE TRIGGER trg_bill_lines_insert_checks
BEFORE INSERT ON bill_lines
BEGIN
    SELECT RAISE(ABORT, 'BILL_IMMUTABLE: lines can only be added to a DRAFT bill')
    WHERE (SELECT status FROM bills WHERE id = NEW.bill_id) <> 'DRAFT';
    SELECT RAISE(ABORT, 'BILL_BAD_CATEGORY: bill lines must use an expense or asset category')
    WHERE (SELECT CASE WHEN t.classification = 'EXPENSE' OR a.account_type IN ('FIXED_ASSET', 'OTHER_ASSET', 'OTHER_CURRENT_ASSET')
                       THEN a.is_system ELSE 1 END
           FROM chart_of_accounts a JOIN account_types t ON t.code = a.account_type
           WHERE a.id = NEW.expense_account_id) IS NOT 0;
END;

CREATE TRIGGER trg_bill_lines_update_draft_only
BEFORE UPDATE ON bill_lines
WHEN (SELECT status FROM bills WHERE id = OLD.bill_id) <> 'DRAFT'
  OR (SELECT status FROM bills WHERE id = NEW.bill_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'BILL_IMMUTABLE: lines can only be changed on a DRAFT bill');
END;

CREATE TRIGGER trg_bill_lines_delete_draft_only
BEFORE DELETE ON bill_lines
WHEN (SELECT status FROM bills WHERE id = OLD.bill_id) <> 'DRAFT'
BEGIN
    SELECT RAISE(ABORT, 'BILL_IMMUTABLE: lines can only be removed from a DRAFT bill');
END;

CREATE TRIGGER trg_billpay_insert_checks
BEFORE INSERT ON bill_payments
BEGIN
    SELECT RAISE(ABORT, 'BILL_NOT_OPEN: payments can only be applied to open bills')
    WHERE (SELECT status FROM bills WHERE id = NEW.bill_id) NOT IN ('UNPAID', 'PARTIAL');
    SELECT RAISE(ABORT, 'BILL_OVERPAYMENT: payment exceeds the open balance')
    WHERE NEW.amount_cents > (SELECT total_cents - amount_paid_cents FROM bills WHERE id = NEW.bill_id);
    SELECT RAISE(ABORT, 'BILL_PAYMENT_BEFORE_BILL: payment date is earlier than the bill date')
    WHERE NEW.payment_date < (SELECT bill_date FROM bills WHERE id = NEW.bill_id);
    SELECT RAISE(ABORT, 'BILL_BAD_PAYMENT_ACCOUNT: pay bills from a Bank or Credit Card account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.payment_account_id) NOT IN ('BANK', 'CREDIT_CARD');
    SELECT RAISE(ABORT, 'BILL_PAYMENT_PREPOSTED: insert the payment first, then attach its journal entry')
    WHERE NEW.journal_entry_id IS NOT NULL;
END;

CREATE TRIGGER trg_billpay_attach_entry_only
BEFORE UPDATE ON bill_payments
BEGIN
    SELECT RAISE(ABORT, 'BILL_PAYMENT_IMMUTABLE: recorded payments cannot be changed')
    WHERE OLD.journal_entry_id IS NOT NULL
       OR NEW.journal_entry_id IS NULL
       OR NEW.bill_id IS NOT OLD.bill_id
       OR NEW.payment_date IS NOT OLD.payment_date
       OR NEW.amount_cents IS NOT OLD.amount_cents
       OR NEW.payment_account_id IS NOT OLD.payment_account_id
       OR NEW.reference IS NOT OLD.reference;
    SELECT RAISE(ABORT, 'BILL_PAYMENT_ENTRY_MISMATCH: payment must reference its POSTED journal entry')
    WHERE NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = 'BILL_PAYMENT' AND source_id = NEW.id
    );
END;

CREATE TRIGGER trg_billpay_no_delete
BEFORE DELETE ON bill_payments
BEGIN
    SELECT RAISE(ABORT, 'BILL_PAYMENT_IMMUTABLE: recorded payments cannot be deleted');
END;
