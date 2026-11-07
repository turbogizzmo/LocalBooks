-- The Customer & Vendor Profile lists: Terms, Payment Method, Customer Type, Vendor Type, Job Type
-- and Customer Message. Each is a short named list the user edits under Lists › Customer & Vendor
-- Profile Lists; an entry that is used can be renamed or made inactive but not deleted, and an
-- inactive entry stays on whatever already has it.
--
-- * Terms: a name, the days until a bill or invoice is due, and optionally an early-payment
--   discount ("2% 10 Net 30": 2 percent off if paid within 10 days, otherwise due in 30). Seeded
--   with the four terms the forms offered until now. Customers and vendors pick a default term;
--   invoices and bills keep the term they were issued with. `terms_days` on customers and vendors
--   stays, and follows the chosen term, so everything that reads it (imports, the customer list)
--   keeps working; a record written with days only (an import) is linked to the matching term.
-- * Payment methods: a name for each way money is received or paid ("Visa", "Check"), each of one
--   of the six kinds the ledger already knows (`payment_method` on payments, receipts and expenses
--   is unchanged). Seeded with one method per kind; existing documents are linked to them.
-- * Customer, vendor and job types, and customer messages, share one table. Types go on customer,
--   vendor and job records; a customer message is printed at the foot of an invoice, sales receipt
--   or estimate.
--
-- None of this posts: terms, methods, types and messages are document details, not amounts.

CREATE TABLE terms (
    id              INTEGER PRIMARY KEY,
    name            TEXT    NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 31 AND name = trim(name)),
    net_days        INTEGER NOT NULL CHECK (net_days BETWEEN 0 AND 365),
    -- Early-payment discount, in parts per million of the total (20000 = 2%); 0 = none.
    discount_ppm    INTEGER NOT NULL DEFAULT 0 CHECK (discount_ppm BETWEEN 0 AND 999999),
    discount_days   INTEGER CHECK (discount_days IS NULL OR discount_days BETWEEN 0 AND 365),
    is_active       INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK ((discount_ppm = 0) = (discount_days IS NULL)),
    CHECK (discount_days IS NULL OR discount_days <= net_days)
) STRICT;
CREATE UNIQUE INDEX ux_terms_name ON terms (name COLLATE NOCASE);

INSERT INTO terms (name, net_days) VALUES ('Due on receipt', 0), ('Net 15', 15), ('Net 30', 30), ('Net 60', 60);

-- Every number of days already on a customer or vendor gets a term, so each record keeps its terms.
INSERT OR IGNORE INTO terms (name, net_days)
SELECT 'Net ' || d, d FROM (SELECT terms_days AS d FROM customers UNION SELECT terms_days FROM vendors)
WHERE d IS NOT NULL AND NOT EXISTS (SELECT 1 FROM terms t WHERE t.net_days = d AND t.discount_ppm = 0);

ALTER TABLE customers ADD COLUMN terms_id INTEGER REFERENCES terms (id) ON DELETE RESTRICT;
ALTER TABLE vendors   ADD COLUMN terms_id INTEGER REFERENCES terms (id) ON DELETE RESTRICT;
ALTER TABLE invoices  ADD COLUMN terms_id INTEGER REFERENCES terms (id) ON DELETE RESTRICT;
ALTER TABLE bills     ADD COLUMN terms_id INTEGER REFERENCES terms (id) ON DELETE RESTRICT;

UPDATE customers SET terms_id = (SELECT id FROM terms t WHERE t.net_days = customers.terms_days AND t.discount_ppm = 0 ORDER BY t.id LIMIT 1)
WHERE terms_days IS NOT NULL;
UPDATE vendors SET terms_id = (SELECT id FROM terms t WHERE t.net_days = vendors.terms_days AND t.discount_ppm = 0 ORDER BY t.id LIMIT 1)
WHERE terms_days IS NOT NULL;

