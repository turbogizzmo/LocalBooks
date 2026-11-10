-- T68: Shopify Payments payout feeds (`payout_feeds/shopify.rs`, docs/plans/payout-feeds.md).
--
-- A feed's processor may now be Shopify Payments; its `account_ref` is the store's
-- `….myshopify.com` address (the access token is in the OS credential store, never here). The
-- table is rebuilt with its rows; payouts keep their feed ids.

CREATE TABLE payout_feeds_new (
    id                    INTEGER PRIMARY KEY,
    processor             TEXT    NOT NULL CHECK (processor IN ('STRIPE', 'PAYPAL', 'SHOPIFY')),
    name                  TEXT    NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 100),
    secret_key            TEXT    NOT NULL UNIQUE,
    account_ref           TEXT    CHECK (account_ref IS NULL OR length(account_ref) BETWEEN 1 AND 255),
    bank_account_id       INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    sales_account_id      INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    fee_account_id        INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    refund_account_id     INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    chargeback_account_id INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    last_fetched_at       TEXT,
    created_at            TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    CHECK (processor <> 'PAYPAL' OR account_ref IN ('LIVE', 'SANDBOX')),
    CHECK (processor <> 'SHOPIFY' OR account_ref LIKE '%_.myshopify.com')
) STRICT;

INSERT INTO payout_feeds_new (id, processor, name, secret_key, account_ref, bank_account_id, sales_account_id, fee_account_id, refund_account_id, chargeback_account_id, last_fetched_at, created_at)
SELECT id, processor, name, secret_key, account_ref, bank_account_id, sales_account_id, fee_account_id, refund_account_id, chargeback_account_id, last_fetched_at, created_at FROM payout_feeds ORDER BY id;

DROP TABLE payout_feeds;
ALTER TABLE payout_feeds_new RENAME TO payout_feeds;
