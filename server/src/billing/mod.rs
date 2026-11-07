//! Plans and billing (D6): our own entitlement service over Stripe, the App
//! Store and Google Play. The server is the one source of an account's
//! plan. Every message from a provider is only a hint: what is stored comes
//! from asking the provider's own API over TLS (§3). A purchase is linked to
//! its account by the account's random `billing_token`, never by the device.
//!
//! Logs hold providers, counts and statuses only: never a token, key,
//! purchase token or subscription body.

mod apple;
mod commitments;
mod http;
mod play;
mod routes;
mod stripe;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use sqlx::{PgPool, Row};
use tokio::time::Instant;
use uuid::Uuid;

pub use apple::AppleConfig;
pub use commitments::{Mailer, OutboxMessage, record_price_change};
pub use play::{PLAY_SCOPE, PlayConfig};
pub(crate) use routes::routes;
pub use stripe::{STRIPE_VERSION, StripeConfig};

use crate::ServerError;
use http::Http;

/// When a subscription counts (D6 §4): active (with three days' slack for a
/// late renewal notice), canceled with its period not over, or past due
/// within its grace period. A past-due row without `grace_until` is one of
/// Stripe's, whose retry schedule is the grace.
pub(crate) const ENTITLED: &str = "((s.status = 'active' AND s.current_period_end > now() - interval '3 days') \
     OR (s.status = 'canceled' AND s.current_period_end > now()) \
     OR (s.status = 'past_due' AND (s.grace_until IS NULL OR s.grace_until > now())))";

/// How long after a charge a refund can be asked for (PRICING.md §5).
pub(crate) const REFUND_WINDOW: &str = "interval '30 days'";

/// The better plan first when an account holds two.
const RANK: &str = "CASE s.plan WHEN 'teams' THEN 3 WHEN 'family' THEN 2 ELSE 1 END";

/// Refetches started by providers' notifications, per minute (§3).
const HINTS_PER_MINUTE: u32 = 300;
/// How often every live subscription is read again (§3).
const DAILY: Duration = Duration::from_secs(24 * 3600);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Provider {
    Stripe,
    AppStore,
    Play,
}

impl Provider {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Stripe => "stripe",
            Self::AppStore => "app_store",
            Self::Play => "play",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "stripe" => Some(Self::Stripe),
            "app_store" => Some(Self::AppStore),
            "play" => Some(Self::Play),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Status {
    Active,
    PastDue,
    Expired,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::PastDue => "past_due",
            Self::Expired => "expired",
        }
    }
}

/// How a provider's subscription names its account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Link {
    /// The account's `billing_token`, passed at purchase.
    Token(Uuid),
    /// Stripe's customer, made for one account.
    StripeCustomer(String),
    None,
}

/// A subscription as its provider describes it right now.
#[derive(Debug, Clone)]
pub(crate) struct Snapshot {
    pub provider: Provider,
    pub provider_ref: String,
    pub link: Link,
    pub plan: &'static str,
    pub status: Status,
    pub price_minor: i64,
    pub currency: String,
    /// The start of the paid period, for the refund's amount (D72 §1).
    pub period_start_ms: Option<i64>,
    pub period_end_ms: i64,
    pub canceling: bool,
    pub grace_until_ms: Option<i64>,
    pub last_charge_ms: Option<i64>,
    pub sandbox: bool,
    /// Play: the purchase this one replaced (an upgrade or a resubscribe).
    pub replaces: Option<String>,
    /// Play: the product of a purchase still to acknowledge.
    pub acknowledge: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BillingError {
    /// Not set up here, or the provider can't be reached or failed.
    Unavailable,
    /// The provider doesn't know it, or it's for a product we don't sell.
    NotFound,
    /// It belongs to another account (§3).
    NotYours,
    /// Play: bought but not paid yet.
    Pending,
}

/// Plans and their product or price ids at one provider, from a setting
/// like `premium=price_123,family=price_456` (§9). A plan may name more than
/// one: the first is what new subscribers buy, and the others are older
/// prices still paid by subscribers a price change holds (D72 §2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Products(Vec<(&'static str, String)>);

impl Products {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let mut pairs = Vec::new();
        for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (plan, id) = part.split_once('=')?;
            let plan = match plan.trim() {
                "premium" => "premium",
                "family" => "family",
                "teams" => "teams",
                _ => return None,
            };
            let id = id.trim();
            let plain = !id.is_empty()
                && id.len() <= 200
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
            if !plain || pairs.iter().any(|(_, i)| i == id) {
                return None;
            }
            pairs.push((plan, id.to_owned()));
        }
        (!pairs.is_empty()).then_some(Self(pairs))
    }

    pub(crate) fn plan_for(&self, id: &str) -> Option<&'static str> {
        self.0.iter().find(|(_, i)| i == id).map(|(p, _)| *p)
    }

    pub(crate) fn id_for(&self, plan: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(p, _)| *p == plan)
            .map(|(_, i)| i.as_str())
    }
}