-- A record given days without a term (imports write days only) is linked to the term for those
-- days, which is added when there is none. Updates that set the term themselves, or that bring
-- the days in step with the record's own term (a term's days changed), are left alone.
CREATE TRIGGER trg_customer_terms_link_insert
AFTER INSERT ON customers
WHEN NEW.terms_days IS NOT NULL AND NEW.terms_id IS NULL
BEGIN
    INSERT OR IGNORE INTO terms (name, net_days)
    SELECT CASE NEW.terms_days WHEN 0 THEN 'Due on receipt' ELSE 'Net ' || NEW.terms_days END, NEW.terms_days
    WHERE NOT EXISTS (SELECT 1 FROM terms WHERE net_days = NEW.terms_days AND discount_ppm = 0);
    UPDATE customers SET terms_id = (SELECT id FROM terms WHERE net_days = NEW.terms_days AND discount_ppm = 0 ORDER BY is_active DESC, id LIMIT 1)
    WHERE id = NEW.id;
END;

CREATE TRIGGER trg_customer_terms_link_update
AFTER UPDATE OF terms_days ON customers
WHEN NEW.terms_id IS OLD.terms_id AND NEW.terms_days IS NOT OLD.terms_days
  AND (NEW.terms_id IS NULL OR NEW.terms_days IS NOT (SELECT net_days FROM terms WHERE id = NEW.terms_id))
BEGIN
    INSERT OR IGNORE INTO terms (name, net_days)
    SELECT CASE NEW.terms_days WHEN 0 THEN 'Due on receipt' ELSE 'Net ' || NEW.terms_days END, NEW.terms_days
    WHERE NEW.terms_days IS NOT NULL AND NOT EXISTS (SELECT 1 FROM terms WHERE net_days = NEW.terms_days AND discount_ppm = 0);
    UPDATE customers SET terms_id = (SELECT id FROM terms WHERE net_days = NEW.terms_days AND discount_ppm = 0 ORDER BY is_active DESC, id LIMIT 1)
    WHERE id = NEW.id;
END;

CREATE TRIGGER trg_vendor_terms_link_insert
AFTER INSERT ON vendors
WHEN NEW.terms_days IS NOT NULL AND NEW.terms_id IS NULL
BEGIN
    INSERT OR IGNORE INTO terms (name, net_days)
    SELECT CASE NEW.terms_days WHEN 0 THEN 'Due on receipt' ELSE 'Net ' || NEW.terms_days END, NEW.terms_days
    WHERE NOT EXISTS (SELECT 1 FROM terms WHERE net_days = NEW.terms_days AND discount_ppm = 0);
    UPDATE vendors SET terms_id = (SELECT id FROM terms WHERE net_days = NEW.terms_days AND discount_ppm = 0 ORDER BY is_active DESC, id LIMIT 1)
    WHERE id = NEW.id;
END;

CREATE TRIGGER trg_vendor_terms_link_update
AFTER UPDATE OF terms_days ON vendors
WHEN NEW.terms_id IS OLD.terms_id AND NEW.terms_days IS NOT OLD.terms_days
  AND (NEW.terms_id IS NULL OR NEW.terms_days IS NOT (SELECT net_days FROM terms WHERE id = NEW.terms_id))
BEGIN
    INSERT OR IGNORE INTO terms (name, net_days)
    SELECT CASE NEW.terms_days WHEN 0 THEN 'Due on receipt' ELSE 'Net ' || NEW.terms_days END, NEW.terms_days
    WHERE NEW.terms_days IS NOT NULL AND NOT EXISTS (SELECT 1 FROM terms WHERE net_days = NEW.terms_days AND discount_ppm = 0);
    UPDATE vendors SET terms_id = (SELECT id FROM terms WHERE net_days = NEW.terms_days AND discount_ppm = 0 ORDER BY is_active DESC, id LIMIT 1)
    WHERE id = NEW.id;
END;

-- ---------------------------------------------------------------------------------------------
-- Payment methods
-- ---------------------------------------------------------------------------------------------
CREATE TABLE payment_methods (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 31 AND name = trim(name)),
    kind        TEXT    NOT NULL CHECK (kind IN ('CASH', 'CHECK', 'CREDIT_CARD', 'ACH', 'WIRE', 'OTHER')),
    is_active   INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_payment_methods_name ON payment_methods (name COLLATE NOCASE);

