-- R14: Accountant's Copy (`accountants_copy.rs`, design in docs/plans/accountants-copy.md).

-- In the client's books: each copy made for the accountant, and the audit record of bringing its
-- changes back. One may be out at a time; while it is, nothing posts on or before its dividing date.
CREATE TABLE accountants_copies (
    id                  INTEGER PRIMARY KEY,
    token               TEXT    NOT NULL UNIQUE CHECK (length(token) = 32),
    dividing_date       TEXT    NOT NULL CHECK (date(dividing_date) = dividing_date),
    file_name           TEXT    NOT NULL,
    status              TEXT    NOT NULL DEFAULT 'OUTSTANDING' CHECK (status IN ('OUTSTANDING', 'RETURNED', 'CANCELLED')),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    created_by          TEXT    NOT NULL,
    closed_at           TEXT,
    closed_by           TEXT,
    entries_posted      INTEGER NOT NULL DEFAULT 0,
    accounts_created    INTEGER NOT NULL DEFAULT 0,
    changes_sha256      TEXT,
    import_summary_json TEXT    CHECK (import_summary_json IS NULL OR json_valid(import_summary_json))
) STRICT;
CREATE UNIQUE INDEX ux_accountants_copies_one_out ON accountants_copies (status) WHERE status = 'OUTSTANDING';

-- In the copy itself (one row): which copy it is, its dividing date, and where the books stood when
-- it was made, so the changes since can be told apart.
CREATE TABLE accountants_copy_info (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    token           TEXT    NOT NULL CHECK (length(token) = 32),
    dividing_date   TEXT    NOT NULL CHECK (date(dividing_date) = dividing_date),
    base_entry_id   INTEGER NOT NULL,
    base_account_id INTEGER NOT NULL,
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
) STRICT;
