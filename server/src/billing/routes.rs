//! The billing API (D6 §7). The apps' calls run under the usual session;
//! the providers' webhooks have none, and are checked by signature (Stripe),
//! by our secret in the URL (Play), or not at all (Apple), since each only
//! makes the server read the subscription again from its provider.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use super::{BillingError, Provider, Summary, apple, first_time, play, stripe, summary};
use crate::api::{ApiError, ApiJson, ApiPath, ApiRouter, AppState};
use crate::session::Authenticated;

/// Providers' notifications can be larger than the apps' requests.
const MAX_WEBHOOK_BYTES: usize = 256 * 1024;

pub(crate) fn routes() -> ApiRouter {
    let webhooks = Router::new()
        .route("/v1/billing/stripe/webhook", post(stripe_webhook))
        .route("/v1/billing/apple/notifications", post(apple_notification))
        .route(
            "/v1/billing/play/notifications/{secret}",
            post(play_notification),
        )
        .layer(DefaultBodyLimit::max(MAX_WEBHOOK_BYTES));
    Router::new()
        .route("/v1/billing", get(status))
        .route("/v1/billing/stripe/checkout", post(stripe_checkout))
        .route("/v1/billing/stripe/cancel", post(stripe_cancel))
        .route("/v1/billing/stripe/resume", post(stripe_resume))
        .route("/v1/billing/stripe/refund", post(stripe_refund))
        .route("/v1/billing/play/refund", post(play_refund))
        .route("/v1/billing/apple/transactions", post(apple_transaction))
        .route("/v1/billing/play/purchases", post(play_purchase))
        .route("/billing/done", get(done))
        .route("/billing/canceled", get(canceled))
        .merge(webhooks)
}

impl From<BillingError> for ApiError {
    fn from(err: BillingError) -> Self {
        match err {
            BillingError::Unavailable => Self::BillingUnavailable,
            BillingError::NotFound => Self::NotFound,
            BillingError::NotYours => Self::Forbidden("purchase_not_yours"),
            BillingError::Pending => Self::Conflict("purchase_pending"),
        }
    }
}

#[derive(Serialize)]
struct Status {
    #[serde(flatten)]
    summary: Summary,
    /// For a store purchase: Apple's `appAccountToken`, Play's
    /// `obfuscatedAccountId`.
    token: Uuid,
}

/// The plan, where it was bought, what this server sells, and the
/// account's billing token.
async fn status(
    State(state): State<AppState>,
    auth: Authenticated,
) -> Result<axum::Json<Status>, ApiError> {
    let token: Uuid = sqlx::query_scalar("SELECT billing_token FROM accounts WHERE id = $1")
        .bind(auth.account_id)
        .fetch_one(&state.pool)
        .await?;
    let summary = summary(&state.pool, &state.billing, auth.account_id).await?;
    Ok(axum::Json(Status { summary, token }))
}

