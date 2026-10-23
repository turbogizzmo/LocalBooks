-- The copy of the company file taken just before a full-ledger import was posted. "Start over"
-- restores it, so the books return exactly to how they were before the import (reversing keeps
-- voided history and deactivated accounts; this does not).
ALTER TABLE ledger_import_batches ADD COLUMN pre_post_backup TEXT;
