-- Two-step sign-in on this computer (gap R29, see `mfa/device.rs` and docs/plans/passkeys.md).
--
-- Beside the authenticator app (migration 0161), a user may let one computer sign them in with
-- Touch ID or Windows Hello. The computer keeps a random secret in its own credential store (the
-- macOS Keychain or Windows Credential Manager), never in this file; this file keeps only a salted
-- SHA-256 hash of it, so a copy of the file can't sign anyone in on another computer. The
-- authenticator app stays: a computer is only added beside it, and goes with it.

CREATE TABLE user_device_factors (
    id            INTEGER PRIMARY KEY,
    user_id       INTEGER NOT NULL REFERENCES users (id),
    -- Names the secret in the computer's credential store; random, 16 bytes.
    credential_id BLOB    NOT NULL UNIQUE CHECK (length(credential_id) = 16),
    -- What the user sees in the list: the computer's name and the kind of check.
    label         TEXT    NOT NULL CHECK (length(label) BETWEEN 1 AND 200),
    kind          TEXT    NOT NULL CHECK (kind IN ('TOUCH_ID', 'WINDOWS_HELLO')),
    secret_hash   BLOB    NOT NULL CHECK (length(secret_hash) = 32),
    enrolled_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    last_used_at  TEXT
) STRICT;
CREATE INDEX idx_user_device_factors_user ON user_device_factors (user_id);

-- A computer is added beside an authenticator app, never instead of one: the app is what works on
-- every other computer and what the backup codes belong to.
CREATE TRIGGER trg_user_device_factors_need_app BEFORE INSERT ON user_device_factors
WHEN NOT EXISTS (SELECT 1 FROM user_factors WHERE user_id = NEW.user_id)
BEGIN
    SELECT RAISE(ABORT, 'MFA_DEVICE_NEEDS_APP: set up an authenticator app first');
END;

-- Whatever removes the authenticator app (turning it off, an Admin's reset, a password reset, the
-- recovery key, the company turning two-step sign-in off, removing the user) removes the computers.
CREATE TRIGGER trg_user_factors_delete_devices AFTER DELETE ON user_factors
BEGIN
    DELETE FROM user_device_factors WHERE user_id = OLD.user_id;
END;
