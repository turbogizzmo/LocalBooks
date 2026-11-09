-- 0083_bill_checks_print_later_bounced_receipts.sql
-- Pay Bills' checks can wait in the print queue, and Record Bounced Check covers sales receipts and
-- foreign-currency payments (gap T13).

-- ---------------------------------------------------------------------------------------------
-- Print later for Pay Bills
-- ---------------------------------------------------------------------------------------------
-- Pay Bills writes one bill payment per bill; the bills it pays a vendor from one bank account go
-- on one check. A queued check's payments share `check_group_id` (the id of the first of them) and
-- have no number until the check prints, the one time a recorded payment's reference may be
-- filled in, as 0066 allows for checks.
ALTER TABLE bill_payments ADD COLUMN to_be_printed INTEGER NOT NULL DEFAULT 0 CHECK (to_be_printed IN (0, 1));
ALTER TABLE bill_payments ADD COLUMN check_group_id INTEGER;

CREATE INDEX ix_bill_payments_to_be_printed ON bill_payments (payment_account_id) WHERE to_be_printed = 1;
CREATE INDEX ix_bill_payments_check_group ON bill_payments (check_group_id) WHERE check_group_id IS NOT NULL;

CREATE TRIGGER trg_billpay_print_queue_insert
BEFORE INSERT ON bill_payments WHEN NEW.to_be_printed = 1
BEGIN
    SELECT RAISE(ABORT, 'PRINT_NOT_A_CHECK: only a check from a bank account can be printed later')
    WHERE NEW.kind <> 'STANDARD' OR (SELECT account_type FROM chart_of_accounts WHERE id = NEW.payment_account_id) IS NOT 'BANK';
    SELECT RAISE(ABORT, 'PRINT_HAS_NUMBER: a check to be printed gets its number when it prints')
    WHERE NEW.reference IS NOT NULL;
END;

CREATE TRIGGER trg_billpay_print_queue_update
BEFORE UPDATE OF to_be_printed ON bill_payments WHEN NEW.to_be_printed = 1 AND OLD.to_be_printed = 0
BEGIN
    SELECT RAISE(ABORT, 'PRINT_NOT_A_CHECK: only a check from a bank account can be printed later')
    WHERE NEW.kind <> 'STANDARD' OR (SELECT account_type FROM chart_of_accounts WHERE id = NEW.payment_account_id) IS NOT 'BANK';
    SELECT RAISE(ABORT, 'PRINT_HAS_NUMBER: this check already has a number')
    WHERE NEW.reference IS NOT NULL;
    SELECT RAISE(ABORT, 'PRINT_VOIDED: a voided check cannot be printed')
    WHERE NEW.is_voided = 1;
END;

CREATE TRIGGER trg_billpay_check_group_fixed
BEFORE UPDATE OF check_group_id ON bill_payments WHEN NEW.check_group_id IS NOT OLD.check_group_id
BEGIN
    SELECT RAISE(ABORT, 'BILL_PAYMENT_IMMUTABLE: recorded payments cannot be changed')
    WHERE OLD.check_group_id IS NOT NULL OR OLD.to_be_printed = 0;
END;