/// The billing settings (§7, §9). None of them set: billing is off and
/// every account is Premium, as on a self-hosted server.
#[derive(Debug, Default)]
pub struct BillingConfig {
    pub stripe: Option<StripeConfig>,
    pub apple: Option<AppleConfig>,
    pub play: Option<PlayConfig>,
}

impl BillingConfig {
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Self, ServerError> {
        Ok(Self {
            stripe: StripeConfig::from_vars(&var)?,
            apple: AppleConfig::from_vars(&var)?,
            play: PlayConfig::from_vars(&var)?,
        })
    }

    fn is_off(&self) -> bool {
        self.stripe.is_none() && self.apple.is_none() && self.play.is_none()
    }
}

struct Inner {
    stripe: Option<StripeConfig>,
    apple: Option<AppleConfig>,
    play: Option<PlayConfig>,
    http: Http,
    hints: Mutex<(Instant, u32)>,
}

/// The handle the routes use. Off, every account is Premium and the billing
/// routes answer `billing_unavailable`.
#[derive(Clone, Default)]
pub struct Billing {
    inner: Option<Arc<Inner>>,
    /// Sends the outbox (D72 §3); none until D32 picks a provider.
    mailer: Option<Arc<dyn Mailer>>,
}

impl Billing {
    pub fn off() -> Self {
        Self::default()
    }

    pub fn new(config: BillingConfig) -> Result<Self, ServerError> {
        if config.is_off() {
            return Ok(Self::off());
        }
        let http = Http::new().ok_or(ServerError::Config(
            "billing needs the system's root certificates",
        ))?;
        Ok(Self {
            mailer: None,
            inner: Some(Arc::new(Inner {
                stripe: config.stripe,
                apple: config.apple,
                play: config.play,
                http,
                hints: Mutex::new((Instant::now(), 0)),
            })),
        })
    }

    pub fn with_mailer(mut self, mailer: Arc<dyn Mailer>) -> Self {
        self.mailer = Some(mailer);
        self
    }

