-- 0009_preferences.sql
-- Company-wide preferences as key/value pairs. Values are validated by the preferences module;
-- the triggers below guard the keys the ledger itself depends on.

CREATE TABLE system_preferences (
    key         TEXT PRIMARY KEY CHECK (length(key) > 0),
    value       TEXT NOT NULL,
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

INSERT INTO system_preferences (key, value) VALUES
    ('company_name',        'LocalBooks Ledger'),
    ('company_legal_name',  ''),
    ('company_ein',         ''),
    ('company_address',     ''),
    ('company_email',       ''),
    ('company_phone',       ''),
    ('fiscal_year_start',   '01-01'),
    ('currency_symbol',     '$'),
    ('tax_form',            'SCHEDULE_C'),
    ('auto_backup_enabled', 'true'),
    ('auto_backup_keep',    '7');

-- Fiscal years begin on the first day of a month ("MM-01"), as in QuickBooks.
CREATE TRIGGER trg_pref_fiscal_year_start
BEFORE UPDATE OF value ON system_preferences
WHEN NEW.key = 'fiscal_year_start'
 AND NOT (NEW.value GLOB '[01][0-9]-01' AND CAST(substr(NEW.value, 1, 2) AS INTEGER) BETWEEN 1 AND 12)
BEGIN
    SELECT RAISE(ABORT, 'PREF_BAD_FISCAL_START: fiscal year must start on the first day of a month (MM-01)');
END;

CREATE TRIGGER trg_pref_no_delete
BEFORE DELETE ON system_preferences
BEGIN
    SELECT RAISE(ABORT, 'PREF_REQUIRED: preferences can be changed but not deleted');
END;
