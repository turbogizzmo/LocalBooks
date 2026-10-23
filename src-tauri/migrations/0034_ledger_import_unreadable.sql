-- Line rows of a full-ledger import that did not decode in full, with whatever of them could be
-- read and what became of each (RECOVERED: used, memo lost; NO_AMOUNT: a blank or text line;
-- UNREADABLE: not used). Shown on the import's review screen so nothing is a bare count.
CREATE TABLE ledger_import_unreadable (
    batch_id            INTEGER NOT NULL,
    line_no             INTEGER NOT NULL,
    kind                TEXT    NOT NULL,
    source_txn_id       INTEGER,
    date                TEXT,
    doc_num             TEXT,
    name                TEXT,
    source_account_id   INTEGER,
    cents               INTEGER,
    outcome             TEXT    NOT NULL CHECK (outcome IN ('RECOVERED', 'NO_AMOUNT', 'UNREADABLE')),
    PRIMARY KEY (batch_id, line_no)
) STRICT;
