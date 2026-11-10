-- Desktop company files staged before LocalBooks read their item type codes correctly labelled
-- fixed asset items "Other charge". Bringing in such a batch's lists now makes "Other charge"
-- items LocalBooks other charges, so those older staged items are relabelled as what they were.
-- Only desktop company files and backups had that mislabel; IIF, QuickBooks for Mac, GnuCash and
-- spreadsheet imports name their other charges correctly and are left alone.
UPDATE ledger_import_items
   SET item_kind = 'Fixed asset'
 WHERE item_kind = 'Other charge'
   AND batch_id IN (
       SELECT id FROM ledger_import_batches
        WHERE source_format IS NULL
          AND (lower(source_path) LIKE '%.qbw' OR lower(source_path) LIKE '%.qbb' OR lower(source_path) LIKE '%.qbm'));