-- 0074's rules, except that a queued check's missing number may be filled in.
DROP TRIGGER trg_billpay_update_rules;
CREATE TRIGGER trg_billpay_update_rules
BEFORE UPDATE ON bill_payments
BEGIN
    SELECT RAISE(ABORT, 'BILL_PAYMENT_IMMUTABLE: recorded payments cannot be changed')
    WHERE (NEW.bill_id IS NOT OLD.bill_id AND NOT (
               OLD.is_voided = 0 AND NEW.is_voided = 0
               AND EXISTS (SELECT 1 FROM v_bill_replacements
                           WHERE old_bill_id = OLD.bill_id AND new_bill_id = NEW.bill_id)))
       OR NEW.payment_date IS NOT OLD.payment_date
       OR NEW.amount_cents IS NOT OLD.amount_cents
       OR NEW.payment_account_id IS NOT OLD.payment_account_id
       OR (NEW.reference IS NOT OLD.reference
           AND NOT (OLD.reference IS NULL AND OLD.to_be_printed = 1 AND NEW.reference IS NOT NULL))
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
-- Bounced sales receipts
-- ---------------------------------------------------------------------------------------------
-- A sales receipt has no invoice to reopen, so its bounce bills the customer instead. One write
-- posts the bounce's own entry (Dr the receipt's income account and any bank fee / Cr the bank the
-- money reached) and a charge-back invoice to the customer (Dr A/R / Cr that same income account,
-- plus any returned-check charge). The income lines cancel; the customer owes the money again on
-- an ordinary invoice that aging, statements and Receive Payments see. Voiding the bounce reverses
-- its entry and voids the charge-back invoice.
CREATE TABLE bounced_sales_receipts (
    id                  INTEGER PRIMARY KEY,
    sales_receipt_id    INTEGER NOT NULL REFERENCES sales_receipts (id) ON DELETE RESTRICT,
    -- The deposit line that banked it, when it went through Undeposited Funds.
    deposit_line_id     INTEGER REFERENCES deposit_lines (id) ON DELETE RESTRICT,
    customer_id         INTEGER NOT NULL REFERENCES customers (id) ON DELETE RESTRICT,
    bank_account_id     INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    bounce_date         TEXT    NOT NULL CHECK (
                            bounce_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                            AND date(bounce_date) = bounce_date
                        ),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    bank_fee_cents      INTEGER NOT NULL DEFAULT 0 CHECK (bank_fee_cents BETWEEN 0 AND 100000000000000),
    bank_fee_account_id INTEGER REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    charge_invoice_id   INTEGER NOT NULL REFERENCES invoices (id) ON DELETE RESTRICT,
    memo                TEXT,
    journal_entry_id    INTEGER NOT NULL UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    is_voided           INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK ((bank_fee_cents = 0) = (bank_fee_account_id IS NULL))
) STRICT;
CREATE UNIQUE INDEX ux_bounced_sales_receipts_live ON bounced_sales_receipts (sales_receipt_id) WHERE is_voided = 0;
CREATE INDEX ix_bounced_sales_receipts_entry ON bounced_sales_receipts (journal_entry_id);

