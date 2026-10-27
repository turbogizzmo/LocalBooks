-- Accounting › Clean Up Books (Client Data Review tools).
--
-- 1. The ledger audit trail gains two event types:
--    * LIST_CHANGE: an edit to a customer, vendor or product/service (renamed, made inactive or
--      active again, or other details changed), written by the triggers below. Accounts already have
--      ACCOUNT_CHANGE; additions are read from each list's own created_at.
--    * CLEANUP: one event per fix made in Clean Up Books (credits applied, invoices written off,
--      payments matched to deposits), naming everything it changed.
--    SQLite cannot change a CHECK constraint in place, so the table is rebuilt exactly as in
--    migration 0054: every row is copied as it is (ids, timestamps, payloads, actors and chain hashes,
--    so the chain still verifies), and the triggers that reference the table are dropped first and
--    recreated unchanged. PRICE_CHANGE (0049) and ACH_FILE (0054) stay allowed.
-- 2. undeposited_fund_matches: a payment or sales receipt sitting in Undeposited Funds that was in
--    fact banked by a deposit recorded without it (the classic double deposit). The deposit's line for
--    that money is matched to the payment, and a correcting entry moves the amount out of Undeposited
--    Funds and off the account the line used (Dr that account / Cr Undeposited Funds). The deposit
--    itself is never changed, so its bank line and any reconciliation stay as they are.

DROP TRIGGER trg_ledger_audit_account;
DROP TRIGGER trg_ledger_audit_insert_unsealed;
DROP TRIGGER trg_ledger_audit_no_delete;
DROP TRIGGER trg_ledger_audit_no_update;
DROP TRIGGER trg_ledger_audit_post;
DROP TRIGGER trg_ledger_audit_void;

CREATE TABLE ledger_audit_log_new (
    id                  INTEGER PRIMARY KEY,
    timestamp           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    action_type         TEXT    NOT NULL CHECK (action_type IN ('POST', 'VOID', 'MIGRATION', 'ACCOUNT_CHANGE', 'FORENSIC_HEAL', 'USER_CHANGE', 'PRICE_CHANGE', 'ACH_FILE', 'LIST_CHANGE', 'CLEANUP')),
    target_entity_id    INTEGER NOT NULL,
    payload_snapshot    TEXT    NOT NULL CHECK (json_valid(payload_snapshot)),
    actor               TEXT,
    prev_hash           TEXT    CHECK (prev_hash IS NULL OR (length(prev_hash) = 64 AND prev_hash NOT GLOB '*[^0-9a-f]*')),
    row_hash            TEXT    CHECK (row_hash IS NULL OR (length(row_hash) = 64 AND row_hash NOT GLOB '*[^0-9a-f]*'))
) STRICT;

INSERT INTO ledger_audit_log_new (id, timestamp, action_type, target_entity_id, payload_snapshot, actor, prev_hash, row_hash)
SELECT id, timestamp, action_type, target_entity_id, payload_snapshot, actor, prev_hash, row_hash FROM ledger_audit_log ORDER BY id;

DROP TABLE ledger_audit_log;
ALTER TABLE ledger_audit_log_new RENAME TO ledger_audit_log;

CREATE INDEX ix_ledger_audit_target ON ledger_audit_log (action_type, target_entity_id);
CREATE INDEX ix_ledger_audit_unsealed ON ledger_audit_log (id) WHERE row_hash IS NULL;

CREATE TRIGGER trg_ledger_audit_account
AFTER UPDATE ON chart_of_accounts
WHEN NEW.name IS NOT OLD.name
  OR NEW.account_number IS NOT OLD.account_number
  OR NEW.account_type IS NOT OLD.account_type
  OR NEW.parent_id IS NOT OLD.parent_id
  OR NEW.is_active IS NOT OLD.is_active
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('ACCOUNT_CHANGE', NEW.id, json_object(
        'before', json_object('accountNumber', OLD.account_number, 'name', OLD.name, 'accountType', OLD.account_type,
                              'parentId', OLD.parent_id, 'isActive', OLD.is_active),
        'after',  json_object('accountNumber', NEW.account_number, 'name', NEW.name, 'accountType', NEW.account_type,
                              'parentId', NEW.parent_id, 'isActive', NEW.is_active)
    ));
END;

CREATE TRIGGER trg_ledger_audit_insert_unsealed BEFORE INSERT ON ledger_audit_log
WHEN NEW.row_hash IS NOT NULL OR NEW.prev_hash IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: audit rows are sealed by the application after they are written');
END;

