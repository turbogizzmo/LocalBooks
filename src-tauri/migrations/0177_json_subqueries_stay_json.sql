-- Keep JSON built from a subquery as JSON on SQLite 3.54 (gap R24).
--
-- SQLite 3.54 no longer carries the JSON subtype out of a scalar subquery, so
-- json_object('lines', (SELECT json_group_array(...))) stored the array as a quoted string. The
-- ledger snapshot view and the list-change audit triggers below nest such subqueries; each is
-- recreated unchanged except that the subquery is wrapped in json(...), which gives back exactly
-- the text earlier SQLite versions wrote, so stored snapshots still verify and hash the same.
-- trg_ledger_audit_post reads v_entry_snapshot by name and needs no change.

DROP VIEW v_entry_snapshot;
CREATE VIEW v_entry_snapshot AS
SELECT
    e.id AS entry_id,
    json_object(
        'entryId',             e.id,
        'entryNumber',         e.entry_number,
        'entryDate',           e.entry_date,
        'memo',                e.memo,
        'sourceType',          e.source_type,
        'sourceId',            e.source_id,
        'reversesEntryId',     e.reverses_entry_id,
        'isAdjusting',         e.is_adjusting,
        'postedAt',            e.posted_at,
        'foreignCurrencyCode', e.foreign_currency_code,
        'exchangeRateE6',      e.exchange_rate_e6,
        'totalDebitCents',     (SELECT COALESCE(SUM(debit_cents), 0)  FROM journal_lines WHERE journal_entry_id = e.id),
        'totalCreditCents',    (SELECT COALESCE(SUM(credit_cents), 0) FROM journal_lines WHERE journal_entry_id = e.id),
        'lines', json((
            SELECT json_group_array(
                       CASE WHEN l.class_id IS NULL
                           THEN json_object(
                               'lineNumber',         l.line_number,
                               'accountId',          l.account_id,
                               'debitCents',         l.debit_cents,
                               'creditCents',        l.credit_cents,
                               'memo',               l.memo,
                               'foreignDebitCents',  l.foreign_debit_cents,
                               'foreignCreditCents', l.foreign_credit_cents
                           )
                           ELSE json_set(json_object(
                               'lineNumber',         l.line_number,
                               'accountId',          l.account_id,
                               'debitCents',         l.debit_cents,
                               'creditCents',        l.credit_cents,
                               'memo',               l.memo,
                               'foreignDebitCents',  l.foreign_debit_cents,
                               'foreignCreditCents', l.foreign_credit_cents
                           ), '$.classId', l.class_id)
                       END ORDER BY l.line_number)
            FROM journal_lines l WHERE l.journal_entry_id = e.id
        ))
    ) AS snapshot
FROM journal_entries e;

DROP TRIGGER trg_ledger_audit_customer;
CREATE TRIGGER trg_ledger_audit_customer
AFTER UPDATE ON customers
WHEN NEW.name IS NOT OLD.name OR NEW.is_active IS NOT OLD.is_active OR NEW.email IS NOT OLD.email
  OR NEW.company_name IS NOT OLD.company_name OR NEW.phone IS NOT OLD.phone OR NEW.billing_address IS NOT OLD.billing_address
  OR NEW.terms_days IS NOT OLD.terms_days OR NEW.notes IS NOT OLD.notes OR NEW.parent_id IS NOT OLD.parent_id
  OR NEW.tax_rate_id IS NOT OLD.tax_rate_id OR NEW.is_tax_exempt IS NOT OLD.is_tax_exempt
  OR NEW.sales_rep_id IS NOT OLD.sales_rep_id
  OR (NEW.terms_id IS NOT OLD.terms_id AND OLD.terms_id IS NOT NULL)
  OR NEW.customer_type_id IS NOT OLD.customer_type_id OR NEW.job_type_id IS NOT OLD.job_type_id
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'CUSTOMER',
        'before', json_object('name', OLD.name, 'isActive', OLD.is_active),
        'after',  json_object('name', NEW.name, 'isActive', NEW.is_active),
        'changed', json((SELECT json_group_array(f) FROM (
            SELECT 'email' AS f WHERE NEW.email IS NOT OLD.email
            UNION ALL SELECT 'company name' WHERE NEW.company_name IS NOT OLD.company_name
            UNION ALL SELECT 'phone' WHERE NEW.phone IS NOT OLD.phone
            UNION ALL SELECT 'billing address' WHERE NEW.billing_address IS NOT OLD.billing_address
            UNION ALL SELECT 'terms' WHERE NEW.terms_days IS NOT OLD.terms_days OR (NEW.terms_id IS NOT OLD.terms_id AND OLD.terms_id IS NOT NULL)
            UNION ALL SELECT 'notes' WHERE NEW.notes IS NOT OLD.notes
            UNION ALL SELECT 'parent customer' WHERE NEW.parent_id IS NOT OLD.parent_id
            UNION ALL SELECT 'sales tax' WHERE NEW.tax_rate_id IS NOT OLD.tax_rate_id OR NEW.is_tax_exempt IS NOT OLD.is_tax_exempt
            UNION ALL SELECT 'sales rep' WHERE NEW.sales_rep_id IS NOT OLD.sales_rep_id
            UNION ALL SELECT 'customer type' WHERE NEW.customer_type_id IS NOT OLD.customer_type_id
            UNION ALL SELECT 'job type' WHERE NEW.job_type_id IS NOT OLD.job_type_id)))
    ));
