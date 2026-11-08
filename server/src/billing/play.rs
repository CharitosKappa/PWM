//! Google Play (D6 §2, §3): the Android app buys with Play Billing, passing
//! the account's billing token as `obfuscatedAccountId`. The server reads
//! the purchase from the Play Developer API (`purchases.subscriptionsv2`)
//! with a service account's OAuth token, made as for FCM (D60), and
//! acknowledges it. Real-time notifications come through a Pub/Sub push to
//! a URL carrying our secret, and only say which purchase to read again.

use std::sync::Mutex;
use std::time::Duration;

use base64::Engine as _;
use http_body_util::Full;
use hyper::body::Bytes;
use pwm_crypto::{Secret, secret_matches};
use serde_json::Value;
use tokio::time::Instant;
use uuid::Uuid;

use super::http::{Http, currency, minor_units, rfc3339_ms, unix_secs};
use super::{BillingError, Link, Products, Provider, Snapshot, Status};
use crate::ServerError;
use crate::push::{google_assertion, pem_private_key};

/// The only scope asked for.
pub const PLAY_SCOPE: &str = "https://www.googleapis.com/auth/androidpublisher";
const PLAY_URL: &str = "https://androidpublisher.googleapis.com";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const ACCESS_MARGIN: Duration = Duration::from_secs(5 * 60);

/// `Debug` shows neither the key, the account nor the secret.
pub struct PlayConfig {
    key: ring::signature::RsaKeyPair,
    client_email: String,
    package: String,
    pub(crate) products: Products,
    notify_secret: Secret<String>,
    url: String,
    token_url: String,
    access: Mutex<Option<(String, Instant)>>,
}

impl std::fmt::Debug for PlayConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PlayConfig { .. }")
    }
}

