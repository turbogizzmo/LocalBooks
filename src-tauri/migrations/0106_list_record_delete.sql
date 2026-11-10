-- 0106_list_record_delete.sql
-- Edit › Delete on the customer, vendor and item lists (gap T33).
--
-- A customer, vendor or item that nothing uses can be deleted: no transaction, job, document,
-- price, rule or other row refers to it (the foreign keys refuse the delete otherwise, and the app
-- checks the names invoices keep). Anything used is made inactive instead. Each delete is a list
-- change in the audit trail, with the name it had.

CREATE TRIGGER trg_ledger_audit_customer_delete
AFTER DELETE ON customers
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', OLD.id, json_object('list', 'CUSTOMER', 'before', json_object('name', OLD.name), 'after', NULL, 'changed', json_array('deleted')));
END;

CREATE TRIGGER trg_ledger_audit_vendor_delete
AFTER DELETE ON vendors
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', OLD.id, json_object('list', 'VENDOR', 'before', json_object('name', OLD.name), 'after', NULL, 'changed', json_array('deleted')));
END;

CREATE TRIGGER trg_ledger_audit_item_delete
AFTER DELETE ON products_services
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', OLD.id, json_object('list', 'ITEM', 'before', json_object('name', OLD.name), 'after', NULL, 'changed', json_array('deleted')));
END;
