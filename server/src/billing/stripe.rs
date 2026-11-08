//! Stripe (D6 §2, §3): Checkout in the browser for the Mac, Windows, Linux
//! and the extensions, one-click cancel and resume, and webhooks checked
//! with their signature, then read again from Stripe's API.

use http_body_util::Full;
use hyper::body::Bytes;
use pwm_crypto::{Secret, webhook_signature_valid};
use serde_json::Value;
use uuid::Uuid;

use super::http::{Http, currency, form, unix_secs};
use super::{BillingError, Link, Products, Provider, Snapshot, Status};
use crate::ServerError;

/// The API version every call names, so Stripe's answers keep one shape.
pub const STRIPE_VERSION: &str = "2025-03-31.basil";
const STRIPE_URL: &str = "https://api.stripe.com";
/// A webhook older than this is refused (Stripe's own default).
const TOLERANCE_SECS: u64 = 300;

/// `Debug` shows neither key.
pub struct StripeConfig {
    key: Secret<String>,
    webhook_secret: Secret<String>,
    pub(crate) prices: Products,
    url: String,
    /// This server's public address, for Checkout's return pages.
    public_url: String,
}

impl std::fmt::Debug for StripeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StripeConfig { .. }")
    }
}

impl StripeConfig {
    pub fn new(
        key: &str,
        webhook_secret: &str,
        prices: &str,
        public_url: &str,
        url: Option<&str>,
    ) -> Result<Self, ServerError> {
        if !(key.starts_with("sk_") || key.starts_with("rk_")) || key.len() > 300 {
            return Err(ServerError::Config(
                "PWM_STRIPE_SECRET_KEY is not a Stripe secret key",
            ));
        }
        if !webhook_secret.starts_with("whsec_") {
            return Err(ServerError::Config(
                "PWM_STRIPE_WEBHOOK_SECRET is not a Stripe webhook secret",
            ));
        }
        let prices = Products::parse(prices).ok_or(ServerError::Config(
            "PWM_STRIPE_PRICES is not a list like premium=price_123",
        ))?;
        let public_url = public_url.trim_end_matches('/');
        if !(public_url.starts_with("https://") || public_url.starts_with("http://")) {
            return Err(ServerError::Config(
                "PWM_PUBLIC_URL is not this server's address, like https://pwm.example",
            ));
        }
        Ok(Self {
            key: Secret::new(key.to_owned()),
            webhook_secret: Secret::new(webhook_secret.to_owned()),
            prices,
            url: url.unwrap_or(STRIPE_URL).trim_end_matches('/').to_owned(),
            public_url: public_url.to_owned(),
        })
    }

    /// `PWM_STRIPE_SECRET_KEY`, `PWM_STRIPE_WEBHOOK_SECRET`,
    /// `PWM_STRIPE_PRICES`, `PWM_PUBLIC_URL` and the optional
    /// `PWM_STRIPE_URL`. No key: Stripe is off.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Option<Self>, ServerError> {
        let get = |name| var(name).filter(|v| !v.is_empty());
        let Some(key) = get("PWM_STRIPE_SECRET_KEY") else {
            return Ok(None);
        };
        let (Some(secret), Some(prices), Some(public)) = (
            get("PWM_STRIPE_WEBHOOK_SECRET"),
            get("PWM_STRIPE_PRICES"),
            get("PWM_PUBLIC_URL"),
        ) else {
            return Err(ServerError::Config(
                "PWM_STRIPE_SECRET_KEY needs PWM_STRIPE_WEBHOOK_SECRET, PWM_STRIPE_PRICES and PWM_PUBLIC_URL",
            ));
        };
        let url = get("PWM_STRIPE_URL");
        Self::new(&key, &secret, &prices, &public, url.as_deref()).map(Some)
    }
}

/// One call to Stripe's API; 404 is `NotFound`, any other failure
/// `Unavailable`.
async fn call(
    config: &StripeConfig,
    http: &Http,
    method: hyper::Method,
    path: &str,
    body: Option<String>,
) -> Result<Value, BillingError> {
    call_once(config, http, method, path, body, None).await
}

