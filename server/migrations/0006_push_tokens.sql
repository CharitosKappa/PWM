-- Push tokens (D50 §3): one per device, for a content-free "your vault
-- changed" push and the fixed-text approval notice. The token is the
-- device's APNs token as bytes; it is never logged. Signing out, revoking
-- or removing the device deletes the row. Never edit this file once merged.

CREATE TABLE push_tokens (
    device_id   uuid PRIMARY KEY REFERENCES devices (id) ON DELETE CASCADE,
    app         text NOT NULL CHECK (app IN ('ios', 'macos')),
    token       bytea NOT NULL UNIQUE CHECK (octet_length(token) BETWEEN 32 AND 100),
    environment text NOT NULL CHECK (environment IN ('sandbox', 'production')),
    updated_at  timestamptz NOT NULL DEFAULT now()
);
