-- Schema v3 (decision D73): the account's plan as the server last said, and
-- when it stops counting if nothing renews (Unix ms; NULL when it has no
-- end). Shipped migrations are never edited.

CREATE TABLE plan (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    plan TEXT NOT NULL CHECK (length(plan) BETWEEN 1 AND 32),
    until_ms INTEGER
) STRICT;
