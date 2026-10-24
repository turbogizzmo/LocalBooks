-- Database backstop for the foreign-currency side of an entry (issue #49).
--
-- The home-currency balance has been re-checked by trg_je_post_must_balance since 0001; the foreign
-- amounts were only checked by the Rust engine. A foreign entry now cannot be posted unless every line
-- carries a foreign amount on the same side as its home amount and the foreign sides balance.

CREATE TRIGGER trg_je_post_foreign_balance
BEFORE UPDATE OF status ON journal_entries
WHEN OLD.status = 'DRAFT' AND NEW.status = 'POSTED' AND NEW.foreign_currency_code IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'JE_FOREIGN_MISSING: every line of a foreign-currency entry needs its foreign amount')
    WHERE EXISTS (
        SELECT 1 FROM journal_lines
        WHERE journal_entry_id = NEW.id
          AND NOT (    (debit_cents  > 0 AND COALESCE(foreign_debit_cents, 0)  > 0 AND COALESCE(foreign_credit_cents, 0) = 0)
                    OR (credit_cents > 0 AND COALESCE(foreign_credit_cents, 0) > 0 AND COALESCE(foreign_debit_cents, 0)  = 0))
    );
    SELECT RAISE(ABORT, 'JE_FOREIGN_UNBALANCED: foreign-currency debits must equal foreign-currency credits')
    WHERE (SELECT SUM(COALESCE(foreign_debit_cents, 0)) - SUM(COALESCE(foreign_credit_cents, 0))
           FROM journal_lines WHERE journal_entry_id = NEW.id) <> 0;
END;

-- A home-currency entry carries no foreign amounts.
CREATE TRIGGER trg_je_post_home_no_foreign
BEFORE UPDATE OF status ON journal_entries
WHEN OLD.status = 'DRAFT' AND NEW.status = 'POSTED' AND NEW.foreign_currency_code IS NULL
BEGIN
    SELECT RAISE(ABORT, 'JE_FOREIGN_ON_HOME: a home-currency entry cannot carry foreign amounts')
    WHERE EXISTS (
        SELECT 1 FROM journal_lines
        WHERE journal_entry_id = NEW.id AND (foreign_debit_cents IS NOT NULL OR foreign_credit_cents IS NOT NULL)
    );
END;
