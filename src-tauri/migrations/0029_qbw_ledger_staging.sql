-- Full-ledger imports from desktop company files that the invoice-line reader cannot read.
--
-- `qbw_company::read_ledger` decodes the file's own accounts and every posting line; the rows land
-- here as staged data, outside the ledger, exactly like the invoice staging tables in 0012. They
-- reach the books only through `qbw_ledger_import::post_batch`, which posts each balanced source
-- transaction through `ledger::post_entry_in_tx`.

CREATE TABLE ledger_import_batches (
    id                              INTEGER PRIMARY KEY,
    source_path                     TEXT    NOT NULL,
    file_name                       TEXT    NOT NULL,
    file_size_bytes                 INTEGER NOT NULL CHECK (file_size_bytes >= 0),
    -- Identifies the company's transaction set, so the same books cannot be posted twice.
    fingerprint                     TEXT    NOT NULL,
    row_pages                       INTEGER NOT NULL CHECK (row_pages >= 0),
    decoded_pages                   INTEGER NOT NULL CHECK (decoded_pages >= 0),
    unreadable_lines                INTEGER NOT NULL CHECK (unreadable_lines >= 0),
    rejected_lines                  INTEGER NOT NULL CHECK (rejected_lines >= 0),
    accounts_matching_stored        INTEGER NOT NULL CHECK (accounts_matching_stored >= 0),
    accounts_with_stored_balance    INTEGER NOT NULL CHECK (accounts_with_stored_balance >= 0),
    imported_at                     TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    posted_at                       TEXT
) STRICT;

CREATE TABLE ledger_import_accounts (
    batch_id                INTEGER NOT NULL,
    source_id               INTEGER NOT NULL,
    name                    TEXT    NOT NULL,
    full_name               TEXT    NOT NULL,
    account_type            TEXT    NOT NULL,
    parent_source_id        INTEGER,
    is_active               INTEGER NOT NULL CHECK (is_active IN (0, 1)),
    stored_balance_cents    INTEGER,
    -- The LocalBooks account the user (or auto-mapping) chose; validated when posting.
    account_id              INTEGER,
    -- Set when the importer created `account_id` for this batch (used by reverse).
    created_by_import       INTEGER NOT NULL DEFAULT 0 CHECK (created_by_import IN (0, 1)),
    PRIMARY KEY (batch_id, source_id)
) STRICT;

CREATE TABLE ledger_import_transactions (
    batch_id            INTEGER NOT NULL,
    source_id           INTEGER NOT NULL,
    kind                TEXT    NOT NULL,
    date                TEXT    NOT NULL,
    doc_num             TEXT,
    name                TEXT,
    memo                TEXT,
    -- Non-zero when a line could not be read; such transactions are listed, never posted.
    imbalance_cents     INTEGER NOT NULL,
    journal_entry_id    INTEGER REFERENCES journal_entries (id) ON DELETE RESTRICT,
    PRIMARY KEY (batch_id, source_id)
) STRICT;

CREATE TABLE ledger_import_lines (
    batch_id            INTEGER NOT NULL,
    source_txn_id       INTEGER NOT NULL,
    line_no             INTEGER NOT NULL,
    source_account_id   INTEGER NOT NULL,
    -- Negative = debit, positive = credit, as stored in the source file.
    cents               INTEGER NOT NULL,
    memo                TEXT,
    PRIMARY KEY (batch_id, source_txn_id, line_no)
) STRICT;

CREATE INDEX ix_ledger_import_lines_account ON ledger_import_lines (batch_id, source_account_id);
