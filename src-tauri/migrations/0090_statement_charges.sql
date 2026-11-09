-- 0090_statement_charges.sql
-- Statement charges (gap T64): a charge entered in a customer's register, without an invoice, and
-- billed on the customer's next statement. Each is kept as an invoice (so it posts to A/R, ages,
-- and takes payments and credits like one) marked here; it never prints as an invoice of its own.

CREATE TABLE statement_charge_invoices (
    invoice_id  INTEGER PRIMARY KEY REFERENCES invoices (id) ON DELETE CASCADE
) STRICT;
