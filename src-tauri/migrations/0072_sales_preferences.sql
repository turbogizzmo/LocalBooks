-- Preferences batch 4 (Sales & Customers, Jobs & Estimates, Send Forms).
--
-- A customer's preferred way to send forms (new customers take the company default from
-- Preferences › Send Forms), and a job's status (its labels can be renamed in Preferences › Jobs &
-- Estimates).

ALTER TABLE customers ADD COLUMN preferred_delivery TEXT
    CHECK (preferred_delivery IS NULL OR preferred_delivery IN ('EMAIL', 'PRINT', 'NONE'));

ALTER TABLE customers ADD COLUMN job_status TEXT
    CHECK (job_status IS NULL OR job_status IN ('PENDING', 'AWARDED', 'IN_PROGRESS', 'CLOSED', 'NOT_AWARDED'));

-- A new customer, however it is added (the Customer Center, typed on a form, an import), takes
-- the Send Forms default unless it was given its own.
CREATE TRIGGER trg_customer_default_delivery
AFTER INSERT ON customers
WHEN NEW.preferred_delivery IS NULL
BEGIN
    UPDATE customers
    SET preferred_delivery = (SELECT CASE value WHEN 'email' THEN 'EMAIL' WHEN 'print' THEN 'PRINT' END
                              FROM system_preferences WHERE key = 'default_delivery_method')
    WHERE id = NEW.id;
END;
