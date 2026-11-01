-- Schema v1 (D25). The server stores ciphertext, sizes, ids and times only,
-- plus the account email. Never edit this file once merged: the server
-- refuses a database whose applied migrations differ. Add a new migration.
--
-- Ids are client-generated UUIDv4 (they are bound into AADs before upload).
-- `revision` is per item and chosen by the client; `seq` is a per-vault
-- change counter assigned by the server for delta sync. Envelopes (D10) are
-- at least 41 bytes.

CREATE TABLE accounts (
    id            uuid PRIMARY KEY,
    -- Trimmed and lowercased by the server before it is stored.
    email         text NOT NULL UNIQUE
                  CHECK (email = btrim(email) AND position('@' IN email) > 1),
    kdf_m         integer NOT NULL CHECK (kdf_m > 0),
    kdf_t         integer NOT NULL CHECK (kdf_t > 0),
    kdf_p         integer NOT NULL CHECK (kdf_p > 0),
    salt_pw       bytea NOT NULL CHECK (octet_length(salt_pw) = 16),
    sealed_keyset bytea NOT NULL CHECK (octet_length(sealed_keyset) >= 41),
    x25519_pub    bytea NOT NULL CHECK (octet_length(x25519_pub) = 32),
    ed25519_pub   bytea NOT NULL CHECK (octet_length(ed25519_pub) = 32),
    -- OPAQUE registration record, set when registration finishes (M3.2).
    opaque_record bytea,
    created_at    timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE devices (
    id           uuid PRIMARY KEY,
    account_id   uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    platform     text NOT NULL
                 CHECK (platform IN ('ios', 'android', 'macos', 'windows', 'linux', 'extension')),
    -- Device name sealed by the client (format defined in M3.2).
    name_sealed  bytea CHECK (octet_length(name_sealed) >= 41),
    created_at   timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now(),
    revoked_at   timestamptz
);
CREATE INDEX devices_account_id ON devices (account_id);

CREATE TABLE vaults (
    id         uuid PRIMARY KEY,
    -- The account a vault belongs to, so deleting it deletes the vault.
    owner_id   uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    -- Last change counter handed out in this vault (0 = no changes yet).
    seq        bigint NOT NULL DEFAULT 0 CHECK (seq >= 0),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX vaults_owner_id ON vaults (owner_id);

-- One row per member of a vault, holding that member's copy of the vault key.
CREATE TABLE vault_keys (
    vault_id    uuid NOT NULL REFERENCES vaults (id) ON DELETE CASCADE,
    account_id  uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    role        text NOT NULL CHECK (role IN ('owner', 'editor', 'viewer')),
    -- 'own': sealed under the member's sym_key (AAD pwm/vault-key).
    -- 'share': the D17 vault-share wire from `shared_by`.
    wrap        text NOT NULL CHECK (wrap IN ('own', 'share')),
    wrapped_key bytea NOT NULL CHECK (octet_length(wrapped_key) >= 41),
    -- Null for 'own', and for a 'share' whose sharer deleted their account.
    shared_by   uuid REFERENCES accounts (id) ON DELETE SET NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (vault_id, account_id),
    CHECK (wrap = 'share' OR shared_by IS NULL)
);
CREATE INDEX vault_keys_account_id ON vault_keys (account_id);

CREATE TABLE items (
    vault_id   uuid NOT NULL REFERENCES vaults (id) ON DELETE CASCADE,
    id         uuid NOT NULL,
    revision   bigint NOT NULL CHECK (revision >= 1),
    seq        bigint NOT NULL CHECK (seq >= 1),
    -- The item envelope (D18).
    blob       bytea NOT NULL CHECK (octet_length(blob) >= 41),
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (vault_id, id)
);
CREATE UNIQUE INDEX items_vault_seq ON items (vault_id, seq);

CREATE TABLE tombstones (
    vault_id   uuid NOT NULL REFERENCES vaults (id) ON DELETE CASCADE,
    item_id    uuid NOT NULL,
    -- The revision the delete was made against.
    revision   bigint NOT NULL CHECK (revision >= 1),
    seq        bigint NOT NULL CHECK (seq >= 1),
    deleted_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (vault_id, item_id)
);
CREATE UNIQUE INDEX tombstones_vault_seq ON tombstones (vault_id, seq);

-- One-time links to people who are not users. The link key stays in the URL
-- fragment and never reaches the server. Revoking deletes the row.
CREATE TABLE shares (
    id         uuid PRIMARY KEY,
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    blob       bytea NOT NULL CHECK (octet_length(blob) >= 41),
    expires_at timestamptz NOT NULL,
    max_views  integer CHECK (max_views >= 1),
    views      integer NOT NULL DEFAULT 0
               CHECK (views >= 0 AND (max_views IS NULL OR views <= max_views)),
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX shares_account_created ON shares (account_id, created_at);
CREATE INDEX shares_expires_at ON shares (expires_at);

-- Paid plans only: no row means Free. Provider-neutral while D6 is open.
CREATE TABLE subscriptions (
    account_id           uuid PRIMARY KEY REFERENCES accounts (id) ON DELETE CASCADE,
    plan                 text NOT NULL CHECK (plan IN ('premium', 'family', 'teams')),
    status               text NOT NULL
                         CHECK (status IN ('active', 'past_due', 'canceled', 'expired')),
    provider             text NOT NULL CHECK (provider IN ('stripe', 'app_store', 'play')),
    provider_ref         text NOT NULL CHECK (provider_ref <> ''),
    price_minor          bigint NOT NULL CHECK (price_minor >= 0),
    currency             text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    current_period_end   timestamptz NOT NULL,
    -- PRICING.md §1: the current price holds until this time.
    price_locked_until   timestamptz,
    cancel_at_period_end boolean NOT NULL DEFAULT false,
    created_at           timestamptz NOT NULL DEFAULT now(),
    updated_at           timestamptz NOT NULL DEFAULT now(),
    UNIQUE (provider, provider_ref)
);
