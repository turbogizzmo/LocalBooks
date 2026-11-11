-- R8: backup safety (`backup_safety.rs`). Each time a backup of these books is saved off the disk
-- the company file is on (another drive, a network share or a synced folder, by Back up to… or the
-- offsite backup), or a backup of them passes a test restore, a row records when, how and where.
-- The Backup safety reminder and screen read the newest row of each kind. Nothing here leaves the
-- computer: `location` is the folder or file the user chose, kept in their own company file.
CREATE TABLE backup_safety_events (
    id          INTEGER PRIMARY KEY,
    kind        TEXT    NOT NULL CHECK (kind IN ('OFF_DISK_COPY', 'TEST_RESTORE')),
    -- BACK_UP_TO (Back up to…), OFFSITE (the offsite backup), LOCAL (a backup file tested here),
    -- or EARLIER (carried over from before this table, without a location).
    source      TEXT    NOT NULL CHECK (source IN ('BACK_UP_TO', 'OFFSITE', 'LOCAL', 'EARLIER')),
    at          TEXT    NOT NULL CHECK (length(at) >= 19),
    location    TEXT    CHECK (location IS NULL OR length(location) <= 4096),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE INDEX idx_backup_safety_events_kind_at ON backup_safety_events (kind, at);

-- The times recorded before this table (in preferences, and the offsite backup's last test
-- restore) carry over. The offsite backup's last copy does not: its folder may be on this same
-- disk, and its next daily copy is recorded here when it is not.
INSERT INTO backup_safety_events (kind, source, at)
SELECT 'OFF_DISK_COPY', 'EARLIER', value FROM system_preferences WHERE key = 'backup_last_off_disk_at' AND length(value) >= 19;
INSERT INTO backup_safety_events (kind, source, at)
SELECT 'TEST_RESTORE', 'EARLIER', value FROM system_preferences WHERE key = 'backup_last_test_restore_at' AND length(value) >= 19;
INSERT INTO backup_safety_events (kind, source, at, location)
SELECT 'TEST_RESTORE', 'OFFSITE', last_test_restore_at, folder FROM offsite_backup WHERE last_test_restore_at IS NOT NULL AND length(last_test_restore_at) >= 19;
DELETE FROM system_preferences WHERE key IN ('backup_last_off_disk_at', 'backup_last_test_restore_at');
