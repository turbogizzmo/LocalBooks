-- Daily exchange rates looked up online (see `fx_rates.rs`), cached so a date is fetched only once
-- and suggestions keep working offline.
--
-- `rate_e6` is how many `quote` units one `base` unit buys, scaled by 1,000,000 like every stored
-- exchange rate. `rate_date` is the day the rate was published. A request for a weekend or holiday
-- is answered with the most recent business day before it, so `covers_through` records the latest
-- requested date this rate answers: a lookup for any date from `rate_date` to `covers_through` is
-- served from this row. The cache only suggests rates; saved transactions keep the rate they were
-- saved with and never read this table.

CREATE TABLE exchange_rate_cache (
    base            TEXT    NOT NULL CHECK (length(base) = 3 AND base = upper(base)),
    quote           TEXT    NOT NULL CHECK (length(quote) = 3 AND quote = upper(quote)),
    rate_date       TEXT    NOT NULL CHECK (date(rate_date) = rate_date),
    covers_through  TEXT    NOT NULL CHECK (date(covers_through) = covers_through),
    rate_e6         INTEGER NOT NULL CHECK (rate_e6 > 0),
    source          TEXT    NOT NULL,
    fetched_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (base, quote, rate_date),
    CHECK (base <> quote),
    CHECK (covers_through >= rate_date)
) STRICT;

-- Off until the user turns it on in Settings › Preferences Console › Accounting & tax.
INSERT INTO system_preferences (key, value) VALUES ('exchange_rate_lookup', 'false');
