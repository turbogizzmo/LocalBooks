-- T45: approvals for bills and purchase orders (`approvals.rs`, design in docs/plans/approvals.md).
-- A document matching a rule is held as a request, not recorded; approving creates it.

CREATE TABLE approval_rules (
    id              INTEGER PRIMARY KEY,
    kind            TEXT    NOT NULL CHECK (kind IN ('BILL', 'PURCHASE_ORDER')),
    min_total_cents INTEGER NOT NULL CHECK (min_total_cents >= 0),
    vendor_id       INTEGER REFERENCES vendors (id) ON DELETE CASCADE,
    created_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
) STRICT;

-- Kept for good, rejected ones included. bill_id and purchase_order_id are plain ids (no foreign
-- key), so condensing an approved bill isn't blocked by its request.
CREATE TABLE approval_requests (
    id                INTEGER PRIMARY KEY,
    kind              TEXT    NOT NULL CHECK (kind IN ('BILL', 'PURCHASE_ORDER')),
    vendor_name       TEXT    NOT NULL CHECK (length(trim(vendor_name)) > 0),
    total_cents       INTEGER NOT NULL CHECK (total_cents > 0),
    document_date     TEXT    NOT NULL CHECK (date(document_date) = document_date),
    reference         TEXT,
    input_json        TEXT    NOT NULL CHECK (json_valid(input_json)),
    status            TEXT    NOT NULL DEFAULT 'PENDING' CHECK (status IN ('PENDING', 'APPROVED', 'REJECTED')),
    requested_by      TEXT    NOT NULL,
    requested_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    decided_by        TEXT,
    decided_at        TEXT,
    note              TEXT,
    bill_id           INTEGER,
    purchase_order_id INTEGER,
    CHECK ((status = 'PENDING') = (decided_at IS NULL)),
    CHECK (status <> 'REJECTED' OR length(trim(note)) > 0),
    CHECK (status <> 'APPROVED' OR (bill_id IS NOT NULL) <> (purchase_order_id IS NOT NULL))
) STRICT;
CREATE INDEX idx_approval_requests_status ON approval_requests (status);