    /// The providers that sell plans here, for the apps to offer.
    pub(crate) fn sells(&self) -> Vec<&'static str> {
        let Some(inner) = &self.inner else {
            return Vec::new();
        };
        let mut sells = Vec::new();
        if inner.stripe.is_some() {
            sells.push("stripe");
        }
        if inner.apple.is_some() {
            sells.push("app_store");
        }
        if inner.play.is_some() {
            sells.push("play");
        }
        sells
    }

    fn inner(&self) -> Result<&Inner, BillingError> {
        self.inner.as_deref().ok_or(BillingError::Unavailable)
    }

    /// One more refetch a notification asked for, within the limit.
    fn take_hint(&self) -> bool {
        let Some(inner) = &self.inner else {
            return false;
        };
        let mut hints = inner.hints.lock().unwrap_or_else(|e| e.into_inner());
        if hints.0.elapsed() >= Duration::from_secs(60) {
            *hints = (Instant::now(), 0);
        }
        if hints.1 >= HINTS_PER_MINUTE {
            return false;
        }
        hints.1 += 1;
        true
    }

    /// Reads one subscription from its provider and stores it. `expect` is
    /// the account that says it bought it: another account's answers
    /// `NotYours`. Returns the account it belongs to, or `None` when it
    /// names no account here.
    pub(crate) async fn refresh(
        &self,
        pool: &PgPool,
        provider: Provider,
        provider_ref: &str,
        sandbox: Option<bool>,
        expect: Option<Uuid>,
    ) -> Result<Option<Uuid>, BillingError> {
        let inner = self.inner()?;
        let snapshot = match provider {
            Provider::Stripe => {
                let config = inner.stripe.as_ref().ok_or(BillingError::Unavailable)?;
                stripe::fetch(config, &inner.http, provider_ref).await?
            }
            Provider::AppStore => {
                let config = inner.apple.as_ref().ok_or(BillingError::Unavailable)?;
                apple::fetch(config, &inner.http, provider_ref, sandbox).await?
            }
            Provider::Play => {
                let config = inner.play.as_ref().ok_or(BillingError::Unavailable)?;
                play::fetch(config, &inner.http, provider_ref).await?
            }
        };
        let account = store(pool, &snapshot, expect).await?;
        if account.is_some()
            && snapshot.status != Status::Expired
            && let (Some(product), Some(config)) = (&snapshot.acknowledge, &inner.play)
        {
            // Google refunds a purchase not acknowledged within three days.
            play::acknowledge(config, &inner.http, &snapshot.provider_ref, product).await;
        }
        Ok(account)
    }

    /// Reads every subscription that isn't expired again, which catches a
    /// lost notification, and forgets notifications older than 30 days.
    pub async fn reconcile(&self, pool: &PgPool) -> Result<(), sqlx::Error> {
        if self.inner.is_none() {
            return Ok(());
        }
        let rows = sqlx::query(
            "SELECT provider, provider_ref, sandbox FROM subscriptions WHERE status <> 'expired'",
        )
        .fetch_all(pool)
        .await?;
        let (mut read, mut failed) = (0, 0);
        for row in rows {
            let Some(provider) = Provider::parse(row.get(0)) else {
                continue;
            };
            let reference: String = row.get(1);
            let sandbox: bool = row.get(2);
            match self
                .refresh(pool, provider, &reference, Some(sandbox), None)
                .await
            {
                Ok(_) => read += 1,
                // Gone at the provider: it ended.
                Err(BillingError::NotFound) => {
                    sqlx::query(
                        "UPDATE subscriptions SET status = 'expired', updated_at = now() \
                         WHERE provider = $1 AND provider_ref = $2",
                    )
                    .bind(provider.as_str())
                    .bind(&reference)
                    .execute(pool)
                    .await?;
                    read += 1;
                }
                Err(_) => failed += 1,
            }
        }
        sqlx::query("DELETE FROM billing_events WHERE received_at < now() - interval '30 days'")
            .execute(pool)
            .await?;
        tracing::info!("billing: daily pass read {read} subscriptions, {failed} failed");
        // The commitments (D72): reminders, the outbox, held prices.
        let scheduled = commitments::schedule_reminders(pool).await?;
        let (sent, waiting) = commitments::deliver_due(pool, self.mailer.as_ref()).await?;
        let moved = commitments::apply_price_moves(self, pool).await?;
        if self.mailer.is_none() && waiting > 0 {
            tracing::warn!(
                "billing: {waiting} reminders and notices are due, and no email provider is set (D32)"
            );
        }
        tracing::info!(
            "billing: {scheduled} reminders scheduled, {sent} sent, {waiting} not sent, {moved} prices moved"
        );
        Ok(())
    }

    /// Runs [`reconcile`](Self::reconcile) a minute after start, then daily.
    pub fn start_daily(&self, pool: PgPool) {
        if self.inner.is_none() {
            return;
        }
        let billing = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let mut every = tokio::time::interval(DAILY);
            loop {
                every.tick().await;
                if billing.reconcile(&pool).await.is_err() {
                    tracing::warn!("billing: the daily pass could not read the database");
                }
            }
        });
    }
}

