-- Selling before the purchase is recorded: stock may go below zero.
--
-- Units sold beyond what is on hand are costed at the item's last purchase cost per unit
-- (`last_unit_cost_cents`; zero if it was never bought), so stock below zero carries a negative
-- value. When stock is bought while below zero, the difference between the actual cost of the units
-- already sold and their provisional cost is moved to Cost of Goods Sold (a COST_TRUE_UP movement in
-- the purchase's own journal entry), so inventory is again valued at actual cost.
--
-- Quantity and value must agree in sign: above zero the value is zero or more, at zero it is zero,
-- below zero it is zero or less. SQLite cannot change CHECK constraints in place, so both tables
-- are rebuilt with their rows and ids unchanged.

DROP TRIGGER trg_inventory_item_accounts;
DROP TRIGGER trg_inventory_item_balances;
DROP TRIGGER trg_stock_ledger_chain;
DROP TRIGGER trg_stock_ledger_apply;
DROP TRIGGER trg_stock_ledger_no_update;
DROP TRIGGER trg_stock_ledger_no_delete;

CREATE TABLE inventory_items_new (
    id                    INTEGER PRIMARY KEY,
    product_id            INTEGER NOT NULL UNIQUE REFERENCES products_services (id) ON DELETE RESTRICT,
    sku                   TEXT    UNIQUE CHECK (sku IS NULL OR length(trim(sku)) > 0),
    asset_account_id      INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    cogs_account_id       INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    qty_on_hand_e4        INTEGER NOT NULL DEFAULT 0,
    value_cents           INTEGER NOT NULL DEFAULT 0,
    -- Cost per unit on the latest purchase, used to cost units sold beyond what is on hand.
    last_unit_cost_cents  INTEGER CHECK (last_unit_cost_cents IS NULL OR last_unit_cost_cents >= 0),
    created_at            TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK ((qty_on_hand_e4 > 0 AND value_cents >= 0) OR (qty_on_hand_e4 = 0 AND value_cents = 0) OR (qty_on_hand_e4 < 0 AND value_cents <= 0))
) STRICT;

INSERT INTO inventory_items_new (id, product_id, sku, asset_account_id, cogs_account_id, qty_on_hand_e4, value_cents, last_unit_cost_cents, created_at)
SELECT i.id, i.product_id, i.sku, i.asset_account_id, i.cogs_account_id, i.qty_on_hand_e4, i.value_cents,
       (SELECT (s.value_change_cents * 10000 * 2 + s.qty_change_e4) / (s.qty_change_e4 * 2)
        FROM stock_ledger_entries s
        WHERE s.item_id = i.id AND s.kind = 'PURCHASE' AND s.qty_change_e4 > 0
          AND NOT EXISTS (SELECT 1 FROM stock_ledger_entries v WHERE v.reverses_id = s.id)
        ORDER BY s.id DESC LIMIT 1),
       i.created_at
FROM inventory_items i ORDER BY i.id;

CREATE TABLE stock_ledger_entries_new (
    id                  INTEGER PRIMARY KEY,
    item_id             INTEGER NOT NULL REFERENCES inventory_items (id) ON DELETE RESTRICT,
    movement_date       TEXT    NOT NULL CHECK (date(movement_date) = movement_date),
    kind                TEXT    NOT NULL CHECK (kind IN ('PURCHASE', 'SALE', 'ADJUSTMENT', 'COST_TRUE_UP', 'VOID')),
    qty_change_e4       INTEGER NOT NULL CHECK (qty_change_e4 <> 0 OR value_change_cents <> 0),
    value_change_cents  INTEGER NOT NULL,
    qty_after_e4        INTEGER NOT NULL,
    value_after_cents   INTEGER NOT NULL,
    journal_entry_id    INTEGER NOT NULL REFERENCES journal_entries (id) ON DELETE RESTRICT,
    reverses_id         INTEGER UNIQUE REFERENCES stock_ledger_entries (id) ON DELETE RESTRICT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK ((qty_after_e4 > 0 AND value_after_cents >= 0) OR (qty_after_e4 = 0 AND value_after_cents = 0) OR (qty_after_e4 < 0 AND value_after_cents <= 0)),
    CHECK ((kind = 'VOID') = (reverses_id IS NOT NULL)),
    CHECK (kind <> 'COST_TRUE_UP' OR qty_change_e4 = 0)
) STRICT;

INSERT INTO stock_ledger_entries_new
    (id, item_id, movement_date, kind, qty_change_e4, value_change_cents, qty_after_e4, value_after_cents, journal_entry_id, reverses_id, created_at)
SELECT id, item_id, movement_date, kind, qty_change_e4, value_change_cents, qty_after_e4, value_after_cents, journal_entry_id, reverses_id, created_at
FROM stock_ledger_entries ORDER BY id;

DROP TABLE stock_ledger_entries;
DROP TABLE inventory_items;
ALTER TABLE inventory_items_new RENAME TO inventory_items;
ALTER TABLE stock_ledger_entries_new RENAME TO stock_ledger_entries;

CREATE INDEX ix_stock_item ON stock_ledger_entries (item_id, id);
CREATE INDEX ix_stock_entry ON stock_ledger_entries (journal_entry_id);

CREATE TRIGGER trg_inventory_item_accounts
BEFORE INSERT ON inventory_items
BEGIN
    SELECT RAISE(ABORT, 'STOCK_BAD_ASSET_ACCOUNT: the inventory account must be an Other Current Asset account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.asset_account_id) <> 'OTHER_CURRENT_ASSET'
       OR (SELECT is_system FROM chart_of_accounts WHERE id = NEW.asset_account_id) = 1;
    SELECT RAISE(ABORT, 'STOCK_BAD_COGS_ACCOUNT: the cost account must be a Cost of Goods Sold account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.cogs_account_id) <> 'COST_OF_GOODS_SOLD';
END;

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
