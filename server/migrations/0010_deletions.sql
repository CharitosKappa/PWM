-- Account deletion (D74): what a deleted account leaves behind, and only for
-- a while. Neither table holds an email, a device or anything typed.
-- Never edit this file once merged.

-- The refresh tokens of a deleted account's sessions, as their SHA-256
-- hashes, each until it would have expired: a device that refreshes with one
-- is told `account_deleted` and wipes itself (§5).
CREATE TABLE deleted_sessions (
    refresh_hash bytea PRIMARY KEY CHECK (octet_length(refresh_hash) = 32),
    expires_at   timestamptz NOT NULL
);
CREATE INDEX deleted_sessions_expires_at ON deleted_sessions (expires_at);

-- Deleted accounts' random ids, kept 60 days, so `mark-restored` can delete
-- them again after a restore (§6). The deletions file outside the database
-- covers the deletions newer than the backup.
CREATE TABLE deleted_accounts (
    account_id uuid PRIMARY KEY,
    deleted_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX deleted_accounts_deleted_at ON deleted_accounts (deleted_at);
