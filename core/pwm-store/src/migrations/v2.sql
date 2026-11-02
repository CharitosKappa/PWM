-- Schema v2 (decision D30): the server epoch last seen, so a device can tell
-- when the server was restored from a backup. Shipped migrations are never
-- edited; a change is a new file plus a frozen fixture in tests/fixtures.

CREATE TABLE sync_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    server_epoch BLOB NOT NULL CHECK (length(server_epoch) = 16)
) STRICT;
