-- Audit trail for the customer fields added in 0065 (shipping address, contacts, phone, fax,
-- website, account number, credit limit, resale number).
--
-- The customer audit trigger from 0063 (rebuilt in 0074) covers the older fields; this one records
-- the new ones, so neither has to be rewritten when the other list grows. It is created here, after
-- 0074 rebuilds ledger_audit_log, because a trigger that writes to that table cannot exist while
-- the table is dropped and renamed. A later migration that rebuilds ledger_audit_log again must
-- drop and recreate this trigger as it does the others.

CREATE TRIGGER trg_ledger_audit_customer_contact
AFTER UPDATE ON customers
WHEN NEW.shipping_address IS NOT OLD.shipping_address OR NEW.contact_name IS NOT OLD.contact_name
  OR NEW.alt_contact_name IS NOT OLD.alt_contact_name OR NEW.alt_phone IS NOT OLD.alt_phone
  OR NEW.fax IS NOT OLD.fax OR NEW.website IS NOT OLD.website OR NEW.account_number IS NOT OLD.account_number
  OR NEW.credit_limit_cents IS NOT OLD.credit_limit_cents OR NEW.resale_number IS NOT OLD.resale_number
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'CUSTOMER',
        'before', json_object('name', OLD.name, 'creditLimitCents', OLD.credit_limit_cents),
        'after',  json_object('name', NEW.name, 'creditLimitCents', NEW.credit_limit_cents),
        'changed', (SELECT json_group_array(f) FROM (
            SELECT 'shipping address' AS f WHERE NEW.shipping_address IS NOT OLD.shipping_address
            UNION ALL SELECT 'contact' WHERE NEW.contact_name IS NOT OLD.contact_name
            UNION ALL SELECT 'alternate contact' WHERE NEW.alt_contact_name IS NOT OLD.alt_contact_name
            UNION ALL SELECT 'alternate phone' WHERE NEW.alt_phone IS NOT OLD.alt_phone
            UNION ALL SELECT 'fax' WHERE NEW.fax IS NOT OLD.fax
            UNION ALL SELECT 'website' WHERE NEW.website IS NOT OLD.website
            UNION ALL SELECT 'account number' WHERE NEW.account_number IS NOT OLD.account_number
            UNION ALL SELECT 'credit limit' WHERE NEW.credit_limit_cents IS NOT OLD.credit_limit_cents
            UNION ALL SELECT 'resale number' WHERE NEW.resale_number IS NOT OLD.resale_number))
    ));
END;
