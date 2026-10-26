-- Credit card credits: a refund or credit from a vendor that posts to a credit card account.
--
-- A card credit is the reverse of a card charge, so it lives in `expenses` as an EXPENSE with
-- `is_credit = 1` and posts with the same journal source type ('EXPENSE'): Dr the card (its
-- balance goes down) / Cr each split account. Its split lines and total stay positive, like a
-- charge's; the direction is what `is_credit` says. A credit is only ever on a credit card
-- account, and the posted entry must run the right way for the flag.

ALTER TABLE expenses ADD COLUMN is_credit INTEGER NOT NULL DEFAULT 0 CHECK (is_credit IN (0, 1));

CREATE TRIGGER trg_exp_credit_insert
BEFORE INSERT ON expenses
WHEN NEW.is_credit = 1
BEGIN
    SELECT RAISE(ABORT, 'EXP_BAD_PAYMENT_ACCOUNT: a card credit goes to a Credit Card account')
    WHERE NEW.kind IS NOT 'EXPENSE'
       OR (SELECT account_type FROM chart_of_accounts WHERE id = NEW.payment_account_id) IS NOT 'CREDIT_CARD';
END;

CREATE TRIGGER trg_exp_credit_immutable
BEFORE UPDATE OF is_credit ON expenses
WHEN NEW.is_credit IS NOT OLD.is_credit
BEGIN
    SELECT RAISE(ABORT, 'EXP_IMMUTABLE: an expense cannot become a card credit, or a card credit an expense');
END;

-- When the journal entry is attached: an expense or check credits the account it was paid from
-- and debits nothing there; a card credit debits the card and credits nothing there.
CREATE TRIGGER trg_exp_posting_direction
BEFORE UPDATE OF journal_entry_id ON expenses
WHEN OLD.journal_entry_id IS NULL AND NEW.journal_entry_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'EXP_WRONG_DIRECTION: a card credit must debit the card account and credit its split accounts')
    WHERE NEW.is_credit = 1
      AND (   EXISTS (SELECT 1 FROM journal_lines WHERE journal_entry_id = NEW.journal_entry_id
                      AND account_id = NEW.payment_account_id AND credit_cents > 0)
           OR NOT EXISTS (SELECT 1 FROM journal_lines WHERE journal_entry_id = NEW.journal_entry_id
                          AND account_id = NEW.payment_account_id AND debit_cents > 0));
    SELECT RAISE(ABORT, 'EXP_WRONG_DIRECTION: an expense must credit the account it was paid from')
    WHERE NEW.is_credit = 0
      AND (   EXISTS (SELECT 1 FROM journal_lines WHERE journal_entry_id = NEW.journal_entry_id
                      AND account_id = NEW.payment_account_id AND debit_cents > 0)
           OR NOT EXISTS (SELECT 1 FROM journal_lines WHERE journal_entry_id = NEW.journal_entry_id
                          AND account_id = NEW.payment_account_id AND credit_cents > 0));
END;
