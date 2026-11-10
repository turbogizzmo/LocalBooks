-- A percent discount or charge item read from an imported company file, in parts per million,
-- so it comes in as a percent item (see products_services.percent_ppm in 0067).
ALTER TABLE ledger_import_items ADD COLUMN percent_ppm INTEGER;
