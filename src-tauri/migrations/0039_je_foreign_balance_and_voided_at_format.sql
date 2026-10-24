-- Enforce foreign-side balancing at post time and strict UTC timestamps for voiding.

CREATE TRIGGER trg_je_post_must_balance_foreign
BEFORE UPDATE OF status ON journal_entries
WHEN OLD.status = 'DRAFT'
 AND NEW.status = 'POSTED'
 AND NEW.foreign_currency_code IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'JE_FOREIGN_UNBALANCED: total foreign debits must equal total foreign credits')
    WHERE (
        SELECT COALESCE(SUM(foreign_debit_cents), 0) - COALESCE(SUM(foreign_credit_cents), 0)
        FROM journal_lines
        WHERE journal_entry_id = NEW.id
    ) <> 0;
END;

CREATE TRIGGER trg_je_voided_at_must_be_iso8601_utc
BEFORE UPDATE OF status, voided_at ON journal_entries
WHEN NEW.status = 'VOID'
 AND (
      NEW.voided_at IS NULL
   OR NEW.voided_at NOT GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]T[0-9][0-9]:[0-9][0-9]:[0-9][0-9].[0-9][0-9][0-9]Z'
   OR strftime('%Y-%m-%dT%H:%M:%fZ', NEW.voided_at) IS NULL
   OR strftime('%Y-%m-%dT%H:%M:%fZ', NEW.voided_at) <> NEW.voided_at
 )
BEGIN
    SELECT RAISE(ABORT, 'JE_VOIDED_AT_INVALID: voided_at must be an ISO-8601 UTC timestamp (YYYY-MM-DDTHH:MM:SS.SSSZ)');
END;
