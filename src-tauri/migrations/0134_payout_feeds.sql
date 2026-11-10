-- T68: payout feeds from payment processors (`payout_feeds.rs`, design in docs/plans/payout-feeds.md).

-- Deductions from a deposit: debits to other accounts that come out of what the bank gets (a
-- processor's fees, refunds and chargebacks on a payout), like cash back but several and labelled.
CREATE TABLE deposit_deductions (
    id           INTEGER PRIMARY KEY,
    deposit_id   INTEGER NOT NULL REFERENCES deposits (id) ON DELETE CASCADE,
    line_number  INTEGER NOT NULL CHECK (line_number > 0),
    account_id   INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    amount_cents INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    memo         TEXT,
    kind         TEXT    NOT NULL CHECK (kind IN ('FEE', 'REFUND', 'CHARGEBACK', 'OTHER')),
    UNIQUE (deposit_id, line_number)
) STRICT;

-- A processor account the user connected. The read-only key is in the OS credential store under
-- `secret_key`, never here.
CREATE TABLE payout_feeds (
    id                    INTEGER PRIMARY KEY,
    processor             TEXT    NOT NULL CHECK (processor IN ('STRIPE')),
    name                  TEXT    NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 100),
    secret_key            TEXT    NOT NULL UNIQUE,
    bank_account_id       INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    sales_account_id      INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    fee_account_id        INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    refund_account_id     INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    chargeback_account_id INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    last_fetched_at       TEXT,
    created_at            TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
) STRICT;

-- A payout fetched from a feed, with its breakdown. `deposit_id` is a plain id (no foreign key),
-- so condensing the deposit isn't blocked; `details_json` lists the charges, refunds and disputes.
CREATE TABLE payouts (
    id                  INTEGER PRIMARY KEY,
    feed_id             INTEGER NOT NULL REFERENCES payout_feeds (id) ON DELETE CASCADE,
    processor_payout_id TEXT    NOT NULL,
    arrival_date        TEXT    NOT NULL CHECK (date(arrival_date) = arrival_date),
    currency            TEXT    NOT NULL,
    net_cents           INTEGER NOT NULL,
    gross_cents         INTEGER NOT NULL,
    fee_cents           INTEGER NOT NULL,
    refund_cents        INTEGER NOT NULL,
    chargeback_cents    INTEGER NOT NULL,
    other_cents         INTEGER NOT NULL,
    details_json        TEXT    NOT NULL CHECK (json_valid(details_json)),
    status              TEXT    NOT NULL DEFAULT 'NEW' CHECK (status IN ('NEW', 'RECORDED', 'IGNORED')),
    deposit_id          INTEGER,
    fetched_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE (feed_id, processor_payout_id),
    CHECK ((status = 'RECORDED') = (deposit_id IS NOT NULL))
) STRICT;
