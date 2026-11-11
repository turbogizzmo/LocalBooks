-- Imports: a desktop company file's item groups (a named set of items, with quantities, that a
-- sales form expands into its lines), staged with its lists and brought in as LocalBooks groups.
-- `members` is a JSON array of [source item id, quantity × 10,000] in the order the group lists
-- them; the ids are the file's own (`ledger_import_items.source_id`).
CREATE TABLE ledger_import_item_groups (
    batch_id          INTEGER NOT NULL,
    source_id         INTEGER NOT NULL,
    name              TEXT    NOT NULL,
    description       TEXT,
    print_items       INTEGER NOT NULL CHECK (print_items IN (0, 1)),
    is_active         INTEGER NOT NULL CHECK (is_active IN (0, 1)),
    members           TEXT    NOT NULL CHECK (json_valid(members)),
    -- The group in LocalBooks once lists are added, and whether the import created it.
    item_group_id     INTEGER,
    created_by_import INTEGER NOT NULL DEFAULT 0 CHECK (created_by_import IN (0, 1)),
    PRIMARY KEY (batch_id, source_id)
) STRICT;
