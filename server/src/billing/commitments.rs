//! The pricing commitments the server keeps itself (D72): price changes
//! that hold existing subscribers for 12 months (PRICING.md §1), notices and
//! renewal reminders through an outbox (§2), and moving a Stripe
//! subscription to a new price only after its notice was delivered.
//!
//! Logs hold counts and providers only, never an account, email or amount.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use sqlx::{PgPool, Row};
use uuid::Uuid;

use super::{Billing, Provider, Snapshot, stripe};
use crate::ServerError;

/// How long existing subscribers keep their price after a change.
pub(crate) const HOLD: &str = "interval '12 months'";
/// The notice before a held price ends, and the least time between
/// delivering it and moving the subscription.
const NOTICE_DAYS: i64 = 60;
const DAY_MS: i64 = 24 * 3600 * 1000;

/// One message from the outbox, for the email provider (D32) to word and
/// send. Times are Unix ms; the amount is in the currency's minor units.
#[derive(Debug, Clone)]
pub struct OutboxMessage {
    pub id: Uuid,
    pub email: String,
    /// `renewal_reminder`, `price_change` or `price_change_reminder`.
    pub kind: String,
    /// `stripe`, `app_store` or `play`: where the plan is managed.
    pub provider: String,
    /// The charge a reminder is about, or when the new price applies.
    pub about_ms: i64,
    pub price_minor: i64,
    pub currency: String,
}

/// Sends one message; `true` once it was handed over. None is configured
/// until D32 picks a provider, and then nothing is sent and no price
/// change applies.
pub trait Mailer: Send + Sync {
    fn deliver<'a>(
        &'a self,
        message: &'a OutboxMessage,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>>;
}

/// Records a price change for one plan at one provider (`pwm-server
/// price-change`). Subscribers paying another price keep it for 12 months
/// (or longer, if an earlier change held it longer), and each gets a notice
/// now and a reminder 60 days before it ends. Returns how many were held.
pub async fn record_price_change(
    pool: &PgPool,
    provider: &str,
    plan: &str,
    price_ref: &str,
    price_minor: i64,
    currency: &str,
) -> Result<u64, ServerError> {
    if Provider::parse(provider).is_none() {
        return Err(ServerError::Config(
            "the provider is stripe, app_store or play",
        ));
    }
    if !matches!(plan, "premium" | "family" | "teams") {
        return Err(ServerError::Config("the plan is premium, family or teams"));
    }
    let plain = !price_ref.is_empty()
        && price_ref.len() <= 200
        && price_ref
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b));
    if !plain {
        return Err(ServerError::Config("the price id is the provider's id"));
    }
    if price_minor < 0 {
        return Err(ServerError::Config(
            "the amount is in minor units, like 2400",
        ));
    }
    let currency = super::http::currency(currency).ok_or(ServerError::Config(
        "the currency is a three-letter code, like EUR",
    ))?;
    let mut tx = pool.begin().await?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO price_changes (provider, plan, price_ref, price_minor, currency, held_until) \
         VALUES ($1, $2, $3, $4, $5, now() + {HOLD})"
    )))
    .bind(provider)
    .bind(plan)
    .bind(price_ref)
    .bind(price_minor)
    .bind(&currency)
    .execute(&mut *tx)
    .await?;
    let held = sqlx::query(sqlx::AssertSqlSafe(format!(
        "UPDATE subscriptions SET price_locked_until = greatest(price_locked_until, now() + {HOLD}), \
            updated_at = now() \
         WHERE provider = $1 AND plan = $2 AND status <> 'expired' AND price_minor <> $3 \
         RETURNING account_id, provider_ref, (extract(epoch FROM price_locked_until) * 1000)::bigint"
    )))
    .bind(provider)
    .bind(plan)
    .bind(price_minor)
    .fetch_all(&mut *tx)
    .await?;
    for row in &held {
        let account: Uuid = row.get(0);
        let reference: String = row.get(1);
        let until: i64 = row.get(2);
        for (kind, send_after) in [
            ("price_change", "now()"),
            (
                "price_change_reminder",
                "to_timestamp($4::float8 / 1000) - interval '60 days'",
            ),
        ] {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "INSERT INTO billing_outbox \
                    (account_id, kind, provider, provider_ref, about, price_minor, currency, send_after) \
                 VALUES ($1, '{kind}', $2, $3, to_timestamp($4::float8 / 1000), $5, $6, {send_after}) \
                 ON CONFLICT DO NOTHING"
            )))
            .bind(account)
            .bind(provider)
            .bind(&reference)
            .bind(until)
            .bind(price_minor)
            .bind(&currency)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await?;
    Ok(held.len() as u64)
}

/// A store renewed a subscription at another price while its price is held:
/// the store's console wasn't set to keep it (PRICING.md, "Where a store
/// limits a commitment"). Warns the operator; the provider only.
pub(crate) async fn warn_on_held_price(pool: &PgPool, s: &Snapshot) {
    let held: Option<bool> = sqlx::query_scalar(
        "SELECT price_minor <> $3 AND price_locked_until > now() FROM subscriptions \
         WHERE provider = $1 AND provider_ref = $2",
    )
    .bind(s.provider.as_str())
    .bind(&s.provider_ref)
    .bind(s.price_minor)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    if held == Some(true) {
        tracing::warn!(
            "billing: a {} subscription renewed at a new price while its price is held",
            s.provider.as_str()
        );
    }
}

