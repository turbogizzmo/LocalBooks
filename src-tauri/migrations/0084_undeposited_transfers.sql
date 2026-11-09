-- 0084_undeposited_transfers.sql
-- Payments moved out of Undeposited Funds into an account that isn't a bank (gap T66).
--
-- A deposit goes into a Bank or Credit Card account (0008). A company file can also move received
-- payments out of Undeposited Funds into another account, for example writing off receipts against
-- an expense. The full import posts such a transaction as a transfer out of Undeposited Funds (its
-- own journal entry, the file's lines) and records here which payments and sales receipts it
-- cleared, so they no longer wait to be deposited.
--
-- Rows are append-only: a transfer is undone by voiding its journal entry, which marks its rows
-- voided and puts the payments back in Undeposited Funds.

CREATE TABLE undeposited_transfers (
    id                  INTEGER PRIMARY KEY,
    journal_entry_id    INTEGER NOT NULL REFERENCES journal_entries (id) ON DELETE RESTRICT,
    invoice_payment_id  INTEGER REFERENCES invoice_payments (id) ON DELETE RESTRICT,
    sales_receipt_id    INTEGER REFERENCES sales_receipts (id) ON DELETE RESTRICT,
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    is_voided           INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK ((invoice_payment_id IS NULL) <> (sales_receipt_id IS NULL))
) STRICT;
CREATE UNIQUE INDEX ux_undeposited_transfers_payment ON undeposited_transfers (invoice_payment_id)
    WHERE invoice_payment_id IS NOT NULL AND is_voided = 0;
CREATE UNIQUE INDEX ux_undeposited_transfers_receipt ON undeposited_transfers (sales_receipt_id)
    WHERE sales_receipt_id IS NOT NULL AND is_voided = 0;
CREATE INDEX ix_undeposited_transfers_entry ON undeposited_transfers (journal_entry_id);

CREATE TRIGGER trg_uf_transfer_insert
BEFORE INSERT ON undeposited_transfers
BEGIN
    SELECT RAISE(ABORT, 'UFT_PREVOIDED: a transfer is recorded live')
    WHERE NEW.is_voided <> 0;
    SELECT RAISE(ABORT, 'UFT_JOURNAL_MISSING: a transfer out of Undeposited Funds must reference its POSTED journal entry')
    WHERE NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = NEW.journal_entry_id AND status = 'POSTED' AND is_voided = 0);
    SELECT RAISE(ABORT, 'UFT_NOT_WAITING: that payment is not waiting in Undeposited Funds for that amount')
    WHERE (NEW.invoice_payment_id IS NOT NULL AND NOT EXISTS (
              SELECT 1 FROM invoice_payments p JOIN chart_of_accounts a ON a.id = p.deposit_account_id
              WHERE p.id = NEW.invoice_payment_id AND p.is_voided = 0 AND p.journal_entry_id IS NOT NULL
                AND a.is_system = 1 AND a.account_type = 'OTHER_CURRENT_ASSET'
                AND COALESCE(p.home_amount_cents, p.amount_cents - p.discount_cents) = NEW.amount_cents))
       OR (NEW.sales_receipt_id IS NOT NULL AND NOT EXISTS (
              SELECT 1 FROM sales_receipts r JOIN chart_of_accounts a ON a.id = r.deposit_account_id
              WHERE r.id = NEW.sales_receipt_id AND r.is_voided = 0 AND r.journal_entry_id IS NOT NULL
                AND a.is_system = 1 AND a.account_type = 'OTHER_CURRENT_ASSET' AND r.total_cents = NEW.amount_cents));
    SELECT RAISE(ABORT, 'UF_ALREADY_DEPOSITED: that payment is already in a deposit')
    WHERE EXISTS (SELECT 1 FROM deposit_lines WHERE is_voided = 0 AND (invoice_payment_id = NEW.invoice_payment_id OR sales_receipt_id = NEW.sales_receipt_id))
       OR EXISTS (SELECT 1 FROM undeposited_fund_matches WHERE is_voided = 0 AND (invoice_payment_id = NEW.invoice_payment_id OR sales_receipt_id = NEW.sales_receipt_id));
