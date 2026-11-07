-- Sales form header and a fuller customer record.
--
-- Customers
-- * billing_address (0026) stays the billing address; shipping_address is the second one.
-- * Two contacts, phone numbers, fax, website, account number, a credit limit in cents and a
--   resale (tax ID) number, all optional. A job (sub-customer) with no address of its own uses
--   the nearest customer above it that has one (application rule, see `sales_header.rs`).
-- * credit_limit_cents only warns: an invoice that would take the customer over it asks before
--   saving and is never refused.
--
-- Sales documents (invoices, sales receipts, estimates, credit memos)
-- * bill_to and ship_to are the document's own copy of the addresses, filled from the customer
--   when the document is written and edited per document. Changing the customer later does not
--   change documents already written. New documents always store text ('' = left blank), so
--   NULL marks a document written before this migration; those keep printing the customer's
--   current billing address, as they always did.
-- * po_number (the customer's purchase order number) and fob (free on board point) are plain
--   text printed in the header.

ALTER TABLE customers ADD COLUMN shipping_address   TEXT;
ALTER TABLE customers ADD COLUMN contact_name       TEXT;
ALTER TABLE customers ADD COLUMN alt_contact_name   TEXT;
ALTER TABLE customers ADD COLUMN alt_phone          TEXT;
ALTER TABLE customers ADD COLUMN fax                TEXT;
ALTER TABLE customers ADD COLUMN website            TEXT;
ALTER TABLE customers ADD COLUMN account_number     TEXT;
ALTER TABLE customers ADD COLUMN credit_limit_cents INTEGER CHECK (credit_limit_cents IS NULL OR credit_limit_cents BETWEEN 0 AND 100000000000000);
ALTER TABLE customers ADD COLUMN resale_number      TEXT;

ALTER TABLE invoices       ADD COLUMN bill_to   TEXT;
ALTER TABLE invoices       ADD COLUMN ship_to   TEXT;
ALTER TABLE invoices       ADD COLUMN po_number TEXT CHECK (po_number IS NULL OR length(po_number) <= 25);
ALTER TABLE invoices       ADD COLUMN fob       TEXT CHECK (fob IS NULL OR length(fob) <= 25);
ALTER TABLE sales_receipts ADD COLUMN bill_to   TEXT;
ALTER TABLE sales_receipts ADD COLUMN ship_to   TEXT;
ALTER TABLE sales_receipts ADD COLUMN po_number TEXT CHECK (po_number IS NULL OR length(po_number) <= 25);
ALTER TABLE sales_receipts ADD COLUMN fob       TEXT CHECK (fob IS NULL OR length(fob) <= 25);
ALTER TABLE estimates      ADD COLUMN bill_to   TEXT;
ALTER TABLE estimates      ADD COLUMN ship_to   TEXT;
ALTER TABLE estimates      ADD COLUMN po_number TEXT CHECK (po_number IS NULL OR length(po_number) <= 25);
ALTER TABLE estimates      ADD COLUMN fob       TEXT CHECK (fob IS NULL OR length(fob) <= 25);
ALTER TABLE credit_memos   ADD COLUMN bill_to   TEXT;
ALTER TABLE credit_memos   ADD COLUMN ship_to   TEXT;
ALTER TABLE credit_memos   ADD COLUMN po_number TEXT CHECK (po_number IS NULL OR length(po_number) <= 25);
ALTER TABLE credit_memos   ADD COLUMN fob       TEXT CHECK (fob IS NULL OR length(fob) <= 25);

-- Changes to the new customer fields are recorded in the audit trail by 0075, which runs after
-- 0074 rebuilds that table.
