-- Bank connections through SimpleFIN (https://www.simplefin.org/protocol.html).
--
-- A connection is created by claiming a one-time setup token, which yields an Access URL: the
-- credentials for read-only access to the user's bank data. The Access URL is never stored in the
-- company file; it lives in the operating system's credential store under `secret_key`. A company
-- file opened on another computer therefore keeps its links but must be reconnected there.
--
-- Each SimpleFIN account can be linked to one Bank or Credit Card account. Syncing adds its posted
-- transactions to that account's review queue (bank_transactions), identified by the fingerprint
-- 'SFIN|<account id>|<transaction id>', so a sync never adds the same transaction twice. Nothing
-- reaches the ledger until the user approves each line, exactly as with imported statement files.
-- `sync_from` is the first date imported, chosen when linking so a sync does not repeat lines
-- already imported from statement files.

CREATE TABLE simplefin_connections (
    id              INTEGER PRIMARY KEY,
    secret_key      TEXT    NOT NULL UNIQUE CHECK (length(secret_key) = 64),
    -- Host of the Access URL (for example bridge.simplefin.org), shown to the user.
    server_host     TEXT    NOT NULL CHECK (length(server_host) > 0),
    -- Protocol major version the server reported at connection (Bridge reports 1.0 as of 2026).
    protocol        INTEGER NOT NULL DEFAULT 1 CHECK (protocol IN (1, 2)),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_synced_at  TEXT,
    -- Messages from the last contact with the server (its error list, or why it failed).
    last_messages   TEXT
) STRICT;

CREATE TABLE simplefin_accounts (
    id               INTEGER PRIMARY KEY,
    connection_id    INTEGER NOT NULL REFERENCES simplefin_connections (id) ON DELETE CASCADE,
    remote_id        TEXT    NOT NULL CHECK (length(remote_id) > 0),
    name             TEXT    NOT NULL,
    institution      TEXT,
    currency         TEXT    NOT NULL,
    balance_cents    INTEGER,
    balance_date     TEXT    CHECK (balance_date IS NULL OR date(balance_date) = balance_date),
    bank_account_id  INTEGER UNIQUE REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    sync_from        TEXT    CHECK (sync_from IS NULL OR date(sync_from) = sync_from),
    last_synced_at   TEXT,
    UNIQUE (connection_id, remote_id),
    CHECK ((bank_account_id IS NULL) = (sync_from IS NULL))
) STRICT;

CREATE TRIGGER trg_simplefin_link_insert
BEFORE INSERT ON simplefin_accounts
WHEN NEW.bank_account_id IS NOT NULL
 AND (SELECT account_type FROM chart_of_accounts WHERE id = NEW.bank_account_id) NOT IN ('BANK', 'CREDIT_CARD')
BEGIN
    SELECT RAISE(ABORT, 'SFIN_ACCOUNT_TYPE: a bank connection can only feed a Bank or Credit Card account');
END;

CREATE TRIGGER trg_simplefin_link_update
BEFORE UPDATE OF bank_account_id ON simplefin_accounts
WHEN NEW.bank_account_id IS NOT NULL
 AND (SELECT account_type FROM chart_of_accounts WHERE id = NEW.bank_account_id) NOT IN ('BANK', 'CREDIT_CARD')
BEGIN
    SELECT RAISE(ABORT, 'SFIN_ACCOUNT_TYPE: a bank connection can only feed a Bank or Credit Card account');
END;
