-- Credits, refunds and fuller customer and vendor records.
--
-- Customer credits
-- * A credit memo credits a customer: Dr each income line / Cr Accounts Receivable. With a refund
--   account it is a refund receipt instead (money paid back straight away): Dr income / Cr that
--   bank or card account, and there is no credit left to use.
-- * Credit is used by applying it to the customer's open invoices (credit_applications). Both sides
--   are already in Accounts Receivable, so an application posts nothing; it counts toward the
--   invoice's amount paid like a payment. Removing an application (is_voided) restores the balance.
-- * Credit left over can be refunded by check or card (credit_refunds): Dr A/R / Cr the account,
--   as its own journal entry. Voiding that entry gives the credit back.
-- A credit memo's remaining credit is its total less live applications and live refunds; open
-- invoices less remaining credits equals the Accounts Receivable balance.
--
-- Vendor credits mirror this for bills: Dr Accounts Payable / Cr each expense line, applied to the
-- vendor's open bills, or refunded to us (Dr the bank account / Cr A/P).
--
-- Credit memos and vendor credits are in the home currency.

-- ---------------------------------------------------------------------------------------------
-- Customer and vendor details
-- ---------------------------------------------------------------------------------------------
ALTER TABLE customers ADD COLUMN company_name TEXT;
ALTER TABLE customers ADD COLUMN phone TEXT;
ALTER TABLE customers ADD COLUMN billing_address TEXT;
ALTER TABLE customers ADD COLUMN terms_days INTEGER CHECK (terms_days IS NULL OR terms_days BETWEEN 0 AND 365);
ALTER TABLE customers ADD COLUMN notes TEXT;

ALTER TABLE vendors ADD COLUMN company_name TEXT;
ALTER TABLE vendors ADD COLUMN phone TEXT;
ALTER TABLE vendors ADD COLUMN address TEXT;
ALTER TABLE vendors ADD COLUMN terms_days INTEGER CHECK (terms_days IS NULL OR terms_days BETWEEN 0 AND 365);
ALTER TABLE vendors ADD COLUMN tax_id TEXT;
ALTER TABLE vendors ADD COLUMN is_1099 INTEGER NOT NULL DEFAULT 0 CHECK (is_1099 IN (0, 1));
ALTER TABLE vendors ADD COLUMN notes TEXT;