CREATE TRIGGER trg_ledger_audit_no_delete BEFORE DELETE ON ledger_audit_log
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the ledger audit trail cannot be changed');
END;

CREATE TRIGGER trg_ledger_audit_no_update BEFORE UPDATE ON ledger_audit_log
WHEN NOT (    OLD.row_hash IS NULL
          AND NEW.row_hash IS NOT NULL
          AND NEW.prev_hash IS NOT NULL
          AND NEW.actor IS NOT NULL
          AND (OLD.actor IS NULL OR NEW.actor IS OLD.actor)
          AND NEW.id IS OLD.id
          AND NEW.timestamp IS OLD.timestamp
          AND NEW.action_type IS OLD.action_type
          AND NEW.target_entity_id IS OLD.target_entity_id
          AND NEW.payload_snapshot IS OLD.payload_snapshot)
BEGIN
    SELECT RAISE(ABORT, 'AUDIT_IMMUTABLE: the ledger audit trail cannot be changed');
END;

CREATE TRIGGER trg_ledger_audit_post
AFTER UPDATE OF status ON journal_entries
WHEN OLD.status = 'DRAFT' AND NEW.status = 'POSTED'
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    SELECT 'POST', NEW.id, snapshot FROM v_entry_snapshot WHERE entry_id = NEW.id;
END;

CREATE TRIGGER trg_ledger_audit_void
AFTER UPDATE OF is_voided ON journal_entries
WHEN OLD.is_voided = 0 AND NEW.is_voided = 1
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    SELECT 'VOID', NEW.id, json_object(
        'entryId',             NEW.id,
        'entryNumber',         NEW.entry_number,
        'reversalEntryId',     r.id,
        'reversalEntryNumber', r.entry_number,
        'reversalEntryDate',   r.entry_date,
        'amountCents',         (SELECT SUM(debit_cents) FROM journal_lines WHERE journal_entry_id = NEW.id)
    )
    FROM journal_entries r WHERE r.reverses_entry_id = NEW.id;
END;

-- ---------------------------------------------------------------------------------------------
-- LIST_CHANGE: edits to customers, vendors and products/services. The payload names the list, the
-- name and active flag before and after, and which other fields changed (by name only, so sensitive
-- values such as tax IDs never enter the audit trail).
-- ---------------------------------------------------------------------------------------------
CREATE TRIGGER trg_ledger_audit_customer
AFTER UPDATE ON customers
WHEN NEW.name IS NOT OLD.name OR NEW.is_active IS NOT OLD.is_active OR NEW.email IS NOT OLD.email
  OR NEW.company_name IS NOT OLD.company_name OR NEW.phone IS NOT OLD.phone OR NEW.billing_address IS NOT OLD.billing_address
  OR NEW.terms_days IS NOT OLD.terms_days OR NEW.notes IS NOT OLD.notes OR NEW.parent_id IS NOT OLD.parent_id
  OR NEW.tax_rate_id IS NOT OLD.tax_rate_id OR NEW.is_tax_exempt IS NOT OLD.is_tax_exempt
  OR NEW.sales_rep_id IS NOT OLD.sales_rep_id
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'CUSTOMER',
        'before', json_object('name', OLD.name, 'isActive', OLD.is_active),
        'after',  json_object('name', NEW.name, 'isActive', NEW.is_active),
        'changed', (SELECT json_group_array(f) FROM (
            SELECT 'email' AS f WHERE NEW.email IS NOT OLD.email
            UNION ALL SELECT 'company name' WHERE NEW.company_name IS NOT OLD.company_name
            UNION ALL SELECT 'phone' WHERE NEW.phone IS NOT OLD.phone
            UNION ALL SELECT 'billing address' WHERE NEW.billing_address IS NOT OLD.billing_address
            UNION ALL SELECT 'terms' WHERE NEW.terms_days IS NOT OLD.terms_days
            UNION ALL SELECT 'notes' WHERE NEW.notes IS NOT OLD.notes
            UNION ALL SELECT 'parent customer' WHERE NEW.parent_id IS NOT OLD.parent_id
            UNION ALL SELECT 'sales tax' WHERE NEW.tax_rate_id IS NOT OLD.tax_rate_id OR NEW.is_tax_exempt IS NOT OLD.is_tax_exempt
            UNION ALL SELECT 'sales rep' WHERE NEW.sales_rep_id IS NOT OLD.sales_rep_id))
    ));
END;