/// Stores a snapshot under its account: the row's own if it has one, else
/// the one its link names. Returns that account, or `None` (nothing stored).
async fn store(
    pool: &PgPool,
    s: &Snapshot,
    expect: Option<Uuid>,
) -> Result<Option<Uuid>, BillingError> {
    let db = |_: sqlx::Error| BillingError::Unavailable;
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT account_id FROM subscriptions WHERE provider = $1 AND provider_ref = $2",
    )
    .bind(s.provider.as_str())
    .bind(&s.provider_ref)
    .fetch_optional(pool)
    .await
    .map_err(db)?;
    let linked: Option<Uuid> = match &s.link {
        Link::Token(token) => {
            sqlx::query_scalar("SELECT id FROM accounts WHERE billing_token = $1")
                .bind(token)
                .fetch_optional(pool)
                .await
                .map_err(db)?
        }
        Link::StripeCustomer(customer) => {
            sqlx::query_scalar("SELECT id FROM accounts WHERE stripe_customer = $1")
                .bind(customer)
                .fetch_optional(pool)
                .await
                .map_err(db)?
        }
        Link::None => None,
    };
    let account = existing.or(linked);
    if let Some(expected) = expect
        && account != Some(expected)
    {
        return Err(BillingError::NotYours);
    }
    let Some(account) = account else {
        return Ok(None);
    };
    if s.provider != Provider::Stripe {
        commitments::warn_on_held_price(pool, s).await;
    }
    let mut tx = pool.begin().await.map_err(db)?;
    // A provider that doesn't say when it charged (Play) is taken to have
    // charged when a new period first shows (D72 §1).
    sqlx::query(
        "INSERT INTO subscriptions (account_id, plan, status, provider, provider_ref, price_minor, \
            currency, current_period_end, cancel_at_period_end, grace_until, last_charge_at, sandbox, \
            period_start) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, to_timestamp($8::float8 / 1000), $9, \
            to_timestamp($10::float8 / 1000), coalesce(to_timestamp($11::float8 / 1000), now()), $12, \
            to_timestamp($13::float8 / 1000)) \
         ON CONFLICT (provider, provider_ref) DO UPDATE SET \
            plan = EXCLUDED.plan, status = EXCLUDED.status, price_minor = EXCLUDED.price_minor, \
            currency = EXCLUDED.currency, current_period_end = EXCLUDED.current_period_end, \
            cancel_at_period_end = EXCLUDED.cancel_at_period_end, grace_until = EXCLUDED.grace_until, \
            last_charge_at = CASE \
                WHEN $11::float8 IS NOT NULL THEN EXCLUDED.last_charge_at \
                WHEN subscriptions.current_period_end <> EXCLUDED.current_period_end THEN now() \
                ELSE subscriptions.last_charge_at END, \
            period_start = coalesce(EXCLUDED.period_start, subscriptions.period_start), \
            sandbox = EXCLUDED.sandbox, updated_at = now()",
    )
    .bind(account)
    .bind(s.plan)
    .bind(s.status.as_str())
    .bind(s.provider.as_str())
    .bind(&s.provider_ref)
    .bind(s.price_minor)
    .bind(&s.currency)
    .bind(s.period_end_ms)
    .bind(s.canceling)
    .bind(s.grace_until_ms)
    .bind(s.last_charge_ms)
    .bind(s.sandbox)
    .bind(s.period_start_ms)
    .execute(&mut *tx)
    .await
    .map_err(db)?;
    if let Some(old) = &s.replaces {
        sqlx::query(
            "UPDATE subscriptions SET status = 'expired', updated_at = now() \
             WHERE provider = $1 AND provider_ref = $2 AND account_id = $3",
        )
        .bind(s.provider.as_str())
        .bind(old)
        .bind(account)
        .execute(&mut *tx)
        .await
        .map_err(db)?;
    }
    tx.commit().await.map_err(db)?;
    Ok(Some(account))
}

