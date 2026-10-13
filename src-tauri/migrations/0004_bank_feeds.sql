-- 0004_bank_feeds.sql
-- Imported bank/credit-card CSV lines awaiting review, and the categorization rules used to suggest
-- an account for each line. Sign convention: amount_cents < 0 is money out, > 0 is money in.
-- Approving a line posts: outflow -> Dr category / Cr bank; inflow -> Dr bank / Cr category.

CREATE TABLE bank_rules (
    id              INTEGER PRIMARY KEY,
    pattern         TEXT    NOT NULL CHECK (length(pattern) > 0),
    direction       TEXT    NOT NULL DEFAULT 'ANY' CHECK (direction IN ('ANY', 'OUTFLOW', 'INFLOW')),
    account_id      INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE CASCADE,
    origin          TEXT    NOT NULL CHECK (origin IN ('BUILT_IN', 'LEARNED')),
    priority        INTEGER NOT NULL DEFAULT 0,
    hit_count       INTEGER NOT NULL DEFAULT 0 CHECK (hit_count >= 0),
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE (pattern, direction)
) STRICT;

CREATE TABLE bank_transactions (
    id                      INTEGER PRIMARY KEY,
    bank_account_id         INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    txn_date                TEXT    NOT NULL CHECK (
                                txn_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'
                                AND date(txn_date) = txn_date
                            ),
    description             TEXT    NOT NULL,
    amount_cents            INTEGER NOT NULL CHECK (amount_cents <> 0 AND abs(amount_cents) <= 100000000000000),
    fingerprint             TEXT    NOT NULL,
    status                  TEXT    NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'APPROVED', 'EXCLUDED')),
    category_account_id     INTEGER REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    journal_entry_id        INTEGER UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    imported_at             TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    reviewed_at             TEXT,
    CHECK ((status = 'APPROVED') = (journal_entry_id IS NOT NULL)),
    CHECK ((status = 'APPROVED') = (category_account_id IS NOT NULL)),
    CHECK ((status = 'PENDING') = (reviewed_at IS NULL)),
    CHECK (category_account_id IS NULL OR category_account_id <> bank_account_id),
    UNIQUE (bank_account_id, fingerprint)
) STRICT;

CREATE INDEX ix_bank_txn_status ON bank_transactions (status, txn_date);

CREATE TRIGGER trg_bank_txn_account_type
BEFORE INSERT ON bank_transactions
WHEN (SELECT account_type FROM chart_of_accounts WHERE id = NEW.bank_account_id) NOT IN ('BANK', 'CREDIT_CARD')
BEGIN
    SELECT RAISE(ABORT, 'BANK_ACCOUNT_TYPE: bank feed lines must belong to a Bank or Credit Card account');
END;

CREATE TRIGGER trg_bank_txn_reviewed_immutable
BEFORE UPDATE ON bank_transactions
WHEN OLD.status = 'APPROVED'
BEGIN
    SELECT RAISE(ABORT, 'BANK_TXN_IMMUTABLE: approved bank lines cannot be changed; void the journal entry instead');
END;

-- Built-in vendor heuristics. Learned rules (created on approval) use priority >= 1000 and win ties.
INSERT INTO bank_rules (pattern, direction, account_id, origin, priority)
SELECT r.pattern, r.direction, a.id, 'BUILT_IN', r.priority
FROM (
    SELECT '\b(UBER EATS|DOORDASH|GRUBHUB|STARBUCKS|DUNKIN|MCDONALD''?S|CHIPOTLE|PANERA|RESTAURANT|CAFE|COFFEE)\b' AS pattern,
           'OUTFLOW' AS direction, '6200' AS number, 60 AS priority
    UNION ALL SELECT '\b(AWS|AMAZON WEB SERVICES|GOOGLE CLOUD|GCP|AZURE|DIGITALOCEAN|HEROKU|GITHUB|VERCEL|NETLIFY|ATLASSIAN|SLACK|ZOOM|DROPBOX|ADOBE|MICROSOFT|OPENAI|ANTHROPIC|NOTION|FIGMA)\b',
           'OUTFLOW', '6350', 50
    UNION ALL SELECT '\b(EXXON|EXXONMOBIL|MOBIL|SHELL OIL|SHELL|CHEVRON|BP|TEXACO|SUNOCO|CITGO|MARATHON|VALERO|ARCO|WAWA|SPEEDWAY|CIRCLE K|JIFFY LUBE)\b',
           'OUTFLOW', '6100', 50
    UNION ALL SELECT '\b(UBER|LYFT|DELTA AIR|UNITED AIRLINES|AMERICAN AIRLINES|SOUTHWEST AIR|JETBLUE|MARRIOTT|HILTON|HYATT|AIRBNB|EXPEDIA|AMTRAK)\b',
           'OUTFLOW', '6450', 40
    UNION ALL SELECT '\b(STAPLES|OFFICE DEPOT|OFFICEMAX)\b', 'OUTFLOW', '6250', 50
    UNION ALL SELECT '\b(USPS|UPS|FEDEX|DHL|STAMPS\.COM|PIRATE ?SHIP)\b', 'OUTFLOW', '6600', 50
    UNION ALL SELECT '\b(COMCAST|XFINITY|VERIZON|AT&T|T-MOBILE|SPECTRUM|COX COMM)\b', 'OUTFLOW', '6550', 50
    UNION ALL SELECT '\b(PG&E|CON ?ED|DUKE ENERGY|ELECTRIC CO|WATER DEPT|WATER UTIL|NATIONAL GRID)\b', 'OUTFLOW', '6500', 50
    UNION ALL SELECT '\b(GEICO|STATE FARM|ALLSTATE|PROGRESSIVE|NATIONWIDE|HISCOX|INSURANCE)\b', 'OUTFLOW', '6150', 50
    UNION ALL SELECT '\b(GOOGLE ADS|GOOGLE \*ADS|FACEBOOK ADS|FB ADS|META ADS|LINKEDIN ADS|MAILCHIMP)\b', 'OUTFLOW', '6000', 70
    UNION ALL SELECT '\b(SERVICE CHARGE|MONTHLY FEE|MAINTENANCE FEE|OVERDRAFT|WIRE FEE|ATM FEE|NSF FEE)\b', 'OUTFLOW', '6050', 80
    UNION ALL SELECT '\b(RENT|LEASE PMT|REGUS|WEWORK)\b', 'OUTFLOW', '6300', 30
    UNION ALL SELECT '\b(UPWORK|FIVERR|TOPTAL|GUSTO CONTRACTOR)\b', 'OUTFLOW', '6400', 50
    UNION ALL SELECT '\b(INTEREST CHARGE|LOAN INTEREST)\b', 'OUTFLOW', '8000', 70
    UNION ALL SELECT '\b(INTEREST PAID|INTEREST EARNED|INTEREST CREDIT|INT EARNED)\b', 'INFLOW', '7000', 70
    UNION ALL SELECT '\b(STRIPE|SQUARE|SHOPIFY|PAYPAL TRANSFER)\b', 'INFLOW', '4000', 50
) r
JOIN chart_of_accounts a ON a.account_number = r.number;
