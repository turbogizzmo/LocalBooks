-- 0107_import_fractional_amounts.sql
-- Amounts an import could not take as whole cents (gap A10).
--
-- Some company-file tables store amounts as doubles. A stored balance, a form line or a payment
-- link whose double is not a whole number of cents is left out of the import (LocalBooks keeps
-- money in whole cents); the batch now keeps the list of them, as a JSON array of descriptions, so
-- the import summary can count and list each one. NULL for imports staged before this.

ALTER TABLE ledger_import_batches ADD COLUMN fractional_amounts TEXT CHECK (fractional_amounts IS NULL OR json_valid(fractional_amounts));
