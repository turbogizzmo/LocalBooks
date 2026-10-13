-- 0008_classic_parity.sql
-- Voiding with reversing entries + audit log, split-line expenses and checks, bank deposits,
-- transfers, estimates, and employee time tracking.
--
-- Voiding model: a voided entry stays POSTED and gains is_voided = 1; a reversing entry
-- (reverses_entry_id = original) dated the same day nets it to zero. Source documents carry their
-- own is_voided flag, which may only be set once their journal entry is voided.
--
-- Bank feed approvals now create real Expense / Deposit documents. Existing approvals are converted
-- here, reusing the bank line id as the document id because their posted entries already carry it as
-- source_id.

-- =============================================================================================
-- New tables first (triggers further down reference them)
-- =============================================================================================

CREATE TABLE audit_log (
    id                  INTEGER PRIMARY KEY,
    occurred_at         TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    action              TEXT    NOT NULL CHECK (action IN ('POST', 'VOID', 'LOCK_RECONCILIATION')),
    journal_entry_id    INTEGER REFERENCES journal_entries (id) ON DELETE RESTRICT,
    reconciliation_id   INTEGER REFERENCES reconciliations (id) ON DELETE RESTRICT,
    source_type         TEXT,
    source_id           INTEGER,
    amount_cents        INTEGER,
    detail              TEXT    NOT NULL
) STRICT;
CREATE INDEX ix_audit_log_time ON audit_log (occurred_at);

CREATE TABLE expenses (
    id                  INTEGER PRIMARY KEY,
    kind                TEXT    NOT NULL CHECK (kind IN ('EXPENSE', 'CHECK')),
    vendor_id           INTEGER REFERENCES vendors (id) ON DELETE RESTRICT,
    payment_account_id  INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    expense_date        TEXT    NOT NULL CHECK (
                            expense_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(expense_date) = expense_date
                        ),
    check_number        TEXT,
    payment_method      TEXT    CHECK (payment_method IS NULL OR payment_method IN ('CASH', 'CHECK', 'CREDIT_CARD', 'ACH', 'WIRE', 'OTHER')),
    memo                TEXT,
    total_cents         INTEGER NOT NULL CHECK (total_cents BETWEEN 1 AND 100000000000000),
    journal_entry_id    INTEGER UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    is_voided           INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (kind = 'CHECK' OR check_number IS NULL)
) STRICT;
CREATE INDEX ix_expenses_date ON expenses (expense_date);

CREATE TABLE expense_lines (
    id              INTEGER PRIMARY KEY,
    expense_id      INTEGER NOT NULL REFERENCES expenses (id) ON DELETE CASCADE,
    line_number     INTEGER NOT NULL CHECK (line_number > 0),
    account_id      INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    amount_cents    INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    memo            TEXT,
    UNIQUE (expense_id, line_number)
) STRICT;

CREATE TABLE deposits (
    id                  INTEGER PRIMARY KEY,
    bank_account_id     INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    deposit_date        TEXT    NOT NULL CHECK (
                            deposit_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(deposit_date) = deposit_date
                        ),
    memo                TEXT,
    total_cents         INTEGER NOT NULL CHECK (total_cents BETWEEN 1 AND 100000000000000),
    journal_entry_id    INTEGER UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    is_voided           INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX ix_deposits_date ON deposits (deposit_date);

CREATE TABLE deposit_lines (
    id                  INTEGER PRIMARY KEY,
    deposit_id          INTEGER NOT NULL REFERENCES deposits (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    invoice_payment_id  INTEGER REFERENCES invoice_payments (id) ON DELETE RESTRICT,
    sales_receipt_id    INTEGER REFERENCES sales_receipts (id) ON DELETE RESTRICT,
    account_id          INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    memo                TEXT,
    is_voided           INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1)),
    CHECK (invoice_payment_id IS NULL OR sales_receipt_id IS NULL),
    UNIQUE (deposit_id, line_number)
) STRICT;
-- A received payment can sit in at most one live deposit.
CREATE UNIQUE INDEX ux_deposit_lines_payment ON deposit_lines (invoice_payment_id)
    WHERE invoice_payment_id IS NOT NULL AND is_voided = 0;
CREATE UNIQUE INDEX ux_deposit_lines_receipt ON deposit_lines (sales_receipt_id)
    WHERE sales_receipt_id IS NOT NULL AND is_voided = 0;

CREATE TABLE transfers (
    id                  INTEGER PRIMARY KEY,
    transfer_date       TEXT    NOT NULL CHECK (
                            transfer_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(transfer_date) = transfer_date
                        ),
    from_account_id     INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    to_account_id       INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    memo                TEXT,
    journal_entry_id    INTEGER UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    is_voided           INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (from_account_id <> to_account_id)
) STRICT;

