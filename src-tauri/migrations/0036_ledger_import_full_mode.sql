-- Full imports: a desktop company file's sales documents become LocalBooks invoices, credit memos,
-- sales receipts, payments and deposits instead of journal entries. The batch records which way it
-- was posted; each staged transaction keeps the document read from the file (JSON, see
-- `qbw_documents::SourceDocument`), what it was posted as, and why when it had to fall back to a
-- journal entry.
ALTER TABLE ledger_import_batches ADD COLUMN mode TEXT NOT NULL DEFAULT 'JOURNAL' CHECK (mode IN ('JOURNAL', 'FULL'));
ALTER TABLE ledger_import_transactions ADD COLUMN document TEXT;
ALTER TABLE ledger_import_transactions ADD COLUMN posted_as TEXT;
ALTER TABLE ledger_import_transactions ADD COLUMN fallback_reason TEXT;

-- QuickBooks lets a payment be dated before the invoice it pays (a prepayment applied later), and
-- a full import keeps the file's dates. Payments entered in LocalBooks still cannot be: Receive
-- Payment checks it (INV_PAYMENT_BEFORE_ISSUE in `invoices::write_payment`). The other rules
-- are unchanged.
DROP TRIGGER trg_pay_insert_checks;
CREATE TRIGGER trg_pay_insert_checks
BEFORE INSERT ON invoice_payments
BEGIN
    SELECT RAISE(ABORT, 'INV_NOT_OPEN: payments can only be applied to open invoices')
    WHERE (SELECT status FROM invoices WHERE id = NEW.invoice_id) NOT IN ('UNPAID', 'PARTIAL')
       OR (SELECT is_voided FROM invoices WHERE id = NEW.invoice_id) = 1;
    SELECT RAISE(ABORT, 'INV_OVERPAYMENT: payment exceeds the open balance')
    WHERE NEW.amount_cents > (SELECT total_cents - amount_paid_cents FROM invoices WHERE id = NEW.invoice_id);
    SELECT RAISE(ABORT, 'INV_BAD_DEPOSIT_ACCOUNT: deposit to a Bank or Undeposited Funds account')
    WHERE (SELECT account_type FROM chart_of_accounts WHERE id = NEW.deposit_account_id) NOT IN ('BANK', 'OTHER_CURRENT_ASSET');
    SELECT RAISE(ABORT, 'INV_PAYMENT_PREPOSTED: insert the payment first, then attach its journal entry')
    WHERE NEW.journal_entry_id IS NOT NULL OR NEW.is_voided <> 0;
END;
