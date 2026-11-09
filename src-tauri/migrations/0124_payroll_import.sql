-- R16: payroll imports. A payroll service's journal export is posted as ordinary journal entries
-- (`payroll_import`); these tables remember what was imported and how its account names matched.

-- One posted import. The file's SHA-256 stops the same file being posted twice while any of its
-- entries is still in force (voiding them all allows it again, so it is not UNIQUE).
CREATE TABLE payroll_imports (
    id           INTEGER PRIMARY KEY,
    file_name    TEXT    NOT NULL CHECK (length(file_name) BETWEEN 1 AND 500),
    file_sha256  TEXT    NOT NULL CHECK (length(file_sha256) = 64),
    imported_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    entry_count  INTEGER NOT NULL CHECK (entry_count > 0),
    total_cents  INTEGER NOT NULL CHECK (total_cents >= 0)
) STRICT;
CREATE INDEX idx_payroll_imports_sha ON payroll_imports (file_sha256);

-- The journal entries an import posted, with the export's entry key (payroll id or date).
CREATE TABLE payroll_import_entries (
    import_id        INTEGER NOT NULL REFERENCES payroll_imports (id),
    journal_entry_id INTEGER NOT NULL UNIQUE REFERENCES journal_entries (id),
    entry_key        TEXT    NOT NULL,
    PRIMARY KEY (import_id, journal_entry_id)
) STRICT;

-- The LocalBooks account each account name in a payroll export went to last time, keyed by the
-- name in lower case with only letters and digits.
CREATE TABLE payroll_account_map (
    source_account TEXT    PRIMARY KEY CHECK (length(source_account) BETWEEN 1 AND 200),
    account_id     INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE CASCADE
) STRICT;
