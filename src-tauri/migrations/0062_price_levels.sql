-- Price levels (QuickBooks-style): named sets of prices a customer can be given by default.
--
-- * FIXED_PERCENT: every item's standard price raised or lowered by `percent_milli` (thousandths of
--   a percent, −100% to +1,000%), then rounded as `rounding` says.
-- * PER_ITEM: listed items get their own price: a fixed price, or the standard price or the unit
--   cost raised or lowered by a percentage (then rounded). Items not listed sell at standard price.
--
-- `rounding` is the JSON the Change Item Prices rounding uses ({"mode":"NONE"}, {"mode":"NEAREST",
-- "stepCents":100}, {"mode":"END_IN99"}, ...). Changes are recorded as PRICE_CHANGE events in the
-- ledger audit trail by the application (see `price_levels.rs`). Documents keep the rate they
-- were written with; price levels only fill in the rate of new lines.

CREATE TABLE price_levels (
    id              INTEGER PRIMARY KEY,
    name            TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    kind            TEXT    NOT NULL CHECK (kind IN ('FIXED_PERCENT', 'PER_ITEM')),
    percent_milli   INTEGER NOT NULL DEFAULT 0 CHECK (percent_milli BETWEEN -100000 AND 1000000),
    rounding        TEXT    NOT NULL DEFAULT '{"mode":"NONE"}' CHECK (json_valid(rounding)),
    is_active       INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_price_levels_name ON price_levels (name COLLATE NOCASE);

CREATE TABLE price_level_items (
    price_level_id  INTEGER NOT NULL REFERENCES price_levels (id) ON DELETE CASCADE,
    product_id      INTEGER NOT NULL REFERENCES products_services (id) ON DELETE CASCADE,
    basis           TEXT    NOT NULL CHECK (basis IN ('FIXED', 'STANDARD', 'COST')),
    price_cents     INTEGER CHECK (price_cents IS NULL OR price_cents BETWEEN 0 AND 100000000000000),
    percent_milli   INTEGER CHECK (percent_milli IS NULL OR percent_milli BETWEEN -100000 AND 1000000),
    PRIMARY KEY (price_level_id, product_id),
    CHECK ((basis = 'FIXED' AND price_cents IS NOT NULL AND percent_milli IS NULL)
        OR (basis <> 'FIXED' AND price_cents IS NULL AND percent_milli IS NOT NULL))
) STRICT;
CREATE INDEX ix_price_level_items_product ON price_level_items (product_id);

ALTER TABLE customers ADD COLUMN price_level_id INTEGER REFERENCES price_levels (id) ON DELETE SET NULL;
CREATE INDEX ix_customers_price_level ON customers (price_level_id);

-- Imports: a desktop file's price levels, staged with its lists, and each customer's default level
-- (by name). `items` is a JSON array of {productId, basis, priceCents, percentMilli} whose productId
-- is the file's item id.
CREATE TABLE ledger_import_price_levels (
    batch_id        INTEGER NOT NULL,
    name            TEXT    NOT NULL,
    kind            TEXT    NOT NULL CHECK (kind IN ('FIXED_PERCENT', 'PER_ITEM')),
    percent_milli   INTEGER NOT NULL,
    is_active       INTEGER NOT NULL CHECK (is_active IN (0, 1)),
    items           TEXT    NOT NULL CHECK (json_valid(items)),
    -- The level in LocalBooks once lists are added, and whether the import created it.
    price_level_id  INTEGER,
    created_by_import INTEGER NOT NULL DEFAULT 0 CHECK (created_by_import IN (0, 1)),
    PRIMARY KEY (batch_id, name)
) STRICT;
ALTER TABLE ledger_import_parties ADD COLUMN price_level TEXT;
