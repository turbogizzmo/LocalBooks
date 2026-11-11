-- Imports: item quantities from a desktop company file (gap R3c).
--
-- A company file stores each inventory item's quantity on hand and value, and the transactions
-- that changed them (purchases, sales, adjustments, assembly builds). The reader stages both: the
-- items it tracks with the quantity and value the file stores for each, and every change with the
-- transaction that made it. A full import starts tracking each item and records the changes as
-- stock movements on the entries those transactions became, then compares each item with what the
-- file stores. Ids are the file's own (`ledger_import_items.source_id`,
-- `ledger_import_transactions.source_id`, `ledger_import_accounts.source_id`).

CREATE TABLE ledger_import_stock_items (
    batch_id                  INTEGER NOT NULL,
    source_item_id            INTEGER NOT NULL,
    asset_source_account_id   INTEGER NOT NULL,
    cogs_source_account_id    INTEGER,
    -- What the file stores: quantity × 10,000 and value in cents; NULL when unreadable.
    stored_qty_e4             INTEGER,
    stored_value_cents        INTEGER,
    -- What the reader found that leaves the item short (sold lines whose cost it couldn't read).
    read_note                 TEXT,
    -- After a full import: the LocalBooks item that tracks it, or why none does.
    inventory_item_id         INTEGER,
    note                      TEXT,
    PRIMARY KEY (batch_id, source_item_id)
) STRICT;

CREATE TABLE ledger_import_stock_moves (
    batch_id            INTEGER NOT NULL,
    seq                 INTEGER NOT NULL,
    source_txn_id       INTEGER NOT NULL,
    source_item_id      INTEGER NOT NULL,
    -- Positive goes into stock.
    qty_change_e4       INTEGER NOT NULL,
    value_change_cents  INTEGER NOT NULL,
    PRIMARY KEY (batch_id, seq)
) STRICT;

CREATE INDEX ix_ledger_import_stock_moves_item ON ledger_import_stock_moves (batch_id, source_item_id);
