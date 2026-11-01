-- Schema v2 (D26): sessions and login attempts. Never edit this file once
-- merged: add a new migration.

-- Every account registers with OPAQUE from M3.2 on.
ALTER TABLE accounts ALTER COLUMN opaque_record SET NOT NULL;

-- Lets a session reference a device of its own account only.
ALTER TABLE devices ADD CONSTRAINT devices_id_account_id_key UNIQUE (id, account_id);

-- One session per device. Only SHA-256 hashes of the tokens are stored.
CREATE TABLE sessions (
    id                 uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id         uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    device_id          uuid NOT NULL UNIQUE,
    access_hash        bytea NOT NULL UNIQUE CHECK (octet_length(access_hash) = 32),
    access_expires_at  timestamptz NOT NULL,
    refresh_hash       bytea NOT NULL UNIQUE CHECK (octet_length(refresh_hash) = 32),
    -- The refresh token this one replaced: presenting it again ends the session.
    prev_refresh_hash  bytea UNIQUE CHECK (octet_length(prev_refresh_hash) = 32),
    refresh_expires_at timestamptz NOT NULL,
    created_at         timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (device_id, account_id) REFERENCES devices (id, account_id) ON DELETE CASCADE
);
CREATE INDEX sessions_account_id ON sessions (account_id);

-- OPAQUE server state between login start and finish (60 s, single use).
-- `account_id` has no foreign key: unknown accounts get a fake attempt too.
CREATE TABLE login_attempts (
    id         uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id uuid NOT NULL,
    state      bytea NOT NULL,
    expires_at timestamptz NOT NULL
);
CREATE INDEX login_attempts_expires_at ON login_attempts (expires_at);
