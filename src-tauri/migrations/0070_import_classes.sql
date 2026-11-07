-- Classes in migrations: an imported company keeps its class list and every line keeps its class.
--
-- `ledger_import_classes` stages the file's class list (full "Parent:Child" names), and
-- `ledger_import_lines.class_name` the class of each posting line. When a batch posts, each class
-- is matched to an existing class by name or created, and `class_id` records which one it became;
-- `created_by_import` marks the ones the import created, so a reverse can make them inactive.

ALTER TABLE ledger_import_lines ADD COLUMN class_name TEXT;

CREATE TABLE ledger_import_classes (
    batch_id            INTEGER NOT NULL,
    full_name           TEXT    NOT NULL COLLATE NOCASE CHECK (length(trim(full_name)) > 0),
    is_active           INTEGER NOT NULL CHECK (is_active IN (0, 1)),
    class_id            INTEGER REFERENCES classes (id) ON DELETE SET NULL,
    created_by_import   INTEGER NOT NULL DEFAULT 0 CHECK (created_by_import IN (0, 1)),
    PRIMARY KEY (batch_id, full_name)
) STRICT;
