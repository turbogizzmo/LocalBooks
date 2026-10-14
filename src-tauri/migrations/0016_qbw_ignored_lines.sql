-- Lines the QuickBooks importer found outside invoice line-item tables. The parser's line-item
-- pattern also matches bytes on pages of unrelated tables (payroll, audit, budgets, internal
-- catalogs); those are counted here and never staged.
ALTER TABLE staging_import_batches ADD COLUMN ignored_line_count INTEGER NOT NULL DEFAULT 0 CHECK (ignored_line_count >= 0);