/// Whether a held subscription may move to a new price now: the hold is
/// over, and its reminder was delivered at least 60 days ago (D72 §2).
pub(crate) fn may_move(
    locked_until_ms: Option<i64>,
    reminded_ms: Option<i64>,
    now_ms: i64,
) -> bool {
    match (locked_until_ms, reminded_ms) {
        (Some(until), Some(reminded)) => {
            until <= now_ms && reminded <= now_ms - NOTICE_DAYS * DAY_MS
        }
        _ => false,
    }
}

/// A reminder 14 days before every renewal, for every provider, unless the
/// subscription is ending; one per charge (D72 §3).
pub(crate) async fn schedule_reminders(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let added = sqlx::query(
        "INSERT INTO billing_outbox \
            (account_id, kind, provider, provider_ref, about, price_minor, currency, send_after) \
         SELECT account_id, 'renewal_reminder', provider, provider_ref, current_period_end, \
                price_minor, currency, current_period_end - interval '14 days' \
         FROM subscriptions \
         WHERE status = 'active' AND NOT cancel_at_period_end AND current_period_end > now() \
         ON CONFLICT DO NOTHING",
    )
    .execute(pool)
    .await?;
    Ok(added.rows_affected())
}

/// Hands every due message to the mailer. A renewal reminder goes only if
/// the subscription still renews then; anything about a past date is
/// dropped. Without a mailer, counts what is waiting.
pub(crate) async fn deliver_due(
    pool: &PgPool,
    mailer: Option<&Arc<dyn Mailer>>,
) -> Result<(u64, u64), sqlx::Error> {
    sqlx::query("DELETE FROM billing_outbox WHERE sent_at IS NULL AND about < now()")
        .execute(pool)
        .await?;
    let rows = sqlx::query(
        "SELECT o.id, a.email, o.kind, o.provider, \
                (extract(epoch FROM o.about) * 1000)::bigint, o.price_minor, o.currency \
         FROM billing_outbox o \
         JOIN accounts a ON a.id = o.account_id \
         JOIN subscriptions s ON s.provider = o.provider AND s.provider_ref = o.provider_ref \
         WHERE o.sent_at IS NULL AND o.send_after <= now() \
           AND (o.kind <> 'renewal_reminder' OR (s.status = 'active' \
                AND NOT s.cancel_at_period_end AND s.current_period_end = o.about)) \
         ORDER BY o.send_after",
    )
    .fetch_all(pool)
    .await?;
    let Some(mailer) = mailer else {
        return Ok((0, rows.len() as u64));
    };
    let (mut sent, mut failed) = (0, 0);
    for row in rows {
        let message = OutboxMessage {
            id: row.get(0),
            email: row.get(1),
            kind: row.get(2),
            provider: row.get(3),
            about_ms: row.get(4),
            price_minor: row.get(5),
            currency: row.get(6),
        };
        if mailer.deliver(&message).await {
            sqlx::query("UPDATE billing_outbox SET sent_at = now() WHERE id = $1")
                .bind(message.id)
                .execute(pool)
                .await?;
            sent += 1;
        } else {
            failed += 1;
        }
    }
    Ok((sent, failed))
}

/// Moves each held Stripe subscription whose hold is over, and whose
/// reminder went out at least 60 days ago, to its plan's latest price from
/// its next renewal (D72 §2). Returns how many moved.
pub(crate) async fn apply_price_moves(
    billing: &Billing,
    pool: &PgPool,
) -> Result<u64, sqlx::Error> {
    let Some(inner) = billing.inner.as_deref() else {
        return Ok(0);
    };
    let Some(config) = &inner.stripe else {
        return Ok(0);
    };
    let rows = sqlx::query(
        "SELECT s.provider_ref, p.price_ref, \
                (extract(epoch FROM s.price_locked_until) * 1000)::bigint, \
                (SELECT (extract(epoch FROM max(o.sent_at)) * 1000)::bigint FROM billing_outbox o \
                  WHERE o.kind = 'price_change_reminder' AND o.provider = s.provider \
                    AND o.provider_ref = s.provider_ref AND o.sent_at IS NOT NULL), \
                (extract(epoch FROM now()) * 1000)::bigint \
         FROM subscriptions s \
         JOIN LATERAL (SELECT price_ref, price_minor FROM price_changes \
                        WHERE provider = 'stripe' AND plan = s.plan \
                        ORDER BY created_at DESC LIMIT 1) p ON true \
         WHERE s.provider = 'stripe' AND s.status = 'active' AND NOT s.cancel_at_period_end \
           AND s.price_minor <> p.price_minor",
    )
    .fetch_all(pool)
    .await?;
    let mut moved = 0;
    for row in rows {
        let reference: String = row.get(0);
        let price: String = row.get(1);
        if !may_move(row.get(2), row.get(3), row.get(4)) {
            continue;
        }
        if stripe::move_price(config, &inner.http, &reference, &price)
            .await
            .is_ok()
        {
            moved += 1;
            let _ = billing
                .refresh(pool, Provider::Stripe, &reference, None, None)
                .await;
        }
    }
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_held_price_moves_only_after_the_hold_and_a_delivered_notice() {
        let now = 1_800_000_000_000;
        let day = DAY_MS;
        assert!(may_move(Some(now - day), Some(now - 61 * day), now));
        assert!(may_move(Some(now), Some(now - 60 * day), now));
        // Still held.
        assert!(!may_move(Some(now + day), Some(now - 90 * day), now));
        // The notice went out less than 60 days ago.
        assert!(!may_move(Some(now - day), Some(now - 59 * day), now));
        // Never delivered (no email provider yet), or never held.
        assert!(!may_move(Some(now - day), None, now));
        assert!(!may_move(None, Some(now - 90 * day), now));
    }
}
