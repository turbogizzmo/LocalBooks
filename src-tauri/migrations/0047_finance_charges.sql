-- Finance charges on overdue invoices (see `finance_charges.rs`).
--
-- Assessing a finance charge writes an ordinary invoice (Dr A/R, Cr the chosen income account), so it
-- shows in A/R aging, on statements and in Receive Payments like any other. These tables mark which
-- invoices are finance charges and record, per overdue invoice, the period each charge covered: the
-- next assessment charges from the end of that period, so no day is charged twice. Voiding a
-- finance-charge invoice reopens the periods it covered.

CREATE TABLE finance_charge_invoices (
    invoice_id          INTEGER PRIMARY KEY REFERENCES invoices (id) ON DELETE RESTRICT,
    assessed_as_of      TEXT    NOT NULL CHECK (date(assessed_as_of) = assessed_as_of),
    -- The charge as calculated, before any change the user made on the Assess Finance Charges screen.
    computed_cents      INTEGER NOT NULL CHECK (computed_cents >= 0),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE finance_charge_basis (
    id                          INTEGER PRIMARY KEY,
    finance_charge_invoice_id   INTEGER NOT NULL REFERENCES finance_charge_invoices (invoice_id) ON DELETE RESTRICT,
    source_invoice_id           INTEGER NOT NULL REFERENCES invoices (id) ON DELETE RESTRICT,
    balance_cents               INTEGER NOT NULL CHECK (balance_cents > 0),
    from_date                   TEXT    NOT NULL CHECK (date(from_date) = from_date),
    to_date                     TEXT    NOT NULL CHECK (date(to_date) = to_date),
    CHECK (to_date > from_date),
    UNIQUE (finance_charge_invoice_id, source_invoice_id)
) STRICT;
CREATE INDEX ix_finance_charge_basis_source ON finance_charge_basis (source_invoice_id);

CREATE TRIGGER trg_finance_charge_invoices_immutable BEFORE UPDATE ON finance_charge_invoices
BEGIN
    SELECT RAISE(ABORT, 'FC_IMMUTABLE: an assessed finance charge cannot be changed; void its invoice instead');
END;
CREATE TRIGGER trg_finance_charge_invoices_no_delete BEFORE DELETE ON finance_charge_invoices
BEGIN
    SELECT RAISE(ABORT, 'FC_IMMUTABLE: an assessed finance charge cannot be changed; void its invoice instead');
END;
CREATE TRIGGER trg_finance_charge_basis_immutable BEFORE UPDATE ON finance_charge_basis
BEGIN
    SELECT RAISE(ABORT, 'FC_IMMUTABLE: an assessed finance charge cannot be changed; void its invoice instead');
END;
CREATE TRIGGER trg_finance_charge_basis_no_delete BEFORE DELETE ON finance_charge_basis
BEGIN
    SELECT RAISE(ABORT, 'FC_IMMUTABLE: an assessed finance charge cannot be changed; void its invoice instead');
END;

-- Off until a rate and an income account are chosen in Settings › Preferences Console › Finance charges.
INSERT INTO system_preferences (key, value) VALUES
    ('finance_charge_annual_rate',        '0'),
    ('finance_charge_minimum',            '0.00'),
    ('finance_charge_grace_days',         '0'),
    ('finance_charge_account_id',         ''),
    ('finance_charge_on_finance_charges', 'false');
