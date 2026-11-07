-- 0076_payment_discounts.sql
-- Early-payment discounts taken in Receive Payments (gap T19).
--
-- A payment that takes the discount its invoice's terms offer posts one entry: the cash to the
-- deposit account, the discount to the company's discount account, and the two together off A/R.
-- `amount_cents` stays what the payment applies to the invoice (cash plus discount), so the
-- invoice's amount paid, its A/R balance and aging need no change; `discount_cents` is the part of
-- it that was a discount rather than money. The money received is `amount_cents - discount_cents`.
-- Foreign-currency payments take no discount here, so their home amount stays the money received.

ALTER TABLE invoice_payments ADD COLUMN discount_cents INTEGER NOT NULL DEFAULT 0
    CHECK (discount_cents >= 0 AND discount_cents < amount_cents AND (discount_cents = 0 OR exchange_rate_e6 IS NULL));

CREATE TRIGGER trg_pay_discount_immutable
BEFORE UPDATE OF discount_cents ON invoice_payments
BEGIN
    SELECT RAISE(ABORT, 'INV_PAYMENT_IMMUTABLE: recorded payments cannot be changed')
    WHERE NEW.discount_cents IS NOT OLD.discount_cents;
END;

-- 0011's deposit-line check, comparing a deposited payment with the money it brought in (not the
-- discount, which never reached Undeposited Funds).
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
          AND p.deposit_account_id = NEW.account_id
          AND COALESCE(p.home_amount_cents, p.amount_cents - p.discount_cents) = NEW.amount_cents
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
