-- QuickBooks Desktop (.qbw) import staging.
--
-- Rows extracted from a .qbw file land here as raw, unvalidated data. Nothing in these tables is
-- part of the ledger: they carry no foreign keys into it, no balancing triggers, and no balances
-- read from them. Data reaches the books only through `qbw_import::post_batch`, which posts each
-- QuickBooks invoice through the ledger's own `post_entry_in_tx` checks.

-- One row per file read.
CREATE TABLE staging_import_batches (
    id                  INTEGER PRIMARY KEY,
    source_path         TEXT    NOT NULL,
    file_name           TEXT    NOT NULL,
    file_size_bytes     INTEGER NOT NULL CHECK (file_size_bytes >= 0),
    page_count          INTEGER NOT NULL CHECK (page_count >= 0),
    attributed_tables   INTEGER NOT NULL CHECK (attributed_tables >= 0),
    header_count        INTEGER NOT NULL CHECK (header_count >= 0),
    -- Line-item records the parser found but whose amount encoding it cannot decode yet.
    skipped_line_count  INTEGER NOT NULL CHECK (skipped_line_count >= 0),
    imported_at         TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    posted_at           TEXT
) STRICT;

-- One row per extracted invoice line item, kept exactly as read (amounts already in i64 cents).
CREATE TABLE staging_raw_transactions (
    id                  INTEGER PRIMARY KEY,
    batch_id            INTEGER NOT NULL,
    date                TEXT,
    raw_description     TEXT    NOT NULL,
    raw_amount_cents    INTEGER NOT NULL,
    raw_account_label   TEXT    NOT NULL,
    qb_invoice_id       TEXT    NOT NULL,
    qb_item_id          TEXT,
    amount_type         TEXT    NOT NULL,
    page_number         INTEGER NOT NULL,
    page_offset         INTEGER NOT NULL,
    UNIQUE (batch_id, page_number, page_offset)
) STRICT;

CREATE INDEX ix_staging_raw_batch_invoice ON staging_raw_transactions (batch_id, qb_invoice_id);
CREATE INDEX ix_staging_raw_batch_label ON staging_raw_transactions (batch_id, raw_account_label);

-- The user's choice of LocalBooks account for each raw label. `account_id` is validated against
-- the chart of accounts when the batch is posted, not here.
CREATE TABLE staging_account_map (
    batch_id            INTEGER NOT NULL,
    raw_account_label   TEXT    NOT NULL,
    account_id          INTEGER,
    PRIMARY KEY (batch_id, raw_account_label)
) STRICT;

-- Ledger-side register of QuickBooks invoices already posted. The primary key makes posting the
-- same invoice twice impossible, even from a second import of the same file. When the posted
-- entry is voided, the row is repointed at the new entry on the next post.
CREATE TABLE qbw_posted_invoices (
    qb_invoice_id       TEXT    PRIMARY KEY,
    journal_entry_id    INTEGER NOT NULL UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    batch_id            INTEGER NOT NULL,
    posted_at           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