END;

DROP TRIGGER trg_ledger_audit_vendor;
CREATE TRIGGER trg_ledger_audit_vendor
AFTER UPDATE ON vendors
WHEN NEW.name IS NOT OLD.name OR NEW.is_active IS NOT OLD.is_active OR NEW.email IS NOT OLD.email
  OR NEW.company_name IS NOT OLD.company_name OR NEW.phone IS NOT OLD.phone OR NEW.address IS NOT OLD.address
  OR NEW.terms_days IS NOT OLD.terms_days OR NEW.notes IS NOT OLD.notes OR NEW.is_1099 IS NOT OLD.is_1099
  OR NEW.tax_id_masked IS NOT OLD.tax_id_masked
  OR (NEW.terms_id IS NOT OLD.terms_id AND OLD.terms_id IS NOT NULL)
  OR NEW.vendor_type_id IS NOT OLD.vendor_type_id
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'VENDOR',
        'before', json_object('name', OLD.name, 'isActive', OLD.is_active),
        'after',  json_object('name', NEW.name, 'isActive', NEW.is_active),
        'changed', json((SELECT json_group_array(f) FROM (
            SELECT 'email' AS f WHERE NEW.email IS NOT OLD.email
            UNION ALL SELECT 'company name' WHERE NEW.company_name IS NOT OLD.company_name
            UNION ALL SELECT 'phone' WHERE NEW.phone IS NOT OLD.phone
            UNION ALL SELECT 'address' WHERE NEW.address IS NOT OLD.address
            UNION ALL SELECT 'terms' WHERE NEW.terms_days IS NOT OLD.terms_days OR (NEW.terms_id IS NOT OLD.terms_id AND OLD.terms_id IS NOT NULL)
            UNION ALL SELECT 'notes' WHERE NEW.notes IS NOT OLD.notes
            UNION ALL SELECT '1099' WHERE NEW.is_1099 IS NOT OLD.is_1099
            UNION ALL SELECT 'tax ID' WHERE NEW.tax_id_masked IS NOT OLD.tax_id_masked
            UNION ALL SELECT 'vendor type' WHERE NEW.vendor_type_id IS NOT OLD.vendor_type_id)))
    ));
END;