-- ---------------------------------------------------------------------------------------------
-- Credit memos and refund receipts
-- ---------------------------------------------------------------------------------------------
CREATE TABLE credit_memos (
    id                  INTEGER PRIMARY KEY,
    credit_number       INTEGER NOT NULL UNIQUE CHECK (credit_number > 0),
    customer_id         INTEGER NOT NULL REFERENCES customers (id) ON DELETE RESTRICT,
    credit_date         TEXT    NOT NULL CHECK (date(credit_date) = credit_date),
    memo                TEXT,
    -- Set for a refund receipt: the money went straight back out of this account.
    refund_account_id   INTEGER REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    total_cents         INTEGER NOT NULL CHECK (total_cents BETWEEN 1 AND 100000000000000),
    journal_entry_id    INTEGER UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    is_voided           INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX ix_credit_memos_customer ON credit_memos (customer_id, credit_date);

CREATE TABLE credit_memo_lines (
    id                  INTEGER PRIMARY KEY,
    credit_memo_id      INTEGER NOT NULL REFERENCES credit_memos (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    product_service_id  INTEGER REFERENCES products_services (id) ON DELETE RESTRICT,
    description         TEXT    NOT NULL,
    quantity_e4         INTEGER NOT NULL CHECK (quantity_e4 BETWEEN 1 AND 10000000000000),
    rate_cents          INTEGER NOT NULL CHECK (rate_cents BETWEEN 1 AND 100000000000000),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    income_account_id   INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    class_id            INTEGER REFERENCES classes (id) ON DELETE RESTRICT,
    CHECK (amount_cents = (quantity_e4 * rate_cents + 5000) / 10000),
    UNIQUE (credit_memo_id, line_number)
) STRICT;

CREATE TABLE credit_applications (
    id                  INTEGER PRIMARY KEY,
    credit_memo_id      INTEGER NOT NULL REFERENCES credit_memos (id) ON DELETE RESTRICT,
    invoice_id          INTEGER NOT NULL REFERENCES invoices (id) ON DELETE RESTRICT,
    applied_date        TEXT    NOT NULL CHECK (date(applied_date) = applied_date),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    is_voided           INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX ix_credit_applications_invoice ON credit_applications (invoice_id);
CREATE INDEX ix_credit_applications_memo ON credit_applications (credit_memo_id);

CREATE TABLE credit_refunds (
    id                  INTEGER PRIMARY KEY,
    credit_memo_id      INTEGER NOT NULL REFERENCES credit_memos (id) ON DELETE RESTRICT,
    account_id          INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    refund_date         TEXT    NOT NULL CHECK (date(refund_date) = refund_date),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    reference           TEXT,
    journal_entry_id    INTEGER NOT NULL UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX ix_credit_refunds_memo ON credit_refunds (credit_memo_id);

-- ---------------------------------------------------------------------------------------------
-- Vendor credits
-- ---------------------------------------------------------------------------------------------
CREATE TABLE vendor_credits (
    id                  INTEGER PRIMARY KEY,
    vendor_id           INTEGER NOT NULL REFERENCES vendors (id) ON DELETE RESTRICT,
    reference           TEXT,
    credit_date         TEXT    NOT NULL CHECK (date(credit_date) = credit_date),
    memo                TEXT,
    total_cents         INTEGER NOT NULL CHECK (total_cents BETWEEN 1 AND 100000000000000),
    journal_entry_id    INTEGER UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    is_voided           INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX ix_vendor_credits_vendor ON vendor_credits (vendor_id, credit_date);

CREATE TABLE vendor_credit_lines (
    id                  INTEGER PRIMARY KEY,
    vendor_credit_id    INTEGER NOT NULL REFERENCES vendor_credits (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    account_id          INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    memo                TEXT,
    class_id            INTEGER REFERENCES classes (id) ON DELETE RESTRICT,
    UNIQUE (vendor_credit_id, line_number)
) STRICT;

CREATE TABLE vendor_credit_applications (
    id                  INTEGER PRIMARY KEY,
    vendor_credit_id    INTEGER NOT NULL REFERENCES vendor_credits (id) ON DELETE RESTRICT,
    bill_id             INTEGER NOT NULL REFERENCES bills (id) ON DELETE RESTRICT,
    applied_date        TEXT    NOT NULL CHECK (date(applied_date) = applied_date),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    is_voided           INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX ix_vendor_credit_applications_bill ON vendor_credit_applications (bill_id);
CREATE INDEX ix_vendor_credit_applications_credit ON vendor_credit_applications (vendor_credit_id);

CREATE TABLE vendor_credit_refunds (
    id                  INTEGER PRIMARY KEY,
    vendor_credit_id    INTEGER NOT NULL REFERENCES vendor_credits (id) ON DELETE RESTRICT,
    account_id          INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    refund_date         TEXT    NOT NULL CHECK (date(refund_date) = refund_date),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    reference           TEXT,
    journal_entry_id    INTEGER NOT NULL UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX ix_vendor_credit_refunds_credit ON vendor_credit_refunds (vendor_credit_id);

-- ---------------------------------------------------------------------------------------------
-- Posted credits are immutable, like every other posted document
-- ---------------------------------------------------------------------------------------------
CREATE TRIGGER trg_credit_memo_immutable
BEFORE UPDATE ON credit_memos
WHEN OLD.journal_entry_id IS NOT NULL
 AND (   NEW.credit_number IS NOT OLD.credit_number OR NEW.customer_id IS NOT OLD.customer_id
      OR NEW.credit_date IS NOT OLD.credit_date OR NEW.total_cents IS NOT OLD.total_cents
      OR NEW.refund_account_id IS NOT OLD.refund_account_id OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
      OR (OLD.is_voided = 1 AND NEW.is_voided = 0))
BEGIN
    SELECT RAISE(ABORT, 'CM_IMMUTABLE: posted credit memos cannot be altered; void instead');
END;

CREATE TRIGGER trg_credit_memo_lines_immutable_upd
BEFORE UPDATE ON credit_memo_lines
WHEN (SELECT journal_entry_id FROM credit_memos WHERE id = OLD.credit_memo_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'CM_IMMUTABLE: lines of a posted credit memo cannot change');
END;

CREATE TRIGGER trg_credit_memo_lines_immutable_del
BEFORE DELETE ON credit_memo_lines
WHEN (SELECT journal_entry_id FROM credit_memos WHERE id = OLD.credit_memo_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'CM_IMMUTABLE: lines of a posted credit memo cannot change');
END;

CREATE TRIGGER trg_vendor_credit_immutable
BEFORE UPDATE ON vendor_credits
WHEN OLD.journal_entry_id IS NOT NULL
 AND (   NEW.vendor_id IS NOT OLD.vendor_id OR NEW.credit_date IS NOT OLD.credit_date
      OR NEW.total_cents IS NOT OLD.total_cents OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
      OR (OLD.is_voided = 1 AND NEW.is_voided = 0))
BEGIN
    SELECT RAISE(ABORT, 'VC_IMMUTABLE: posted vendor credits cannot be altered; void instead');
END;

CREATE TRIGGER trg_vendor_credit_lines_immutable_upd
BEFORE UPDATE ON vendor_credit_lines
WHEN (SELECT journal_entry_id FROM vendor_credits WHERE id = OLD.vendor_credit_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'VC_IMMUTABLE: lines of a posted vendor credit cannot change');
END;

CREATE TRIGGER trg_vendor_credit_lines_immutable_del
BEFORE DELETE ON vendor_credit_lines
WHEN (SELECT journal_entry_id FROM vendor_credits WHERE id = OLD.vendor_credit_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'VC_IMMUTABLE: lines of a posted vendor credit cannot change');
END;

-- An application only ever goes from live to removed.
CREATE TRIGGER trg_credit_applications_once
BEFORE UPDATE ON credit_applications
WHEN NEW.is_voided < OLD.is_voided OR NEW.amount_cents IS NOT OLD.amount_cents OR NEW.invoice_id IS NOT OLD.invoice_id
  OR NEW.credit_memo_id IS NOT OLD.credit_memo_id
BEGIN
    SELECT RAISE(ABORT, 'CM_APPLICATION_IMMUTABLE: a credit application can only be removed');
END;

CREATE TRIGGER trg_vendor_credit_applications_once
BEFORE UPDATE ON vendor_credit_applications
WHEN NEW.is_voided < OLD.is_voided OR NEW.amount_cents IS NOT OLD.amount_cents OR NEW.bill_id IS NOT OLD.bill_id
  OR NEW.vendor_credit_id IS NOT OLD.vendor_credit_id
BEGIN
    SELECT RAISE(ABORT, 'VC_APPLICATION_IMMUTABLE: a credit application can only be removed');
END;

-- ---------------------------------------------------------------------------------------------
-- Amount paid now counts applied credits as well as payments
-- ---------------------------------------------------------------------------------------------
DROP TRIGGER trg_inv_amount_paid_matches;
CREATE TRIGGER trg_inv_amount_paid_matches
BEFORE UPDATE OF amount_paid_cents ON invoices
BEGIN
    SELECT RAISE(ABORT, 'INV_PAYMENT_UNPOSTED: every payment must be posted before it is applied')
    WHERE EXISTS (SELECT 1 FROM invoice_payments WHERE invoice_id = NEW.id AND journal_entry_id IS NULL);
    SELECT RAISE(ABORT, 'INV_PAID_MISMATCH: amount paid must equal the sum of recorded payments and applied credits')
    WHERE NEW.amount_paid_cents <>
          COALESCE((SELECT SUM(amount_cents) FROM invoice_payments WHERE invoice_id = NEW.id AND is_voided = 0), 0)
        + COALESCE((SELECT SUM(amount_cents) FROM credit_applications WHERE invoice_id = NEW.id AND is_voided = 0), 0);
END;

DROP TRIGGER trg_bill_amount_paid_matches;
CREATE TRIGGER trg_bill_amount_paid_matches
BEFORE UPDATE OF amount_paid_cents ON bills
BEGIN
    SELECT RAISE(ABORT, 'BILL_PAYMENT_UNPOSTED: every payment must be posted before it is applied')
    WHERE EXISTS (SELECT 1 FROM bill_payments WHERE bill_id = NEW.id AND journal_entry_id IS NULL);
    SELECT RAISE(ABORT, 'BILL_PAID_MISMATCH: amount paid must equal the sum of recorded payments and applied credits')
    WHERE NEW.amount_paid_cents <>
          COALESCE((SELECT SUM(amount_cents) FROM bill_payments WHERE bill_id = NEW.id AND is_voided = 0), 0)
        + COALESCE((SELECT SUM(amount_cents) FROM vendor_credit_applications WHERE bill_id = NEW.id AND is_voided = 0), 0);
END;

DROP VIEW v_invoice_balances;
CREATE VIEW v_invoice_balances AS
SELECT
    i.id AS invoice_id,
    COALESCE(i.home_total_cents, i.total_cents) AS home_total_cents,
    COALESCE(i.home_total_cents, i.total_cents)
      - COALESCE((SELECT SUM(COALESCE(p.ar_relief_cents, p.amount_cents))
                  FROM invoice_payments p WHERE p.invoice_id = i.id AND p.is_voided = 0), 0)
      - COALESCE((SELECT SUM(a.amount_cents) FROM credit_applications a WHERE a.invoice_id = i.id AND a.is_voided = 0), 0)
      AS home_open_cents
FROM invoices i;

-- Credit still available on each live credit memo (zero for refund receipts).
CREATE VIEW v_credit_memo_balances AS
SELECT
    m.id AS credit_memo_id,
    m.customer_id,
    CASE WHEN m.refund_account_id IS NOT NULL OR m.is_voided = 1 THEN 0 ELSE
        m.total_cents
        - COALESCE((SELECT SUM(a.amount_cents) FROM credit_applications a WHERE a.credit_memo_id = m.id AND a.is_voided = 0), 0)
        - COALESCE((SELECT SUM(r.amount_cents) FROM credit_refunds r JOIN journal_entries e ON e.id = r.journal_entry_id
                    WHERE r.credit_memo_id = m.id AND e.is_voided = 0), 0)
    END AS remaining_cents
FROM credit_memos m;

CREATE VIEW v_vendor_credit_balances AS
SELECT
    c.id AS vendor_credit_id,
    c.vendor_id,
    CASE WHEN c.is_voided = 1 THEN 0 ELSE
        c.total_cents
        - COALESCE((SELECT SUM(a.amount_cents) FROM vendor_credit_applications a WHERE a.vendor_credit_id = c.id AND a.is_voided = 0), 0)
        - COALESCE((SELECT SUM(r.amount_cents) FROM vendor_credit_refunds r JOIN journal_entries e ON e.id = r.journal_entry_id
                    WHERE r.vendor_credit_id = c.id AND e.is_voided = 0), 0)
    END AS remaining_cents
FROM vendor_credits c;

-- ---------------------------------------------------------------------------------------------
-- The customer or vendor behind each journal entry, for registers and detail reports
-- ---------------------------------------------------------------------------------------------
CREATE VIEW v_entry_party AS
SELECT e.id AS journal_entry_id,
       CASE e.source_type
           WHEN 'INVOICE'         THEN (SELECT c.name FROM invoices d JOIN customers c ON c.id = d.customer_id WHERE d.id = e.source_id)
           WHEN 'RECEIVE_PAYMENT' THEN (SELECT c.name FROM invoice_payments p JOIN invoices d ON d.id = p.invoice_id JOIN customers c ON c.id = d.customer_id WHERE p.id = e.source_id)
           WHEN 'SALES_RECEIPT'   THEN (SELECT c.name FROM sales_receipts d JOIN customers c ON c.id = d.customer_id WHERE d.id = e.source_id)
           WHEN 'CREDIT_MEMO'     THEN (SELECT c.name FROM credit_memos d JOIN customers c ON c.id = d.customer_id WHERE d.id = e.source_id)
           WHEN 'BILL'            THEN (SELECT v.name FROM bills d JOIN vendors v ON v.id = d.vendor_id WHERE d.id = e.source_id)
           WHEN 'BILL_PAYMENT'    THEN (SELECT v.name FROM bill_payments p JOIN bills d ON d.id = p.bill_id JOIN vendors v ON v.id = d.vendor_id WHERE p.id = e.source_id)
           WHEN 'VENDOR_CREDIT'   THEN (SELECT v.name FROM vendor_credits d JOIN vendors v ON v.id = d.vendor_id WHERE d.id = e.source_id)
           WHEN 'EXPENSE'         THEN (SELECT v.name FROM expenses d JOIN vendors v ON v.id = d.vendor_id WHERE d.id = e.source_id)
           WHEN 'CHECK'           THEN (SELECT v.name FROM expenses d JOIN vendors v ON v.id = d.vendor_id WHERE d.id = e.source_id)
           WHEN 'JOURNAL'         THEN COALESCE(
               (SELECT c.name FROM credit_refunds r JOIN credit_memos d ON d.id = r.credit_memo_id JOIN customers c ON c.id = d.customer_id WHERE r.journal_entry_id = e.id),
               (SELECT v.name FROM vendor_credit_refunds r JOIN vendor_credits d ON d.id = r.vendor_credit_id JOIN vendors v ON v.id = d.vendor_id WHERE r.journal_entry_id = e.id))
       END AS party_name
FROM journal_entries e;
