-- Mileage tracking (see `mileage.rs`): vehicles, standard mileage rates with effective dates, and
-- trips. Trips never post to the ledger; they are records. A billable trip can be billed on an
-- invoice, which posts like any other invoice, and the trip remembers that invoice.
--
-- Miles and odometer readings are integers scaled by 10,000 (12.5 miles → 125_000), like invoice
-- quantities. A mileage rate is dollars per mile scaled by 10,000 (0.725 → 7_250), because standard
-- rates have fractions of a cent. Rates are entered by the user; none are built in.

CREATE TABLE vehicles (
    id           INTEGER PRIMARY KEY,
    name         TEXT    NOT NULL UNIQUE COLLATE NOCASE CHECK (length(trim(name)) > 0 AND length(name) <= 100),
    description  TEXT,
    is_active    INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    created_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TABLE mileage_rates (
    id              INTEGER PRIMARY KEY,
    -- The rate applies to trips on or after this date, until the next rate's date.
    effective_date  TEXT    NOT NULL UNIQUE CHECK (date(effective_date) = effective_date),
    rate_e4         INTEGER NOT NULL CHECK (rate_e4 > 0 AND rate_e4 <= 1000000),
    note            TEXT
) STRICT;

CREATE TABLE mileage_trips (
    id                 INTEGER PRIMARY KEY,
    vehicle_id         INTEGER NOT NULL REFERENCES vehicles (id) ON DELETE RESTRICT,
    trip_date          TEXT    NOT NULL CHECK (date(trip_date) = trip_date),
    odometer_start_e4  INTEGER CHECK (odometer_start_e4 >= 0),
    odometer_end_e4    INTEGER,
    miles_e4           INTEGER NOT NULL CHECK (miles_e4 > 0),
    notes              TEXT,
    customer_id        INTEGER REFERENCES customers (id) ON DELETE RESTRICT,
    billable           INTEGER NOT NULL DEFAULT 0 CHECK (billable IN (0, 1)),
    -- The invoice the trip was billed on. A voided invoice frees the trip to be billed again.
    invoice_id         INTEGER REFERENCES invoices (id) ON DELETE RESTRICT,
    created_at         TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK ((odometer_start_e4 IS NULL) = (odometer_end_e4 IS NULL)),
    CHECK (odometer_start_e4 IS NULL OR odometer_end_e4 - odometer_start_e4 = miles_e4),
    CHECK (billable = 0 OR customer_id IS NOT NULL),
    CHECK (invoice_id IS NULL OR billable = 1)
) STRICT;

CREATE INDEX ix_mileage_trips_date     ON mileage_trips (trip_date);
CREATE INDEX ix_mileage_trips_vehicle  ON mileage_trips (vehicle_id);
CREATE INDEX ix_mileage_trips_customer ON mileage_trips (customer_id) WHERE customer_id IS NOT NULL;
CREATE INDEX ix_mileage_trips_invoice  ON mileage_trips (invoice_id) WHERE invoice_id IS NOT NULL;
