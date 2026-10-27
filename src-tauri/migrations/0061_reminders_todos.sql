-- To Do list and Reminders.
--
-- todos: tasks with a title, note and optional due date, about at most one customer, vendor or
-- employee and optionally one invoice or bill, assigned to a user when the file has users. Nothing
-- here touches the ledger.
--
-- reminder_settings: which reminder categories each user sees and how far ahead they look, as JSON
-- the reminders module validates. user_id 0 holds the settings of a file without users.

CREATE TABLE todos (
    id                  INTEGER PRIMARY KEY,
    title               TEXT    NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 200),
    note                TEXT    NOT NULL DEFAULT '' CHECK (length(note) <= 4000),
    due_date            TEXT    CHECK (due_date IS NULL OR date(due_date) = due_date),
    customer_id         INTEGER REFERENCES customers (id) ON DELETE SET NULL,
    vendor_id           INTEGER REFERENCES vendors (id) ON DELETE SET NULL,
    employee_id         INTEGER REFERENCES employees (id) ON DELETE SET NULL,
    link_kind           TEXT    CHECK (link_kind IN ('INVOICE', 'BILL')),
    link_id             INTEGER,
    assigned_user_id    INTEGER REFERENCES users (id) ON DELETE SET NULL,
    created_by_user_id  INTEGER REFERENCES users (id) ON DELETE SET NULL,
    is_done             INTEGER NOT NULL DEFAULT 0 CHECK (is_done IN (0, 1)),
    done_at             TEXT,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CHECK ((link_kind IS NULL) = (link_id IS NULL)),
    CHECK ((customer_id IS NOT NULL) + (vendor_id IS NOT NULL) + (employee_id IS NOT NULL) <= 1),
    CHECK ((is_done = 1) = (done_at IS NOT NULL))
) STRICT;
CREATE INDEX ix_todos_open_due ON todos (is_done, due_date);
CREATE INDEX ix_todos_customer ON todos (customer_id) WHERE customer_id IS NOT NULL;
CREATE INDEX ix_todos_vendor ON todos (vendor_id) WHERE vendor_id IS NOT NULL;

CREATE TABLE reminder_settings (
    user_id     INTEGER PRIMARY KEY CHECK (user_id >= 0),
    settings    TEXT    NOT NULL CHECK (json_valid(settings)),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