CREATE TABLE estimates (
    id                  INTEGER PRIMARY KEY,
    estimate_number     INTEGER NOT NULL UNIQUE CHECK (estimate_number > 0),
    customer_id         INTEGER NOT NULL REFERENCES customers (id) ON DELETE RESTRICT,
    estimate_date       TEXT    NOT NULL CHECK (
                            estimate_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(estimate_date) = estimate_date
                        ),
    expiration_date     TEXT    CHECK (expiration_date IS NULL OR date(expiration_date) = expiration_date),
    status              TEXT    NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'ACCEPTED', 'REJECTED', 'CLOSED')),
    memo                TEXT,
    total_cents         INTEGER NOT NULL CHECK (total_cents BETWEEN 1 AND 100000000000000),
    invoice_id          INTEGER UNIQUE REFERENCES invoices (id) ON DELETE RESTRICT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (expiration_date IS NULL OR expiration_date >= estimate_date),
    CHECK ((status = 'CLOSED') = (invoice_id IS NOT NULL))
) STRICT;

CREATE TABLE estimate_lines (
    id                  INTEGER PRIMARY KEY,
    estimate_id         INTEGER NOT NULL REFERENCES estimates (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    product_service_id  INTEGER REFERENCES products_services (id) ON DELETE RESTRICT,
    description         TEXT    NOT NULL,
    quantity            INTEGER NOT NULL CHECK (quantity BETWEEN 1 AND 1000000000),
    rate_cents          INTEGER NOT NULL CHECK (rate_cents BETWEEN 1 AND 100000000000000),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    income_account_id   INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    CHECK (amount_cents = quantity * rate_cents),
    UNIQUE (estimate_id, line_number)
) STRICT;

CREATE TABLE employees (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    email       TEXT    CHECK (email IS NULL OR email LIKE '%_@_%._%'),
    is_active   INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_employees_name ON employees (name COLLATE NOCASE);

CREATE TABLE time_activities (
    id                  INTEGER PRIMARY KEY,
    employee_id         INTEGER NOT NULL REFERENCES employees (id) ON DELETE RESTRICT,
    activity_date       TEXT    NOT NULL CHECK (
                            activity_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(activity_date) = activity_date
                        ),
    customer_id         INTEGER REFERENCES customers (id) ON DELETE RESTRICT,
    product_service_id  INTEGER REFERENCES products_services (id) ON DELETE RESTRICT,
    minutes             INTEGER NOT NULL CHECK (minutes BETWEEN 1 AND 1440),
    billable            INTEGER NOT NULL DEFAULT 0 CHECK (billable IN (0, 1)),
    hourly_rate_cents   INTEGER CHECK (hourly_rate_cents IS NULL OR hourly_rate_cents BETWEEN 0 AND 100000000000),
    description         TEXT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (billable = 0 OR (customer_id IS NOT NULL AND hourly_rate_cents IS NOT NULL))
) STRICT;
CREATE INDEX ix_time_employee_date ON time_activities (employee_id, activity_date);

-- =============================================================================================
-- Convert existing bank feed approvals into Expense / Deposit documents
-- =============================================================================================
INSERT INTO expenses (id, kind, payment_account_id, expense_date, memo, total_cents, journal_entry_id)
SELECT id, 'EXPENSE', bank_account_id, txn_date, description, -amount_cents, journal_entry_id
FROM bank_transactions WHERE status = 'APPROVED' AND amount_cents < 0;

INSERT INTO expense_lines (expense_id, line_number, account_id, amount_cents, memo)
SELECT id, 1, category_account_id, -amount_cents, description
FROM bank_transactions WHERE status = 'APPROVED' AND amount_cents < 0;

INSERT INTO deposits (id, bank_account_id, deposit_date, memo, total_cents, journal_entry_id)
SELECT id, bank_account_id, txn_date, description, amount_cents, journal_entry_id
FROM bank_transactions WHERE status = 'APPROVED' AND amount_cents > 0;

INSERT INTO deposit_lines (deposit_id, line_number, account_id, amount_cents, memo)
SELECT id, 1, category_account_id, amount_cents, description
FROM bank_transactions WHERE status = 'APPROVED' AND amount_cents > 0;

-- =============================================================================================
-- Journal entry voiding + audit log
-- =============================================================================================
ALTER TABLE journal_entries ADD COLUMN is_voided INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1));

