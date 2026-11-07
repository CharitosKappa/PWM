//! The App Store (D6 §2, §3): the iOS app buys with StoreKit 2, passing the
//! account's billing token as `appAccountToken`. The server reads the
//! subscription from the App Store Server API over TLS, with an ES256
//! token like APNs', so Apple's signed payloads are never trusted on their
//! own. A purchase Apple doesn't know in production is looked for in the
//! sandbox, where App Review buys.

use std::sync::Mutex;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http_body_util::Full;
use hyper::body::Bytes;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::pkcs8::DecodePrivateKey;
use serde_json::Value;
use tokio::time::Instant;
use uuid::Uuid;

use super::http::{Http, currency, jws_payload, minor_units, unix_secs};
use super::{BillingError, Link, Products, Provider, Snapshot, Status};
use crate::ServerError;

const PRODUCTION_URL: &str = "https://api.storekit.itunes.apple.com";
const SANDBOX_URL: &str = "https://api.storekit-sandbox.itunes.apple.com";
/// Apple refuses a token valid more than an hour; one is made every 20 minutes.
const TOKEN_LASTS: u64 = 30 * 60;
const TOKEN_EVERY: Duration = Duration::from_secs(20 * 60);

/// `Debug` shows neither the key nor the ids.
pub struct AppleConfig {
    key: SigningKey,
    key_id: String,
    issuer_id: String,
    bundle_id: String,
    pub(crate) products: Products,
    url: String,
    sandbox_url: String,
    token: Mutex<Option<(String, Instant)>>,
}

impl std::fmt::Debug for AppleConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AppleConfig { .. }")
    }
}

impl AppleConfig {
    /// From the in-app purchase key's `.p8` PEM text and the ids Apple
    /// gives with it; the URLs replace Apple's (tests).
    pub fn new(
        key_pem: &str,
        key_id: &str,
        issuer_id: &str,
        bundle_id: &str,
        products: &str,
        urls: Option<(&str, &str)>,
    ) -> Result<Self, ServerError> {
        let key = SigningKey::from_pkcs8_pem(key_pem.trim()).map_err(|_| {
            ServerError::Config("PWM_APPLE_IAP_KEY_FILE is not an in-app purchase .p8 key")
        })?;
        let plain = |s: &str, extra: &[u8]| {
            !s.is_empty()
                && s.len() <= 100
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
        };
        if !plain(key_id, b"") || !plain(issuer_id, b"-") || !plain(bundle_id, b".-") {
            return Err(ServerError::Config(
                "PWM_APPLE_IAP_KEY_ID, PWM_APPLE_IAP_ISSUER_ID and PWM_APPLE_BUNDLE_ID are Apple's ids",
            ));
        }
        let products = Products::parse(products).ok_or(ServerError::Config(
            "PWM_APPLE_PRODUCTS is not a list like premium=com.example.premium.yearly",
        ))?;
        let (url, sandbox_url) = urls.unwrap_or((PRODUCTION_URL, SANDBOX_URL));
        Ok(Self {
            key,
            key_id: key_id.to_owned(),
            issuer_id: issuer_id.to_owned(),
            bundle_id: bundle_id.to_owned(),
            products,
            url: url.trim_end_matches('/').to_owned(),
            sandbox_url: sandbox_url.trim_end_matches('/').to_owned(),
            token: Mutex::new(None),
        })
    }

