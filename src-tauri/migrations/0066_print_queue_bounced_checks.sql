-- Print later and Record Bounced Check.
--
-- Print later: checks and invoices can be marked "To be printed" and printed together from
-- File › Print Forms. A queued check has no number yet; printing gives it the next number, the one
-- time a posted check's number may be filled in.
--
-- Bounced checks: a customer's check that the bank returned. The bounce posts its own entry
-- (Dr A/R, plus any bank fee to an expense account / Cr the bank) and reopens the invoice the
-- payment paid: while the bounce is live, the payment no longer counts toward the invoice's
-- amount paid. Voiding the bounce's entry undoes both.

-- ---------------------------------------------------------------------------------------------
-- Print later
-- ---------------------------------------------------------------------------------------------
ALTER TABLE expenses ADD COLUMN to_be_printed INTEGER NOT NULL DEFAULT 0 CHECK (to_be_printed IN (0, 1));
ALTER TABLE invoices ADD COLUMN to_be_printed INTEGER NOT NULL DEFAULT 0 CHECK (to_be_printed IN (0, 1));

CREATE INDEX ix_expenses_to_be_printed ON expenses (payment_account_id) WHERE to_be_printed = 1;
CREATE INDEX ix_invoices_to_be_printed ON invoices (invoice_number) WHERE to_be_printed = 1;

CREATE TRIGGER trg_exp_print_queue_insert
BEFORE INSERT ON expenses WHEN NEW.to_be_printed = 1
BEGIN
    SELECT RAISE(ABORT, 'PRINT_NOT_A_CHECK: only checks can be printed later')
    WHERE NEW.kind <> 'CHECK';
    SELECT RAISE(ABORT, 'PRINT_HAS_NUMBER: a check to be printed gets its number when it prints')
    WHERE NEW.check_number IS NOT NULL;
END;

CREATE TRIGGER trg_exp_print_queue_update
BEFORE UPDATE OF to_be_printed ON expenses WHEN NEW.to_be_printed = 1 AND OLD.to_be_printed = 0
BEGIN
    SELECT RAISE(ABORT, 'PRINT_NOT_A_CHECK: only checks can be printed later')
    WHERE NEW.kind <> 'CHECK';
    SELECT RAISE(ABORT, 'PRINT_HAS_NUMBER: this check already has a number; print it from its row instead')
    WHERE NEW.check_number IS NOT NULL;
    SELECT RAISE(ABORT, 'PRINT_VOIDED: a voided check cannot be printed')
    WHERE NEW.is_voided = 1;
END;

-- Same rules as 0008, except that a queued check's missing number may be filled in.
DROP TRIGGER trg_exp_update_rules;
CREATE TRIGGER trg_exp_update_rules
BEFORE UPDATE ON expenses
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

-- ---------------------------------------------------------------------------------------------
-- Bounced checks
-- ---------------------------------------------------------------------------------------------
CREATE TABLE bounced_checks (
    id                  INTEGER PRIMARY KEY,
    -- What bounced: a customer's invoice payment, or an "other funds" line of a deposit.
    invoice_payment_id  INTEGER REFERENCES invoice_payments (id) ON DELETE RESTRICT,
    deposit_line_id     INTEGER REFERENCES deposit_lines (id) ON DELETE RESTRICT,
    -- The invoice the payment paid, reopened while the bounce is live.
    invoice_id          INTEGER REFERENCES invoices (id) ON DELETE RESTRICT,
    customer_id         INTEGER REFERENCES customers (id) ON DELETE RESTRICT,
    bank_account_id     INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    bounce_date         TEXT    NOT NULL CHECK (
                            bounce_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(bounce_date) = bounce_date
                        ),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    bank_fee_cents      INTEGER NOT NULL DEFAULT 0 CHECK (bank_fee_cents BETWEEN 0 AND 100000000000000),
    bank_fee_account_id INTEGER REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    -- The invoice that charges the customer a returned-check fee, when there is one.
    fee_invoice_id      INTEGER REFERENCES invoices (id) ON DELETE RESTRICT,
    memo                TEXT,
    journal_entry_id    INTEGER NOT NULL UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    is_voided           INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK ((invoice_payment_id IS NULL) <> (deposit_line_id IS NULL)),
    CHECK ((invoice_payment_id IS NULL) = (invoice_id IS NULL)),
    CHECK ((bank_fee_cents = 0) = (bank_fee_account_id IS NULL))
) STRICT;
CREATE INDEX ix_bounced_checks_invoice ON bounced_checks (invoice_id) WHERE invoice_id IS NOT NULL;
CREATE INDEX ix_bounced_checks_entry ON bounced_checks (journal_entry_id);
-- A payment or deposit line bounces at most once at a time.
CREATE UNIQUE INDEX ux_bounced_checks_payment ON bounced_checks (invoice_payment_id)
    WHERE invoice_payment_id IS NOT NULL AND is_voided = 0;
CREATE UNIQUE INDEX ux_bounced_checks_deposit_line ON bounced_checks (deposit_line_id)
    WHERE deposit_line_id IS NOT NULL AND is_voided = 0;

