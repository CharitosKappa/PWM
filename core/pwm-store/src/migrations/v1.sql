-- Schema v1 (decision D20). Shipped migrations are never edited; a change is
-- a new file plus a frozen fixture in tests/fixtures.

CREATE TABLE account (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    account_id BLOB NOT NULL CHECK (length(account_id) = 16),
    salt BLOB NOT NULL CHECK (length(salt) = 16),
    kdf_memory_kib INTEGER NOT NULL,
    kdf_iterations INTEGER NOT NULL,
    kdf_parallelism INTEGER NOT NULL,
    sealed_keyset BLOB NOT NULL
) STRICT;

CREATE TABLE vaults (
    vault_id BLOB PRIMARY KEY CHECK (length(vault_id) = 16),
    wrapped_key BLOB NOT NULL,
    sync_cursor INTEGER NOT NULL CHECK (sync_cursor >= 0)
) STRICT, WITHOUT ROWID;

CREATE TABLE items (
    vault_id BLOB NOT NULL REFERENCES vaults (vault_id) ON DELETE CASCADE,
    item_id BLOB NOT NULL CHECK (length(item_id) = 16),
    revision INTEGER NOT NULL CHECK (revision >= 0),
    envelope BLOB NOT NULL,
    state INTEGER NOT NULL CHECK (state IN (0, 1, 2)),
    PRIMARY KEY (vault_id, item_id)
) STRICT, WITHOUT ROWID;

CREATE INDEX items_pending ON items (state) WHERE state <> 0;