INSERT INTO payment_methods (name, kind) VALUES
    ('Check', 'CHECK'), ('Cash', 'CASH'), ('Credit card', 'CREDIT_CARD'),
    ('ACH / bank transfer', 'ACH'), ('Wire', 'WIRE'), ('Other', 'OTHER');

ALTER TABLE invoice_payments ADD COLUMN payment_method_id INTEGER REFERENCES payment_methods (id) ON DELETE RESTRICT;
ALTER TABLE sales_receipts   ADD COLUMN payment_method_id INTEGER REFERENCES payment_methods (id) ON DELETE RESTRICT;
ALTER TABLE expenses         ADD COLUMN payment_method_id INTEGER REFERENCES payment_methods (id) ON DELETE RESTRICT;

UPDATE invoice_payments SET payment_method_id = (SELECT id FROM payment_methods m WHERE m.kind = invoice_payments.payment_method ORDER BY m.id LIMIT 1);
UPDATE sales_receipts SET payment_method_id = (SELECT id FROM payment_methods m WHERE m.kind = sales_receipts.payment_method ORDER BY m.id LIMIT 1);
UPDATE expenses SET payment_method_id = (SELECT id FROM payment_methods m WHERE m.kind = expenses.payment_method ORDER BY m.id LIMIT 1)
WHERE payment_method IS NOT NULL;

-- ---------------------------------------------------------------------------------------------
-- Customer, vendor and job types; customer messages
-- ---------------------------------------------------------------------------------------------
CREATE TABLE profile_entries (
    id          INTEGER PRIMARY KEY,
    list        TEXT    NOT NULL CHECK (list IN ('CUSTOMER_TYPE', 'VENDOR_TYPE', 'JOB_TYPE', 'CUSTOMER_MESSAGE')),
    name        TEXT    NOT NULL CHECK (name = trim(name) AND length(name) BETWEEN 1 AND (CASE list WHEN 'CUSTOMER_MESSAGE' THEN 101 ELSE 31 END)),
    is_active   INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_profile_entries_name ON profile_entries (list, name COLLATE NOCASE);

ALTER TABLE customers ADD COLUMN customer_type_id INTEGER REFERENCES profile_entries (id) ON DELETE RESTRICT;
ALTER TABLE customers ADD COLUMN job_type_id      INTEGER REFERENCES profile_entries (id) ON DELETE RESTRICT;
ALTER TABLE vendors   ADD COLUMN vendor_type_id   INTEGER REFERENCES profile_entries (id) ON DELETE RESTRICT;

ALTER TABLE invoices       ADD COLUMN customer_message_id INTEGER REFERENCES profile_entries (id) ON DELETE RESTRICT;
ALTER TABLE sales_receipts ADD COLUMN customer_message_id INTEGER REFERENCES profile_entries (id) ON DELETE RESTRICT;
ALTER TABLE estimates      ADD COLUMN customer_message_id INTEGER REFERENCES profile_entries (id) ON DELETE RESTRICT;

-- ---------------------------------------------------------------------------------------------
-- The audit trail also notes a changed term or type (otherwise as in 0063).
-- ---------------------------------------------------------------------------------------------
DROP TRIGGER trg_ledger_audit_customer;
CREATE TRIGGER trg_ledger_audit_customer
AFTER UPDATE ON customers
WHEN NEW.name IS NOT OLD.name OR NEW.is_active IS NOT OLD.is_active OR NEW.email IS NOT OLD.email
  OR NEW.company_name IS NOT OLD.company_name OR NEW.phone IS NOT OLD.phone OR NEW.billing_address IS NOT OLD.billing_address
  OR NEW.terms_days IS NOT OLD.terms_days OR NEW.notes IS NOT OLD.notes OR NEW.parent_id IS NOT OLD.parent_id
  OR NEW.tax_rate_id IS NOT OLD.tax_rate_id OR NEW.is_tax_exempt IS NOT OLD.is_tax_exempt
  OR NEW.sales_rep_id IS NOT OLD.sales_rep_id
  OR (NEW.terms_id IS NOT OLD.terms_id AND OLD.terms_id IS NOT NULL)
  OR NEW.customer_type_id IS NOT OLD.customer_type_id OR NEW.job_type_id IS NOT OLD.job_type_id
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'CUSTOMER',
        'before', json_object('name', OLD.name, 'isActive', OLD.is_active),
        'after',  json_object('name', NEW.name, 'isActive', NEW.is_active),
        'changed', (SELECT json_group_array(f) FROM (
            SELECT 'email' AS f WHERE NEW.email IS NOT OLD.email
            UNION ALL SELECT 'company name' WHERE NEW.company_name IS NOT OLD.company_name
            UNION ALL SELECT 'phone' WHERE NEW.phone IS NOT OLD.phone
            UNION ALL SELECT 'billing address' WHERE NEW.billing_address IS NOT OLD.billing_address
            UNION ALL SELECT 'terms' WHERE NEW.terms_days IS NOT OLD.terms_days OR (NEW.terms_id IS NOT OLD.terms_id AND OLD.terms_id IS NOT NULL)
            UNION ALL SELECT 'notes' WHERE NEW.notes IS NOT OLD.notes
            UNION ALL SELECT 'parent customer' WHERE NEW.parent_id IS NOT OLD.parent_id
            UNION ALL SELECT 'sales tax' WHERE NEW.tax_rate_id IS NOT OLD.tax_rate_id OR NEW.is_tax_exempt IS NOT OLD.is_tax_exempt
            UNION ALL SELECT 'sales rep' WHERE NEW.sales_rep_id IS NOT OLD.sales_rep_id
            UNION ALL SELECT 'customer type' WHERE NEW.customer_type_id IS NOT OLD.customer_type_id
            UNION ALL SELECT 'job type' WHERE NEW.job_type_id IS NOT OLD.job_type_id))
    ));
