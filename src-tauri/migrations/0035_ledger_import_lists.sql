-- Customers, vendors and items read from a desktop company file for a full-ledger import, staged
-- outside the books like the import's accounts and transactions. They become LocalBooks records
-- only through `qbw_ledger_import::import_lists`, which matches existing records by name first.

CREATE TABLE ledger_import_parties (
    batch_id            INTEGER NOT NULL,
    kind                TEXT    NOT NULL CHECK (kind IN ('CUSTOMER', 'VENDOR')),
    source_id           INTEGER NOT NULL,
    name                TEXT    NOT NULL,
    parent_source_id    INTEGER,
    company_name        TEXT,
    email               TEXT,
    phone               TEXT,
    address             TEXT,
    terms_days          INTEGER,
    tax_id              TEXT,
    is_1099             INTEGER NOT NULL DEFAULT 0 CHECK (is_1099 IN (0, 1)),
    is_active           INTEGER NOT NULL CHECK (is_active IN (0, 1)),
    balance_cents       INTEGER,
    notes               TEXT,
    -- The LocalBooks customer or vendor this maps to, and whether the import created it.
    party_id            INTEGER,
    created_by_import   INTEGER NOT NULL DEFAULT 0 CHECK (created_by_import IN (0, 1)),
    PRIMARY KEY (batch_id, kind, source_id)
) STRICT;

CREATE TABLE ledger_import_items (
    batch_id                    INTEGER NOT NULL,
    source_id                   INTEGER NOT NULL,
    name                        TEXT    NOT NULL,
    description                 TEXT,
    rate_cents                  INTEGER NOT NULL,
    income_source_account_id    INTEGER,
    item_kind                   TEXT    NOT NULL,
    is_taxable                  INTEGER NOT NULL CHECK (is_taxable IN (0, 1)),
    is_active                   INTEGER NOT NULL CHECK (is_active IN (0, 1)),
    product_id                  INTEGER,
    created_by_import           INTEGER NOT NULL DEFAULT 0 CHECK (created_by_import IN (0, 1)),
    PRIMARY KEY (batch_id, source_id)
) STRICT;