async fn answer(state: &AppState, account: Uuid) -> Result<axum::Json<Summary>, ApiError> {
    Ok(axum::Json(
        summary(&state.pool, &state.billing, account).await?,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Checkout {
    plan: String,
}

#[derive(Serialize)]
struct CheckoutUrl {
    url: String,
}

/// A Stripe Checkout page for a plan. An account that already has a plan
/// answers `already_subscribed`, so our own checkout never sells a second.
async fn stripe_checkout(
    State(state): State<AppState>,
    auth: Authenticated,
    ApiJson(req): ApiJson<Checkout>,
) -> Result<axum::Json<CheckoutUrl>, ApiError> {
    let inner = state.billing.inner()?;
    let config = inner.stripe.as_ref().ok_or(ApiError::BillingUnavailable)?;
    let price = config
        .prices
        .id_for(&req.plan)
        .ok_or(ApiError::InvalidRequest)?
        .to_owned();
    let current = summary(&state.pool, &state.billing, auth.account_id).await?;
    if current.subscription.is_some() {
        return Err(ApiError::Conflict("already_subscribed"));
    }
    let row = sqlx::query("SELECT billing_token, stripe_customer FROM accounts WHERE id = $1")
        .bind(auth.account_id)
        .fetch_one(&state.pool)
        .await?;
    let token: Uuid = row.get(0);
    let customer = match row.get::<Option<String>, _>(1) {
        Some(customer) => customer,
        None => {
            let made = stripe::create_customer(config, &inner.http, token).await?;
            // Two checkouts at once: the first customer stored wins.
            sqlx::query(
                "UPDATE accounts SET stripe_customer = $2 WHERE id = $1 AND stripe_customer IS NULL",
            )
            .bind(auth.account_id)
            .bind(&made)
            .execute(&state.pool)
            .await?;
            sqlx::query_scalar("SELECT stripe_customer FROM accounts WHERE id = $1")
                .bind(auth.account_id)
                .fetch_one(&state.pool)
                .await?
        }
    };
    let url = stripe::checkout(config, &inner.http, &customer, token, &price).await?;
    Ok(axum::Json(CheckoutUrl { url }))
}

async fn stripe_cancel(
    state: State<AppState>,
    auth: Authenticated,
) -> Result<axum::Json<Summary>, ApiError> {
    set_canceling(state, auth, true).await
}

async fn stripe_resume(
    state: State<AppState>,
    auth: Authenticated,
) -> Result<axum::Json<Summary>, ApiError> {
    set_canceling(state, auth, false).await
}

/// One click either way (PRICING.md §4): the account's Stripe subscription
/// ends at the end of its period, or renews again. One bought in a store
/// answers `managed_elsewhere`; none at all, 404.
async fn set_canceling(
    State(state): State<AppState>,
    auth: Authenticated,
    canceling: bool,
) -> Result<axum::Json<Summary>, ApiError> {
    let inner = state.billing.inner()?;
    let config = inner.stripe.as_ref().ok_or(ApiError::BillingUnavailable)?;
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT s.provider_ref FROM subscriptions s \
         WHERE s.account_id = $1 AND s.provider = 'stripe' AND {} \
         ORDER BY s.current_period_end DESC LIMIT 1",
        super::ENTITLED
    )))
    .bind(auth.account_id)
    .fetch_optional(&state.pool)
    .await?;
    let Some(row) = row else {
        let current = summary(&state.pool, &state.billing, auth.account_id).await?;
        return Err(if current.subscription.is_some() {
            ApiError::Conflict("managed_elsewhere")
        } else {
            ApiError::NotFound
        });
    };
    let reference: String = row.get(0);
    stripe::set_canceling(config, &inner.http, &reference, canceling).await?;
    state
        .billing
        .refresh(
            &state.pool,
            Provider::Stripe,
            &reference,
            None,
            Some(auth.account_id),
        )
        .await?;
    answer(&state, auth.account_id).await
}

/// The account's subscription at `provider` that counts now, if any, with
/// whether its last charge is within the refund window.
async fn refundable(
    state: &AppState,
    account: Uuid,
    provider: Provider,
) -> Result<(String, bool), ApiError> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT s.provider_ref, coalesce(s.last_charge_at > now() - {}, false) \
         FROM subscriptions s \
         WHERE s.account_id = $1 AND s.provider = $2 AND {} \
         ORDER BY s.current_period_end DESC LIMIT 1",
        super::REFUND_WINDOW,
        super::ENTITLED
    )))
    .bind(account)
    .bind(provider.as_str())
    .fetch_optional(&state.pool)
    .await?;
    match row {
        Some(row) => Ok((row.get(0), row.get(1))),
        None => {
            let current = summary(&state.pool, &state.billing, account).await?;
            Err(if current.subscription.is_some() {
                ApiError::Conflict("managed_elsewhere")
            } else {
                ApiError::NotFound
            })
        }
    }
}