DROP TRIGGER trg_ledger_audit_product;
CREATE TRIGGER trg_ledger_audit_product
AFTER UPDATE ON products_services
WHEN NEW.name IS NOT OLD.name OR NEW.is_active IS NOT OLD.is_active OR NEW.description IS NOT OLD.description
  OR NEW.rate_cents IS NOT OLD.rate_cents OR NEW.income_account_id IS NOT OLD.income_account_id
  OR NEW.is_taxable IS NOT OLD.is_taxable
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'ITEM',
        'before', json_object('name', OLD.name, 'isActive', OLD.is_active, 'rateCents', OLD.rate_cents),
        'after',  json_object('name', NEW.name, 'isActive', NEW.is_active, 'rateCents', NEW.rate_cents),
        'changed', json((SELECT json_group_array(f) FROM (
            SELECT 'description' AS f WHERE NEW.description IS NOT OLD.description
            UNION ALL SELECT 'price' WHERE NEW.rate_cents IS NOT OLD.rate_cents
            UNION ALL SELECT 'income account' WHERE NEW.income_account_id IS NOT OLD.income_account_id
            UNION ALL SELECT 'taxable' WHERE NEW.is_taxable IS NOT OLD.is_taxable)))
    ));
END;

DROP TRIGGER trg_ledger_audit_customer_contact;
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
        'changed', json((SELECT json_group_array(f) FROM (
            SELECT 'shipping address' AS f WHERE NEW.shipping_address IS NOT OLD.shipping_address
            UNION ALL SELECT 'contact' WHERE NEW.contact_name IS NOT OLD.contact_name
            UNION ALL SELECT 'alternate contact' WHERE NEW.alt_contact_name IS NOT OLD.alt_contact_name
            UNION ALL SELECT 'alternate phone' WHERE NEW.alt_phone IS NOT OLD.alt_phone
            UNION ALL SELECT 'fax' WHERE NEW.fax IS NOT OLD.fax
            UNION ALL SELECT 'website' WHERE NEW.website IS NOT OLD.website
            UNION ALL SELECT 'account number' WHERE NEW.account_number IS NOT OLD.account_number
            UNION ALL SELECT 'credit limit' WHERE NEW.credit_limit_cents IS NOT OLD.credit_limit_cents
            UNION ALL SELECT 'resale number' WHERE NEW.resale_number IS NOT OLD.resale_number)))
    ));
END;

DROP TRIGGER trg_ledger_audit_item_purchase;
CREATE TRIGGER trg_ledger_audit_item_purchase
AFTER UPDATE OF purchase_description, purchase_cost_cents ON products_services
WHEN NEW.purchase_description IS NOT OLD.purchase_description OR NEW.purchase_cost_cents IS NOT OLD.purchase_cost_cents
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'ITEM',
        'before', json_object('name', OLD.name, 'purchaseCostCents', OLD.purchase_cost_cents),
        'after',  json_object('name', NEW.name, 'purchaseCostCents', NEW.purchase_cost_cents),
        'changed', json((SELECT json_group_array(f) FROM (
            SELECT 'purchase description' AS f WHERE NEW.purchase_description IS NOT OLD.purchase_description
            UNION ALL SELECT 'cost' WHERE NEW.purchase_cost_cents IS NOT OLD.purchase_cost_cents)))));
END;

DROP TRIGGER trg_ledger_audit_other_name_update;
CREATE TRIGGER trg_ledger_audit_other_name_update
AFTER UPDATE ON other_names
WHEN NEW.name IS NOT OLD.name OR NEW.is_active IS NOT OLD.is_active OR NEW.company_name IS NOT OLD.company_name OR NEW.email IS NOT OLD.email
  OR NEW.phone IS NOT OLD.phone OR NEW.address IS NOT OLD.address OR NEW.notes IS NOT OLD.notes
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'OTHER_NAME',
        'before', json_object('name', OLD.name, 'isActive', OLD.is_active),
        'after',  json_object('name', NEW.name, 'isActive', NEW.is_active),
        'changed', json((SELECT json_group_array(f) FROM (
            SELECT 'name' AS f WHERE NEW.name IS NOT OLD.name
            UNION ALL SELECT 'active' WHERE NEW.is_active IS NOT OLD.is_active
            UNION ALL SELECT 'company name' WHERE NEW.company_name IS NOT OLD.company_name
            UNION ALL SELECT 'email' WHERE NEW.email IS NOT OLD.email
            UNION ALL SELECT 'phone' WHERE NEW.phone IS NOT OLD.phone
            UNION ALL SELECT 'address' WHERE NEW.address IS NOT OLD.address
            UNION ALL SELECT 'notes' WHERE NEW.notes IS NOT OLD.notes)))));
END;