CREATE TRIGGER trg_bounce_insert_checks
BEFORE INSERT ON bounced_checks
BEGIN
    SELECT RAISE(ABORT, 'BOUNCE_PREVOIDED: a bounced check is recorded live')
    WHERE NEW.is_voided <> 0;
    SELECT RAISE(ABORT, 'BOUNCE_JOURNAL_MISSING: a bounced check must reference its POSTED journal entry')
    WHERE NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = 'JOURNAL' AND is_voided = 0
    );
    SELECT RAISE(ABORT, 'BOUNCE_PAYMENT_VOIDED: a voided payment cannot bounce')
    WHERE NEW.invoice_payment_id IS NOT NULL
      AND (SELECT is_voided FROM invoice_payments WHERE id = NEW.invoice_payment_id) <> 0;
    SELECT RAISE(ABORT, 'BOUNCE_WRONG_INVOICE: the reopened invoice must be the one the payment paid')
    WHERE NEW.invoice_payment_id IS NOT NULL
      AND NEW.invoice_id IS NOT (SELECT invoice_id FROM invoice_payments WHERE id = NEW.invoice_payment_id);
    SELECT RAISE(ABORT, 'BOUNCE_WRONG_AMOUNT: a payment bounces for its full amount')
    WHERE NEW.invoice_payment_id IS NOT NULL
      AND NEW.amount_cents IS NOT (SELECT amount_cents FROM invoice_payments WHERE id = NEW.invoice_payment_id);
    SELECT RAISE(ABORT, 'BOUNCE_BAD_BANK: a bounced check comes out of a Bank account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.bank_account_id) IS NOT 'BANK';
END;

CREATE TRIGGER trg_bounce_update_rules
BEFORE UPDATE ON bounced_checks
BEGIN
    SELECT RAISE(ABORT, 'BOUNCE_IMMUTABLE: a recorded bounced check cannot be changed; void it instead')
    WHERE NEW.invoice_payment_id IS NOT OLD.invoice_payment_id OR NEW.deposit_line_id IS NOT OLD.deposit_line_id
       OR NEW.invoice_id IS NOT OLD.invoice_id OR NEW.customer_id IS NOT OLD.customer_id
       OR NEW.bank_account_id IS NOT OLD.bank_account_id OR NEW.bounce_date IS NOT OLD.bounce_date
       OR NEW.amount_cents IS NOT OLD.amount_cents OR NEW.bank_fee_cents IS NOT OLD.bank_fee_cents
       OR NEW.bank_fee_account_id IS NOT OLD.bank_fee_account_id OR NEW.fee_invoice_id IS NOT OLD.fee_invoice_id
       OR NEW.memo IS NOT OLD.memo OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
       OR NEW.created_at IS NOT OLD.created_at;
    SELECT RAISE(ABORT, 'BOUNCE_UNVOID: a voided bounced check cannot be restored')
    WHERE OLD.is_voided = 1 AND NEW.is_voided = 0;
    SELECT RAISE(ABORT, 'BOUNCE_VOID_ENTRY_ACTIVE: the bounced check''s journal entry must be voided first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
END;

CREATE TRIGGER trg_bounce_no_delete
BEFORE DELETE ON bounced_checks
BEGIN
    SELECT RAISE(ABORT, 'BOUNCE_IMMUTABLE: a recorded bounced check cannot be deleted; void it instead');
END;

-- Amount paid: payments and applied credits, less payments that bounced (0026's rule plus bounces).
DROP TRIGGER trg_inv_amount_paid_matches;
CREATE TRIGGER trg_inv_amount_paid_matches
BEFORE UPDATE OF amount_paid_cents ON invoices
BEGIN
    SELECT RAISE(ABORT, 'INV_PAYMENT_UNPOSTED: every payment must be posted before it is applied')
    WHERE EXISTS (SELECT 1 FROM invoice_payments WHERE invoice_id = NEW.id AND journal_entry_id IS NULL);
    SELECT RAISE(ABORT, 'INV_PAID_MISMATCH: amount paid must equal the sum of recorded payments and applied credits, less bounced checks')
    WHERE NEW.amount_paid_cents <>
          COALESCE((SELECT SUM(amount_cents) FROM invoice_payments WHERE invoice_id = NEW.id AND is_voided = 0), 0)
        + COALESCE((SELECT SUM(amount_cents) FROM credit_applications WHERE invoice_id = NEW.id AND is_voided = 0), 0)
        - COALESCE((SELECT SUM(amount_cents) FROM bounced_checks WHERE invoice_id = NEW.id AND is_voided = 0), 0);
END;

-- Home-currency A/R still open on each invoice; a live bounce puts its amount back.
DROP VIEW v_invoice_balances;
CREATE VIEW v_invoice_balances AS
SELECT
    i.id AS invoice_id,
    COALESCE(i.home_total_cents, i.total_cents) AS home_total_cents,
    COALESCE(i.home_total_cents, i.total_cents)
      - COALESCE((SELECT SUM(COALESCE(p.ar_relief_cents, p.amount_cents))
                  FROM invoice_payments p WHERE p.invoice_id = i.id AND p.is_voided = 0), 0)
      - COALESCE((SELECT SUM(a.amount_cents) FROM credit_applications a WHERE a.invoice_id = i.id AND a.is_voided = 0), 0)
      + COALESCE((SELECT SUM(b.amount_cents) FROM bounced_checks b WHERE b.invoice_id = i.id AND b.is_voided = 0), 0)
      AS home_open_cents
FROM invoices i;

-- Names for each entry (0026's view, plus the customer of a bounced check).
DROP VIEW v_entry_party;
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
               (SELECT v.name FROM vendor_credit_refunds r JOIN vendor_credits d ON d.id = r.vendor_credit_id JOIN vendors v ON v.id = d.vendor_id WHERE r.journal_entry_id = e.id),
               (SELECT c.name FROM bounced_checks b JOIN customers c ON c.id = b.customer_id WHERE b.journal_entry_id = e.id))
       END AS party_name
FROM journal_entries e;
