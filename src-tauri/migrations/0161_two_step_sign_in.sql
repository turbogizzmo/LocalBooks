-- Two-step sign-in (gap R26, see `mfa.rs` and docs/plans/multi-factor-sign-in.md).
--
-- Optional and off by default, like whole-file encryption. An Admin turns it on for the company;
-- each user may then add an authenticator app (TOTP), and sign-in asks that user for a code. A
-- stricter setting makes Admins and External Accountants, or everyone, set one up.

-- The company's choice: OFF (default), OPTIONAL (anyone may set it up), ADMINS (Admins and External
-- Accountants must), ALL (everyone must). One row.
CREATE TABLE two_step_policy (
    id     INTEGER PRIMARY KEY CHECK (id = 1),
    policy TEXT    NOT NULL DEFAULT 'OFF' CHECK (policy IN ('OFF', 'OPTIONAL', 'ADMINS', 'ALL'))
) STRICT;
INSERT INTO two_step_policy (id, policy) VALUES (1, 'OFF');

-- One authenticator per user. The TOTP secret is never stored in clear: it is sealed with
-- AES-256-GCM under a key derived by Argon2id from the user's password (its own salt, cost stored
-- beside it, like `key_slots`), so the file without the password can't make codes.
CREATE TABLE user_factors (
    user_id       INTEGER PRIMARY KEY REFERENCES users (id),
    kind          TEXT    NOT NULL DEFAULT 'TOTP' CHECK (kind = 'TOTP'),
    memory_kib    INTEGER NOT NULL CHECK (memory_kib > 0),
    iterations    INTEGER NOT NULL CHECK (iterations > 0),
    parallelism   INTEGER NOT NULL CHECK (parallelism > 0),
    salt          BLOB    NOT NULL CHECK (length(salt) = 16),
    nonce         BLOB    NOT NULL CHECK (length(nonce) = 12),
    sealed_secret BLOB    NOT NULL CHECK (length(sealed_secret) > 16),
    -- The last 30-second step a code was accepted for: a code works once on this file.
    last_step     INTEGER NOT NULL DEFAULT 0 CHECK (last_step >= 0),
    enrolled_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- One-time backup codes, as salted SHA-256 hashes (each code carries 50 random bits, so a slow
-- hash adds nothing). A used code keeps its row with `used_at` set.
CREATE TABLE user_backup_codes (
    id        INTEGER PRIMARY KEY,
    user_id   INTEGER NOT NULL REFERENCES users (id),
    salt      BLOB    NOT NULL CHECK (length(salt) = 16),
    code_hash BLOB    NOT NULL CHECK (length(code_hash) = 32),
    used_at   TEXT
) STRICT;
CREATE INDEX idx_user_backup_codes_user ON user_backup_codes (user_id);

CREATE TRIGGER trg_user_factors_user_exists BEFORE INSERT ON user_factors
WHEN NOT EXISTS (SELECT 1 FROM users WHERE id = NEW.user_id)
BEGIN
    SELECT RAISE(ABORT, 'MFA_NO_USER: a sign-in factor needs an existing user');
END;

-- A factor only exists while the company has two-step sign-in on: turning it off removes them all.
CREATE TRIGGER trg_two_step_policy_off AFTER UPDATE OF policy ON two_step_policy
WHEN NEW.policy = 'OFF'
BEGIN
    DELETE FROM user_factors;
    DELETE FROM user_backup_codes;
END;

-- Removing every user (the last resort on the sign-in screen) removes their factors first.
CREATE TRIGGER trg_users_delete_factors BEFORE DELETE ON users
BEGIN
    DELETE FROM user_factors WHERE user_id = OLD.id;
    DELETE FROM user_backup_codes WHERE user_id = OLD.id;
END;
