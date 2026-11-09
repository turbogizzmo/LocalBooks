-- 0102_document_inbox.sql
-- The document inbox (gap T44): receipts and bills waiting to be entered.
--
-- A file dropped in the inbox, or picked for it, is kept inside the company file (so it's in every
-- backup) until it is entered: turning it into a bill, expense or check attaches it to the posted
-- transaction (an ordinary `attachments` row) and takes it out of the inbox. What LocalBooks could
-- read from a PDF's own text (no OCR) is kept as hints for the form: a total, a date and the name
-- at the top of the page. The inbox is not part of the books: a file can be removed from it.

CREATE TABLE inbox_documents (
    id              INTEGER PRIMARY KEY,
    file_name       TEXT    NOT NULL CHECK (length(file_name) BETWEEN 1 AND 255),
    media_type      TEXT    NOT NULL,
    size_bytes      INTEGER NOT NULL CHECK (size_bytes BETWEEN 1 AND 26214400),
    content         BLOB    NOT NULL,
    note            TEXT    CHECK (note IS NULL OR length(note) <= 500),
    amount_cents    INTEGER CHECK (amount_cents IS NULL OR amount_cents BETWEEN 1 AND 100000000000000),
    document_date   TEXT    CHECK (document_date IS NULL OR date(document_date) = document_date),
    name_hint       TEXT    CHECK (name_hint IS NULL OR length(name_hint) <= 100),
    added_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX ix_inbox_documents_added ON inbox_documents (added_at);
