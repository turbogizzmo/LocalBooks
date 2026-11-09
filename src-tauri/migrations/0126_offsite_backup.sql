-- R22: offsite backup (`offsite.rs`). One row when it is on: the folder copies go to, how many to
-- keep, and how the backup key was derived from the backup password (Argon2id salt and cost). The
-- key itself is kept in this computer's credential store, never here; `key_check` names it there
-- and confirms it is the key these settings made.
CREATE TABLE offsite_backup (
    id                   INTEGER PRIMARY KEY CHECK (id = 1),
    folder               TEXT    NOT NULL CHECK (length(folder) BETWEEN 1 AND 4096),
    keep                 INTEGER NOT NULL DEFAULT 10 CHECK (keep BETWEEN 1 AND 100),
    salt                 TEXT    NOT NULL,
    kdf_memory_kib       INTEGER NOT NULL CHECK (kdf_memory_kib BETWEEN 8 AND 1048576),
    kdf_iterations       INTEGER NOT NULL CHECK (kdf_iterations BETWEEN 1 AND 16),
    kdf_parallelism      INTEGER NOT NULL CHECK (kdf_parallelism BETWEEN 1 AND 8),
    key_check            TEXT    NOT NULL CHECK (length(key_check) = 32),
    last_backup_at       TEXT,
    last_backup_file     TEXT,
    last_error           TEXT,
    last_test_restore_at TEXT
) STRICT;
