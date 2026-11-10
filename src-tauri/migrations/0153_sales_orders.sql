-- Sales orders (gaps row T69, docs/plans/sales-orders.md, `sales_orders.rs`).
--
-- A sales order records what a customer ordered before it is shipped and billed. It posts nothing:
-- only the invoices made from it post. Each order line keeps how much of it has been invoiced and
-- how much was closed without being invoiced; `sales_order_invoiced` links each invoice to the order
-- lines it took, so voiding that invoice gives the quantities back, and an edited invoice (a new
-- invoice replacing it, `edits.rs`) takes the links over.

CREATE TABLE sales_orders (
    id                  INTEGER PRIMARY KEY,
    order_number        INTEGER NOT NULL UNIQUE CHECK (order_number > 0),
    customer_id         INTEGER NOT NULL REFERENCES customers (id) ON DELETE RESTRICT,
    order_date          TEXT    NOT NULL CHECK (order_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]' AND date(order_date) = order_date),
    expected_date       TEXT    CHECK (expected_date IS NULL OR date(expected_date) = expected_date),
    memo                TEXT,
    tax_rate_id         INTEGER REFERENCES tax_rates (id) ON DELETE RESTRICT,
    sales_rep_id        INTEGER REFERENCES sales_reps (id) ON DELETE RESTRICT,
    ship_method_id      INTEGER REFERENCES ship_methods (id) ON DELETE RESTRICT,
    customer_message_id INTEGER REFERENCES profile_entries (id) ON DELETE RESTRICT,
    bill_to             TEXT,
    ship_to             TEXT,
    po_number           TEXT    CHECK (po_number IS NULL OR length(po_number) <= 25),
    fob                 TEXT    CHECK (fob IS NULL OR length(fob) <= 25),
    -- Closed by hand: whatever is left on its lines won't be invoiced.
    is_closed           INTEGER NOT NULL DEFAULT 0 CHECK (is_closed IN (0, 1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK (expected_date IS NULL OR expected_date >= order_date)
) STRICT;
CREATE INDEX ix_sales_orders_customer ON sales_orders (customer_id);

CREATE TABLE sales_order_lines (
    id                  INTEGER PRIMARY KEY,
    sales_order_id      INTEGER NOT NULL REFERENCES sales_orders (id) ON DELETE CASCADE,
    line_number         INTEGER NOT NULL CHECK (line_number > 0),
    product_service_id  INTEGER REFERENCES products_services (id) ON DELETE RESTRICT,
    description         TEXT    NOT NULL,
    quantity_e4         INTEGER NOT NULL CHECK (quantity_e4 BETWEEN 1 AND 10000000000000),
    rate_cents          INTEGER NOT NULL CHECK (rate_cents BETWEEN 0 AND 100000000000000),
    amount_cents        INTEGER NOT NULL CHECK (amount_cents BETWEEN 0 AND 100000000000000),
    income_account_id   INTEGER NOT NULL REFERENCES chart_of_accounts (id) ON DELETE RESTRICT,
    class_id            INTEGER REFERENCES classes (id) ON DELETE RESTRICT,
    is_taxable          INTEGER NOT NULL DEFAULT 0 CHECK (is_taxable IN (0, 1)),
    -- Kept by `sales_orders.rs` and the triggers below, never summed from the links: a condense can
    -- drop the links of invoices it replaces.
    invoiced_qty_e4     INTEGER NOT NULL DEFAULT 0 CHECK (invoiced_qty_e4 >= 0),
    -- Closed without being invoiced (the order or this line closed by hand).
    closed_qty_e4       INTEGER NOT NULL DEFAULT 0 CHECK (closed_qty_e4 >= 0),
    CHECK (amount_cents = (quantity_e4 * rate_cents + 5000) / 10000),
    CHECK (invoiced_qty_e4 + closed_qty_e4 <= quantity_e4),
    UNIQUE (sales_order_id, line_number)
) STRICT;
CREATE INDEX ix_sales_order_lines_item ON sales_order_lines (product_service_id);

CREATE TABLE sales_order_invoiced (
    id                  INTEGER PRIMARY KEY,
    sales_order_line_id INTEGER NOT NULL REFERENCES sales_order_lines (id) ON DELETE RESTRICT,
    invoice_id          INTEGER NOT NULL REFERENCES invoices (id) ON DELETE RESTRICT,
    quantity_e4         INTEGER NOT NULL CHECK (quantity_e4 > 0),
    -- The invoice was voided; its quantity went back onto the order line.
    is_reversed         INTEGER NOT NULL DEFAULT 0 CHECK (is_reversed IN (0, 1)),
    UNIQUE (sales_order_line_id, invoice_id)
) STRICT;
CREATE INDEX ix_sales_order_invoiced_line ON sales_order_invoiced (sales_order_line_id);
CREATE INDEX ix_sales_order_invoiced_invoice ON sales_order_invoiced (invoice_id);

-- A link adds its quantity to the order line it took from (the line's CHECK refuses too much).
CREATE TRIGGER trg_sales_order_invoiced_add AFTER INSERT ON sales_order_invoiced
BEGIN
    UPDATE sales_order_lines SET invoiced_qty_e4 = invoiced_qty_e4 + NEW.quantity_e4 WHERE id = NEW.sales_order_line_id;
END;

-- Voiding an invoice, on any path, gives its quantities back to the order lines.
CREATE TRIGGER trg_sales_order_invoice_voided AFTER UPDATE OF is_voided ON invoices
WHEN OLD.is_voided = 0 AND NEW.is_voided = 1
BEGIN
    UPDATE sales_order_lines
    SET invoiced_qty_e4 = invoiced_qty_e4 - (SELECT SUM(k.quantity_e4) FROM sales_order_invoiced k
                                             WHERE k.sales_order_line_id = sales_order_lines.id AND k.invoice_id = NEW.id AND k.is_reversed = 0)
    WHERE id IN (SELECT sales_order_line_id FROM sales_order_invoiced WHERE invoice_id = NEW.id AND is_reversed = 0);
    UPDATE sales_order_invoiced SET is_reversed = 1 WHERE invoice_id = NEW.id AND is_reversed = 0;
END;

-- An invoice line that came from an order can't be taken off the order but by voiding the invoice.
CREATE TRIGGER trg_sales_order_invoiced_no_update BEFORE UPDATE OF sales_order_line_id, quantity_e4 ON sales_order_invoiced
BEGIN
    SELECT RAISE(ABORT, 'SO_LINK_IMMUTABLE: an invoice''s order lines and quantities can''t change; void the invoice instead');
END;

-- A line can't be cut below what is already invoiced or closed (its CHECK), nor deleted once invoiced.
CREATE TRIGGER trg_sales_order_lines_no_delete_invoiced BEFORE DELETE ON sales_order_lines
WHEN OLD.invoiced_qty_e4 > 0 OR EXISTS (SELECT 1 FROM sales_order_invoiced WHERE sales_order_line_id = OLD.id)
BEGIN
    SELECT RAISE(ABORT, 'SO_LINE_INVOICED: a line that has been invoiced can''t be removed');
END;
