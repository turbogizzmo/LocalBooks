-- Vendor tax IDs as users without access to sensitive data see them (see `tax_ids.rs`).
--
-- When the company file protects sensitive data, `vendors.tax_id` holds the encrypted value and
-- `tax_id_masked` the same ID with every digit but the last four starred ("***-**-6789",
-- "**-***6789"), so lists and the 1099 report can show which vendor it is and whether one is
-- missing without unlocking anything. Both are written together; the masked form is filled in when
-- a value is encrypted.

ALTER TABLE vendors ADD COLUMN tax_id_masked TEXT;