END;

DROP TRIGGER trg_ledger_audit_vendor;
CREATE TRIGGER trg_ledger_audit_vendor
AFTER UPDATE ON vendors
WHEN NEW.name IS NOT OLD.name OR NEW.is_active IS NOT OLD.is_active OR NEW.email IS NOT OLD.email
  OR NEW.company_name IS NOT OLD.company_name OR NEW.phone IS NOT OLD.phone OR NEW.address IS NOT OLD.address
  OR NEW.terms_days IS NOT OLD.terms_days OR NEW.notes IS NOT OLD.notes OR NEW.is_1099 IS NOT OLD.is_1099
  OR NEW.tax_id_masked IS NOT OLD.tax_id_masked
  OR (NEW.terms_id IS NOT OLD.terms_id AND OLD.terms_id IS NOT NULL)
  OR NEW.vendor_type_id IS NOT OLD.vendor_type_id
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'VENDOR',
        'before', json_object('name', OLD.name, 'isActive', OLD.is_active),
        'after',  json_object('name', NEW.name, 'isActive', NEW.is_active),
        'changed', (SELECT json_group_array(f) FROM (
            SELECT 'email' AS f WHERE NEW.email IS NOT OLD.email
            UNION ALL SELECT 'company name' WHERE NEW.company_name IS NOT OLD.company_name
            UNION ALL SELECT 'phone' WHERE NEW.phone IS NOT OLD.phone
            UNION ALL SELECT 'address' WHERE NEW.address IS NOT OLD.address
            UNION ALL SELECT 'terms' WHERE NEW.terms_days IS NOT OLD.terms_days OR (NEW.terms_id IS NOT OLD.terms_id AND OLD.terms_id IS NOT NULL)
            UNION ALL SELECT 'notes' WHERE NEW.notes IS NOT OLD.notes
            UNION ALL SELECT '1099' WHERE NEW.is_1099 IS NOT OLD.is_1099
            UNION ALL SELECT 'tax ID' WHERE NEW.tax_id_masked IS NOT OLD.tax_id_masked
            UNION ALL SELECT 'vendor type' WHERE NEW.vendor_type_id IS NOT OLD.vendor_type_id))
    ));
END;
