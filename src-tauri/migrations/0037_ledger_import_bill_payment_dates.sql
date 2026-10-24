-- As with received payments (0036), QuickBooks lets a bill payment be dated before the bill it
-- pays, and a full import keeps the file's dates. Bill payments entered in LocalBooks still
-- cannot be: Pay Bills checks it (BILL_PAYMENT_BEFORE_BILL in `expenses::write_bill_payment`).
-- The other rules are unchanged.
DROP TRIGGER trg_billpay_insert_checks;
CREATE TRIGGER trg_billpay_insert_checks
BEFORE INSERT ON bill_payments
BEGIN
    SELECT RAISE(ABORT, 'BILL_NOT_OPEN: payments can only be applied to open bills')
    WHERE (SELECT status FROM bills WHERE id = NEW.bill_id) NOT IN ('UNPAID', 'PARTIAL')
       OR (SELECT is_voided FROM bills WHERE id = NEW.bill_id) = 1;
    SELECT RAISE(ABORT, 'BILL_OVERPAYMENT: payment exceeds the open balance')
    WHERE NEW.amount_cents > (SELECT total_cents - amount_paid_cents FROM bills WHERE id = NEW.bill_id);
    SELECT RAISE(ABORT, 'BILL_BAD_PAYMENT_ACCOUNT: pay bills from a Bank or Credit Card account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.payment_account_id) NOT IN ('BANK', 'CREDIT_CARD');
    SELECT RAISE(ABORT, 'BILL_PAYMENT_PREPOSTED: insert the payment first, then attach its journal entry')
    WHERE NEW.journal_entry_id IS NOT NULL OR NEW.is_voided <> 0;
END;
