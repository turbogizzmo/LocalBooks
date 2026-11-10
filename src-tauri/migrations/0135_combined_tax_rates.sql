-- R11: combined sales tax rates split by agency, and the sales tax return per agency
-- (`sales_tax.rs`, `sales_tax/combined.rs`).

-- A combined rate (a "group") is made of component rates, each with its own agency and liability
-- account; its rate is theirs added up. A component is an ordinary rate, never another group.
ALTER TABLE tax_rates ADD COLUMN is_group INTEGER NOT NULL DEFAULT 0 CHECK (is_group IN (0, 1));

CREATE TABLE tax_rate_components (
    group_id     INTEGER NOT NULL REFERENCES tax_rates (id) ON DELETE CASCADE,
    component_id INTEGER NOT NULL REFERENCES tax_rates (id) ON DELETE RESTRICT,
    position     INTEGER NOT NULL CHECK (position > 0),
    PRIMARY KEY (group_id, component_id),
    CHECK (group_id <> component_id)
) STRICT;

-- How a sale at a combined rate split its tax, per component, as posted: the return reads these,
-- so changing a group later never changes the tax already reported.
CREATE TABLE document_tax_parts (
    journal_entry_id INTEGER NOT NULL REFERENCES journal_entries (id) ON DELETE CASCADE,
    tax_rate_id      INTEGER NOT NULL REFERENCES tax_rates (id) ON DELETE RESTRICT,
    cents            INTEGER NOT NULL CHECK (cents >= 0),
    PRIMARY KEY (journal_entry_id, tax_rate_id)
) STRICT;
