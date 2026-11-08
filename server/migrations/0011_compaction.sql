-- Tombstone compaction (D76 §2). Tombstones older than the horizon are
-- deleted; `purged_seq` is the highest `seq` deleted so far in the vault.
-- A delta sync whose `since` is below it may have missed a delete, so it
-- answers 410 `resync_required`. Never edit this file once merged.

ALTER TABLE vaults ADD COLUMN purged_seq bigint NOT NULL DEFAULT 0 CHECK (purged_seq >= 0);

CREATE INDEX tombstones_deleted_at ON tombstones (deleted_at);