/// One click (PRICING.md §5, D72 §1): within 30 days of the last charge,
/// the unused part of it comes back and the plan ends now. The refund goes
/// first, once whatever the retries; then the subscription ends.
async fn stripe_refund(
    State(state): State<AppState>,
    auth: Authenticated,
) -> Result<axum::Json<Summary>, ApiError> {
    let inner = state.billing.inner()?;
    let config = inner.stripe.as_ref().ok_or(ApiError::BillingUnavailable)?;
    let (reference, _) = refundable(&state, auth.account_id, Provider::Stripe).await?;
    let charge = stripe::last_charge(config, &inner.http, &reference).await?;
    let now = i64::try_from(stripe::now()).unwrap_or(i64::MAX);
    if now - charge.paid_at > 30 * 24 * 3600 {
        return Err(ApiError::Conflict("refund_window_passed"));
    }
    let amount = stripe::refund_amount(
        charge.amount_paid,
        charge.period_start,
        charge.period_end,
        now,
    );
    if amount > 0 {
        stripe::refund(config, &inner.http, &charge.invoice, amount).await?;
    }
    stripe::end_now(config, &inner.http, &reference).await?;
    state
        .billing
        .refresh(
            &state.pool,
            Provider::Stripe,
            &reference,
            None,
            Some(auth.account_id),
        )
        .await?;
    answer(&state, auth.account_id).await
}

