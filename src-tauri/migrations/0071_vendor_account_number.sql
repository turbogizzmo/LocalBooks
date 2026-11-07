-- The account number a vendor gave you (your account with them), kept on the vendor. With
-- Preferences › Checking › "Autofill payee account number in check memo", a new check to the
-- vendor starts with it as the memo.
ALTER TABLE vendors ADD COLUMN account_number TEXT CHECK (account_number IS NULL OR length(account_number) BETWEEN 1 AND 40);
