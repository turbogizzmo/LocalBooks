-- T50: comments on report figures (`report_comments.rs`). The first comment on a report keeps a
-- copy of it as shown (columns and rows as JSON), so the figures commented on stay as they were
-- even after the books change; each comment points at one cell of that copy.
CREATE TABLE commented_reports (
    id            INTEGER PRIMARY KEY,
    title         TEXT    NOT NULL CHECK (length(trim(title)) BETWEEN 1 AND 200),
    subtitle      TEXT    NOT NULL DEFAULT '' CHECK (length(subtitle) <= 500),
    snapshot_json TEXT    NOT NULL CHECK (json_valid(snapshot_json)),
    created_by    TEXT    NOT NULL,
    created_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
) STRICT;

CREATE TABLE report_comments (
    id                  INTEGER PRIMARY KEY,
    commented_report_id INTEGER NOT NULL REFERENCES commented_reports (id) ON DELETE CASCADE,
    row_index           INTEGER NOT NULL CHECK (row_index >= 0),
    col_index           INTEGER NOT NULL CHECK (col_index >= 0),
    -- The figure as it was shown, and the line it is on, for the list of comments.
    figure              TEXT    NOT NULL,
    line_label          TEXT    NOT NULL DEFAULT '',
    body                TEXT    NOT NULL CHECK (length(trim(body)) BETWEEN 1 AND 2000),
    author              TEXT    NOT NULL,
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
) STRICT;
CREATE INDEX idx_report_comments_report ON report_comments (commented_report_id);