CREATE TRIGGER trg_bounced_receipt_insert_checks
BEFORE INSERT ON bounced_sales_receipts
BEGIN
    SELECT RAISE(ABORT, 'BOUNCE_PREVOIDED: a bounced check is recorded live')
    WHERE NEW.is_voided <> 0;
    SELECT RAISE(ABORT, 'BOUNCE_JOURNAL_MISSING: a bounced check must reference its POSTED journal entry')
    WHERE NOT EXISTS (
        SELECT 1 FROM journal_entries
        WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND source_type = 'JOURNAL' AND is_voided = 0
    );
    SELECT RAISE(ABORT, 'BOUNCE_PAYMENT_VOIDED: a voided sales receipt cannot bounce')
    WHERE (SELECT is_voided FROM sales_receipts WHERE id = NEW.sales_receipt_id) <> 0;
    SELECT RAISE(ABORT, 'BOUNCE_WRONG_AMOUNT: a sales receipt bounces for its full amount')
    WHERE NEW.amount_cents IS NOT (SELECT total_cents FROM sales_receipts WHERE id = NEW.sales_receipt_id);
    SELECT RAISE(ABORT, 'BOUNCE_WRONG_CUSTOMER: the charge-back goes to the sales receipt''s customer')
    WHERE NEW.customer_id IS NOT (SELECT customer_id FROM sales_receipts WHERE id = NEW.sales_receipt_id)
       OR NEW.customer_id IS NOT (SELECT customer_id FROM invoices WHERE id = NEW.charge_invoice_id);
    SELECT RAISE(ABORT, 'BOUNCE_BAD_BANK: a bounced check comes out of a Bank account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.bank_account_id) IS NOT 'BANK';
END;

CREATE TRIGGER trg_bounced_receipt_update_rules
BEFORE UPDATE ON bounced_sales_receipts
BEGIN
    SELECT RAISE(ABORT, 'BOUNCE_IMMUTABLE: a recorded bounced check cannot be changed; void it instead')
    WHERE NEW.sales_receipt_id IS NOT OLD.sales_receipt_id OR NEW.deposit_line_id IS NOT OLD.deposit_line_id
       OR NEW.customer_id IS NOT OLD.customer_id OR NEW.bank_account_id IS NOT OLD.bank_account_id
       OR NEW.bounce_date IS NOT OLD.bounce_date OR NEW.amount_cents IS NOT OLD.amount_cents
       OR NEW.bank_fee_cents IS NOT OLD.bank_fee_cents OR NEW.bank_fee_account_id IS NOT OLD.bank_fee_account_id
       OR NEW.charge_invoice_id IS NOT OLD.charge_invoice_id OR NEW.memo IS NOT OLD.memo
       OR NEW.journal_entry_id IS NOT OLD.journal_entry_id OR NEW.created_at IS NOT OLD.created_at;
    SELECT RAISE(ABORT, 'BOUNCE_UNVOID: a voided bounced check cannot be restored')
    WHERE OLD.is_voided = 1 AND NEW.is_voided = 0;
    SELECT RAISE(ABORT, 'BOUNCE_VOID_ENTRY_ACTIVE: the bounced check''s journal entry must be voided first')
    WHERE OLD.is_voided = 0 AND NEW.is_voided = 1
      AND NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1);
END;

CREATE TRIGGER trg_bounced_receipt_no_delete
BEFORE DELETE ON bounced_sales_receipts
BEGIN
    SELECT RAISE(ABORT, 'BOUNCE_IMMUTABLE: a recorded bounced check cannot be deleted; void it instead');
END;

-- A bounced receipt can't be voided until its bounce is (the bounce undoes part of it).
CREATE TRIGGER trg_bounced_receipt_void_receipt
BEFORE UPDATE OF is_voided ON sales_receipts
WHEN NEW.is_voided = 1 AND OLD.is_voided = 0
BEGIN
    SELECT RAISE(ABORT, 'VOID_BOUNCED: this sales receipt bounced; void the bounced check first')
    WHERE EXISTS (SELECT 1 FROM bounced_sales_receipts WHERE sales_receipt_id = NEW.id AND is_voided = 0);
END;

-- ---------------------------------------------------------------------------------------------
-- Bounced foreign-currency payments
-- ---------------------------------------------------------------------------------------------
-- A bounce's amount is the payment's, in the invoice's currency (0066 checks this), which is what
-- the invoice's amount paid counts. A/R is home currency: a bounced foreign-currency payment puts
-- back the A/R it relieved, not its foreign amount. 0066's view, with that.
DROP VIEW v_invoice_balances;
CREATE VIEW v_invoice_balances AS
SELECT
    i.id AS invoice_id,
    COALESCE(i.home_total_cents, i.total_cents) AS home_total_cents,
    COALESCE(i.home_total_cents, i.total_cents)
      - COALESCE((SELECT SUM(COALESCE(p.ar_relief_cents, p.amount_cents))
                  FROM invoice_payments p WHERE p.invoice_id = i.id AND p.is_voided = 0), 0)
      - COALESCE((SELECT SUM(a.amount_cents) FROM credit_applications a WHERE a.invoice_id = i.id AND a.is_voided = 0), 0)
      + COALESCE((SELECT SUM(COALESCE(p.ar_relief_cents, b.amount_cents)) FROM bounced_checks b
                  LEFT JOIN invoice_payments p ON p.id = b.invoice_payment_id
                  WHERE b.invoice_id = i.id AND b.is_voided = 0), 0)
      AS home_open_cents
FROM invoices i;
