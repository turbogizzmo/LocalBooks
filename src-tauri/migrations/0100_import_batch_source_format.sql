-- T62: what a staged import was read from, when its file's extension doesn't say it. A spreadsheet
-- the user laid out is a .csv or .xlsx like a report export, so it is recorded as 'SHEET'. Batches
-- staged before this column existed (and every other import) keep NULL and are told apart by
-- their file's extension, as before.
ALTER TABLE ledger_import_batches ADD COLUMN source_format TEXT CHECK (source_format IS NULL OR source_format IN ('SHEET'));