END;

-- A row only ever goes from live to voided, and only with its entry.
CREATE TRIGGER trg_uf_transfer_update
BEFORE UPDATE ON undeposited_transfers
BEGIN
    SELECT RAISE(ABORT, 'UFT_IMMUTABLE: a transfer out of Undeposited Funds can only be undone by voiding its entry')
    WHERE NEW.journal_entry_id IS NOT OLD.journal_entry_id OR NEW.invoice_payment_id IS NOT OLD.invoice_payment_id
       OR NEW.sales_receipt_id IS NOT OLD.sales_receipt_id OR NEW.amount_cents IS NOT OLD.amount_cents
       OR NEW.created_at IS NOT OLD.created_at OR NEW.is_voided < OLD.is_voided
       OR (NEW.is_voided = 1 AND OLD.is_voided = 0
           AND NOT EXISTS (SELECT 1 FROM journal_entries WHERE id = OLD.journal_entry_id AND is_voided = 1));
END;

CREATE TRIGGER trg_uf_transfer_no_delete BEFORE DELETE ON undeposited_transfers
BEGIN
    SELECT RAISE(ABORT, 'UFT_IMMUTABLE: a transfer out of Undeposited Funds can only be undone by voiding its entry');
END;

-- Voiding the transfer's entry puts its payments back in Undeposited Funds.
CREATE TRIGGER trg_uf_transfer_entry_voided
AFTER UPDATE OF is_voided ON journal_entries
WHEN OLD.is_voided = 0 AND NEW.is_voided = 1
BEGIN
    UPDATE undeposited_transfers SET is_voided = 1 WHERE journal_entry_id = NEW.id AND is_voided = 0;
END;

-- A transferred payment counts as gone from Undeposited Funds: it can't go into a deposit or be
-- matched, and it can't be voided while the transfer stands.
CREATE TRIGGER trg_uf_transferred_not_deposited
BEFORE INSERT ON deposit_lines
WHEN NEW.invoice_payment_id IS NOT NULL OR NEW.sales_receipt_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'DEP_ALREADY_DEPOSITED: that payment was moved out of Undeposited Funds by a transfer')
    WHERE EXISTS (SELECT 1 FROM undeposited_transfers WHERE is_voided = 0
                  AND (invoice_payment_id = NEW.invoice_payment_id OR sales_receipt_id = NEW.sales_receipt_id));
END;

CREATE TRIGGER trg_uf_transferred_not_matched
BEFORE INSERT ON undeposited_fund_matches
BEGIN
    SELECT RAISE(ABORT, 'UF_ALREADY_DEPOSITED: that payment was moved out of Undeposited Funds by a transfer')
    WHERE EXISTS (SELECT 1 FROM undeposited_transfers WHERE is_voided = 0
                  AND (invoice_payment_id = NEW.invoice_payment_id OR sales_receipt_id = NEW.sales_receipt_id));
END;

CREATE TRIGGER trg_uf_transferred_payment_void
BEFORE UPDATE OF is_voided ON invoice_payments
WHEN NEW.is_voided = 1 AND OLD.is_voided = 0
BEGIN
    SELECT RAISE(ABORT, 'VOID_DEPOSITED: this payment was moved out of Undeposited Funds by a transfer; void that first')
    WHERE EXISTS (SELECT 1 FROM undeposited_transfers WHERE invoice_payment_id = NEW.id AND is_voided = 0);
END;

CREATE TRIGGER trg_uf_transferred_receipt_void
BEFORE UPDATE OF is_voided ON sales_receipts
WHEN NEW.is_voided = 1 AND OLD.is_voided = 0
BEGIN
    SELECT RAISE(ABORT, 'VOID_DEPOSITED: this sales receipt was moved out of Undeposited Funds by a transfer; void that first')
    WHERE EXISTS (SELECT 1 FROM undeposited_transfers WHERE sales_receipt_id = NEW.id AND is_voided = 0);
END;