    /// `PWM_APPLE_IAP_KEY_FILE`, `PWM_APPLE_IAP_KEY_ID`,
    /// `PWM_APPLE_IAP_ISSUER_ID`, `PWM_APPLE_BUNDLE_ID`,
    /// `PWM_APPLE_PRODUCTS`, and the optional `PWM_APPLE_URL` and
    /// `PWM_APPLE_SANDBOX_URL` (both or neither). No key: the App Store is off.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Option<Self>, ServerError> {
        let get = |name| var(name).filter(|v| !v.is_empty());
        let Some(path) = get("PWM_APPLE_IAP_KEY_FILE") else {
            return Ok(None);
        };
        let pem = std::fs::read_to_string(path)
            .map_err(|_| ServerError::Config("PWM_APPLE_IAP_KEY_FILE cannot be read"))?;
        let (Some(key_id), Some(issuer), Some(bundle), Some(products)) = (
            get("PWM_APPLE_IAP_KEY_ID"),
            get("PWM_APPLE_IAP_ISSUER_ID"),
            get("PWM_APPLE_BUNDLE_ID"),
            get("PWM_APPLE_PRODUCTS"),
        ) else {
            return Err(ServerError::Config(
                "PWM_APPLE_IAP_KEY_FILE needs PWM_APPLE_IAP_KEY_ID, PWM_APPLE_IAP_ISSUER_ID, PWM_APPLE_BUNDLE_ID and PWM_APPLE_PRODUCTS",
            ));
        };
        let urls = match (get("PWM_APPLE_URL"), get("PWM_APPLE_SANDBOX_URL")) {
            (Some(a), Some(b)) => Some((a, b)),
            (None, None) => None,
            _ => {
                return Err(ServerError::Config(
                    "PWM_APPLE_URL and PWM_APPLE_SANDBOX_URL go together",
                ));
            }
        };
        Self::new(
            &pem,
            &key_id,
            &issuer,
            &bundle,
            &products,
            urls.as_ref().map(|(a, b)| (a.as_str(), b.as_str())),
        )
        .map(Some)
    }

    /// The API token, made again every 20 minutes.
    fn bearer(&self) -> Option<String> {
        let mut cached = self.token.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((token, at)) = cached.as_ref()
            && at.elapsed() < TOKEN_EVERY
        {
            return Some(token.clone());
        }
        let token = api_token(self, unix_secs())?;
        *cached = Some((token.clone(), Instant::now()));
        Some(token)
    }

    fn forget_token(&self) {
        *self.token.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// The App Store Server API's ES256 token.
fn api_token(config: &AppleConfig, iat: u64) -> Option<String> {
    let header = serde_json::json!({ "alg": "ES256", "kid": config.key_id, "typ": "JWT" });
    let claims = serde_json::json!({
        "iss": config.issuer_id,
        "iat": iat,
        "exp": iat + TOKEN_LASTS,
        "aud": "appstoreconnect-v1",
        "bid": config.bundle_id,
    });
    let signed = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    let signature: Signature = config.key.try_sign(signed.as_bytes()).ok()?;
    Some(format!(
        "{signed}.{}",
        URL_SAFE_NO_PAD.encode(signature.to_bytes())
    ))
}

/// A transaction id: digits only.
pub(crate) fn plain_transaction(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.bytes().all(|b| b.is_ascii_digit())
}

/// The subscription a transaction belongs to, from Apple. `sandbox` says
/// where to ask; `None` asks production, then the sandbox.
pub(crate) async fn fetch(
    config: &AppleConfig,
    http: &Http,
    transaction_id: &str,
    sandbox: Option<bool>,
) -> Result<Snapshot, BillingError> {
    if !plain_transaction(transaction_id) {
        return Err(BillingError::NotFound);
    }
    let places: &[bool] = match sandbox {
        Some(true) => &[true],
        Some(false) => &[false],
        None => &[false, true],
    };
    for &in_sandbox in places {
        let base = if in_sandbox {
            &config.sandbox_url
        } else {
            &config.url
        };
        let bearer = config.bearer().ok_or(BillingError::Unavailable)?;
        let request =
            hyper::Request::get(format!("{base}/inApps/v1/subscriptions/{transaction_id}"))
                .header("authorization", format!("Bearer {bearer}"))
                .body(Full::new(Bytes::new()))
                .map_err(|_| BillingError::Unavailable)?;
        let (status, body) = http.send(request).await?;
        match status {
            200 => {
                let answer: Value =
                    serde_json::from_slice(&body).map_err(|_| BillingError::Unavailable)?;
                return statuses(config, &answer, transaction_id, in_sandbox)
                    .ok_or(BillingError::NotFound);
            }
            404 => continue,
            401 => {
                config.forget_token();
                tracing::warn!("billing: the App Store refused the API token");
                return Err(BillingError::Unavailable);
            }
            status => {
                tracing::warn!("billing: the App Store answered {status}");
                return Err(BillingError::Unavailable);
            }
        }
    }
    Err(BillingError::NotFound)
}

/// "Get All Subscription Statuses" as a snapshot of the subscription that
/// `transaction_id` belongs to.
pub(crate) fn statuses(
    config: &AppleConfig,
    answer: &Value,
    transaction_id: &str,
    sandbox: bool,
) -> Option<Snapshot> {
    if answer["bundleId"].as_str() != Some(config.bundle_id.as_str()) {
        return None;
    }
    let groups = answer["data"].as_array()?;
    let mut found = None;
    for last in groups
        .iter()
        .filter_map(|g| g["lastTransactions"].as_array())
        .flatten()
    {
        let Some(transaction) = last["signedTransactionInfo"].as_str().and_then(jws_payload) else {
            continue;
        };
        let original = transaction["originalTransactionId"].as_str().unwrap_or("");
        let id = transaction["transactionId"].as_str().unwrap_or("");
        if original == transaction_id || id == transaction_id || found.is_none() {
            let exact = original == transaction_id || id == transaction_id;
            found = Some((last.clone(), transaction));
            if exact {
                break;
            }
        }
    }
    let (last, transaction) = found?;
    let renewal = last["signedRenewalInfo"]
        .as_str()
        .and_then(jws_payload)
        .unwrap_or(Value::Null);
    if transaction["bundleId"].as_str() != Some(config.bundle_id.as_str()) {
        return None;
    }
    let plan = config
        .products
        .plan_for(transaction["productId"].as_str()?)?;
    let original = transaction["originalTransactionId"].as_str()?;
    if !plain_transaction(original) {
        return None;
    }
    let expires = transaction["expiresDate"].as_i64()?;
    let (status, grace) = match last["status"].as_i64()? {
        1 => (Status::Active, None),
        // Billing grace period: still counts until it ends.
        4 => (
            Status::PastDue,
            Some(
                renewal["gracePeriodExpiresDate"]
                    .as_i64()
                    .unwrap_or(expires),
            ),
        ),
        // Billing retry without grace: doesn't count, but may recover.
        3 => (Status::PastDue, Some(expires)),
        _ => (Status::Expired, None),
    };
    let status = if transaction["revocationDate"].is_i64() {
        Status::Expired
    } else {
        status
    };
    let currency_code = transaction["currency"]
        .as_str()
        .and_then(currency)
        .unwrap_or_else(|| "USD".to_owned());
    let price = transaction["price"]
        .as_i64()
        .and_then(|p| minor_units(p, 3, &currency_code))
        .unwrap_or(0);
    let link = transaction["appAccountToken"]
        .as_str()
        .and_then(|t| t.parse::<Uuid>().ok())
        .map_or(Link::None, Link::Token);
    Some(Snapshot {
        provider: Provider::AppStore,
        provider_ref: original.to_owned(),
        link,
        plan,
        status,
        price_minor: price,
        currency: currency_code,
        period_end_ms: expires,
        canceling: renewal["autoRenewStatus"].as_i64() == Some(0),
        grace_until_ms: grace,
        last_charge_ms: transaction["purchaseDate"].as_i64(),
        sandbox,
        replaces: None,
        acknowledge: None,
    })
}

/// What an App Store Server Notification (V2) points at: its id, the
/// original transaction and whether it came from the sandbox. Nothing in
/// it is trusted beyond that (§3). `None` for one without a transaction,
/// such as Apple's test notification.
pub(crate) fn notification(
    config: &AppleConfig,
    body: &Value,
) -> Option<(String, Option<(String, bool)>)> {
    let payload = jws_payload(body["signedPayload"].as_str()?)?;
    let id = payload["notificationUUID"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 100)?
        .to_owned();
    let data = &payload["data"];
    if data["bundleId"].as_str() != Some(config.bundle_id.as_str()) {
        return Some((id, None));
    }
    let sandbox = data["environment"].as_str() == Some("Sandbox");
    let original = data["signedTransactionInfo"]
        .as_str()
        .and_then(jws_payload)
        .and_then(|t| t["originalTransactionId"].as_str().map(str::to_owned))
        .filter(|t| plain_transaction(t));
    Some((id, original.map(|t| (t, sandbox))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::VerifyingKey;
    use p256::ecdsa::signature::Verifier;
    use p256::pkcs8::{EncodePrivateKey, LineEnding};

    pub(crate) fn jws(payload: &Value) -> String {
        format!(
            "eyJhbGciOiJFUzI1NiJ9.{}.c2ln",
            URL_SAFE_NO_PAD.encode(payload.to_string())
        )
    }

    fn config() -> (AppleConfig, VerifyingKey) {
        let key = SigningKey::from_slice(&[9u8; 32]).unwrap();
        let verifying = *key.verifying_key();
        let pem = key.to_pkcs8_pem(LineEnding::LF).unwrap();
        let config = AppleConfig::new(
            &pem,
            "KEY1234567",
            "57246542-96fe-1a63-e053-0824d011072a",
            "invalid.pwm.ios",
            "premium=invalid.pwm.premium.yearly",
            None,
        )
        .unwrap();
        (config, verifying)
    }

    #[test]
    fn the_api_token_is_es256_for_the_app() {
        let (config, verifying) = config();
        let jwt = api_token(&config, 1_700_000_000).unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        let header: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        let claims: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(
            header,
            serde_json::json!({ "alg": "ES256", "kid": "KEY1234567", "typ": "JWT" })
        );
        assert_eq!(
            claims,
            serde_json::json!({ "iss": "57246542-96fe-1a63-e053-0824d011072a",
                "iat": 1_700_000_000, "exp": 1_700_001_800,
                "aud": "appstoreconnect-v1", "bid": "invalid.pwm.ios" })
        );
        let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
        let signed = format!("{}.{}", parts[0], parts[1]);
        assert!(verifying.verify(signed.as_bytes(), &signature).is_ok());
        assert_eq!(format!("{config:?}"), "AppleConfig { .. }");
    }

    #[test]
    fn statuses_become_snapshots() {
        let (config, _) = config();
        let token = Uuid::from_u128(5);
        let transaction = serde_json::json!({
            "transactionId": "2000000002", "originalTransactionId": "2000000001",
            "bundleId": "invalid.pwm.ios", "productId": "invalid.pwm.premium.yearly",
            "purchaseDate": 1_700_000_000_000i64, "expiresDate": 1_731_536_000_000i64,
            "appAccountToken": token.to_string(), "price": 23_990, "currency": "EUR",
        });
        let answer = |status: i64, renewal: Value| {
            serde_json::json!({ "bundleId": "invalid.pwm.ios", "data": [{ "lastTransactions": [{
                "originalTransactionId": "2000000001", "status": status,
                "signedTransactionInfo": jws(&transaction), "signedRenewalInfo": jws(&renewal),
            }]}]})
        };
        let s = statuses(
            &config,
            &answer(1, serde_json::json!({ "autoRenewStatus": 0 })),
            "2000000002",
            true,
        )
        .unwrap();
        assert_eq!(s.provider_ref, "2000000001");
        assert_eq!(s.link, Link::Token(token));
        assert_eq!(
            (s.status, s.canceling, s.sandbox),
            (Status::Active, true, true)
        );
        assert_eq!((s.price_minor, s.currency.as_str()), (2399, "EUR"));
        assert_eq!(s.period_end_ms, 1_731_536_000_000);

        let grace = statuses(
            &config,
            &answer(
                4,
                serde_json::json!({ "gracePeriodExpiresDate": 1_731_999_000_000i64 }),
            ),
            "2000000001",
            false,
        )
        .unwrap();
        assert_eq!(grace.status, Status::PastDue);
        assert_eq!(grace.grace_until_ms, Some(1_731_999_000_000));
        let expired = statuses(&config, &answer(2, Value::Null), "2000000001", false).unwrap();
        assert_eq!(expired.status, Status::Expired);

        let mut other_app = answer(1, Value::Null);
        other_app["bundleId"] = "com.other".into();
        assert!(statuses(&config, &other_app, "2000000001", false).is_none());
    }

    #[test]
    fn notifications_point_at_a_transaction() {
        let (config, _) = config();
        let payload = serde_json::json!({
            "notificationUUID": "n-1", "notificationType": "DID_RENEW",
            "data": { "bundleId": "invalid.pwm.ios", "environment": "Sandbox",
                "signedTransactionInfo": jws(&serde_json::json!({ "originalTransactionId": "77" })) },
        });
        let body = serde_json::json!({ "signedPayload": jws(&payload) });
        assert_eq!(
            notification(&config, &body),
            Some(("n-1".into(), Some(("77".into(), true))))
        );
        let test = serde_json::json!({ "signedPayload": jws(&serde_json::json!({
            "notificationUUID": "n-2", "notificationType": "TEST",
            "data": { "bundleId": "invalid.pwm.ios" } })) });
        assert_eq!(notification(&config, &test), Some(("n-2".into(), None)));
        assert_eq!(notification(&config, &serde_json::json!({})), None);
    }
}
