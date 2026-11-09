-- 0082_deposit_received_from.sql
-- A deposit line's Received From name (gap T15).
--
-- An "other funds" line of a deposit can name the customer or vendor the money came from. A
-- deposit whose named lines all name the same customer or vendor belongs to that name: registers
-- show it as the payee, and the name's transactions and reports by name list it. A deposit whose
-- lines name different people, or none, has no name, as before.
--
-- A deposited payment or sales receipt already belongs to its customer, so only the other lines
-- take a name. A line's name is fixed once written, like the rest of the line.

ALTER TABLE deposit_lines ADD COLUMN customer_id INTEGER REFERENCES customers (id) ON DELETE RESTRICT;
ALTER TABLE deposit_lines ADD COLUMN vendor_id INTEGER REFERENCES vendors (id) ON DELETE RESTRICT;

CREATE INDEX ix_deposit_lines_customer ON deposit_lines (customer_id) WHERE customer_id IS NOT NULL;
CREATE INDEX ix_deposit_lines_vendor ON deposit_lines (vendor_id) WHERE vendor_id IS NOT NULL;

CREATE TRIGGER trg_dep_lines_name_checks
BEFORE INSERT ON deposit_lines
WHEN NEW.customer_id IS NOT NULL OR NEW.vendor_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'DEP_TWO_NAMES: a deposit line is received from a customer or a vendor, not both')
    WHERE NEW.customer_id IS NOT NULL AND NEW.vendor_id IS NOT NULL;
    SELECT RAISE(ABORT, 'DEP_PAYMENT_NAME: a deposited payment is already from its customer')
    WHERE NEW.invoice_payment_id IS NOT NULL OR NEW.sales_receipt_id IS NOT NULL;
END;

CREATE TRIGGER trg_dep_lines_name_fixed
BEFORE UPDATE OF customer_id, vendor_id ON deposit_lines
WHEN NEW.customer_id IS NOT OLD.customer_id OR NEW.vendor_id IS NOT OLD.vendor_id
BEGIN
    SELECT RAISE(ABORT, 'DEP_IMMUTABLE: deposit lines can only be marked void along with their deposit');
END;

-- The one customer or vendor a deposit's named lines share; no row when they name none, or more
-- than one.
CREATE VIEW v_deposit_party AS
SELECT deposit_id, MAX(customer_id) AS customer_id, MAX(vendor_id) AS vendor_id
FROM deposit_lines
WHERE customer_id IS NOT NULL OR vendor_id IS NOT NULL
GROUP BY deposit_id
HAVING COUNT(DISTINCT COALESCE('c' || customer_id, 'v' || vendor_id)) = 1;

-- 0077's view, plus deposits. A void's reversal still takes the name of the entry it reverses.
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
           WHEN 'EXPENSE'         THEN (SELECT v.name FROM expenses d JOIN vendors v ON v.id = d.vendor_id WHERE d.id = x.source_id)
           WHEN 'CHECK'           THEN (SELECT v.name FROM expenses d JOIN vendors v ON v.id = d.vendor_id WHERE d.id = x.source_id)
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
