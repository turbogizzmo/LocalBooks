-- Saved column layouts for bank CSV files. When a file's headers are not recognized automatically,
-- the user maps them once; files with the same headers (the same bank's export) then import directly.
-- Columns are stored by header text; `header_signature` is the file's lower-cased headers joined by |.
CREATE TABLE bank_format_templates (
    id                  INTEGER PRIMARY KEY,
    header_signature    TEXT    NOT NULL UNIQUE,
    date_column         TEXT    NOT NULL,
    description_column  TEXT    NOT NULL,
    amount_column       TEXT,
    money_in_column     TEXT,
    money_out_column    TEXT,
    date_order          TEXT    NOT NULL DEFAULT 'MDY' CHECK (date_order IN ('MDY', 'DMY')),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_used_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (
        (amount_column IS NOT NULL AND money_in_column IS NULL AND money_out_column IS NULL)
        OR (amount_column IS NULL AND money_in_column IS NOT NULL AND money_out_column IS NOT NULL
            AND money_in_column <> money_out_column)
    )
) STRICT;
