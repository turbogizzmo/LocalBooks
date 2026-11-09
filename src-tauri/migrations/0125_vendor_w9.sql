-- R10: a vendor's Form W-9 details (`form_1099.rs`). The TIN itself stays in vendors.tax_id, where
-- it is encrypted once users are on; this table holds what else the W-9 says and when it came in.
CREATE TABLE vendor_w9 (
    vendor_id          INTEGER PRIMARY KEY REFERENCES vendors (id) ON DELETE CASCADE,
    tin_type           TEXT    CHECK (tin_type IS NULL OR tin_type IN ('SSN', 'EIN')),
    legal_name         TEXT    CHECK (legal_name IS NULL OR length(legal_name) BETWEEN 1 AND 150),
    business_name      TEXT    CHECK (business_name IS NULL OR length(business_name) BETWEEN 1 AND 150),
    tax_classification TEXT    CHECK (tax_classification IS NULL OR tax_classification IN
                           ('INDIVIDUAL', 'C_CORPORATION', 'S_CORPORATION', 'PARTNERSHIP', 'TRUST_ESTATE', 'LLC_C', 'LLC_S', 'LLC_P', 'OTHER')),
    exempt_payee_code  TEXT    CHECK (exempt_payee_code IS NULL OR length(exempt_payee_code) BETWEEN 1 AND 2),
    received_on        TEXT    CHECK (received_on IS NULL OR date(received_on) = received_on)
) STRICT;
