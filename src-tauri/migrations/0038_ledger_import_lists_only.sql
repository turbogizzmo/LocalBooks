-- A file with lists but no transactions (for example an IIF of !ACCNT, !CUST, !VEND and !INVITEM
-- rows) posts no entries: "posting" it adds its accounts, customers, vendors and items. This flag
-- records that they are in the books, until Reverse deactivates them again.
ALTER TABLE ledger_import_batches ADD COLUMN lists_added INTEGER NOT NULL DEFAULT 0 CHECK (lists_added IN (0, 1));
