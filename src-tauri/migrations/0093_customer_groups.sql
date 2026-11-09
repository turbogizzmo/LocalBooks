-- 0093_customer_groups.sql
-- Customer groups (gap T57): named groups of customers, picked by hand or kept up to date by rules
-- (open balance, customer type, terms, rep, state, ZIP code, tax code, credit limit, active status).
-- A group with rules (`rules_json`) works its members out from the customers' current details each
-- time it is read, so they change when a customer's details do; a group without rules has the
-- members listed in customer_group_members. A group can carry a statement reminder: every so many
-- days its customers' statements come up in Reminders until they are marked sent.

CREATE TABLE customer_groups (
    id                      INTEGER PRIMARY KEY,
    name                    TEXT    NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 60),
    rules_json              TEXT    CHECK (rules_json IS NULL OR json_valid(rules_json)),
    statement_every_days    INTEGER CHECK (statement_every_days IS NULL OR statement_every_days BETWEEN 1 AND 366),
    statement_last_sent     TEXT    CHECK (statement_last_sent IS NULL OR date(statement_last_sent) IS statement_last_sent),
    statement_open_only     INTEGER NOT NULL DEFAULT 0 CHECK (statement_open_only IN (0, 1)),
    statement_style_id      INTEGER REFERENCES form_styles (id) ON DELETE SET NULL,
    statement_message       TEXT    CHECK (statement_message IS NULL OR length(statement_message) <= 2000),
    created_at              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE UNIQUE INDEX ux_customer_groups_name ON customer_groups (name COLLATE NOCASE);

CREATE TABLE customer_group_members (
    group_id     INTEGER NOT NULL REFERENCES customer_groups (id) ON DELETE CASCADE,
    customer_id  INTEGER NOT NULL REFERENCES customers (id) ON DELETE CASCADE,
    PRIMARY KEY (group_id, customer_id)
) STRICT;
CREATE INDEX ix_customer_group_members_customer ON customer_group_members (customer_id);
