-- An estimate can now be loaded into the invoice form and edited before the invoice posts (hours
-- adjusted, a line added), so the invoice's amount may differ from the estimate's. It must still be
-- a posted invoice for the same customer. Every other rule is unchanged.
DROP TRIGGER trg_est_status_rules;
CREATE TRIGGER trg_est_status_rules
BEFORE UPDATE ON estimates
BEGIN
    SELECT RAISE(ABORT, 'EST_CLOSED: a converted estimate cannot be changed')
    WHERE OLD.status = 'CLOSED';
    SELECT RAISE(ABORT, 'EST_IMMUTABLE: only the status of an estimate can change after it is saved')
    WHERE NEW.estimate_number IS NOT OLD.estimate_number OR NEW.customer_id IS NOT OLD.customer_id
       OR NEW.estimate_date IS NOT OLD.estimate_date OR NEW.expiration_date IS NOT OLD.expiration_date
       OR NEW.total_cents IS NOT OLD.total_cents OR NEW.memo IS NOT OLD.memo OR NEW.created_at IS NOT OLD.created_at;
    SELECT RAISE(ABORT, 'EST_INVOICE_MISMATCH: an estimate closes only into a posted invoice for the same customer')
    WHERE NEW.status = 'CLOSED' AND NOT EXISTS (
        SELECT 1 FROM invoices
        WHERE id = NEW.invoice_id AND status <> 'DRAFT' AND customer_id = NEW.customer_id
    );
END;
