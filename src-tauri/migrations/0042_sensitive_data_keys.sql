-- Keys for sensitive fields such as vendor tax IDs (see `crypto.rs`).
--
-- A company file that protects sensitive data has one random data key, which encrypts those fields
-- with AES-256-GCM. The data key itself is never stored: each key slot holds it encrypted with a key
-- derived (Argon2id) from a user's password or from the file's recovery key. The slots live in the
-- file, so it can be moved, copied or restored on another computer and still be unlocked.
--
-- Nothing here is created until sensitive-data protection is turned on; until then both tables are
-- empty and the company file works as before.

CREATE TABLE data_key (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    -- Binds slots and encrypted values to this key, so neither can be copied in from another file.
    key_id      TEXT    NOT NULL CHECK (length(key_id) = 32 AND key_id NOT GLOB '*[^0-9a-f]*'),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE TRIGGER trg_data_key_no_update BEFORE UPDATE ON data_key
BEGIN
    SELECT RAISE(ABORT, 'DATA_KEY_IMMUTABLE: the data key cannot be changed');
END;

CREATE TABLE key_slots (
    id          INTEGER PRIMARY KEY,
    kind        TEXT    NOT NULL CHECK (kind IN ('PASSWORD', 'RECOVERY')),
    -- The user whose password opens a PASSWORD slot (the users table arrives with sign-in).
    user_id     INTEGER CHECK ((kind = 'PASSWORD') = (user_id IS NOT NULL)),
    -- Argon2id cost, kept per slot so it can be raised later without breaking older slots.
    memory_kib  INTEGER NOT NULL CHECK (memory_kib >= 8),
    iterations  INTEGER NOT NULL CHECK (iterations >= 1),
    parallelism INTEGER NOT NULL CHECK (parallelism >= 1),
    salt        BLOB    NOT NULL CHECK (length(salt) = 16),
    nonce       BLOB    NOT NULL CHECK (length(nonce) = 12),
    -- The 32-byte data key and its 16-byte authentication tag.
    wrapped_key BLOB    NOT NULL CHECK (length(wrapped_key) = 48),
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

CREATE UNIQUE INDEX ux_key_slots_recovery ON key_slots (kind) WHERE kind = 'RECOVERY';
CREATE UNIQUE INDEX ux_key_slots_user ON key_slots (user_id) WHERE user_id IS NOT NULL;

CREATE TRIGGER trg_key_slots_need_data_key BEFORE INSERT ON key_slots
WHEN NOT EXISTS (SELECT 1 FROM data_key)
BEGIN
    SELECT RAISE(ABORT, 'DATA_KEY_MISSING: create the data key before adding key slots');
END;
