-- 0095_attachments_library.sql
-- The attachments library (T58) and retention of attachments (R9).
--
-- 1. Files on records other than posted transactions: customers, vendors, items, accounts,
--    estimates, purchase orders and reconciliations. `attachments` (0028) belongs to a journal
--    entry and stays as it is; `record_attachments` names its record by kind and id. Condense keeps
--    it whole and treats a file on an estimate or purchase order as a link that keeps that
--    document (see `condense::FORM_LINKS`).
-- 2. Both kinds gain keywords (for searching the library) and a flag to send the file with the
--    form when it is emailed.
-- 3. Deleting is reversible: `deleted_at` hides a file and the library can restore it. Only a file
--    already deleted can be removed for good, and the screens ask first.

CREATE TABLE record_attachments (
    id                  INTEGER PRIMARY KEY,
    record_kind         TEXT    NOT NULL CHECK (record_kind IN ('CUSTOMER', 'VENDOR', 'ITEM', 'ACCOUNT', 'ESTIMATE', 'PURCHASE_ORDER', 'RECONCILIATION')),
    record_id           INTEGER NOT NULL,
    file_name           TEXT    NOT NULL CHECK (length(file_name) BETWEEN 1 AND 255),
    media_type          TEXT    NOT NULL,
    size_bytes          INTEGER NOT NULL CHECK (size_bytes BETWEEN 1 AND 26214400),
    content             BLOB    NOT NULL,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    keywords            TEXT    CHECK (keywords IS NULL OR length(keywords) BETWEEN 1 AND 500),
    send_with_form      INTEGER NOT NULL DEFAULT 0 CHECK (send_with_form IN (0, 1)),
    deleted_at          TEXT
) STRICT;
CREATE INDEX ix_record_attachments_record ON record_attachments (record_kind, record_id);

ALTER TABLE attachments ADD COLUMN keywords TEXT CHECK (keywords IS NULL OR length(keywords) BETWEEN 1 AND 500);
ALTER TABLE attachments ADD COLUMN send_with_form INTEGER NOT NULL DEFAULT 0 CHECK (send_with_form IN (0, 1));
ALTER TABLE attachments ADD COLUMN deleted_at TEXT;

-- A file is removed for good only once it has been deleted (moved to Deleted in the library).
CREATE TRIGGER trg_attachments_delete_only_deleted BEFORE DELETE ON attachments
WHEN OLD.deleted_at IS NULL
BEGIN
    SELECT RAISE(ABORT, 'ATTACH_NOT_DELETED: delete the file first; only deleted files can be removed for good');
END;
CREATE TRIGGER trg_record_attachments_delete_only_deleted BEFORE DELETE ON record_attachments
WHEN OLD.deleted_at IS NULL
BEGIN
    SELECT RAISE(ABORT, 'ATTACH_NOT_DELETED: delete the file first; only deleted files can be removed for good');
END;
