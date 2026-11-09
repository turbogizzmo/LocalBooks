-- 0086_reconciliation_statements.sql
-- The bank or card statement attached to a reconciliation (T54). Files on transactions live in
-- `attachments`, which belong to a journal entry; a statement belongs to the reconciliation
-- itself, in progress or finished. Cancelling a reconciliation in progress removes its files with
-- it; a finished one can't be deleted (0005), so its files stay until someone removes them.

CREATE TABLE reconciliation_statements (
    id                  INTEGER PRIMARY KEY,
    reconciliation_id   INTEGER NOT NULL REFERENCES reconciliations (id) ON DELETE CASCADE,
    file_name           TEXT    NOT NULL CHECK (length(file_name) BETWEEN 1 AND 255),
    media_type          TEXT    NOT NULL,
    size_bytes          INTEGER NOT NULL CHECK (size_bytes BETWEEN 1 AND 26214400),
    content             BLOB    NOT NULL,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX ix_reconciliation_statements ON reconciliation_statements (reconciliation_id);