/// [`call`] with an idempotency key, so a retry is applied once.
async fn call_once(
    config: &StripeConfig,
    http: &Http,
    method: hyper::Method,
    path: &str,
    body: Option<String>,
    idempotency_key: Option<&str>,
) -> Result<Value, BillingError> {
    let mut request = hyper::Request::builder()
        .method(method)
        .uri(format!("{}{path}", config.url))
        .header(
            "authorization",
            format!("Bearer {}", config.key.expose_secret()),
        )
        .header("stripe-version", STRIPE_VERSION);
    if body.is_some() {
        request = request.header("content-type", "application/x-www-form-urlencoded");
    }
    if let Some(key) = idempotency_key {
        request = request.header("idempotency-key", key);
    }
    let request = request
        .body(Full::new(Bytes::from(body.unwrap_or_default())))
        .map_err(|_| BillingError::Unavailable)?;
    let (status, bytes) = http.send(request).await?;
    match status {
        200 => serde_json::from_slice(&bytes).map_err(|_| BillingError::Unavailable),
        404 => Err(BillingError::NotFound),
        status => {
            tracing::warn!("billing: Stripe answered {status}");
            Err(BillingError::Unavailable)
        }
    }
}

fn plain_id(id: &str, prefix: &str) -> bool {
    id.strip_prefix(prefix).is_some_and(|rest| {
        !rest.is_empty() && rest.len() <= 64 && rest.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

/// A new customer for one account, named by nothing but its billing token.
pub(crate) async fn create_customer(
    config: &StripeConfig,
    http: &Http,
    token: Uuid,
) -> Result<String, BillingError> {
    let body = form(&[("metadata[billing_token]", &token.to_string())]);
    let customer = call(
        config,
        http,
        hyper::Method::POST,
        "/v1/customers",
        Some(body),
    )
    .await?;
    let id = customer["id"].as_str().unwrap_or_default();
    if plain_id(id, "cus_") {
        Ok(id.to_owned())
    } else {
        Err(BillingError::Unavailable)
    }
}

/// A Checkout page for `price`, returning to this server's pages.
pub(crate) async fn checkout(
    config: &StripeConfig,
    http: &Http,
    customer: &str,
    token: Uuid,
    price: &str,
) -> Result<String, BillingError> {
    let token = token.to_string();
    let done = format!("{}/billing/done", config.public_url);
    let canceled = format!("{}/billing/canceled", config.public_url);
    let body = form(&[
        ("mode", "subscription"),
        ("customer", customer),
        ("client_reference_id", &token),
        ("line_items[0][price]", price),
        ("line_items[0][quantity]", "1"),
        ("subscription_data[metadata][billing_token]", &token),
        ("success_url", &done),
        ("cancel_url", &canceled),
    ]);
    let session = call(
        config,
        http,
        hyper::Method::POST,
        "/v1/checkout/sessions",
        Some(body),
    )
    .await?;
    session["url"]
        .as_str()
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
        .map(str::to_owned)
        .ok_or(BillingError::Unavailable)
}

/// Sets or clears `cancel_at_period_end` (§5, commitment 4).
pub(crate) async fn set_canceling(
    config: &StripeConfig,
    http: &Http,
    id: &str,
    canceling: bool,
) -> Result<(), BillingError> {
    if !plain_id(id, "sub_") {
        return Err(BillingError::NotFound);
    }
    let body = form(&[(
        "cancel_at_period_end",
        if canceling { "true" } else { "false" },
    )]);
    call(
        config,
        http,
        hyper::Method::POST,
        &format!("/v1/subscriptions/{id}"),
        Some(body),
    )
    .await
    .map(|_| ())
}

/// The unused part of what the last charge paid, in minor units, rounded
/// down (D72 §1). Times are Unix seconds.
pub(crate) fn refund_amount(paid: i64, start: i64, end: i64, now: i64) -> i64 {
    if paid <= 0 || end <= start || now >= end {
        return 0;
    }
    let unused = i128::from(end - now.max(start));
    let length = i128::from(end - start);
    i64::try_from(i128::from(paid) * unused / length).unwrap_or(0)
}

/// What a refund needs from Stripe: the last invoice, when it was paid and
/// what it paid, and the period it paid for (Unix seconds).
pub(crate) struct LastCharge {
    pub invoice: String,
    pub paid_at: i64,
    pub amount_paid: i64,
    pub period_start: i64,
    pub period_end: i64,
}

pub(crate) async fn last_charge(
    config: &StripeConfig,
    http: &Http,
    id: &str,
) -> Result<LastCharge, BillingError> {
    if !plain_id(id, "sub_") {
        return Err(BillingError::NotFound);
    }
    let path = format!("/v1/subscriptions/{id}?expand%5B%5D=latest_invoice");
    let s = call(config, http, hyper::Method::GET, &path, None).await?;
    let invoice = &s["latest_invoice"];
    let item = &s["items"]["data"][0];
    let read = || {
        Some(LastCharge {
            invoice: invoice["id"]
                .as_str()
                .filter(|i| plain_id(i, "in_"))?
                .to_owned(),
            paid_at: invoice["status_transitions"]["paid_at"].as_i64()?,
            amount_paid: invoice["amount_paid"].as_i64()?,
            period_start: item["current_period_start"].as_i64()?,
            period_end: item["current_period_end"].as_i64()?,
        })
    };
    read().ok_or(BillingError::NotFound)
}

/// Refunds `amount` of the invoice's payment, once whatever the retries:
/// the idempotency key is the invoice's.
pub(crate) async fn refund(
    config: &StripeConfig,
    http: &Http,
    invoice: &str,
    amount: i64,
) -> Result<(), BillingError> {
    if !plain_id(invoice, "in_") {
        return Err(BillingError::NotFound);
    }
    let path = format!("/v1/invoices/{invoice}?expand%5B%5D=payments");
    let invoice_json = call(config, http, hyper::Method::GET, &path, None).await?;
    let intent = invoice_json["payments"]["data"]
        .as_array()
        .into_iter()
        .flatten()
        .find_map(|p| p["payment"]["payment_intent"].as_str())
        .filter(|pi| plain_id(pi, "pi_"))
        .ok_or(BillingError::NotFound)?
        .to_owned();
    let body = form(&[("payment_intent", &intent), ("amount", &amount.to_string())]);
    call_once(
        config,
        http,
        hyper::Method::POST,
        "/v1/refunds",
        Some(body),
        Some(&format!("pwm-refund-{invoice}")),
    )
    .await
    .map(|_| ())
}

/// Deletes a customer and its saved payment methods (D74 §3). Stripe keeps
/// its own records of the charges and invoices.
pub(crate) async fn delete_customer(
    config: &StripeConfig,
    http: &Http,
    id: &str,
) -> Result<(), BillingError> {
    if !plain_id(id, "cus_") {
        return Err(BillingError::NotFound);
    }
    call(
        config,
        http,
        hyper::Method::DELETE,
        &format!("/v1/customers/{id}"),
        None,
    )
    .await
    .map(|_| ())
}

/// Ends a subscription now, without another invoice.
pub(crate) async fn end_now(
    config: &StripeConfig,
    http: &Http,
    id: &str,
) -> Result<(), BillingError> {
    if !plain_id(id, "sub_") {
        return Err(BillingError::NotFound);
    }
    call(
        config,
        http,
        hyper::Method::DELETE,
        &format!("/v1/subscriptions/{id}"),
        None,
    )
    .await
    .map(|_| ())
}

/// Moves a subscription to `price` from its next renewal (D72 §2).
pub(crate) async fn move_price(
    config: &StripeConfig,
    http: &Http,
    id: &str,
    price: &str,
) -> Result<(), BillingError> {
    if !plain_id(id, "sub_") {
        return Err(BillingError::NotFound);
    }
    let path = format!("/v1/subscriptions/{id}");
    let s = call(config, http, hyper::Method::GET, &path, None).await?;
    let item = s["items"]["data"][0]["id"]
        .as_str()
        .filter(|i| plain_id(i, "si_"))
        .ok_or(BillingError::NotFound)?
        .to_owned();
    let body = form(&[
        ("items[0][id]", &item),
        ("items[0][price]", price),
        ("proration_behavior", "none"),
    ]);
    call(config, http, hyper::Method::POST, &path, Some(body))
        .await
        .map(|_| ())
}

/// The subscription as Stripe has it now.
pub(crate) async fn fetch(
    config: &StripeConfig,
    http: &Http,
    id: &str,
) -> Result<Snapshot, BillingError> {
    if !plain_id(id, "sub_") {
        return Err(BillingError::NotFound);
    }
    let path = format!("/v1/subscriptions/{id}?expand%5B%5D=latest_invoice");
    let subscription = call(config, http, hyper::Method::GET, &path, None).await?;
    snapshot(config, &subscription).ok_or(BillingError::NotFound)
}

/// A subscription's JSON as a snapshot; `None` for another product or a
/// shape we don't know.
pub(crate) fn snapshot(config: &StripeConfig, s: &Value) -> Option<Snapshot> {
    let id = s["id"].as_str().filter(|id| plain_id(id, "sub_"))?;
    let item = &s["items"]["data"][0];
    let plan = config.prices.plan_for(item["price"]["id"].as_str()?)?;
    let period_end = item["current_period_end"]
        .as_i64()
        .or_else(|| s["current_period_end"].as_i64())?;
    let status = match s["status"].as_str()? {
        "active" | "trialing" => Status::Active,
        "past_due" => Status::PastDue,
        _ => Status::Expired,
    };
    let link = match s["metadata"]["billing_token"]
        .as_str()
        .and_then(|t| t.parse::<Uuid>().ok())
    {
        Some(token) => Link::Token(token),
        None => match s["customer"].as_str() {
            Some(customer) if plain_id(customer, "cus_") => {
                Link::StripeCustomer(customer.to_owned())
            }
            _ => Link::None,
        },
    };
    let paid_at = s["latest_invoice"]["status_transitions"]["paid_at"].as_i64();
    Some(Snapshot {
        provider: Provider::Stripe,
        provider_ref: id.to_owned(),
        link,
        plan,
        status,
        price_minor: item["price"]["unit_amount"].as_i64().unwrap_or(0).max(0),
        currency: currency(item["price"]["currency"].as_str().unwrap_or("usd"))?,
        period_start_ms: item["current_period_start"]
            .as_i64()
            .and_then(|t| t.checked_mul(1000)),
        period_end_ms: period_end.checked_mul(1000)?,
        canceling: s["cancel_at_period_end"].as_bool().unwrap_or(false),
        grace_until_ms: None,
        last_charge_ms: paid_at.and_then(|t| t.checked_mul(1000)),
        sandbox: s["livemode"].as_bool() == Some(false),
        replaces: None,
        acknowledge: None,
    })
}

/// Checks `Stripe-Signature` (`t=<secs>,v1=<hex>[,v1=…]`) over the raw
/// body, within five minutes of `now`.
pub(crate) fn signature_valid(config: &StripeConfig, header: &str, body: &[u8], now: u64) -> bool {
    let mut timestamp = None;
    let mut signatures = Vec::new();
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", t)) => timestamp = t.parse::<u64>().ok(),
            Some(("v1", s)) => signatures.push(s),
            _ => {}
        }
    }
    let Some(t) = timestamp else {
        return false;
    };
    if now.abs_diff(t) > TOLERANCE_SECS {
        return false;
    }
    let mut signed = format!("{t}.").into_bytes();
    signed.extend_from_slice(body);
    let secret = config.webhook_secret.expose_secret().as_bytes();
    signatures
        .iter()
        .any(|s| webhook_signature_valid(secret, &signed, s))
}

pub(crate) fn now() -> u64 {
    unix_secs()
}

/// The subscription a webhook event is about, if any.
pub(crate) fn event_subscription(event: &Value) -> Option<String> {
    let object = &event["data"]["object"];
    let id = match object["object"].as_str()? {
        "subscription" => object["id"].as_str(),
        "checkout.session" => object["subscription"].as_str(),
        "invoice" => object["subscription"]
            .as_str()
            .or_else(|| object["parent"]["subscription_details"]["subscription"].as_str()),
        _ => None,
    }?;
    plain_id(id, "sub_").then(|| id.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> StripeConfig {
        StripeConfig::new(
            "sk_test_abc",
            "whsec_test",
            "premium=price_p,family=price_f",
            "https://pwm.test/",
            None,
        )
        .unwrap()
    }

    #[test]
    fn settings_are_checked_and_never_printed() {
        let config = config();
        assert_eq!(format!("{config:?}"), "StripeConfig { .. }");
        assert_eq!(config.public_url, "https://pwm.test");
        assert!(StripeConfig::new("pk_x", "whsec_x", "premium=p", "https://a", None).is_err());
        assert!(StripeConfig::new("sk_x", "x", "premium=p", "https://a", None).is_err());
        assert!(StripeConfig::new("sk_x", "whsec_x", "gold=p", "https://a", None).is_err());
        assert!(StripeConfig::new("sk_x", "whsec_x", "premium=p", "pwm.test", None).is_err());
        assert!(StripeConfig::from_vars(|_| None).unwrap().is_none());
        assert!(
            StripeConfig::from_vars(|n| (n == "PWM_STRIPE_SECRET_KEY").then(|| "sk_x".into()))
                .is_err()
        );
    }

    #[test]
    fn signatures_are_checked_with_the_time() {
        let config = config();
        let body = br#"{"id":"evt_1"}"#;
        // HMAC-SHA256 of `1700000000.{"id":"evt_1"}` under `whsec_test`.
        let mac = "c89214b5b5da833daed6f0b8c5bb6bd58cea9022bd80ccc78230f3942d632925";
        let header = format!("t=1700000000,v1=00ff,v1={mac},v0=abc");
        assert!(signature_valid(&config, &header, body, 1_700_000_100));
        assert!(!signature_valid(&config, &header, body, 1_700_000_301));
        assert!(!signature_valid(&config, &header, b"{}", 1_700_000_100));
        assert!(!signature_valid(
            &config,
            &format!("v1={mac}"),
            body,
            1_700_000_000
        ));
        assert!(!signature_valid(
            &config,
            "t=1700000000,v1=00",
            body,
            1_700_000_000
        ));
    }

    #[test]
    fn subscriptions_become_snapshots() {
        let config = config();
        let token = Uuid::from_u128(7);
        let json = serde_json::json!({
            "id": "sub_1", "object": "subscription", "status": "active", "livemode": false,
            "cancel_at_period_end": true, "customer": "cus_1",
            "metadata": { "billing_token": token.to_string() },
            "items": { "data": [{ "current_period_end": 1_800_000_000,
                "price": { "id": "price_f", "unit_amount": 4800, "currency": "eur" } }] },
            "latest_invoice": { "status_transitions": { "paid_at": 1_700_000_000 } },
        });
        let s = snapshot(&config, &json).unwrap();
        assert_eq!(s.plan, "family");
        assert_eq!(s.status, Status::Active);
        assert_eq!(s.link, Link::Token(token));
        assert_eq!(s.price_minor, 4800);
        assert_eq!(s.currency, "EUR");
        assert_eq!(s.period_end_ms, 1_800_000_000_000);
        assert!(s.canceling && s.sandbox);
        assert_eq!(s.last_charge_ms, Some(1_700_000_000_000));

        let mut other = json.clone();
        other["items"]["data"][0]["price"]["id"] = "price_other".into();
        assert!(snapshot(&config, &other).is_none());
        let mut ended = json.clone();
        ended["status"] = "canceled".into();
        ended["metadata"] = serde_json::json!({});
        let s = snapshot(&config, &ended).unwrap();
        assert_eq!(s.status, Status::Expired);
        assert_eq!(s.link, Link::StripeCustomer("cus_1".into()));
    }

    #[test]
    fn events_name_their_subscription() {
        let event = |object: Value| serde_json::json!({ "data": { "object": object } });
        assert_eq!(
            event_subscription(&event(
                serde_json::json!({ "object": "subscription", "id": "sub_a" })
            )),
            Some("sub_a".into())
        );
        assert_eq!(
            event_subscription(&event(
                serde_json::json!({ "object": "checkout.session", "subscription": "sub_b" })
            )),
            Some("sub_b".into())
        );
        assert_eq!(
            event_subscription(&event(serde_json::json!({ "object": "invoice",
                "parent": { "subscription_details": { "subscription": "sub_c" } } }))),
            Some("sub_c".into())
        );
        assert_eq!(
            event_subscription(&event(
                serde_json::json!({ "object": "subscription", "id": "sub_../x" })
            )),
            None
        );
        assert_eq!(
            event_subscription(&event(
                serde_json::json!({ "object": "charge", "id": "ch_1" })
            )),
            None
        );
    }
}