CREATE TRIGGER trg_je_void_flag
BEFORE UPDATE OF is_voided ON journal_entries
WHEN NEW.is_voided IS NOT OLD.is_voided
BEGIN
    SELECT RAISE(ABORT, 'JE_UNVOID: a voided entry cannot be restored')
    WHERE OLD.is_voided = 1;
    SELECT RAISE(ABORT, 'JE_VOID_NOT_POSTED: only posted entries can be voided')
    WHERE OLD.status <> 'POSTED';
    SELECT RAISE(ABORT, 'JE_VOID_REVERSAL: a reversing entry cannot itself be voided')
    WHERE OLD.reverses_entry_id IS NOT NULL;
    SELECT RAISE(ABORT, 'JE_VOID_NO_REVERSAL: the reversing entry must be posted before the void is flagged')
    WHERE NOT EXISTS (SELECT 1 FROM journal_entries WHERE reverses_entry_id = OLD.id AND status = 'POSTED');
    SELECT RAISE(ABORT, 'REC_LOCKED: this transaction is cleared or reconciled and cannot be voided')
    WHERE EXISTS (SELECT 1 FROM journal_lines WHERE journal_entry_id = OLD.id AND reconciliation_id IS NOT NULL);
END;

CREATE TRIGGER trg_jl_reconcile_not_voided
BEFORE UPDATE OF reconciliation_id ON journal_lines
WHEN NEW.reconciliation_id IS NOT NULL
 AND EXISTS (SELECT 1 FROM journal_entries WHERE id = NEW.journal_entry_id AND (is_voided = 1 OR reverses_entry_id IS NOT NULL))
BEGIN
    SELECT RAISE(ABORT, 'REC_VOIDED: voided transactions and their reversals net to zero and are not reconciled');
END;

CREATE TRIGGER trg_audit_post
AFTER UPDATE OF status ON journal_entries
WHEN OLD.status = 'DRAFT' AND NEW.status = 'POSTED'
BEGIN
    INSERT INTO audit_log (action, journal_entry_id, source_type, source_id, amount_cents, detail)
    VALUES ('POST', NEW.id, NEW.source_type, NEW.source_id,
            (SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = NEW.id),
            'Posted entry #' || NEW.entry_number || ' dated ' || NEW.entry_date || COALESCE(': ' || NEW.memo, ''));
END;

CREATE TRIGGER trg_audit_void
AFTER UPDATE OF is_voided ON journal_entries
WHEN OLD.is_voided = 0 AND NEW.is_voided = 1
BEGIN
    INSERT INTO audit_log (action, journal_entry_id, source_type, source_id, amount_cents, detail)
    VALUES ('VOID', NEW.id, NEW.source_type, NEW.source_id,
            (SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = NEW.id),
            'Voided entry #' || NEW.entry_number || ' (reversed by entry #' ||
            (SELECT entry_number FROM journal_entries WHERE reverses_entry_id = NEW.id) || ')' ||
            COALESCE(': ' || NEW.memo, ''));
END;

CREATE TRIGGER trg_audit_reconcile
AFTER UPDATE OF is_locked ON reconciliations
WHEN OLD.is_locked = 0 AND NEW.is_locked = 1
BEGIN
    INSERT INTO audit_log (action, reconciliation_id, amount_cents, detail)
    VALUES ('LOCK_RECONCILIATION', NEW.id, NEW.statement_ending_balance_cents,
            'Reconciled ' || (SELECT name FROM chart_of_accounts WHERE id = NEW.account_id) ||
            ' through ' || NEW.statement_end_date);
END;

CREATE TRIGGER trg_audit_immutable_upd BEFORE UPDATE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the audit log cannot be changed');
END;
CREATE TRIGGER trg_audit_immutable_del BEFORE DELETE ON audit_log
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the audit log cannot be changed');
END;

-- Backfill history for everything posted before the log existed.
INSERT INTO audit_log (occurred_at, action, journal_entry_id, source_type, source_id, amount_cents, detail)
SELECT e.posted_at, 'POST', e.id, e.source_type, e.source_id,
       (SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = e.id),
       'Posted entry #' || e.entry_number || ' dated ' || e.entry_date || COALESCE(': ' || e.memo, '')
FROM journal_entries e WHERE e.status IN ('POSTED', 'VOID') ORDER BY e.id;
INSERT INTO audit_log (occurred_at, action, reconciliation_id, amount_cents, detail)
SELECT r.locked_at, 'LOCK_RECONCILIATION', r.id, r.statement_ending_balance_cents,
       'Reconciled ' || (SELECT name FROM chart_of_accounts WHERE id = r.account_id) || ' through ' || r.statement_end_date
FROM reconciliations r WHERE r.is_locked = 1 ORDER BY r.locked_at;

-- =============================================================================================
-- Bill lines: account_id / memo naming
-- =============================================================================================
ALTER TABLE bill_lines RENAME COLUMN expense_account_id TO account_id;
ALTER TABLE bill_lines RENAME COLUMN description TO memo;

-- =============================================================================================
-- Document void flags
-- =============================================================================================
ALTER TABLE invoices ADD COLUMN is_voided INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1));
ALTER TABLE invoice_payments ADD COLUMN is_voided INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1));
ALTER TABLE sales_receipts ADD COLUMN is_voided INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1));
ALTER TABLE bills ADD COLUMN is_voided INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1));
ALTER TABLE bill_payments ADD COLUMN is_voided INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1));

