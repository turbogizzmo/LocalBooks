-- Desktop company files staged before LocalBooks read their item type codes correctly labelled
-- fixed asset items (type 4) "Other charge"; that older reader labelled real other charges
-- (type 5) plain "Item", so in those batches "Other charge" can only mean a fixed asset.
-- The corrected reader shipped together with migration 0150, so a batch staged before 0150 was
-- applied to this file came from the older reader. Batches staged after it are already right and
-- are left alone, as are IIF, QuickBooks for Mac, GnuCash and spreadsheet imports.
UPDATE ledger_import_items
   SET item_kind = 'Fixed asset'
 WHERE item_kind = 'Other charge'
   AND batch_id IN (
       SELECT b.id FROM ledger_import_batches b
        WHERE b.source_format IS NULL
          AND (lower(b.source_path) LIKE '%.qbw' OR lower(b.source_path) LIKE '%.qbb' OR lower(b.source_path) LIKE '%.qbm')
          AND julianday(b.imported_at) < (SELECT julianday(installed_on) FROM _sqlx_migrations WHERE version = 150));
