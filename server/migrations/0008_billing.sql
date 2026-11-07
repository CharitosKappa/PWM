-- Billing (D6 §8): an account may hold more than one subscription (say one
-- from the App Store and one from Stripe, by mistake), so a subscription is
-- keyed by its provider's reference. `grace_until` bounds a failed renewal,
-- `last_charge_at` is for the refund window (PRICING.md §5), and `sandbox`
-- marks Apple's test purchases, which App Review makes against the
-- production server. Each account gets a random `billing_token` that the
-- apps pass to the stores, so a purchase is linked to its account without
-- trusting the device. Never edit this file once merged.

ALTER TABLE subscriptions DROP CONSTRAINT subscriptions_pkey;
ALTER TABLE subscriptions DROP CONSTRAINT subscriptions_provider_provider_ref_key;
ALTER TABLE subscriptions ADD PRIMARY KEY (provider, provider_ref);
CREATE INDEX subscriptions_account ON subscriptions (account_id);

ALTER TABLE subscriptions ADD COLUMN grace_until timestamptz;
ALTER TABLE subscriptions ADD COLUMN last_charge_at timestamptz;
ALTER TABLE subscriptions ADD COLUMN sandbox boolean NOT NULL DEFAULT false;
-- Play's purchase tokens are the longest references.
ALTER TABLE subscriptions ADD CONSTRAINT subscriptions_provider_ref_length_check
    CHECK (octet_length(provider_ref) <= 1024);

-- A volatile default gives every existing account its own token.
ALTER TABLE accounts ADD COLUMN billing_token uuid NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE accounts ADD CONSTRAINT accounts_billing_token_key UNIQUE (billing_token);
ALTER TABLE accounts ADD COLUMN stripe_customer text;
ALTER TABLE accounts ADD CONSTRAINT accounts_stripe_customer_key UNIQUE (stripe_customer);
ALTER TABLE accounts ADD CONSTRAINT accounts_stripe_customer_check
    CHECK (stripe_customer ~ '^cus_[A-Za-z0-9]{1,64}$');

-- The providers' notifications already handled, to drop repeats. Only the
-- provider's id for each one, never its body; kept 30 days.
CREATE TABLE billing_events (
    provider    text NOT NULL CHECK (provider IN ('stripe', 'app_store', 'play')),
    event_id    text NOT NULL CHECK (event_id <> '' AND octet_length(event_id) <= 200),
    received_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, event_id)
);
CREATE INDEX billing_events_received_at ON billing_events (received_at);