/// Records a provider's notification id; `false` if it was seen before.
async fn first_time(pool: &PgPool, provider: Provider, id: &str) -> Result<bool, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO billing_events (provider, event_id) VALUES ($1, $2) \
         ON CONFLICT DO NOTHING RETURNING 1",
    )
    .bind(provider.as_str())
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_some())
}

/// What `GET /v1/account` and `GET /v1/billing` say about the plan (§7).
#[derive(Debug, Serialize)]
pub(crate) struct Summary {
    /// `free`, `premium`, `family` or `teams`.
    pub plan: String,
    /// The subscription that gives the plan, if any.
    pub subscription: Option<SubscriptionJson>,
    /// The providers that sell plans here; empty on a server without
    /// billing, where every account is Premium.
    pub billing: Vec<&'static str>,
}

/// Times are Unix milliseconds.
#[derive(Debug, Serialize)]
pub(crate) struct SubscriptionJson {
    pub provider: String,
    pub plan: String,
    pub status: String,
    /// The end of the paid period.
    pub until: i64,
    /// It ends at `until` instead of renewing.
    pub canceling: bool,
    pub price_locked_until: Option<i64>,
    /// Until when a refund can be asked for here (Stripe and Play, 30 days
    /// after the last charge, D72 §1); null when it can't.
    pub refundable_until: Option<i64>,
    /// The providers of other subscriptions that count too, bought by
    /// mistake: the app says where to cancel them.
    pub also: Vec<String>,
}

pub(crate) async fn summary(
    pool: &PgPool,
    billing: &Billing,
    account: Uuid,
) -> Result<Summary, sqlx::Error> {
    let sells = billing.sells();
    if billing.inner.is_none() {
        return Ok(Summary {
            plan: "premium".to_owned(),
            subscription: None,
            billing: sells,
        });
    }
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT s.plan, s.status, s.provider, \
                (extract(epoch FROM s.current_period_end) * 1000)::bigint, \
                s.cancel_at_period_end, \
                (extract(epoch FROM s.price_locked_until) * 1000)::bigint, \
                CASE WHEN s.provider IN ('stripe', 'play') \
                       AND s.last_charge_at > now() - {REFUND_WINDOW} \
                     THEN (extract(epoch FROM s.last_charge_at + {REFUND_WINDOW}) * 1000)::bigint END \
         FROM subscriptions s WHERE s.account_id = $1 AND {ENTITLED} \
         ORDER BY {RANK} DESC, s.current_period_end DESC"
    )))
    .bind(account)
    .fetch_all(pool)
    .await?;
    let mut rows = rows.into_iter();
    let Some(first) = rows.next() else {
        return Ok(Summary {
            plan: "free".to_owned(),
            subscription: None,
            billing: sells,
        });
    };
    let plan: String = first.get(0);
    Ok(Summary {
        plan: plan.clone(),
        subscription: Some(SubscriptionJson {
            provider: first.get(2),
            plan,
            status: first.get(1),
            until: first.get(3),
            canceling: first.get(4),
            price_locked_until: first.get(5),
            refundable_until: first.get(6),
            also: rows.map(|r| r.get(2)).collect(),
        }),
        billing: sells,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_may_name_older_prices_after_its_current_one() {
        let products = Products::parse("premium=price_new, premium=price_old,family=f").unwrap();
        assert_eq!(products.id_for("premium"), Some("price_new"));
        assert_eq!(products.plan_for("price_old"), Some("premium"));
        assert_eq!(products.plan_for("f"), Some("family"));
        assert!(
            Products::parse("premium=p,family=p").is_none(),
            "one id, one plan"
        );
        assert!(Products::parse("gold=p").is_none());
        assert!(Products::parse("").is_none());
    }
}
