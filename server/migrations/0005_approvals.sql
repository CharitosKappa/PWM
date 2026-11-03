-- Device approval requests (D46): a new device asks, a signed-in device of
-- the same account answers. Only public keys, a commitment, one ciphertext,
-- the platform and times. `account_id` is null for an unknown email (or
-- past the pending cap), so the answer reveals nothing. A request lives
-- 10 minutes. Never edit this file once merged.

CREATE TABLE approvals (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id   uuid REFERENCES accounts (id) ON DELETE CASCADE,
    platform     text NOT NULL
                 CHECK (platform IN ('ios', 'android', 'macos', 'windows', 'linux', 'extension')),
    commitment   bytea NOT NULL CHECK (octet_length(commitment) = 32),
    -- SHA-256 of the new device's poll token.
    poll_hash    bytea NOT NULL UNIQUE CHECK (octet_length(poll_hash) = 32),
    existing_pub bytea CHECK (octet_length(existing_pub) = 32),
    new_pub      bytea CHECK (octet_length(new_pub) = 32),
    -- The sealed Secret Key, until the new device has fetched it once.
    envelope     bytea CHECK (octet_length(envelope) BETWEEN 41 AND 256),
    state        text NOT NULL DEFAULT 'pending'
                 CHECK (state IN ('pending', 'accepted', 'revealed', 'completed', 'delivered', 'ended')),
    created_at   timestamptz NOT NULL DEFAULT now(),
    expires_at   timestamptz NOT NULL
);

CREATE INDEX approvals_account_id ON approvals (account_id);
CREATE INDEX approvals_expires_at ON approvals (expires_at);