impl PlayConfig {
    /// From the service account's JSON file; the URLs replace Google's (tests).
    pub fn new(
        credentials: &str,
        package: &str,
        products: &str,
        notify_secret: &str,
        urls: Option<(&str, &str)>,
    ) -> Result<Self, ServerError> {
        let bad = || {
            ServerError::Config("PWM_PLAY_CREDENTIALS_FILE is not a service account's JSON file")
        };
        let file: Value = serde_json::from_str(credentials).map_err(|_| bad())?;
        let client_email = file["client_email"]
            .as_str()
            .filter(|e| {
                !e.is_empty()
                    && e.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.@".contains(&b))
            })
            .ok_or_else(bad)?;
        let der = file["private_key"]
            .as_str()
            .and_then(pem_private_key)
            .ok_or_else(bad)?;
        let key = ring::signature::RsaKeyPair::from_pkcs8(&der).map_err(|_| bad())?;
        let package_ok = !package.is_empty()
            && package.len() <= 150
            && package
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._".contains(&b));
        if !package_ok {
            return Err(ServerError::Config(
                "PWM_PLAY_PACKAGE is the Android app's package name",
            ));
        }
        let products = Products::parse(products).ok_or(ServerError::Config(
            "PWM_PLAY_PRODUCTS is not a list like premium=premium_yearly",
        ))?;
        if notify_secret.len() < 32 || !notify_secret.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(ServerError::Config(
                "PWM_PLAY_NOTIFY_SECRET is at least 32 letters and digits",
            ));
        }
        let (url, token_url) = urls.unwrap_or((PLAY_URL, TOKEN_URL));
        Ok(Self {
            key,
            client_email: client_email.to_owned(),
            package: package.to_owned(),
            products,
            notify_secret: Secret::new(notify_secret.to_owned()),
            url: url.trim_end_matches('/').to_owned(),
            token_url: token_url.trim_end_matches('/').to_owned(),
            access: Mutex::new(None),
        })
    }

    /// `PWM_PLAY_CREDENTIALS_FILE`, `PWM_PLAY_PACKAGE`, `PWM_PLAY_PRODUCTS`,
    /// `PWM_PLAY_NOTIFY_SECRET`, and the optional `PWM_PLAY_URL` and
    /// `PWM_PLAY_TOKEN_URL` (both or neither). No file: Play is off.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Option<Self>, ServerError> {
        let get = |name| var(name).filter(|v| !v.is_empty());
        let Some(path) = get("PWM_PLAY_CREDENTIALS_FILE") else {
            return Ok(None);
        };
        let text = std::fs::read_to_string(path)
            .map_err(|_| ServerError::Config("PWM_PLAY_CREDENTIALS_FILE cannot be read"))?;
        let (Some(package), Some(products), Some(secret)) = (
            get("PWM_PLAY_PACKAGE"),
            get("PWM_PLAY_PRODUCTS"),
            get("PWM_PLAY_NOTIFY_SECRET"),
        ) else {
            return Err(ServerError::Config(
                "PWM_PLAY_CREDENTIALS_FILE needs PWM_PLAY_PACKAGE, PWM_PLAY_PRODUCTS and PWM_PLAY_NOTIFY_SECRET",
            ));
        };
        let urls = match (get("PWM_PLAY_URL"), get("PWM_PLAY_TOKEN_URL")) {
            (Some(a), Some(b)) => Some((a, b)),
            (None, None) => None,
            _ => {
                return Err(ServerError::Config(
                    "PWM_PLAY_URL and PWM_PLAY_TOKEN_URL go together",
                ));
            }
        };
        Self::new(
            &text,
            &package,
            &products,
            &secret,
            urls.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
        )
        .map(Some)
    }

    /// Whether a push URL's last segment is our secret.
    pub(crate) fn secret_ok(&self, given: &str) -> bool {
        secret_matches(
            given.as_bytes(),
            self.notify_secret.expose_secret().as_bytes(),
        )
    }

    /// The OAuth access token, kept until shortly before it expires.
    async fn access_token(&self, http: &Http) -> Result<String, BillingError> {
        {
            let cached = self.access.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((token, until)) = cached.as_ref()
                && Instant::now() < *until
            {
                return Ok(token.clone());
            }
        }
        let assertion = google_assertion(
            &self.key,
            &self.client_email,
            PLAY_SCOPE,
            &self.token_url,
            unix_secs(),
        )
        .ok_or(BillingError::Unavailable)?;
        let body = format!(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={assertion}"
        );
        let request = hyper::Request::post(&self.token_url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Full::new(Bytes::from(body)))
            .map_err(|_| BillingError::Unavailable)?;
        let (status, bytes) = http.send(request).await?;
        if status != 200 {
            tracing::warn!("billing: Google's token endpoint answered {status}");
            return Err(BillingError::Unavailable);
        }
        let answer: Value =
            serde_json::from_slice(&bytes).map_err(|_| BillingError::Unavailable)?;
        let token = answer["access_token"]
            .as_str()
            .filter(|t| !t.is_empty())
            .ok_or(BillingError::Unavailable)?
            .to_owned();
        let lasts = Duration::from_secs(answer["expires_in"].as_u64().unwrap_or(3600));
        let until = Instant::now()
            + lasts
                .saturating_sub(ACCESS_MARGIN)
                .max(Duration::from_secs(60));
        *self.access.lock().unwrap_or_else(|e| e.into_inner()) = Some((token.clone(), until));
        Ok(token)
    }

    fn forget_token(&self) {
        *self.access.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// A purchase token: letters, digits, `.`, `-` and `_`.
pub(crate) fn plain_token(token: &str) -> bool {
    (10..=1024).contains(&token.len())
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// The purchase as Play has it now.
pub(crate) async fn fetch(
    config: &PlayConfig,
    http: &Http,
    token: &str,
) -> Result<Snapshot, BillingError> {
    if !plain_token(token) {
        return Err(BillingError::NotFound);
    }
    let access = config.access_token(http).await?;
    let request = hyper::Request::get(format!(
        "{}/androidpublisher/v3/applications/{}/purchases/subscriptionsv2/tokens/{token}",
        config.url, config.package
    ))
    .header("authorization", format!("Bearer {access}"))
    .body(Full::new(Bytes::new()))
    .map_err(|_| BillingError::Unavailable)?;
    let (status, body) = http.send(request).await?;
    match status {
        200 => {
            let purchase: Value =
                serde_json::from_slice(&body).map_err(|_| BillingError::Unavailable)?;
            snapshot(config, &purchase, token)
        }
        // Unknown or too old a token.
        400 | 404 | 410 => Err(BillingError::NotFound),
        401 => {
            config.forget_token();
            tracing::warn!("billing: Play refused the access token");
            Err(BillingError::Unavailable)
        }
        status => {
            tracing::warn!("billing: Play answered {status}");
            Err(BillingError::Unavailable)
        }
    }
}

/// A `SubscriptionPurchaseV2` as a snapshot.
pub(crate) fn snapshot(
    config: &PlayConfig,
    p: &Value,
    token: &str,
) -> Result<Snapshot, BillingError> {
    let item = &p["lineItems"][0];
    let product = item["productId"].as_str().unwrap_or("");
    let plan = config
        .products
        .plan_for(product)
        .ok_or(BillingError::NotFound)?;
    let expiry = item["expiryTime"]
        .as_str()
        .and_then(rfc3339_ms)
        .ok_or(BillingError::NotFound)?;
    let renews = item["autoRenewingPlan"]["autoRenewEnabled"]
        .as_bool()
        .unwrap_or(false);
    let (status, canceling, grace) = match p["subscriptionState"].as_str().unwrap_or("") {
        "SUBSCRIPTION_STATE_ACTIVE" => (Status::Active, !renews, None),
        // Turned off but paid to the end of the period.
        "SUBSCRIPTION_STATE_CANCELED" => (Status::Active, true, None),
        "SUBSCRIPTION_STATE_IN_GRACE_PERIOD" => (Status::PastDue, false, Some(expiry)),
        "SUBSCRIPTION_STATE_ON_HOLD" => (Status::PastDue, false, Some(expiry.min(now_ms()))),
        "SUBSCRIPTION_STATE_PENDING" => return Err(BillingError::Pending),
        "SUBSCRIPTION_STATE_PAUSED"
        | "SUBSCRIPTION_STATE_EXPIRED"
        | "SUBSCRIPTION_STATE_PENDING_PURCHASE_CANCELED" => (Status::Expired, false, None),
        _ => return Err(BillingError::Unavailable),
    };
    let price = &item["autoRenewingPlan"]["recurringPrice"];
    let currency_code = price["currencyCode"]
        .as_str()
        .and_then(currency)
        .unwrap_or_else(|| "USD".to_owned());
    let units = price["units"]
        .as_str()
        .and_then(|u| u.parse::<i64>().ok())
        .unwrap_or(0);
    let nanos = price["nanos"].as_i64().unwrap_or(0);
    let price_minor = units
        .checked_mul(1_000_000_000)
        .and_then(|u| u.checked_add(nanos))
        .and_then(|amount| minor_units(amount, 9, &currency_code))
        .unwrap_or(0);
    let link = p["externalAccountIdentifiers"]["obfuscatedExternalAccountId"]
        .as_str()
        .and_then(|t| t.parse::<Uuid>().ok())
        .map_or(Link::None, Link::Token);
    let replaces = p["linkedPurchaseToken"]
        .as_str()
        .filter(|t| plain_token(t) && *t != token)
        .map(str::to_owned);
    Ok(Snapshot {
        provider: Provider::Play,
        provider_ref: token.to_owned(),
        link,
        plan,
        status,
        price_minor,
        currency: currency_code,
        period_start_ms: None,
        period_end_ms: expiry,
        canceling,
        grace_until_ms: grace,
        last_charge_ms: None,
        sandbox: p["testPurchase"].is_object(),
        replaces,
        acknowledge: (p["acknowledgementState"].as_str() == Some("ACKNOWLEDGEMENT_STATE_PENDING"))
            .then(|| product.to_owned()),
    })
}

fn now_ms() -> i64 {
    i64::try_from(unix_secs()).unwrap_or(i64::MAX / 1000) * 1000
}

/// Acknowledges a purchase, as Google requires within three days. Best
/// effort: a failure is retried on the next read.
pub(crate) async fn acknowledge(config: &PlayConfig, http: &Http, token: &str, product: &str) {
    let Ok(access) = config.access_token(http).await else {
        return;
    };
    let request = hyper::Request::post(format!(
        "{}/androidpublisher/v3/applications/{}/purchases/subscriptions/{product}/tokens/{token}:acknowledge",
        config.url, config.package
    ))
    .header("authorization", format!("Bearer {access}"))
    .header("content-type", "application/json")
    .body(Full::new(Bytes::from_static(b"{}")));
    let Ok(request) = request else {
        return;
    };
    match http.send(request).await {
        Ok((200 | 204, _)) => {}
        Ok((status, _)) => tracing::warn!("billing: Play's acknowledge answered {status}"),
        Err(_) => tracing::warn!("billing: Play's acknowledge failed"),
    }
}

/// Refunds the unused part of a purchase and ends it (D72 §1): Google
/// works out the pro-rata amount.
pub(crate) async fn revoke_prorated(
    config: &PlayConfig,
    http: &Http,
    token: &str,
) -> Result<(), BillingError> {
    if !plain_token(token) {
        return Err(BillingError::NotFound);
    }
    let access = config.access_token(http).await?;
    let request = hyper::Request::post(format!(
        "{}/androidpublisher/v3/applications/{}/purchases/subscriptionsv2/tokens/{token}:revoke",
        config.url, config.package
    ))
    .header("authorization", format!("Bearer {access}"))
    .header("content-type", "application/json")
    .body(Full::new(Bytes::from_static(
        br#"{"revocationContext":{"proratedRefund":{}}}"#,
    )))
    .map_err(|_| BillingError::Unavailable)?;
    match http.send(request).await? {
        (200 | 204, _) => Ok(()),
        (400 | 404 | 410, _) => Err(BillingError::NotFound),
        (status, _) => {
            tracing::warn!("billing: Play's revoke answered {status}");
            Err(BillingError::Unavailable)
        }
    }
}

/// What a Pub/Sub push carries: its message id, and the purchase token of
/// a subscription notification for our package. Nothing else is trusted.
pub(crate) fn notification(config: &PlayConfig, body: &Value) -> Option<(String, Option<String>)> {
    let message = &body["message"];
    let id = message["messageId"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 100)?
        .to_owned();
    let data = base64::engine::general_purpose::STANDARD
        .decode(message["data"].as_str().unwrap_or(""))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or(Value::Null);
    if data["packageName"].as_str() != Some(config.package.as_str()) {
        return Some((id, None));
    }
    let token = data["subscriptionNotification"]["purchaseToken"]
        .as_str()
        .or_else(|| data["voidedPurchaseNotification"]["purchaseToken"].as_str())
        .filter(|t| plain_token(t))
        .map(str::to_owned);
    Some((id, token))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_RSA_KEY: &str = include_str!("../../tests/fixtures/fcm-test-key.pem");

    pub(crate) fn credentials() -> String {
        serde_json::json!({
            "type": "service_account",
            "client_email": "billing@pwm-test.iam.gserviceaccount.com",
            "private_key": TEST_RSA_KEY,
        })
        .to_string()
    }

    fn config() -> PlayConfig {
        PlayConfig::new(
            &credentials(),
            "invalid.pwm.android",
            "premium=premium_yearly",
            "0123456789abcdef0123456789abcdef",
            None,
        )
        .unwrap()
    }

    #[test]
    fn settings_are_checked_and_never_printed() {
        let config = config();
        assert_eq!(format!("{config:?}"), "PlayConfig { .. }");
        assert!(config.secret_ok("0123456789abcdef0123456789abcdef"));
        assert!(!config.secret_ok("0123456789abcdef0123456789abcdeF"));
        assert!(
            PlayConfig::new(&credentials(), "a b", "premium=p", &"x".repeat(32), None).is_err()
        );
        assert!(PlayConfig::new(&credentials(), "a.b", "premium=p", "short", None).is_err());
        assert!(PlayConfig::new("{}", "a.b", "premium=p", &"x".repeat(32), None).is_err());
        assert!(PlayConfig::from_vars(|_| None).unwrap().is_none());
    }

    #[test]
    fn purchases_become_snapshots() {
        let config = config();
        let token = Uuid::from_u128(3);
        let purchase = |state: &str| {
            serde_json::json!({
                "subscriptionState": state, "testPurchase": {},
                "linkedPurchaseToken": "old.token-123",
                "externalAccountIdentifiers": { "obfuscatedExternalAccountId": token.to_string() },
                "lineItems": [{ "productId": "premium_yearly", "expiryTime": "2027-01-01T00:00:00Z",
                    "autoRenewingPlan": { "autoRenewEnabled": true,
                        "recurringPrice": { "currencyCode": "EUR", "units": "23", "nanos": 990_000_000 } } }],
            })
        };
        let s = snapshot(
            &config,
            &purchase("SUBSCRIPTION_STATE_ACTIVE"),
            "new.token-456",
        )
        .unwrap();
        assert_eq!(s.link, Link::Token(token));
        assert_eq!(
            (s.status, s.canceling, s.sandbox),
            (Status::Active, false, true)
        );
        assert_eq!((s.price_minor, s.currency.as_str()), (2399, "EUR"));
        assert_eq!(s.period_end_ms, 1_798_761_600_000);
        assert_eq!(s.replaces.as_deref(), Some("old.token-123"));
        let canceled = snapshot(
            &config,
            &purchase("SUBSCRIPTION_STATE_CANCELED"),
            "new.token-456",
        )
        .unwrap();
        assert_eq!(
            (canceled.status, canceled.canceling),
            (Status::Active, true)
        );
        let grace = snapshot(
            &config,
            &purchase("SUBSCRIPTION_STATE_IN_GRACE_PERIOD"),
            "new.token-456",
        )
        .unwrap();
        assert_eq!(grace.grace_until_ms, Some(1_798_761_600_000));
        assert_eq!(
            snapshot(
                &config,
                &purchase("SUBSCRIPTION_STATE_PENDING"),
                "t".repeat(20).as_str()
            )
            .unwrap_err(),
            BillingError::Pending
        );
        let mut other = purchase("SUBSCRIPTION_STATE_ACTIVE");
        other["lineItems"][0]["productId"] = "other".into();
        assert_eq!(
            snapshot(&config, &other, "new.token-456").unwrap_err(),
            BillingError::NotFound
        );
    }

    #[test]
    fn notifications_point_at_a_purchase() {
        let config = config();
        let push = |data: Value| {
            serde_json::json!({ "message": { "messageId": "m-1",
                "data": base64::engine::general_purpose::STANDARD.encode(data.to_string()) },
                "subscription": "projects/p/subscriptions/s" })
        };
        assert_eq!(
            notification(
                &config,
                &push(serde_json::json!({ "packageName": "invalid.pwm.android",
                "subscriptionNotification": { "purchaseToken": "tok.en-123456", "notificationType": 4 } }))
            ),
            Some(("m-1".into(), Some("tok.en-123456".into())))
        );
        assert_eq!(
            notification(
                &config,
                &push(serde_json::json!({ "packageName": "other",
                "subscriptionNotification": { "purchaseToken": "tok.en-123456" } }))
            ),
            Some(("m-1".into(), None))
        );
        assert_eq!(notification(&config, &serde_json::json!({})), None);
    }
}
