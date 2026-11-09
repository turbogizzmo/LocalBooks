-- 0087_foreign_payment_discounts.sql
-- Early-payment discounts on invoices in another currency (gap T39).
--
-- 0076's `discount_cents` is for home-currency payments only (its CHECK refuses one with an
-- exchange rate). A foreign-currency payment keeps its discount here instead, in the invoice's
-- currency, with the same meaning: `amount_cents` is what the payment applies to the invoice
-- (money plus discount) and the money received is `amount_cents - foreign_discount_cents`, which
-- the payment converts at its own rate (`home_amount_cents`). The discount is converted at the
-- invoice's booked rate, so the A/R the payment relieves (`ar_relief_cents`) splits into the money
-- at its rate and the discount at the booked rate, with the difference a realized exchange gain or
-- loss as before.

ALTER TABLE invoice_payments ADD COLUMN foreign_discount_cents INTEGER NOT NULL DEFAULT 0
    CHECK (foreign_discount_cents >= 0 AND foreign_discount_cents < amount_cents AND (foreign_discount_cents = 0 OR exchange_rate_e6 IS NOT NULL));

CREATE TRIGGER trg_pay_foreign_discount_immutable
BEFORE UPDATE OF foreign_discount_cents ON invoice_payments
BEGIN
    SELECT RAISE(ABORT, 'INV_PAYMENT_IMMUTABLE: recorded payments cannot be changed')
    WHERE NEW.foreign_discount_cents IS NOT OLD.foreign_discount_cents;
END;
