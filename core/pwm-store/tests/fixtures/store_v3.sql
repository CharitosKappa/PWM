-- Frozen copy of a schema v3 cache (decisions D20, D30, D73). Never edit:
-- every later schema version must still open this file and read the rows below.
PRAGMA application_id = 1347898673; -- "PWM1"
PRAGMA user_version = 3;

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

CREATE TABLE sync_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    server_epoch BLOB NOT NULL CHECK (length(server_epoch) = 16)
) STRICT;

CREATE TABLE plan (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    plan TEXT NOT NULL CHECK (length(plan) BETWEEN 1 AND 32),
    until_ms INTEGER
) STRICT;

INSERT INTO vaults VALUES (
    X'B0B1B2B3B4B5B6B7B8B9BABBBCBDBEBF',
    X'01000000000000000000000000000000000000000000000000000000000000000000000000000000AA',
    9
);
INSERT INTO items VALUES (
    X'B0B1B2B3B4B5B6B7B8B9BABBBCBDBEBF',
    X'C0C1C2C3C4C5C6C7C8C9CACBCCCDCECF',
    3,
    X'01000000000000000000000000000000000000000000000000000000000000000000000000000000BB',
    0
);
INSERT INTO sync_meta VALUES (1, X'E0E1E2E3E4E5E6E7E8E9EAEBECEDEEEF');
INSERT INTO plan VALUES (1, 'premium', 1800000000000);
