-- 0118_item_receipts.sql
-- Item receipts (gap T12, design in docs/plans/item-receipts.md): goods received from a vendor,
-- often against a purchase order, before the vendor's bill arrives. A receipt posts
-- Dr Inventory Asset (or the line's expense account) / Cr Items Received Not Billed, a system
-- current liability, so A/P, Pay Bills and vendor balances only ever hold real bills. Entering the
-- bill posts Dr Items Received Not Billed (the receipt's amount) / Cr A/P (the bill's amount), with
-- any price difference going to Inventory Asset or Cost of Goods Sold, or to the line's expense
-- account. The receipt's entry is a JOURNAL entry; this table names it.
--
-- Receipts, their lines and their bill links are never changed or removed: a receipt is voided
-- through its journal entry (refused while a live bill uses it), and a bill is undone by voiding it.

CREATE TABLE item_receipts (
    id                  INTEGER PRIMARY KEY,
    receipt_number      INTEGER NOT NULL UNIQUE CHECK (receipt_number > 0),
    vendor_id           INTEGER NOT NULL REFERENCES vendors (id) ON DELETE RESTRICT,
    purchase_order_id   INTEGER REFERENCES purchase_orders (id) ON DELETE RESTRICT,
    receipt_date        TEXT    NOT NULL CHECK (date(receipt_date) = receipt_date),
    reference           TEXT,
    memo                TEXT,
    total_cents         INTEGER NOT NULL CHECK (total_cents BETWEEN 1 AND 100000000000000),
    journal_entry_id    INTEGER NOT NULL UNIQUE REFERENCES journal_entries (id) ON DELETE RESTRICT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX ix_item_receipts_vendor ON item_receipts (vendor_id);
CREATE INDEX ix_item_receipts_po ON item_receipts (purchase_order_id);

CREATE TABLE item_receipt_lines (
    id                      INTEGER PRIMARY KEY,
    item_receipt_id         INTEGER NOT NULL REFERENCES item_receipts (id) ON DELETE RESTRICT,
    line_number             INTEGER NOT NULL CHECK (line_number > 0),
    purchase_order_line_id  INTEGER REFERENCES purchase_order_lines (id) ON DELETE RESTRICT,
    account_id              INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    product_service_id      INTEGER REFERENCES products_services (id) ON DELETE RESTRICT,
    description             TEXT    NOT NULL DEFAULT '',
    quantity_e4             INTEGER CHECK (quantity_e4 IS NULL OR quantity_e4 > 0),
    rate_cents              INTEGER CHECK (rate_cents IS NULL OR rate_cents >= 0),
    amount_cents            INTEGER NOT NULL CHECK (amount_cents BETWEEN 1 AND 100000000000000),
    class_id                INTEGER REFERENCES classes (id) ON DELETE RESTRICT,
    UNIQUE (item_receipt_id, line_number)
) STRICT;
CREATE INDEX ix_item_receipt_lines_po_line ON item_receipt_lines (purchase_order_line_id);

-- The bill entered for a receipt. A receipt is billed while one of its bills is not voided.
CREATE TABLE item_receipt_bills (
    id                  INTEGER PRIMARY KEY,
    item_receipt_id     INTEGER NOT NULL REFERENCES item_receipts (id) ON DELETE RESTRICT,
    bill_id             INTEGER NOT NULL UNIQUE REFERENCES bills (id) ON DELETE RESTRICT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX ix_item_receipt_bills_receipt ON item_receipt_bills (item_receipt_id);

CREATE TRIGGER trg_item_receipts_no_update BEFORE UPDATE ON item_receipts
BEGIN
    SELECT RAISE(ABORT, 'RECEIPT_IMMUTABLE: an item receipt is voided, never changed');
END;
CREATE TRIGGER trg_item_receipts_no_delete BEFORE DELETE ON item_receipts
BEGIN
    SELECT RAISE(ABORT, 'RECEIPT_IMMUTABLE: an item receipt is voided, never removed');
END;
CREATE TRIGGER trg_item_receipt_lines_no_update BEFORE UPDATE ON item_receipt_lines
BEGIN
    SELECT RAISE(ABORT, 'RECEIPT_IMMUTABLE: an item receipt is voided, never changed');
END;
CREATE TRIGGER trg_item_receipt_lines_no_delete BEFORE DELETE ON item_receipt_lines
BEGIN
    SELECT RAISE(ABORT, 'RECEIPT_IMMUTABLE: an item receipt is voided, never removed');
END;
CREATE TRIGGER trg_item_receipt_bills_no_update BEFORE UPDATE ON item_receipt_bills
BEGIN
    SELECT RAISE(ABORT, 'RECEIPT_IMMUTABLE: a receipt''s bill is voided, never changed');
END;
CREATE TRIGGER trg_item_receipt_bills_no_delete BEFORE DELETE ON item_receipt_bills
BEGIN
    SELECT RAISE(ABORT, 'RECEIPT_IMMUTABLE: a receipt''s bill is voided, never removed');
END;