-- ---------------------------------------------------------------------------------------------
-- Invoices: payments may now be voided, so paid amounts can move back down.
-- ---------------------------------------------------------------------------------------------
DROP TRIGGER trg_inv_status_transition;
CREATE TRIGGER trg_inv_status_transition
BEFORE UPDATE OF status ON invoices
WHEN NOT (
       (OLD.status = 'DRAFT' AND NEW.status IN ('DRAFT', 'UNPAID'))
    OR (OLD.status IN ('UNPAID', 'PARTIAL', 'PAID') AND NEW.status IN ('UNPAID', 'PARTIAL', 'PAID'))
)
BEGIN
    SELECT RAISE(ABORT, 'INV_INVALID_TRANSITION: a posted invoice cannot return to DRAFT');
END;

DROP TRIGGER trg_inv_amount_paid_matches;
CREATE TRIGGER trg_inv_amount_paid_matches
BEFORE UPDATE OF amount_paid_cents ON invoices
BEGIN
    SELECT RAISE(ABORT, 'INV_PAYMENT_UNPOSTED: every payment must be posted before it is applied')
    WHERE EXISTS (SELECT 1 FROM invoice_payments WHERE invoice_id = NEW.id AND journal_entry_id IS NULL);
    SELECT RAISE(ABORT, 'INV_PAID_MISMATCH: amount paid must equal the sum of recorded payments')
    WHERE NEW.amount_paid_cents <> COALESCE(
        (SELECT SUM(amount_cents) FROM invoice_payments WHERE invoice_id = NEW.id AND is_voided = 0), 0);
END;

CREATE TRIGGER trg_inv_void
BEFORE UPDATE OF is_voided ON invoices
WHEN NEW.is_voided IS NOT OLD.is_voided
BEGIN
    SELECT RAISE(ABORT, 'INV_UNVOID: a voided invoice cannot be restored') WHERE OLD.is_voided = 1;
    SELECT RAISE(ABORT, 'INV_VOID_HAS_PAYMENTS: void the payments applied to this invoice first')
    WHERE OLD.amount_paid_cents <> 0;
    SELECT RAISE(ABORT, 'INV_VOID_ENTRY_ACTIVE: the invoice''s journal entry must be voided first')
    WHERE NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
END;

