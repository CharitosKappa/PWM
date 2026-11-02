-- Restore detection (D30). One row holding a random epoch. After a database
-- is restored from a backup, `pwm-server mark-restored` replaces it; clients
-- see the new value on `GET /v1/vaults` and pull everything again, uploading
-- what the backup lost. Never edit this file once merged.

CREATE TABLE server_meta (
    singleton boolean PRIMARY KEY DEFAULT true CHECK (singleton),
    epoch     uuid NOT NULL DEFAULT gen_random_uuid()
);

INSERT INTO server_meta DEFAULT VALUES;