CREATE TRIGGER trg_ledger_audit_vendor
AFTER UPDATE ON vendors
WHEN NEW.name IS NOT OLD.name OR NEW.is_active IS NOT OLD.is_active OR NEW.email IS NOT OLD.email
  OR NEW.company_name IS NOT OLD.company_name OR NEW.phone IS NOT OLD.phone OR NEW.address IS NOT OLD.address
  OR NEW.terms_days IS NOT OLD.terms_days OR NEW.notes IS NOT OLD.notes OR NEW.is_1099 IS NOT OLD.is_1099
  OR NEW.tax_id_masked IS NOT OLD.tax_id_masked
BEGIN
    INSERT INTO ledger_audit_log (action_type, target_entity_id, payload_snapshot)
    VALUES ('LIST_CHANGE', NEW.id, json_object(
        'list', 'VENDOR',
        'before', json_object('name', OLD.name, 'isActive', OLD.is_active),
        'after',  json_object('name', NEW.name, 'isActive', NEW.is_active),
        'changed', (SELECT json_group_array(f) FROM (
            SELECT 'email' AS f WHERE NEW.email IS NOT OLD.email
            UNION ALL SELECT 'company name' WHERE NEW.company_name IS NOT OLD.company_name
            UNION ALL SELECT 'phone' WHERE NEW.phone IS NOT OLD.phone
            UNION ALL SELECT 'address' WHERE NEW.address IS NOT OLD.address
            UNION ALL SELECT 'terms' WHERE NEW.terms_days IS NOT OLD.terms_days
            UNION ALL SELECT 'notes' WHERE NEW.notes IS NOT OLD.notes
            UNION ALL SELECT '1099' WHERE NEW.is_1099 IS NOT OLD.is_1099
            UNION ALL SELECT 'tax ID' WHERE NEW.tax_id_masked IS NOT OLD.tax_id_masked))
    ));
END;

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
        'changed', (SELECT json_group_array(f) FROM (
            SELECT 'description' AS f WHERE NEW.description IS NOT OLD.description
            UNION ALL SELECT 'price' WHERE NEW.rate_cents IS NOT OLD.rate_cents
            UNION ALL SELECT 'income account' WHERE NEW.income_account_id IS NOT OLD.income_account_id
            UNION ALL SELECT 'taxable' WHERE NEW.is_taxable IS NOT OLD.is_taxable))
    ));
END;