DROP TRIGGER trg_pay_insert_checks;
CREATE TRIGGER trg_pay_insert_checks
BEFORE INSERT ON invoice_payments
BEGIN
    SELECT RAISE(ABORT, 'INV_NOT_OPEN: payments can only be applied to open invoices')
    WHERE (SELECT status FROM invoices WHERE id = NEW.invoice_id) NOT IN ('UNPAID', 'PARTIAL')
       OR (SELECT is_voided FROM invoices WHERE id = NEW.invoice_id) = 1;
    SELECT RAISE(ABORT, 'INV_OVERPAYMENT: payment exceeds the open balance')
    WHERE NEW.amount_cents > (SELECT total_cents - amount_paid_cents FROM invoices WHERE id = NEW.invoice_id);
    SELECT RAISE(ABORT, 'INV_PAYMENT_BEFORE_ISSUE: payment date is earlier than the invoice date')
    WHERE NEW.payment_date < (SELECT issue_date FROM invoices WHERE id = NEW.invoice_id);
    SELECT RAISE(ABORT, 'INV_BAD_DEPOSIT_ACCOUNT: deposit to a Bank or Undeposited Funds account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.deposit_account_id) NOT IN ('BANK', 'OTHER_CURRENT_ASSET');
    SELECT RAISE(ABORT, 'INV_PAYMENT_PREPOSTED: insert the payment first, then attach its journal entry')
    WHERE NEW.journal_entry_id IS NOT NULL OR NEW.is_voided <> 0;
END;

DROP TRIGGER trg_pay_attach_entry_only;
CREATE TRIGGER trg_pay_update_rules
BEFORE UPDATE ON invoice_payments
BEGIN
    SELECT RAISE(ABORT, 'INV_PAYMENT_IMMUTABLE: recorded payments cannot be changed')
    WHERE NEW.invoice_id IS NOT OLD.invoice_id
       OR NEW.payment_date IS NOT OLD.payment_date
       OR NEW.amount_cents IS NOT OLD.amount_cents
       OR NEW.payment_method IS NOT OLD.payment_method
       OR NEW.reference IS NOT OLD.reference
       OR NEW.deposit_account_id IS NOT OLD.deposit_account_id
       OR NEW.created_at IS NOT OLD.created_at
       OR (OLD.journal_entry_id IS NOT NULL AND NEW.journal_entry_id IS NOT OLD.journal_entry_id)
       OR NEW.journal_entry_id IS NULL;
    SELECT RAISE(ABORT, 'INV_PAYMENT_ENTRY_MISMATCH: payment must reference its POSTED journal entry')
    WHERE OLD.journal_entry_id IS NULL AND NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = 'RECEIVE_PAYMENT' AND source_id = NEW.id
    );
    SELECT RAISE(ABORT, 'INV_PAYMENT_UNVOID: a voided payment cannot be restored')
    WHERE OLD.is_voided = 1 AND NEW.is_voided = 0;
    SELECT RAISE(ABORT, 'INV_PAYMENT_VOID_ENTRY_ACTIVE: the payment''s journal entry must be voided first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
    SELECT RAISE(ABORT, 'INV_PAYMENT_DEPOSITED: void the bank deposit that includes this payment first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND EXISTS (SELECT 1 FROM deposit_lines WHERE invoice_payment_id = OLD.id AND is_voided = 0);
END;

-- ---------------------------------------------------------------------------------------------
-- Sales receipts
-- ---------------------------------------------------------------------------------------------
DROP TRIGGER trg_sr_attach_entry;
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

-- ---------------------------------------------------------------------------------------------
-- Bills and bill payments
-- ---------------------------------------------------------------------------------------------
DROP TRIGGER trg_bill_status_transition;
CREATE TRIGGER trg_bill_status_transition
BEFORE UPDATE OF status ON bills
WHEN NOT (
       (OLD.status = 'DRAFT' AND NEW.status IN ('DRAFT', 'UNPAID'))
    OR (OLD.status IN ('UNPAID', 'PARTIAL', 'PAID') AND NEW.status IN ('UNPAID', 'PARTIAL', 'PAID'))
)
BEGIN
    SELECT RAISE(ABORT, 'BILL_INVALID_TRANSITION: a posted bill cannot return to DRAFT');
END;

DROP TRIGGER trg_bill_amount_paid_matches;
CREATE TRIGGER trg_bill_amount_paid_matches
BEFORE UPDATE OF amount_paid_cents ON bills
BEGIN
    SELECT RAISE(ABORT, 'BILL_PAYMENT_UNPOSTED: every payment must be posted before it is applied')
    WHERE EXISTS (SELECT 1 FROM bill_payments WHERE bill_id = NEW.id AND journal_entry_id IS NULL);
    SELECT RAISE(ABORT, 'BILL_PAID_MISMATCH: amount paid must equal the sum of recorded payments')
    WHERE NEW.amount_paid_cents <> COALESCE(
        (SELECT SUM(amount_cents) FROM bill_payments WHERE bill_id = NEW.id AND is_voided = 0), 0);
END;

CREATE TRIGGER trg_bill_void
BEFORE UPDATE OF is_voided ON bills
WHEN NEW.is_voided IS NOT OLD.is_voided
BEGIN
    SELECT RAISE(ABORT, 'BILL_UNVOID: a voided bill cannot be restored') WHERE OLD.is_voided = 1;
    SELECT RAISE(ABORT, 'BILL_VOID_HAS_PAYMENTS: void the payments made on this bill first')
    WHERE OLD.amount_paid_cents <> 0;
    SELECT RAISE(ABORT, 'BILL_VOID_ENTRY_ACTIVE: the bill''s journal entry must be voided first')
    WHERE NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
END;

DROP TRIGGER trg_billpay_insert_checks;
CREATE TRIGGER trg_billpay_insert_checks
BEFORE INSERT ON bill_payments
BEGIN
    SELECT RAISE(ABORT, 'BILL_NOT_OPEN: payments can only be applied to open bills')
    WHERE (SELECT status FROM bills WHERE id = NEW.bill_id) NOT IN ('UNPAID', 'PARTIAL')
       OR (SELECT is_voided FROM bills WHERE id = NEW.bill_id) = 1;
    SELECT RAISE(ABORT, 'BILL_OVERPAYMENT: payment exceeds the open balance')
    WHERE NEW.amount_cents > (SELECT total_cents - amount_paid_cents FROM bills WHERE id = NEW.bill_id);
    SELECT RAISE(ABORT, 'BILL_PAYMENT_BEFORE_BILL: payment date is earlier than the bill date')
    WHERE NEW.payment_date < (SELECT bill_date FROM bills WHERE id = NEW.bill_id);
    SELECT RAISE(ABORT, 'BILL_BAD_PAYMENT_ACCOUNT: pay bills from a Bank or Credit Card account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.payment_account_id) NOT IN ('BANK', 'CREDIT_CARD');
    SELECT RAISE(ABORT, 'BILL_PAYMENT_PREPOSTED: insert the payment first, then attach its journal entry')
    WHERE NEW.journal_entry_id IS NOT NULL OR NEW.is_voided <> 0;
END;

DROP TRIGGER trg_billpay_attach_entry_only;
CREATE TRIGGER trg_billpay_update_rules
BEFORE UPDATE ON bill_payments
BEGIN
    SELECT RAISE(ABORT, 'BILL_PAYMENT_IMMUTABLE: recorded payments cannot be changed')
    WHERE NEW.bill_id IS NOT OLD.bill_id
       OR NEW.payment_date IS NOT OLD.payment_date
       OR NEW.amount_cents IS NOT OLD.amount_cents
       OR NEW.payment_account_id IS NOT OLD.payment_account_id
       OR NEW.reference IS NOT OLD.reference
       OR NEW.created_at IS NOT OLD.created_at
       OR (OLD.journal_entry_id IS NOT NULL AND NEW.journal_entry_id IS NOT OLD.journal_entry_id)
       OR NEW.journal_entry_id IS NULL;
    SELECT RAISE(ABORT, 'BILL_PAYMENT_ENTRY_MISMATCH: payment must reference its POSTED journal entry')
    WHERE OLD.journal_entry_id IS NULL AND NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = 'BILL_PAYMENT' AND source_id = NEW.id
    );
    SELECT RAISE(ABORT, 'BILL_PAYMENT_UNVOID: a voided payment cannot be restored')
    WHERE OLD.is_voided = 1 AND NEW.is_voided = 0;
    SELECT RAISE(ABORT, 'BILL_PAYMENT_VOID_ENTRY_ACTIVE: the payment''s journal entry must be voided first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
END;

-- ---------------------------------------------------------------------------------------------
-- Bank feed lines return to review when the document created from them is voided.
-- ---------------------------------------------------------------------------------------------
DROP TRIGGER trg_bank_txn_reviewed_immutable;
CREATE TRIGGER trg_bank_txn_reviewed_immutable
BEFORE UPDATE ON bank_transactions
WHEN OLD.status = 'APPROVED'
BEGIN
    SELECT RAISE(ABORT, 'BANK_TXN_IMMUTABLE: approved bank lines cannot be changed; void the transaction instead')
    WHERE NOT (
            NEW.status = 'PENDING' AND NEW.journal_entry_id IS NULL AND NEW.category_account_id IS NULL
        AND NEW.reviewed_at IS NULL
        AND NEW.bank_account_id IS OLD.bank_account_id AND NEW.txn_date IS OLD.txn_date
        AND NEW.description IS OLD.description AND NEW.amount_cents IS OLD.amount_cents
        AND NEW.fingerprint IS OLD.fingerprint AND NEW.imported_at IS OLD.imported_at
        AND EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1)
    );
END;

-- =============================================================================================
-- Expenses & checks
-- =============================================================================================
CREATE TRIGGER trg_exp_insert_checks
BEFORE INSERT ON expenses
BEGIN
    SELECT RAISE(ABORT, 'EXP_PREPOSTED: insert the expense first, then attach its journal entry')
    WHERE NEW.journal_entry_id IS NOT NULL OR NEW.is_voided <> 0;
    SELECT RAISE(ABORT, 'EXP_BAD_PAYMENT_ACCOUNT: checks are written from a Bank account; expenses from a Bank or Credit Card account')
    WHERE (NEW.kind = 'CHECK' AND (SELECT account_type FROM chart_of_accounts WHERE id = NEW.payment_account_id) IS NOT 'BANK')
       OR (NEW.kind = 'EXPENSE' AND (SELECT account_type FROM chart_of_accounts WHERE id = NEW.payment_account_id) NOT IN ('BANK', 'CREDIT_CARD'));
END;

CREATE TRIGGER trg_exp_update_rules
BEFORE UPDATE ON expenses
BEGIN
    SELECT RAISE(ABORT, 'EXP_IMMUTABLE: posted expenses cannot be changed')
    WHERE OLD.journal_entry_id IS NOT NULL
      AND (   NEW.kind IS NOT OLD.kind OR NEW.vendor_id IS NOT OLD.vendor_id
           OR NEW.payment_account_id IS NOT OLD.payment_account_id OR NEW.expense_date IS NOT OLD.expense_date
           OR NEW.check_number IS NOT OLD.check_number OR NEW.payment_method IS NOT OLD.payment_method
           OR NEW.memo IS NOT OLD.memo OR NEW.total_cents IS NOT OLD.total_cents
           OR NEW.journal_entry_id IS NOT OLD.journal_entry_id OR NEW.created_at IS NOT OLD.created_at);
    SELECT RAISE(ABORT, 'EXP_TOTAL_MISMATCH: expense total must equal the sum of its split lines')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NEW.total_cents IS NOT (SELECT SUM(amount_cents) FROM expense_lines WHERE expense_id = NEW.id);
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

CREATE TRIGGER trg_exp_no_delete_posted
BEFORE DELETE ON expenses WHEN OLD.journal_entry_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'EXP_IMMUTABLE: posted expenses cannot be deleted');
END;

CREATE TRIGGER trg_exp_lines_insert_checks
BEFORE INSERT ON expense_lines
BEGIN
    SELECT RAISE(ABORT, 'EXP_IMMUTABLE: lines can only be added before the expense is posted')
    WHERE (SELECT journal_entry_id FROM expenses WHERE id = NEW.expense_id) IS NOT NULL;
    SELECT RAISE(ABORT, 'EXP_BAD_CATEGORY: a split line cannot use the payment account, A/R or A/P')
    WHERE NEW.account_id = (SELECT payment_account_id FROM expenses WHERE id = NEW.expense_id)
       OR (SELECT account_type FROM chart_of_accounts WHERE id = NEW.account_id) IN ('ACCOUNTS_RECEIVABLE', 'ACCOUNTS_PAYABLE');
END;

CREATE TRIGGER trg_exp_lines_update_unposted
BEFORE UPDATE ON expense_lines
WHEN (SELECT journal_entry_id FROM expenses WHERE id = OLD.expense_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'EXP_IMMUTABLE: lines of a posted expense cannot be changed');
END;

CREATE TRIGGER trg_exp_lines_delete_unposted
BEFORE DELETE ON expense_lines
WHEN (SELECT journal_entry_id FROM expenses WHERE id = OLD.expense_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'EXP_IMMUTABLE: lines of a posted expense cannot be removed');
END;

-- =============================================================================================
-- Deposits
-- =============================================================================================
CREATE TRIGGER trg_dep_insert_checks
BEFORE INSERT ON deposits
BEGIN
    SELECT RAISE(ABORT, 'DEP_PREPOSTED: insert the deposit first, then attach its journal entry')
    WHERE NEW.journal_entry_id IS NOT NULL OR NEW.is_voided <> 0;
    SELECT RAISE(ABORT, 'DEP_BAD_ACCOUNT: deposits go into a Bank or Credit Card account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.bank_account_id) NOT IN ('BANK', 'CREDIT_CARD');
END;

CREATE TRIGGER trg_dep_update_rules
BEFORE UPDATE ON deposits
BEGIN
    SELECT RAISE(ABORT, 'DEP_IMMUTABLE: posted deposits cannot be changed')
    WHERE OLD.journal_entry_id IS NOT NULL
      AND (   NEW.bank_account_id IS NOT OLD.bank_account_id OR NEW.deposit_date IS NOT OLD.deposit_date
           OR NEW.memo IS NOT OLD.memo OR NEW.total_cents IS NOT OLD.total_cents
           OR NEW.journal_entry_id IS NOT OLD.journal_entry_id OR NEW.created_at IS NOT OLD.created_at);
    SELECT RAISE(ABORT, 'DEP_TOTAL_MISMATCH: deposit total must equal the sum of its lines')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NEW.total_cents IS NOT (SELECT SUM(amount_cents) FROM deposit_lines WHERE deposit_id = NEW.id);
    SELECT RAISE(ABORT, 'DEP_JOURNAL_MISSING: deposit must reference its POSTED journal entry')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NOT EXISTS (
          SELECT 1 FROM journal_entries
          WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = 'DEPOSIT' AND source_id = NEW.id
      );
    SELECT RAISE(ABORT, 'DEP_UNVOID: a voided deposit cannot be restored')
    WHERE OLD.is_voided = 1 AND NEW.is_voided = 0;
    SELECT RAISE(ABORT, 'DEP_VOID_ENTRY_ACTIVE: the deposit''s journal entry must be voided first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
END;

CREATE TRIGGER trg_dep_no_delete_posted
BEFORE DELETE ON deposits WHEN OLD.journal_entry_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'DEP_IMMUTABLE: posted deposits cannot be deleted');
END;

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
          AND p.deposit_account_id = NEW.account_id AND p.amount_cents = NEW.amount_cents
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

CREATE TRIGGER trg_dep_lines_update_rules
BEFORE UPDATE ON deposit_lines
BEGIN
    SELECT RAISE(ABORT, 'DEP_IMMUTABLE: deposit lines can only be marked void along with their deposit')
    WHERE NEW.deposit_id IS NOT OLD.deposit_id OR NEW.line_number IS NOT OLD.line_number
       OR NEW.invoice_payment_id IS NOT OLD.invoice_payment_id OR NEW.sales_receipt_id IS NOT OLD.sales_receipt_id
       OR NEW.account_id IS NOT OLD.account_id OR NEW.amount_cents IS NOT OLD.amount_cents OR NEW.memo IS NOT OLD.memo
       OR (OLD.is_voided = 1 AND NEW.is_voided = 0)
       OR (NEW.is_voided = 1 AND (SELECT is_voided FROM deposits WHERE id = OLD.deposit_id) = 0);
END;

CREATE TRIGGER trg_dep_lines_delete_unposted
BEFORE DELETE ON deposit_lines
WHEN (SELECT journal_entry_id FROM deposits WHERE id = OLD.deposit_id) IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'DEP_IMMUTABLE: lines of a posted deposit cannot be removed');
END;

-- =============================================================================================
-- Transfers
-- =============================================================================================
CREATE TRIGGER trg_xfer_insert_checks
BEFORE INSERT ON transfers
BEGIN
    SELECT RAISE(ABORT, 'XFER_PREPOSTED: insert the transfer first, then attach its journal entry')
    WHERE NEW.journal_entry_id IS NOT NULL OR NEW.is_voided <> 0;
    SELECT RAISE(ABORT, 'XFER_BAD_ACCOUNT: transfer between bank, credit card, other current asset or liability accounts')
    WHERE EXISTS (
        SELECT 1 FROM chart_of_accounts
        WHERE id IN (NEW.from_account_id, NEW.to_account_id)
          AND (is_system = 1 OR account_type NOT IN ('BANK', 'CREDIT_CARD', 'OTHER_CURRENT_ASSET', 'OTHER_CURRENT_LIABILITY', 'LONG_TERM_LIABILITY'))
    );
END;

CREATE TRIGGER trg_xfer_update_rules
BEFORE UPDATE ON transfers
BEGIN
    SELECT RAISE(ABORT, 'XFER_IMMUTABLE: posted transfers cannot be changed')
    WHERE OLD.journal_entry_id IS NOT NULL
      AND (   NEW.transfer_date IS NOT OLD.transfer_date OR NEW.from_account_id IS NOT OLD.from_account_id
           OR NEW.to_account_id IS NOT OLD.to_account_id OR NEW.amount_cents IS NOT OLD.amount_cents
           OR NEW.memo IS NOT OLD.memo OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
           OR NEW.created_at IS NOT OLD.created_at);
    SELECT RAISE(ABORT, 'XFER_JOURNAL_MISSING: transfer must reference its POSTED journal entry')
    WHERE OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
      AND NOT EXISTS (
          SELECT 1 FROM journal_entries
          WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = 'TRANSFER' AND source_id = NEW.id
      );
    SELECT RAISE(ABORT, 'XFER_UNVOID: a voided transfer cannot be restored')
    WHERE OLD.is_voided = 1 AND NEW.is_voided = 0;
    SELECT RAISE(ABORT, 'XFER_VOID_ENTRY_ACTIVE: the transfer''s journal entry must be voided first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
END;

CREATE TRIGGER trg_xfer_no_delete_posted
BEFORE DELETE ON transfers WHEN OLD.journal_entry_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'XFER_IMMUTABLE: posted transfers cannot be deleted');
END;

-- =============================================================================================
-- Estimates (non-posting) and time tracking (non-posting)
-- =============================================================================================
CREATE TRIGGER trg_est_status_rules
BEFORE UPDATE ON estimates
BEGIN
    SELECT RAISE(ABORT, 'EST_CLOSED: a converted estimate cannot be changed')
    WHERE OLD.status = 'CLOSED';
    SELECT RAISE(ABORT, 'EST_IMMUTABLE: only the status of an estimate can change after it is saved')
    WHERE NEW.estimate_number IS NOT OLD.estimate_number OR NEW.customer_id IS NOT OLD.customer_id
       OR NEW.estimate_date IS NOT OLD.estimate_date OR NEW.expiration_date IS NOT OLD.expiration_date
       OR NEW.total_cents IS NOT OLD.total_cents OR NEW.memo IS NOT OLD.memo OR NEW.created_at IS NOT OLD.created_at;
    SELECT RAISE(ABORT, 'EST_INVOICE_MISMATCH: an estimate closes only into a posted invoice for the same customer and amount')
    WHERE NEW.status = 'CLOSED' AND NOT EXISTS (
        SELECT 1 FROM invoices
        WHERE id = NEW.invoice_id AND status <> 'DRAFT' AND customer_id = NEW.customer_id AND total_cents = NEW.total_cents
    );
END;

CREATE TRIGGER trg_est_lines_frozen_upd
BEFORE UPDATE ON estimate_lines
BEGIN
    SELECT RAISE(ABORT, 'EST_IMMUTABLE: estimate lines cannot be changed after saving');
END;

CREATE TRIGGER trg_time_daily_limit
BEFORE INSERT ON time_activities
WHEN NEW.minutes + COALESCE((SELECT SUM(minutes) FROM time_activities
                             WHERE employee_id = NEW.employee_id AND activity_date = NEW.activity_date), 0) > 1440
BEGIN
    SELECT RAISE(ABORT, 'TIME_OVER_24H: an employee cannot log more than 24 hours in one day');
END;
