-- The pricing commitments (D72): when the paid period started, for the
-- refund's amount; price changes, which hold existing subscribers for 12
-- months; and the outbox of reminders and notices, sent once an email
-- provider exists (D32). The outbox names an account and the dates and
-- amounts a message needs, never an email address. Never edit this file
-- once merged.

ALTER TABLE subscriptions ADD COLUMN period_start timestamptz;

CREATE TABLE price_changes (
    id          uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    provider    text NOT NULL CHECK (provider IN ('stripe', 'app_store', 'play')),
    plan        text NOT NULL CHECK (plan IN ('premium', 'family', 'teams')),
    price_ref   text NOT NULL CHECK (price_ref <> '' AND octet_length(price_ref) <= 200),
    price_minor bigint NOT NULL CHECK (price_minor >= 0),
    currency    text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    held_until  timestamptz NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX price_changes_plan ON price_changes (provider, plan, created_at);

CREATE TABLE billing_outbox (
    id           uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id   uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    kind         text NOT NULL
                 CHECK (kind IN ('renewal_reminder', 'price_change', 'price_change_reminder')),
    provider     text NOT NULL,
    provider_ref text NOT NULL,
    -- The charge a reminder is about, or when a new price applies.
    about        timestamptz NOT NULL,
    price_minor  bigint NOT NULL CHECK (price_minor >= 0),
    currency     text NOT NULL CHECK (currency ~ '^[A-Z]{3}$'),
    send_after   timestamptz NOT NULL,
    sent_at      timestamptz,
    created_at   timestamptz NOT NULL DEFAULT now(),
    FOREIGN KEY (provider, provider_ref)
        REFERENCES subscriptions (provider, provider_ref) ON DELETE CASCADE,
    UNIQUE (kind, provider, provider_ref, about)
);
CREATE INDEX billing_outbox_due ON billing_outbox (send_after) WHERE sent_at IS NULL;
