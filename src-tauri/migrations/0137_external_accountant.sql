-- External Accountant users (gaps row R25, see `users.rs` and `permissions.rs`).
--
-- An External Accountant works like a Standard user, may post on or before the closing date, and
-- never sees sensitive data: the role can't hold the data key. The users table is rebuilt to allow
-- the role (a CHECK constraint can't be changed in place); every row is copied as it is, and the
-- triggers of migration 0043 are made again.

CREATE TABLE users_new (
    id                   INTEGER PRIMARY KEY,
    username             TEXT    NOT NULL COLLATE NOCASE UNIQUE
                                 CHECK (length(trim(username)) BETWEEN 1 AND 50 AND username = trim(username)),
    display_name         TEXT    NOT NULL CHECK (length(trim(display_name)) > 0),
    role                 TEXT    NOT NULL CHECK (role IN ('ADMIN', 'STANDARD', 'VIEW_ONLY', 'EXTERNAL_ACCOUNTANT')),
    -- An Admin always may: they hold the data key to grant it to others. An External Accountant never may.
    can_see_sensitive    INTEGER NOT NULL DEFAULT 0 CHECK (can_see_sensitive IN (0, 1)
                                 AND (role <> 'ADMIN' OR can_see_sensitive = 1)
                                 AND (role <> 'EXTERNAL_ACCOUNTANT' OR can_see_sensitive = 0)),
    is_active            INTEGER NOT NULL DEFAULT 1 CHECK (is_active IN (0, 1)),
    -- `argon2id$<memory KiB>$<iterations>$<parallelism>$<salt>$<hash>`, base64 without padding.
    password_verifier    TEXT    NOT NULL CHECK (password_verifier LIKE 'argon2id$%'),
    -- Set when an Admin chose the password (a new user or a reset): the user picks their own next.
    must_change_password INTEGER NOT NULL DEFAULT 0 CHECK (must_change_password IN (0, 1)),
    last_signed_in_at    TEXT,
    created_at           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

INSERT INTO users_new (id, username, display_name, role, can_see_sensitive, is_active, password_verifier,
                       must_change_password, last_signed_in_at, created_at, updated_at)
SELECT id, username, display_name, role, can_see_sensitive, is_active, password_verifier,
       must_change_password, last_signed_in_at, created_at, updated_at
FROM users ORDER BY id;

-- key_slots' trigger names the users table; it goes while the table is swapped.
DROP TRIGGER trg_key_slots_user_exists;
DROP TABLE users;
ALTER TABLE users_new RENAME TO users;

CREATE TRIGGER trg_key_slots_user_exists BEFORE INSERT ON key_slots
WHEN NEW.user_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM users WHERE id = NEW.user_id)
BEGIN
    SELECT RAISE(ABORT, 'KEY_SLOT_NO_USER: a key slot needs an existing user');
END;

CREATE TRIGGER trg_users_no_delete BEFORE DELETE ON users
WHEN (SELECT value FROM user_reset_in_progress) IS NULL
BEGIN
    SELECT RAISE(ABORT, 'USER_NO_DELETE: users are turned off, not deleted');
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

-- An External Accountant may post on or before the closing date. `closing::check_open` sets this
-- one-row flag inside the write transaction that does so, and `ledger::commit_write` clears it
-- before the commit, so the closed-period backstop still holds for every other write.
CREATE TABLE closing_exception_in_progress (
    value INTEGER CHECK (value IS NULL OR value = 1)
) STRICT;
INSERT INTO closing_exception_in_progress (value) VALUES (NULL);

DROP TRIGGER trg_je_closed_period;
CREATE TRIGGER trg_je_closed_period
BEFORE UPDATE OF status ON journal_entries
WHEN OLD.status = 'DRAFT' AND NEW.status = 'POSTED'
 AND NEW.entry_date <= (SELECT lock_date FROM closing_locks ORDER BY id DESC LIMIT 1)
 AND (SELECT value FROM closing_exception_in_progress) IS NULL
BEGIN
    SELECT RAISE(ABORT, 'PERIOD_CLOSED: Cannot write transaction: Target period is legally closed.');
END;
