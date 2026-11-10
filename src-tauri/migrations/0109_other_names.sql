-- 0109_other_names.sql
-- The Other Names list (gap T56): people and businesses paid or named on transactions who are
-- neither customers nor vendors, such as an owner taking a draw or a one-off payee. Each has the
-- contact details and notes a customer or vendor has, and the information panel beside the list.
--
-- A check or expense can be paid to an other name (`expenses.other_name_id`) instead of a vendor;
-- never both. Registers, reports by name and the party of an entry then show the other name.
-- Changes to the list are list changes in the audit trail.

CREATE TABLE other_names (
    id              INTEGER PRIMARY KEY,
    name            TEXT    NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 100 AND instr(name, ':') = 0),
    company_name    TEXT    CHECK (company_name IS NULL OR length(company_name) <= 100),
    email           TEXT    CHECK (email IS NULL OR length(email) <= 254),
    phone           TEXT    CHECK (phone IS NULL OR length(phone) <= 40),
    address         TEXT    CHECK (address IS NULL OR length(address) <= 500),
    notes           TEXT    CHECK (notes IS NULL OR length(notes) <= 10000),
    is_active       INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_other_names_name ON other_names (name COLLATE NOCASE);

ALTER TABLE expenses ADD COLUMN other_name_id INTEGER REFERENCES other_names (id) ON DELETE RESTRICT;
CREATE INDEX ix_expenses_other_name ON expenses (other_name_id) WHERE other_name_id IS NOT NULL;

CREATE TRIGGER trg_expenses_one_payee
BEFORE INSERT ON expenses
WHEN NEW.other_name_id IS NOT NULL AND NEW.vendor_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'EXP_TWO_PAYEES: a check is paid to a vendor or to an other name, not both');
END;

CREATE TRIGGER trg_expenses_other_name_fixed
BEFORE UPDATE OF other_name_id ON expenses
WHEN NEW.other_name_id IS NOT OLD.other_name_id
BEGIN
    SELECT RAISE(ABORT, 'EXP_IMMUTABLE: a posted check keeps its payee; edit it to change who it is to');
END;

CREATE TRIGGER trg_ledger_audit_other_name_insert
AFTER INSERT ON other_names
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object('list', 'OTHER_NAME', 'before', NULL, 'after', json_object('name', NEW.name), 'changed', json_array('added')));
END;

CREATE TRIGGER trg_ledger_audit_other_name_update
AFTER UPDATE ON other_names
WHEN NEW.name IS NOT OLD.name OR NEW.is_active IS NOT OLD.is_active OR NEW.company_name IS NOT OLD.company_name OR NEW.email IS NOT OLD.email
  OR NEW.phone IS NOT OLD.phone OR NEW.address IS NOT OLD.address OR NEW.notes IS NOT OLD.notes
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'OTHER_NAME',
        'before', json_object('name', OLD.name, 'isActive', OLD.is_active),
        'after',  json_object('name', NEW.name, 'isActive', NEW.is_active),
        'changed', (SELECT json_group_array(f) FROM (
            SELECT 'name' AS f WHERE NEW.name IS NOT OLD.name
            UNION ALL SELECT 'active' WHERE NEW.is_active IS NOT OLD.is_active
            UNION ALL SELECT 'company name' WHERE NEW.company_name IS NOT OLD.company_name
            UNION ALL SELECT 'email' WHERE NEW.email IS NOT OLD.email
            UNION ALL SELECT 'phone' WHERE NEW.phone IS NOT OLD.phone
            UNION ALL SELECT 'address' WHERE NEW.address IS NOT OLD.address
            UNION ALL SELECT 'notes' WHERE NEW.notes IS NOT OLD.notes))));
END;

-- 0082's view, with a check or expense paid to an other name taking that name.
DROP VIEW v_entry_party;
CREATE VIEW v_entry_party AS
SELECT e.id AS journal_entry_id,
       CASE x.source_type
           WHEN 'INVOICE'         THEN (SELECT c.name FROM invoices d JOIN customers c ON c.id = d.customer_id WHERE d.id = x.source_id)
           WHEN 'RECEIVE_PAYMENT' THEN (SELECT c.name FROM invoice_payments p JOIN invoices d ON d.id = p.invoice_id JOIN customers c ON c.id = d.customer_id WHERE p.id = x.source_id)
           WHEN 'SALES_RECEIPT'   THEN (SELECT c.name FROM sales_receipts d JOIN customers c ON c.id = d.customer_id WHERE d.id = x.source_id)
           WHEN 'CREDIT_MEMO'     THEN (SELECT c.name FROM credit_memos d JOIN customers c ON c.id = d.customer_id WHERE d.id = x.source_id)
           WHEN 'BILL'            THEN (SELECT v.name FROM bills d JOIN vendors v ON v.id = d.vendor_id WHERE d.id = x.source_id)
           WHEN 'BILL_PAYMENT'    THEN (SELECT v.name FROM bill_payments p JOIN bills d ON d.id = p.bill_id JOIN vendors v ON v.id = d.vendor_id WHERE p.id = x.source_id)
           WHEN 'VENDOR_CREDIT'   THEN (SELECT v.name FROM vendor_credits d JOIN vendors v ON v.id = d.vendor_id WHERE d.id = x.source_id)
           WHEN 'EXPENSE'         THEN (SELECT COALESCE(v.name, o2.name) FROM expenses d LEFT JOIN vendors v ON v.id = d.vendor_id
                                        LEFT JOIN other_names o2 ON o2.id = d.other_name_id WHERE d.id = x.source_id)
           WHEN 'CHECK'           THEN (SELECT COALESCE(v.name, o2.name) FROM expenses d LEFT JOIN vendors v ON v.id = d.vendor_id
                                        LEFT JOIN other_names o2 ON o2.id = d.other_name_id WHERE d.id = x.source_id)
           WHEN 'DEPOSIT'         THEN (SELECT COALESCE(c.name, v.name) FROM v_deposit_party d
                                        LEFT JOIN customers c ON c.id = d.customer_id LEFT JOIN vendors v ON v.id = d.vendor_id
                                        WHERE d.deposit_id = x.source_id)
           WHEN 'JOURNAL'         THEN COALESCE(
               (SELECT c.name FROM credit_refunds r JOIN credit_memos d ON d.id = r.credit_memo_id JOIN customers c ON c.id = d.customer_id WHERE r.journal_entry_id = x.id),
               (SELECT v.name FROM vendor_credit_refunds r JOIN vendor_credits d ON d.id = r.vendor_credit_id JOIN vendors v ON v.id = d.vendor_id WHERE r.journal_entry_id = x.id),
               (SELECT c.name FROM bounced_checks b JOIN customers c ON c.id = b.customer_id WHERE b.journal_entry_id = x.id))
       END AS party_name
FROM journal_entries e
LEFT JOIN journal_entries o ON o.id = e.reverses_entry_id
JOIN journal_entries x ON x.id = COALESCE(o.id, e.id);
