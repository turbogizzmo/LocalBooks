-- Memorized reports: a customized report (see `custom_reports.rs`) saved under a name to run again.
--
-- `settings` is the report screen's settings as JSON: the engine's `ReportSettings` plus the
-- screen's own fields, such as the report period it opens with. `preset` is the preset it was
-- customized from, if any. Saving never changes the books.

CREATE TABLE saved_reports (
    id          INTEGER PRIMARY KEY,
    name        TEXT    NOT NULL UNIQUE COLLATE NOCASE CHECK (length(trim(name)) > 0 AND length(name) <= 100),
    preset      TEXT,
    settings    TEXT    NOT NULL CHECK (json_valid(settings) AND json_type(settings) = 'object'),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
