-- 0104_party_contacts.sql
-- Any number of contacts per customer or vendor, each with a role (gap T51).
--
-- A customer had a contact and an alternate contact (0065); a vendor had none. Each contact is now
-- a row here, in the order the record shows them. A customer's contact can be the default for
-- invoices (printed in Bill To, and sent invoices, estimates, sales receipts and credit memos when
-- it has an email) and one for statements (printed on statements and sent them). A vendor's
-- `for_invoices` contact is their main contact.
--
-- customers.contact_name and alt_contact_name stay, kept by the app as the invoice contact (or the
-- first one) and the next one, so everything that reads them (Bill To, list reports, exports)
-- carries on. The existing values become the first rows here.

CREATE TABLE party_contacts (
    id              INTEGER PRIMARY KEY,
    customer_id     INTEGER REFERENCES customers (id) ON DELETE CASCADE,
    vendor_id       INTEGER REFERENCES vendors (id) ON DELETE CASCADE,
    position        INTEGER NOT NULL CHECK (position > 0),
    name            TEXT    NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 100),
    role            TEXT    CHECK (role IS NULL OR length(role) <= 50),
    email           TEXT    CHECK (email IS NULL OR length(email) <= 254),
    phone           TEXT    CHECK (phone IS NULL OR length(phone) <= 40),
    mobile          TEXT    CHECK (mobile IS NULL OR length(mobile) <= 40),
    for_invoices    INTEGER NOT NULL DEFAULT 0 CHECK (for_invoices IN (0, 1)),
    for_statements  INTEGER NOT NULL DEFAULT 0 CHECK (for_statements IN (0, 1)),
    CHECK ((customer_id IS NULL) <> (vendor_id IS NULL)),
    CHECK (vendor_id IS NULL OR for_statements = 0)
) STRICT;
CREATE INDEX ix_party_contacts_customer ON party_contacts (customer_id, position) WHERE customer_id IS NOT NULL;
CREATE INDEX ix_party_contacts_vendor ON party_contacts (vendor_id, position) WHERE vendor_id IS NOT NULL;
-- One default per use.
CREATE UNIQUE INDEX ux_party_contacts_customer_invoices ON party_contacts (customer_id) WHERE customer_id IS NOT NULL AND for_invoices = 1;
CREATE UNIQUE INDEX ux_party_contacts_customer_statements ON party_contacts (customer_id) WHERE customer_id IS NOT NULL AND for_statements = 1;
CREATE UNIQUE INDEX ux_party_contacts_vendor_main ON party_contacts (vendor_id) WHERE vendor_id IS NOT NULL AND for_invoices = 1;

INSERT INTO party_contacts (customer_id, position, name, role, for_invoices, for_statements)
SELECT id, 1, substr(trim(contact_name), 1, 100), 'Main', 1, 1 FROM customers WHERE trim(COALESCE(contact_name, '')) <> '';

INSERT INTO party_contacts (customer_id, position, name, role, phone)
SELECT id, CASE WHEN trim(COALESCE(contact_name, '')) <> '' THEN 2 ELSE 1 END, substr(trim(alt_contact_name), 1, 100), 'Alternate',
       substr(NULLIF(trim(COALESCE(alt_phone, '')), ''), 1, 40)
FROM customers WHERE trim(COALESCE(alt_contact_name, '')) <> '';

-- Contact changes are list changes, in the audit trail like the customer and vendor fields.
CREATE TRIGGER trg_ledger_audit_contact_insert
AFTER INSERT ON party_contacts
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', COALESCE(NEW.customer_id, NEW.vendor_id), json_object(
        'list', CASE WHEN NEW.customer_id IS NOT NULL THEN 'CUSTOMER' ELSE 'VENDOR' END,
        'before', json_object('contact', NULL),
        'after', json_object('contact', NEW.name, 'role', NEW.role),
        'changed', json_array('contact added')));
END;

CREATE TRIGGER trg_ledger_audit_contact_update
AFTER UPDATE ON party_contacts
WHEN NEW.name IS NOT OLD.name OR NEW.role IS NOT OLD.role OR NEW.email IS NOT OLD.email OR NEW.phone IS NOT OLD.phone
  OR NEW.mobile IS NOT OLD.mobile OR NEW.for_invoices IS NOT OLD.for_invoices OR NEW.for_statements IS NOT OLD.for_statements
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', COALESCE(NEW.customer_id, NEW.vendor_id), json_object(
        'list', CASE WHEN NEW.customer_id IS NOT NULL THEN 'CUSTOMER' ELSE 'VENDOR' END,
        'before', json_object('contact', OLD.name, 'role', OLD.role),
        'after', json_object('contact', NEW.name, 'role', NEW.role),
        'changed', json_array('contact')));
END;

CREATE TRIGGER trg_ledger_audit_contact_delete
AFTER DELETE ON party_contacts
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', COALESCE(OLD.customer_id, OLD.vendor_id), json_object(
        'list', CASE WHEN OLD.customer_id IS NOT NULL THEN 'CUSTOMER' ELSE 'VENDOR' END,
        'before', json_object('contact', OLD.name, 'role', OLD.role),
        'after', json_object('contact', NULL),
        'changed', json_array('contact removed')));
END;
