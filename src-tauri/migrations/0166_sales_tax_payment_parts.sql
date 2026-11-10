-- How a sales tax payment against a combined rate split among the combined rate's rates, as
-- posted: one row per rate, with the liability account debited for it (`sales_tax.rs`,
-- `pay_sales_tax`). The sales tax return files each part under its own rate's agency.
-- A payment against an ordinary rate has no rows here.
CREATE TABLE sales_tax_payment_parts (
    payment_id           INTEGER NOT NULL REFERENCES sales_tax_payments (id) ON DELETE CASCADE,
    tax_rate_id          INTEGER NOT NULL REFERENCES tax_rates (id) ON DELETE RESTRICT,
    liability_account_id INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    cents                INTEGER NOT NULL CHECK (cents >= 0),
    PRIMARY KEY (payment_id, tax_rate_id)
) STRICT;

-- The split is as fixed as the posted payment.
CREATE TRIGGER trg_stpp_immutable
BEFORE UPDATE ON sales_tax_payment_parts
BEGIN
    SELECT RAISE(ABORT, 'TAX_PAYMENT_IMMUTABLE: a posted sales tax payment cannot be altered; void it instead');
END;
