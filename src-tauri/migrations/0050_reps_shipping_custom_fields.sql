-- Three QuickBooks lists for sales forms: sales reps, shipping methods ("Ship Via") and custom
-- fields. All three are optional; a company that never fills them in sees no difference.
--
-- * A sales rep is a short set of initials with an optional full name. A customer may carry a
--   default rep; invoices, sales receipts and estimates carry the rep they were sold by.
-- * Shipping methods name a carrier or service. Invoices, sales receipts and estimates may carry
--   a ship date and a shipping method.
-- * Custom fields are user-defined text fields on customers, vendors or items, up to 15 per list.
--   A customer or item field may be marked for sales forms. A customer field marked that way is
--   copied onto each new sales document (editable there) and kept with it, so a printed form
--   shows what it was issued with; an item field marked that way prints under the line.
--
-- None of this posts: reps, ship details and custom values are document details, not amounts, so
-- the existing immutability triggers (which list the columns that may not change) are unaffected.

CREATE TABLE sales_reps (
    id          INTEGER PRIMARY KEY,
    initials    TEXT    NOT NULL CHECK (length(trim(initials)) BETWEEN 1 AND 5 AND initials = trim(initials)),
    name        TEXT    CHECK (name IS NULL OR length(trim(name)) > 0),
    is_active   INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_sales_reps_initials ON sales_reps (initials COLLATE NOCASE);

CREATE TABLE ship_methods (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 31 AND name = trim(name)),
    is_active   INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_ship_methods_name ON ship_methods (name COLLATE NOCASE);

ALTER TABLE customers ADD COLUMN sales_rep_id INTEGER REFERENCES sales_reps (id) ON DELETE RESTRICT;

ALTER TABLE invoices       ADD COLUMN sales_rep_id   INTEGER REFERENCES sales_reps (id) ON DELETE RESTRICT;
ALTER TABLE invoices       ADD COLUMN ship_date      TEXT CHECK (ship_date IS NULL OR date(ship_date) = ship_date);
ALTER TABLE invoices       ADD COLUMN ship_method_id INTEGER REFERENCES ship_methods (id) ON DELETE RESTRICT;
ALTER TABLE sales_receipts ADD COLUMN sales_rep_id   INTEGER REFERENCES sales_reps (id) ON DELETE RESTRICT;
ALTER TABLE sales_receipts ADD COLUMN ship_date      TEXT CHECK (ship_date IS NULL OR date(ship_date) = ship_date);
ALTER TABLE sales_receipts ADD COLUMN ship_method_id INTEGER REFERENCES ship_methods (id) ON DELETE RESTRICT;
ALTER TABLE estimates      ADD COLUMN sales_rep_id   INTEGER REFERENCES sales_reps (id) ON DELETE RESTRICT;
ALTER TABLE estimates      ADD COLUMN ship_date      TEXT CHECK (ship_date IS NULL OR date(ship_date) = ship_date);
ALTER TABLE estimates      ADD COLUMN ship_method_id INTEGER REFERENCES ship_methods (id) ON DELETE RESTRICT;

CREATE INDEX ix_invoices_sales_rep       ON invoices (sales_rep_id) WHERE sales_rep_id IS NOT NULL;
CREATE INDEX ix_sales_receipts_sales_rep ON sales_receipts (sales_rep_id) WHERE sales_rep_id IS NOT NULL;

-- ---------------------------------------------------------------------------------------------
-- Custom fields
-- ---------------------------------------------------------------------------------------------
CREATE TABLE custom_fields (
    id              INTEGER PRIMARY KEY,
    list            TEXT    NOT NULL CHECK (list IN ('CUSTOMER', 'VENDOR', 'ITEM')),
    label           TEXT    NOT NULL CHECK (length(trim(label)) BETWEEN 1 AND 31 AND label = trim(label)),
    -- Display order within the list.
    position        INTEGER NOT NULL CHECK (position BETWEEN 1 AND 15),
    on_sales_forms  INTEGER NOT NULL DEFAULT 0 CHECK (on_sales_forms IN (0, 1)),
    CHECK (on_sales_forms = 0 OR list IN ('CUSTOMER', 'ITEM')),
    UNIQUE (list, position)
) STRICT;
CREATE UNIQUE INDEX ux_custom_fields_label ON custom_fields (list, label COLLATE NOCASE);

-- One value per field and record. `record_id` is the customer, vendor or product, by the field's list.
CREATE TABLE custom_field_values (
    field_id    INTEGER NOT NULL REFERENCES custom_fields (id) ON DELETE CASCADE,
    record_id   INTEGER NOT NULL,
    value       TEXT    NOT NULL CHECK (length(trim(value)) BETWEEN 1 AND 100 AND value = trim(value)),
    PRIMARY KEY (field_id, record_id)
) STRICT;

-- A customer field's value as printed on one sales document.
CREATE TABLE sales_document_fields (
    document_kind   TEXT    NOT NULL CHECK (document_kind IN ('INVOICE', 'SALES_RECEIPT', 'ESTIMATE')),
    document_id     INTEGER NOT NULL,
    field_id        INTEGER NOT NULL REFERENCES custom_fields (id) ON DELETE CASCADE,
    value           TEXT    NOT NULL CHECK (length(trim(value)) BETWEEN 1 AND 100 AND value = trim(value)),
    PRIMARY KEY (document_kind, document_id, field_id)
) STRICT;

-- ---------------------------------------------------------------------------------------------
-- Imports: a desktop file's reps, ship methods and custom field values, staged with the lists
-- ---------------------------------------------------------------------------------------------
-- The customer's rep (initials, or "initials:name"), and each record's custom fields as a JSON
-- object of label → value.
ALTER TABLE ledger_import_parties ADD COLUMN sales_rep TEXT;
ALTER TABLE ledger_import_parties ADD COLUMN custom_fields TEXT;
ALTER TABLE ledger_import_items   ADD COLUMN custom_fields TEXT;

-- The file's sales rep and shipping method lists. A rep is "initials" or "initials:name".
CREATE TABLE ledger_import_sales_lists (
    batch_id    INTEGER NOT NULL,
    kind        TEXT    NOT NULL CHECK (kind IN ('REP', 'SHIP_METHOD')),
    name        TEXT    NOT NULL,
    is_active   INTEGER NOT NULL CHECK (is_active IN (0, 1)),
    PRIMARY KEY (batch_id, kind, name)
) STRICT;
