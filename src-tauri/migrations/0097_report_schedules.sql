-- 0097_report_schedules.sql
-- Scheduled reports (gap T60): chosen reports run daily, weekly, monthly or quarterly from a start
-- date (to an optional end date) and are emailed through the company's email server as PDF or
-- Excel attachments, or saved to a folder on this computer. Schedules run only while LocalBooks
-- is open; a run missed while it was closed happens once, the next time it opens.

CREATE TABLE report_schedules (
    id            INTEGER PRIMARY KEY,
    name          TEXT    NOT NULL CHECK (length(trim(name)) BETWEEN 1 AND 80),
    reports_json  TEXT    NOT NULL CHECK (json_valid(reports_json)),
    frequency     TEXT    NOT NULL CHECK (frequency IN ('DAILY', 'WEEKLY', 'MONTHLY', 'QUARTERLY')),
    start_date    TEXT    NOT NULL CHECK (date(start_date) IS start_date),
    end_date      TEXT    CHECK (end_date IS NULL OR date(end_date) IS end_date),
    next_run      TEXT    CHECK (next_run IS NULL OR date(next_run) IS next_run),
    destination   TEXT    NOT NULL CHECK (destination IN ('EMAIL', 'FOLDER')),
    recipients    TEXT    CHECK (recipients IS NULL OR length(recipients) <= 2000),
    folder        TEXT    CHECK (folder IS NULL OR length(folder) <= 4096),
    format        TEXT    NOT NULL DEFAULT 'PDF' CHECK (format IN ('PDF', 'XLSX')),
    is_active     INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    last_run_at   TEXT,
    last_result   TEXT,
    created_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