-- ---------------------------------------------------------------------------------------------
-- Payments in Undeposited Funds matched to a deposit that already banked them
-- ---------------------------------------------------------------------------------------------
CREATE TABLE undeposited_fund_matches (
    id                  INTEGER PRIMARY KEY,
    invoice_payment_id  INTEGER REFERENCES invoice_payments (id) ON DELETE RESTRICT,
    sales_receipt_id    INTEGER REFERENCES sales_receipts (id) ON DELETE RESTRICT,
    deposit_id          INTEGER NOT NULL REFERENCES deposits (id) ON DELETE RESTRICT,
    deposit_line_id     INTEGER NOT NULL REFERENCES deposit_lines (id) ON DELETE RESTRICT,
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    -- Dr the line's account / Cr Undeposited Funds; NULL when the line already credited Undeposited Funds.
    journal_entry_id    INTEGER UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    is_voided           INTEGER NOT NULL DEFAULT 0 CHECK (is_voided IN (0, 1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK ((invoice_payment_id IS NULL) <> (sales_receipt_id IS NULL))
) STRICT;
CREATE UNIQUE INDEX ux_uf_matches_payment ON undeposited_fund_matches (invoice_payment_id)
    WHERE invoice_payment_id IS NOT NULL AND is_voided = 0;
CREATE UNIQUE INDEX ux_uf_matches_receipt ON undeposited_fund_matches (sales_receipt_id)
    WHERE sales_receipt_id IS NOT NULL AND is_voided = 0;
CREATE INDEX ix_uf_matches_line ON undeposited_fund_matches (deposit_line_id);

CREATE TRIGGER trg_uf_match_insert
BEFORE INSERT ON undeposited_fund_matches
BEGIN
    SELECT RAISE(ABORT, 'UF_ALREADY_DEPOSITED: that payment is already in a deposit')
    WHERE EXISTS (SELECT 1 FROM deposit_lines WHERE is_voided = 0 AND (invoice_payment_id = NEW.invoice_payment_id OR sales_receipt_id = NEW.sales_receipt_id));
    SELECT RAISE(ABORT, 'UF_BAD_LINE: match to a live line of the deposit that is not itself a payment')
    WHERE NOT EXISTS (SELECT 1 FROM deposit_lines l JOIN deposits d ON d.id = l.deposit_id
                      WHERE l.id = NEW.deposit_line_id AND l.deposit_id = NEW.deposit_id AND l.is_voided = 0 AND d.is_voided = 0
                        AND l.invoice_payment_id IS NULL AND l.sales_receipt_id IS NULL);
    SELECT RAISE(ABORT, 'UF_LINE_USED: the matches on that deposit line would add up to more than the line')
    WHERE NEW.amount_cents + COALESCE((SELECT SUM(amount_cents) FROM undeposited_fund_matches WHERE deposit_line_id = NEW.deposit_line_id AND is_voided = 0), 0)
        > (SELECT amount_cents FROM deposit_lines WHERE id = NEW.deposit_line_id);
END;

-- A match only ever goes from live to undone.
CREATE TRIGGER trg_uf_match_once
BEFORE UPDATE ON undeposited_fund_matches
WHEN NEW.is_voided < OLD.is_voided OR NEW.invoice_payment_id IS NOT OLD.invoice_payment_id OR NEW.sales_receipt_id IS NOT OLD.sales_receipt_id
  OR NEW.deposit_line_id IS NOT OLD.deposit_line_id OR NEW.amount_cents IS NOT OLD.amount_cents
  OR NEW.journal_entry_id IS NOT OLD.journal_entry_id
BEGIN
    SELECT RAISE(ABORT, 'UF_MATCH_IMMUTABLE: a match can only be undone');
END;

CREATE TRIGGER trg_uf_match_no_delete BEFORE DELETE ON undeposited_fund_matches
BEGIN
    SELECT RAISE(ABORT, 'UF_MATCH_IMMUTABLE: a match can only be undone');
END;

-- Voiding a match's correcting entry (from the journal) undoes the match.
CREATE TRIGGER trg_uf_match_entry_voided
AFTER UPDATE OF is_voided ON journal_entries
WHEN OLD.is_voided = 0 AND NEW.is_voided = 1
BEGIN
    UPDATE undeposited_fund_matches SET is_voided = 1 WHERE journal_entry_id = NEW.id AND is_voided = 0;
END;

-- A matched payment counts as deposited: it can't go into another deposit, and neither it nor the
-- deposit it was matched to can be voided until the match is undone.
CREATE TRIGGER trg_uf_matched_not_redeposited
BEFORE INSERT ON deposit_lines
WHEN NEW.invoice_payment_id IS NOT NULL OR NEW.sales_receipt_id IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'DEP_ALREADY_DEPOSITED: that payment was matched to an earlier deposit in Clean Up Books')
    WHERE EXISTS (SELECT 1 FROM undeposited_fund_matches WHERE is_voided = 0
                  AND (invoice_payment_id = NEW.invoice_payment_id OR sales_receipt_id = NEW.sales_receipt_id));
END;

CREATE TRIGGER trg_uf_matched_payment_void
BEFORE UPDATE OF is_voided ON invoice_payments
WHEN NEW.is_voided = 1 AND OLD.is_voided = 0
BEGIN
    SELECT RAISE(ABORT, 'VOID_DEPOSITED: this payment is matched to a deposit; undo the match in Clean Up Books first')
    WHERE EXISTS (SELECT 1 FROM undeposited_fund_matches WHERE invoice_payment_id = NEW.id AND is_voided = 0);
END;

CREATE TRIGGER trg_uf_matched_receipt_void
BEFORE UPDATE OF is_voided ON sales_receipts
WHEN NEW.is_voided = 1 AND OLD.is_voided = 0
BEGIN
    SELECT RAISE(ABORT, 'VOID_DEPOSITED: this sales receipt is matched to a deposit; undo the match in Clean Up Books first')
    WHERE EXISTS (SELECT 1 FROM undeposited_fund_matches WHERE sales_receipt_id = NEW.id AND is_voided = 0);
END;

CREATE TRIGGER trg_uf_matched_deposit_void
BEFORE UPDATE OF is_voided ON deposits
WHEN NEW.is_voided = 1 AND OLD.is_voided = 0
BEGIN
    SELECT RAISE(ABORT, 'VOID_MATCHED: payments in Undeposited Funds are matched to this deposit; undo those matches in Clean Up Books first')
    WHERE EXISTS (SELECT 1 FROM undeposited_fund_matches WHERE deposit_id = NEW.id AND is_voided = 0);
END;
