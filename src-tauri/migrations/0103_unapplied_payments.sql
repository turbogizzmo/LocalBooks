-- 0103_unapplied_payments.sql
-- Overpayments, prepayments and payments from a customer with no open invoice (gap T24; design in
-- docs/plans/overpayments.md).
--
-- Money received that is not applied to an invoice becomes a customer credit: a credit_memos row
-- with no lines, telling where the money went (payment_account_id) and how it was paid. It posts
-- Dr that account / Cr Accounts Receivable, and is then applied, refunded or written off like any
-- other credit. A write-off is a credit_refunds row to an income account, marked is_write_off.

ALTER TABLE credit_memos ADD COLUMN payment_account_id INTEGER REFERENCES chart_of_accounts (id) ON DELETE RESTRICT;
ALTER TABLE credit_memos ADD COLUMN payment_method TEXT;
ALTER TABLE credit_memos ADD COLUMN payment_method_id INTEGER REFERENCES payment_methods (id) ON DELETE RESTRICT;
ALTER TABLE credit_memos ADD COLUMN payment_reference TEXT;

ALTER TABLE credit_refunds ADD COLUMN is_write_off INTEGER NOT NULL DEFAULT 0 CHECK (is_write_off IN (0, 1));

-- An unapplied payment can't also be a refund receipt, and has no lines.
CREATE TRIGGER trg_cm_payment_shape
BEFORE INSERT ON credit_memos
WHEN NEW.payment_account_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'CM_PAYMENT_REFUND: an unapplied payment is not a refund receipt')
    WHERE NEW.refund_account_id IS NOT NULL;
END;

CREATE TRIGGER trg_cm_payment_no_lines
BEFORE INSERT ON credit_memo_lines
BEGIN
    SELECT RAISE(ABORT, 'CM_PAYMENT_LINES: an unapplied payment has no lines')
    WHERE (SELECT payment_account_id FROM credit_memos WHERE id = NEW.credit_memo_id) IS NOT NULL;
END;

CREATE TRIGGER trg_cm_payment_fixed
BEFORE UPDATE OF payment_account_id, payment_method, payment_method_id, payment_reference ON credit_memos
WHEN NEW.payment_account_id IS NOT OLD.payment_account_id OR NEW.payment_method IS NOT OLD.payment_method
  OR NEW.payment_method_id IS NOT OLD.payment_method_id OR NEW.payment_reference IS NOT OLD.payment_reference
BEGIN
    SELECT RAISE(ABORT, 'CM_IMMUTABLE: an unapplied payment''s account and method are fixed once posted');
END;

-- Deposited from Undeposited Funds like any other payment, into one live deposit at a time.
ALTER TABLE deposit_lines ADD COLUMN credit_memo_id INTEGER REFERENCES credit_memos (id) ON DELETE RESTRICT;
CREATE UNIQUE INDEX ux_deposit_lines_credit_memo ON deposit_lines (credit_memo_id)
    WHERE credit_memo_id IS NOT NULL AND is_voided = 0;

CREATE TRIGGER trg_dep_lines_credit_memo
BEFORE INSERT ON deposit_lines
WHEN NEW.credit_memo_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'DEP_BAD_ITEM: a deposit line deposits one payment')
    WHERE NEW.invoice_payment_id IS NOT NULL OR NEW.sales_receipt_id IS NOT NULL
       OR NEW.customer_id IS NOT NULL OR NEW.vendor_id IS NOT NULL;
    SELECT RAISE(ABORT, 'DEP_BAD_ITEM: only an unapplied payment can be deposited')
    WHERE (SELECT payment_account_id FROM credit_memos WHERE id = NEW.credit_memo_id) IS NULL;
END;

CREATE TRIGGER trg_dep_lines_credit_memo_fixed
BEFORE UPDATE OF credit_memo_id ON deposit_lines
WHEN NEW.credit_memo_id IS NOT OLD.credit_memo_id
BEGIN
    SELECT RAISE(ABORT, 'DEP_IMMUTABLE: deposit lines can only be marked void along with their deposit');
END;

CREATE TRIGGER trg_cm_payment_deposited_void
BEFORE UPDATE OF is_voided ON credit_memos
WHEN NEW.is_voided = 1 AND OLD.is_voided = 0
BEGIN
    SELECT RAISE(ABORT, 'VOID_DEPOSITED: void the bank deposit that includes this payment first')
    WHERE EXISTS (SELECT 1 FROM deposit_lines WHERE credit_memo_id = OLD.id AND is_voided = 0);
END;
