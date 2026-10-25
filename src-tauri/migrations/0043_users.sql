-- Company-file users (see `users.rs`).
--
-- A company file with no users opens without signing in, as before. Turning users on creates the
-- first Admin together with the data key of migration 0042; from then on the file asks who is
-- signing in. Each user has a password verifier (Argon2id) for signing in; users who may see
-- sensitive data also have a key slot opened by the same password.
--
-- Users are never deleted, only turned off, so the audit trail can always name who did what.

CREATE TABLE users (
    id                   INTEGER PRIMARY KEY,
    username             TEXT    NOT NULL COLLATE NOCASE UNIQUE
                                 CHECK (length(trim(username)) BETWEEN 1 AND 50 AND username = trim(username)),
    display_name         TEXT    NOT NULL CHECK (length(trim(display_name)) > 0),
    role                 TEXT    NOT NULL CHECK (role IN ('ADMIN', 'STANDARD', 'VIEW_ONLY')),
    -- An Admin always may: they hold the data key to grant it to others.
    can_see_sensitive    INTEGER NOT NULL DEFAULT 0 CHECK (can_see_sensitive IN (0, 1) AND (role <> 'ADMIN' OR can_see_sensitive = 1)),
    is_active            INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    -- `argon2id$<memory KiB>$<iterations>$<parallelism>$<salt>$<hash>`, base64 without padding.
    password_verifier    TEXT    NOT NULL CHECK (password_verifier LIKE 'argon2id$%'),
    -- Set when an Admin chose the password (a new user or a reset): the user picks their own next.
    must_change_password INTEGER NOT NULL DEFAULT 0 CHECK (must_change_password IN (0, 1)),
    last_signed_in_at    TEXT,
    created_at           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- Removing every user (the last resort when no Admin password or recovery key is left) is the one
-- delete allowed; `users::remove_all_users` sets this row for the duration of its transaction.
CREATE TABLE user_reset_in_progress (
    value INTEGER
) STRICT;
INSERT INTO user_reset_in_progress (value) VALUES (NULL);

CREATE TRIGGER trg_users_no_delete BEFORE DELETE ON users
WHEN (SELECT value FROM user_reset_in_progress) IS NULL
BEGIN
    SELECT RAISE(ABORT, 'USER_NO_DELETE: users are turned off, not deleted');
END;

-- A user's key slot must belong to a user that exists, and goes when they lose access.
CREATE TRIGGER trg_key_slots_user_exists BEFORE INSERT ON key_slots
WHEN NEW.user_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM users WHERE id = NEW.user_id)
BEGIN
    SELECT RAISE(ABORT, 'KEY_SLOT_NO_USER: a key slot needs an existing user');
END;

CREATE TRIGGER trg_users_sensitive_revoked AFTER UPDATE OF can_see_sensitive, is_active ON users
WHEN NEW.can_see_sensitive = 0 OR NEW.is_active = 0
BEGIN
    DELETE FROM key_slots WHERE user_id = NEW.id;
END;

-- At least one active Admin remains while there are users at all.
CREATE TRIGGER trg_users_keep_an_admin AFTER UPDATE OF role, is_active ON users
WHEN OLD.role = 'ADMIN' AND OLD.is_active = 1 AND (NEW.role <> 'ADMIN' OR NEW.is_active = 0)
     AND NOT EXISTS (SELECT 1 FROM users WHERE role = 'ADMIN' AND is_active = 1)
BEGIN
    SELECT RAISE(ABORT, 'USER_LAST_ADMIN: keep at least one active Admin');
END;
