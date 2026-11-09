-- 0096_reconciliation_files.sql
-- Statement files on reconciliations (0086, T54) join the attachments library (0095, T58): each
-- `reconciliation_statements` row becomes a `record_attachments` row of kind RECONCILIATION, with
-- its name, type, bytes and date, and the old table goes. Files on a reconciliation then search,
-- preview and delete (reversibly) like any other attachment.

INSERT INTO record_attachments (record_kind, record_id, file_name, media_type, size_bytes, content, created_at)
SELECT 'RECONCILIATION', reconciliation_id, file_name, media_type, size_bytes, content, created_at
FROM reconciliation_statements
ORDER BY id;

DROP TABLE reconciliation_statements;