/// As for Stripe, with Google working out the pro-rata amount.
async fn play_refund(
    State(state): State<AppState>,
    auth: Authenticated,
) -> Result<axum::Json<Summary>, ApiError> {
    let inner = state.billing.inner()?;
    let config = inner.play.as_ref().ok_or(ApiError::BillingUnavailable)?;
    let (reference, in_window) = refundable(&state, auth.account_id, Provider::Play).await?;
    if !in_window {
        return Err(ApiError::Conflict("refund_window_passed"));
    }
    play::revoke_prorated(config, &inner.http, &reference).await?;
    state
        .billing
        .refresh(
            &state.pool,
            Provider::Play,
            &reference,
            None,
            Some(auth.account_id),
        )
        .await?;
    answer(&state, auth.account_id).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AppleTransaction {
    transaction_id: String,
}

/// Right after a purchase in the iOS app: the server reads the
/// subscription from Apple and stores it if its token is this account's.
async fn apple_transaction(
    State(state): State<AppState>,
    auth: Authenticated,
    ApiJson(req): ApiJson<AppleTransaction>,
) -> Result<axum::Json<Summary>, ApiError> {
    if !apple::plain_transaction(&req.transaction_id) {
        return Err(ApiError::InvalidRequest);
    }
    state
        .billing
        .inner()?
        .apple
        .as_ref()
        .ok_or(ApiError::BillingUnavailable)?;
    state
        .billing
        .refresh(
            &state.pool,
            Provider::AppStore,
            &req.transaction_id,
            None,
            Some(auth.account_id),
        )
        .await?;
    answer(&state, auth.account_id).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlayPurchase {
    product_id: String,
    purchase_token: String,
}

/// Right after a purchase in the Android app, as for Apple.
async fn play_purchase(
    State(state): State<AppState>,
    auth: Authenticated,
    ApiJson(req): ApiJson<PlayPurchase>,
) -> Result<axum::Json<Summary>, ApiError> {
    if !play::plain_token(&req.purchase_token) {
        return Err(ApiError::InvalidRequest);
    }
    let inner = state.billing.inner()?;
    let config = inner.play.as_ref().ok_or(ApiError::BillingUnavailable)?;
    if config.products.plan_for(&req.product_id).is_none() {
        return Err(ApiError::InvalidRequest);
    }
    state
        .billing
        .refresh(
            &state.pool,
            Provider::Play,
            &req.purchase_token,
            None,
            Some(auth.account_id),
        )
        .await?;
    answer(&state, auth.account_id).await
}

/// After a webhook: 200 when handled or ignored, 503 when the provider
/// couldn't be read, so it sends the notification again.
fn webhook_result(result: Result<(), BillingError>) -> Response {
    match result {
        Ok(()) | Err(BillingError::NotFound | BillingError::NotYours | BillingError::Pending) => {
            StatusCode::OK.into_response()
        }
        Err(BillingError::Unavailable) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

/// Reads one subscription again for a notification, within the limit
/// per minute, and records the notification once it was handled.
async fn handle_hint(
    state: &AppState,
    provider: Provider,
    event_id: &str,
    target: Option<(String, Option<bool>)>,
) -> Response {
    match first_time_seen(state, provider, event_id).await {
        Ok(false) => return StatusCode::OK.into_response(),
        Ok(true) => {}
        Err(status) => return status.into_response(),
    }
    let Some((reference, sandbox)) = target else {
        return remember(state, provider, event_id).await;
    };
    if !state.billing.take_hint() {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let result = state
        .billing
        .refresh(&state.pool, provider, &reference, sandbox, None)
        .await
        .map(|_| ());
    if result != Err(BillingError::Unavailable) {
        return remember(state, provider, event_id).await;
    }
    webhook_result(result)
}

/// Whether a notification is new, without recording it yet.
async fn first_time_seen(
    state: &AppState,
    provider: Provider,
    event_id: &str,
) -> Result<bool, StatusCode> {
    let seen: Option<i32> =
        sqlx::query_scalar("SELECT 1 FROM billing_events WHERE provider = $1 AND event_id = $2")
            .bind(provider.as_str())
            .bind(event_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(seen.is_none())
}

async fn remember(state: &AppState, provider: Provider, event_id: &str) -> Response {
    match first_time(&state.pool, provider, event_id).await {
        Ok(_) => StatusCode::OK.into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

async fn stripe_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Ok(inner) = state.billing.inner() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(config) = &inner.stripe else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let signature = headers
        .get("stripe-signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !stripe::signature_valid(config, signature, &body, stripe::now()) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(event) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let Some(id) = event["id"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 200)
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let target = stripe::event_subscription(&event).map(|s| (s, None));
    handle_hint(&state, Provider::Stripe, id, target).await
}

async fn apple_notification(
    State(state): State<AppState>,
    ApiJson(body): ApiJson<serde_json::Value>,
) -> Response {
    let Ok(inner) = state.billing.inner() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(config) = &inner.apple else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((id, target)) = apple::notification(config, &body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let target = target.map(|(t, sandbox)| (t, Some(sandbox)));
    handle_hint(&state, Provider::AppStore, &id, target).await
}

async fn play_notification(
    State(state): State<AppState>,
    ApiPath(secret): ApiPath<String>,
    ApiJson(body): ApiJson<serde_json::Value>,
) -> Response {
    let Ok(inner) = state.billing.inner() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some(config) = &inner.play else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !config.secret_ok(&secret) {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some((id, token)) = play::notification(config, &body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    handle_hint(&state, Provider::Play, &id, token.map(|t| (t, None))).await
}

const PAGE: &str = "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>PWM</title>\
<body style=\"font-family:system-ui,sans-serif;max-width:32rem;margin:4rem auto;padding:0 1rem\">";

/// Where Stripe's Checkout returns after paying.
async fn done() -> impl IntoResponse {
    page("<h1>Thank you</h1><p>Your plan is ready. You can close this page and go back to PWM.</p>")
}

/// Where Stripe's Checkout returns when the user leaves it.
async fn canceled() -> impl IntoResponse {
    page("<h1>Nothing was charged</h1><p>You can close this page and go back to PWM.</p>")
}

fn page(body: &str) -> impl IntoResponse {
    (
        [
            (header::CACHE_CONTROL, "no-store"),
            (
                header::CONTENT_SECURITY_POLICY,
                "default-src 'none'; style-src 'unsafe-inline'",
            ),
        ],
        Html(format!("{PAGE}{body}</body></html>")),
    )
}
