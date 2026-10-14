-- Inventory: quantity and value on hand per tracked product, kept by an append-only stock ledger.
--
-- Valuation is moving average cost, held as total value rather than a per-unit cost so rounding
-- never accumulates: selling q of Q units on hand costs round(value × q / Q), and selling the last
-- unit takes exactly the remaining value. Every movement carries the journal entry that records its
-- money side (bill, invoice, sales receipt or adjustment), so the stock ledger and the Inventory
-- Asset account always agree. Quantities are scaled by 10,000 like every other quantity.

INSERT INTO chart_of_accounts (account_number, name, account_type, description)
SELECT CASE WHEN EXISTS (SELECT 1 FROM chart_of_accounts WHERE account_number = '1400') THEN NULL ELSE '1400' END,
       'Inventory Asset', 'OTHER_CURRENT_ASSET', 'Cost of products on hand, at moving average cost'
WHERE NOT EXISTS (SELECT 1 FROM chart_of_accounts WHERE parent_id IS NULL AND name = 'Inventory Asset' COLLATE NOCASE);

CREATE TABLE inventory_items (
    id                  INTEGER PRIMARY KEY,
    product_id          INTEGER NOT NULL UNIQUE REFERENCES products_services (id) ON DELETE RESTRICT,
    sku                 TEXT    UNIQUE CHECK (sku IS NULL OR length(trim(sku)) > 0),
    asset_account_id    INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    cogs_account_id     INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    qty_on_hand_e4      INTEGER NOT NULL DEFAULT 0 CHECK (qty_on_hand_e4 >= 0),
    value_cents         INTEGER NOT NULL DEFAULT 0 CHECK (value_cents >= 0),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (qty_on_hand_e4 > 0 OR value_cents = 0)
) STRICT;

CREATE TRIGGER trg_inventory_item_accounts
BEFORE INSERT ON inventory_items
BEGIN
    SELECT RAISE(ABORT, 'STOCK_BAD_ASSET_ACCOUNT: the inventory account must be an Other Current Asset account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.asset_account_id) <> 'OTHER_CURRENT_ASSET'
       OR (SELECT is_system FROM chart_of_accounts WHERE id = NEW.asset_account_id) = 1;
    SELECT RAISE(ABORT, 'STOCK_BAD_COGS_ACCOUNT: the cost account must be a Cost of Goods Sold account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.cogs_account_id) <> 'COST_OF_GOODS_SOLD';
END;

CREATE TABLE stock_ledger_entries (
    id                  INTEGER PRIMARY KEY,
    item_id             INTEGER NOT NULL REFERENCES inventory_items (id) ON DELETE RESTRICT,
    movement_date       TEXT    NOT NULL CHECK (date(movement_date) = movement_date),
    kind                TEXT    NOT NULL CHECK (kind IN ('PURCHASE', 'SALE', 'ADJUSTMENT', 'VOID')),
    qty_change_e4       INTEGER NOT NULL CHECK (qty_change_e4 <> 0 OR value_change_cents <> 0),
    value_change_cents  INTEGER NOT NULL,
    qty_after_e4        INTEGER NOT NULL CHECK (qty_after_e4 >= 0),
    value_after_cents   INTEGER NOT NULL CHECK (value_after_cents >= 0),
    -- The journal entry holding this movement's debit/credit to the inventory account.
    journal_entry_id    INTEGER NOT NULL REFERENCES journal_entries (id) ON DELETE RESTRICT,
    reverses_id         INTEGER UNIQUE REFERENCES stock_ledger_entries (id) ON DELETE RESTRICT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (qty_after_e4 > 0 OR value_after_cents = 0),
    CHECK ((kind = 'VOID') = (reverses_id IS NOT NULL))
) STRICT;

CREATE INDEX ix_stock_item ON stock_ledger_entries (item_id, id);
CREATE INDEX ix_stock_entry ON stock_ledger_entries (journal_entry_id);

-- Quantity and value change only through stock movements.
CREATE TRIGGER trg_inventory_item_balances
BEFORE UPDATE OF qty_on_hand_e4, value_cents ON inventory_items
WHEN NOT EXISTS (
    SELECT 1 FROM stock_ledger_entries s
    WHERE s.id = (SELECT MAX(id) FROM stock_ledger_entries WHERE item_id = NEW.id)
      AND s.qty_after_e4 = NEW.qty_on_hand_e4 AND s.value_after_cents = NEW.value_cents
)
BEGIN
    SELECT RAISE(ABORT, 'STOCK_DIRECT_EDIT: quantity and value change only through stock movements');
END;

CREATE TRIGGER trg_stock_ledger_chain
BEFORE INSERT ON stock_ledger_entries
BEGIN
    SELECT RAISE(ABORT, 'STOCK_CHAIN: a movement must continue from the item''s current quantity and value')
    WHERE NEW.qty_after_e4 <> (SELECT qty_on_hand_e4 FROM inventory_items WHERE id = NEW.item_id) + NEW.qty_change_e4
       OR NEW.value_after_cents <> (SELECT value_cents FROM inventory_items WHERE id = NEW.item_id) + NEW.value_change_cents;
END;

CREATE TRIGGER trg_stock_ledger_apply
AFTER INSERT ON stock_ledger_entries
BEGIN
    UPDATE inventory_items SET qty_on_hand_e4 = NEW.qty_after_e4, value_cents = NEW.value_after_cents WHERE id = NEW.item_id;
END;

CREATE TRIGGER trg_stock_ledger_no_update BEFORE UPDATE ON stock_ledger_entries
BEGIN
    SELECT RAISE(ABORT, 'STOCK_IMMUTABLE: stock movements cannot be changed; void the transaction instead');
END;

CREATE TRIGGER trg_stock_ledger_no_delete BEFORE DELETE ON stock_ledger_entries
BEGIN
    SELECT RAISE(ABORT, 'STOCK_IMMUTABLE: stock movements cannot be changed; void the transaction instead');
END;

-- Which product each sales and purchase line was for (sales receipts and estimates already record it).
ALTER TABLE invoice_items ADD COLUMN product_service_id INTEGER REFERENCES products_services (id) ON DELETE RESTRICT;
ALTER TABLE bill_lines ADD COLUMN product_service_id INTEGER REFERENCES products_services (id) ON DELETE RESTRICT;
