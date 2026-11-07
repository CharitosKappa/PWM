//! Push over APNs (D50 §3) and FCM (D60). After an item write or delete
//! commits, the other devices of the vault's members get a content-free
//! background push (`{"aps":{"content-available":1}}`, or FCM data
//! `{"t":"sync"}`), at most one a minute per device. A device approval
//! request sends a notice to the account's devices: fixed text over APNs,
//! `{"t":"approval"}` over FCM, whose app has the words. Best effort:
//! nothing is retried or queued on disk, and logs hold counts and statuses
//! only, never a token.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper_util::client::legacy::Client;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::rt::TokioExecutor;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::pkcs8::DecodePrivateKey;
use sqlx::{PgPool, Row};
use tokio::sync::mpsc;
use tokio::time::Instant;
use uuid::Uuid;

use crate::ServerError;

/// The fixed text of the approval notice. It names no email or device.
pub const APPROVAL_NOTICE: &str = "A new device wants to sign in. Open PWM to compare the code.";
/// The background push, byte for byte.
pub const BACKGROUND_PAYLOAD: &str = r#"{"aps":{"content-available":1}}"#;
/// At most one background push per device in this time.
pub const PUSH_EVERY: Duration = Duration::from_secs(60);
/// A provider token is made again after this long (APNs refuses one older
/// than an hour).
const JWT_EVERY: Duration = Duration::from_secs(50 * 60);
/// A background push still worth delivering to a device that was off.
const BACKGROUND_EXPIRES_SECS: u64 = 3600;
/// An approval request lives 10 minutes (D46), so its notice does too.
const APPROVAL_EXPIRES_SECS: u64 = 600;
const PRODUCTION_URL: &str = "https://api.push.apple.com";
const SANDBOX_URL: &str = "https://api.sandbox.push.apple.com";
/// Events waiting for the sender; more are dropped.
const QUEUE: usize = 1024;
/// FCM's send API (HTTP v1).
const FCM_URL: &str = "https://fcm.googleapis.com";
/// Google's OAuth token endpoint, which takes the signed assertion.
const FCM_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// The only scope asked for.
pub const FCM_SCOPE: &str = "https://www.googleapis.com/auth/firebase.messaging";
/// FCM's data for a change, byte for byte (D60 §3).
pub const FCM_SYNC_DATA: &str = r#"{"t":"sync"}"#;
/// FCM's data for a new device's request; the app has the words.
pub const FCM_APPROVAL_DATA: &str = r#"{"t":"approval"}"#;
/// An access token is made again this long before it expires.
const ACCESS_MARGIN: Duration = Duration::from_secs(5 * 60);

/// The FCM settings, from the service account's file. `Debug` shows
/// neither the key nor the account.
pub struct FcmConfig {
    key: ring::signature::RsaKeyPair,
    client_email: String,
    project_id: String,
    send_url: String,
    token_url: String,
}

impl std::fmt::Debug for FcmConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FcmConfig { .. }")
    }
}

impl FcmConfig {
    /// From the service account's JSON file (`project_id`, `client_email`
    /// and `private_key`); the URLs replace Google's (tests).
    pub fn new(
        credentials: &str,
        send_url: Option<&str>,
        token_url: Option<&str>,
    ) -> Result<Self, ServerError> {
        let bad =
            || ServerError::Config("PWM_FCM_CREDENTIALS_FILE is not a service account's JSON file");
        let file: serde_json::Value = serde_json::from_str(credentials).map_err(|_| bad())?;
        let field = |name: &str| {
            file[name]
                .as_str()
                .filter(|v| !v.is_empty())
                .ok_or_else(bad)
        };
        let project_id = field("project_id")?;
        let client_email = field("client_email")?;
        let der = pem_private_key(field("private_key")?).ok_or_else(bad)?;
        let key = ring::signature::RsaKeyPair::from_pkcs8(&der).map_err(|_| bad())?;
        let plain = |s: &str| {
            s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.@".contains(&b))
        };
        if !plain(project_id) || !plain(client_email) {
            return Err(bad());
        }
        let url =
            |u: Option<&str>, default: &str| u.unwrap_or(default).trim_end_matches('/').to_owned();
        Ok(Self {
            key,
            client_email: client_email.to_owned(),
            project_id: project_id.to_owned(),
            send_url: url(send_url, FCM_URL),
            token_url: url(token_url, FCM_TOKEN_URL),
        })
    }

    /// `PWM_FCM_CREDENTIALS_FILE`, and the optional `PWM_FCM_URL` and
    /// `PWM_FCM_TOKEN_URL`. Not set: FCM is off.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Option<Self>, ServerError> {
        let get = |name| var(name).filter(|v| !v.is_empty());
        let Some(path) = get("PWM_FCM_CREDENTIALS_FILE") else {
            return Ok(None);
        };
        let text = std::fs::read_to_string(path)
            .map_err(|_| ServerError::Config("PWM_FCM_CREDENTIALS_FILE cannot be read"))?;
        Self::new(
            &text,
            get("PWM_FCM_URL").as_deref(),
            get("PWM_FCM_TOKEN_URL").as_deref(),
        )
        .map(Some)
    }
}

/// The DER inside a `PRIVATE KEY` PEM block (PKCS#8, as Google writes it).
pub(crate) fn pem_private_key(pem: &str) -> Option<Vec<u8>> {
    let start = "-----BEGIN PRIVATE KEY-----";
    let end = "-----END PRIVATE KEY-----";
    let body = pem.split_once(start)?.1.split_once(end)?.0;
    let text: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD.decode(text).ok()
}

/// The APNs settings. `Debug` shows neither the key nor the ids.
pub struct PushConfig {
    key: SigningKey,
    key_id: String,
    team_id: String,
    ios_topic: String,
    macos_topic: String,
    /// Replaces both APNs addresses (tests).
    url: Option<String>,
}

impl std::fmt::Debug for PushConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PushConfig { .. }")
    }
}

impl PushConfig {
    /// From the `.p8` key's PEM text and the ids Apple gives with it.
    pub fn new(
        key_pem: &str,
        key_id: &str,
        team_id: &str,
        ios_topic: &str,
        macos_topic: &str,
        url: Option<&str>,
    ) -> Result<Self, ServerError> {
        let key = SigningKey::from_pkcs8_pem(key_pem.trim())
            .map_err(|_| ServerError::Config("PWM_APNS_KEY_FILE is not an APNs .p8 key"))?;
        let plain = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric());
        if !plain(key_id) || !plain(team_id) {
            return Err(ServerError::Config(
                "PWM_APNS_KEY_ID and PWM_APNS_TEAM_ID are Apple's 10-character ids",
            ));
        }
        let topic = |s: &str| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        };
        if !topic(ios_topic) || !topic(macos_topic) {
            return Err(ServerError::Config(
                "PWM_APNS_IOS_TOPIC and PWM_APNS_MACOS_TOPIC are the apps' bundle ids",
            ));
        }
        Ok(Self {
            key,
            key_id: key_id.to_owned(),
            team_id: team_id.to_owned(),
            ios_topic: ios_topic.to_owned(),
            macos_topic: macos_topic.to_owned(),
            url: url.map(|u| u.trim_end_matches('/').to_owned()),
        })
    }

    /// `PWM_APNS_KEY_FILE`, `PWM_APNS_KEY_ID`, `PWM_APNS_TEAM_ID`,
    /// `PWM_APNS_IOS_TOPIC`, `PWM_APNS_MACOS_TOPIC` and the optional
    /// `PWM_APNS_URL`. None set: push is off.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Result<Option<Self>, ServerError> {
        let get = |name| var(name).filter(|v| !v.is_empty());
        let Some(path) = get("PWM_APNS_KEY_FILE") else {
            return Ok(None);
        };
        let pem = std::fs::read_to_string(path)
            .map_err(|_| ServerError::Config("PWM_APNS_KEY_FILE cannot be read"))?;
        let missing = ServerError::Config(
            "PWM_APNS_KEY_FILE needs PWM_APNS_KEY_ID, PWM_APNS_TEAM_ID, PWM_APNS_IOS_TOPIC and PWM_APNS_MACOS_TOPIC",
        );
        let (Some(key_id), Some(team_id), Some(ios), Some(macos)) = (
            get("PWM_APNS_KEY_ID"),
            get("PWM_APNS_TEAM_ID"),
            get("PWM_APNS_IOS_TOPIC"),
            get("PWM_APNS_MACOS_TOPIC"),
        ) else {
            return Err(missing);
        };
        let url = get("PWM_APNS_URL");
        Self::new(&pem, &key_id, &team_id, &ios, &macos, url.as_deref()).map(Some)
    }
}

enum Event {
    VaultChanged { vault_id: Uuid, by_device: Uuid },
    ApprovalAsked { account_id: Uuid },
}

/// The handle the routes use. Off, it does nothing.
#[derive(Clone, Default)]
pub struct Push {
    queue: Option<mpsc::Sender<Event>>,
}

impl Push {
    pub fn off() -> Self {
        Self::default()
    }

    /// Starts the sender on the current Tokio runtime, with APNs, FCM or
    /// both. Without either, or the system's root certificates, push stays
    /// off.
    pub fn start(pool: PgPool, apns: Option<PushConfig>, fcm: Option<FcmConfig>) -> Self {
        if apns.is_none() && fcm.is_none() {
            return Self::off();
        }
        let Some(sender) = Sender::new(pool, apns, fcm) else {
            tracing::warn!("push: no root certificates, so push is off");
            return Self::off();
        };
        let (queue, events) = mpsc::channel(QUEUE);
        tokio::spawn(sender.run(events));
        Self { queue: Some(queue) }
    }

    /// After an item write or delete has committed.
    pub(crate) fn vault_changed(&self, vault_id: Uuid, by_device: Uuid) {
        self.send(Event::VaultChanged {
            vault_id,
            by_device,
        });
    }

    /// After an approval request for a known account has committed.
    pub(crate) fn approval_asked(&self, account_id: Uuid) {
        self.send(Event::ApprovalAsked { account_id });
    }

    fn send(&self, event: Event) {
        if let Some(queue) = &self.queue {
            // Full or stopped: dropped, like a push APNs loses.
            let _ = queue.try_send(event);
        }
    }
}

struct Target {
    device_id: Uuid,
    app: String,
    token: Vec<u8>,
    sandbox: bool,
}

#[derive(Clone, Copy)]
enum Kind {
    Background,
    Approval,
}

struct Sender {
    pool: PgPool,
    apns: Option<Arc<PushConfig>>,
    fcm: Option<Arc<FcmConfig>>,
    client: Client<hyper_rustls::HttpsConnector<HttpConnector>, Full<Bytes>>,
    jwt: Option<(String, Instant)>,
    /// FCM's OAuth access token, and when to make a new one.
    access: Option<(String, Instant)>,
    last: HashMap<Uuid, Instant>,
}

impl Sender {
    fn new(pool: PgPool, apns: Option<PushConfig>, fcm: Option<FcmConfig>) -> Option<Self> {
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_native_roots()
            .ok()?
            .https_or_http()
            .enable_http2()
            .build();
        let client = Client::builder(TokioExecutor::new())
            .http2_only(true)
            .build(connector);
        Some(Self {
            pool,
            apns: apns.map(Arc::new),
            fcm: fcm.map(Arc::new),
            client,
            jwt: None,
            access: None,
            last: HashMap::new(),
        })
    }

    async fn run(mut self, mut events: mpsc::Receiver<Event>) {
        while let Some(event) = events.recv().await {
            let (targets, kind) = match event {
                Event::VaultChanged {
                    vault_id,
                    by_device,
                } => (
                    self.vault_targets(vault_id, by_device).await,
                    Kind::Background,
                ),
                Event::ApprovalAsked { account_id } => {
                    (self.account_targets(account_id).await, Kind::Approval)
                }
            };
            let Ok(targets) = targets else {
                tracing::warn!("push: the device list could not be read");
                continue;
            };
            self.send_all(targets, kind).await;
        }
    }

    async fn vault_targets(&self, vault_id: Uuid, by: Uuid) -> Result<Vec<Target>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT p.device_id, p.app, p.token, p.environment FROM push_tokens p \
             JOIN devices d ON d.id = p.device_id \
             JOIN vault_keys k ON k.account_id = d.account_id \
             WHERE k.vault_id = $1 AND d.revoked_at IS NULL AND p.device_id <> $2",
        )
        .bind(vault_id)
        .bind(by)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(target).collect())
    }

    async fn account_targets(&self, account_id: Uuid) -> Result<Vec<Target>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT p.device_id, p.app, p.token, p.environment FROM push_tokens p \
             JOIN devices d ON d.id = p.device_id \
             WHERE d.account_id = $1 AND d.revoked_at IS NULL",
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.iter().map(target).collect())
    }

    async fn send_all(&mut self, targets: Vec<Target>, kind: Kind) {
        let now = Instant::now();
        self.last
            .retain(|_, at| now.duration_since(*at) < PUSH_EVERY);
        let (mut sent, mut failed, mut dropped) = (0, 0, 0);
        for target in targets {
            if matches!(kind, Kind::Background) {
                if self.last.contains_key(&target.device_id) {
                    continue;
                }
                self.last.insert(target.device_id, now);
            }
            match self.send_one(&target, kind).await {
                Answer::Sent => sent += 1,
                Answer::Gone => {
                    dropped += 1;
                    let _ = sqlx::query("DELETE FROM push_tokens WHERE device_id = $1")
                        .bind(target.device_id)
                        .execute(&self.pool)
                        .await;
                }
                Answer::Failed => failed += 1,
                // Its provider is off on this server.
                Answer::Skipped => {}
            }
        }
        if sent + failed + dropped > 0 {
            tracing::info!("push: {sent} sent, {failed} failed, {dropped} tokens dropped");
        }
    }

    async fn send_one(&mut self, target: &Target, kind: Kind) -> Answer {
        if target.app == "android" {
            self.send_fcm(target, kind).await
        } else {
            self.send_apns(target, kind).await
        }
    }

    /// One FCM data message (D60 §3): `{"t":"sync"}` at normal priority, or
    /// `{"t":"approval"}` at high priority, with nothing else in it.
    async fn send_fcm(&mut self, target: &Target, kind: Kind) -> Answer {
        let Some(config) = self.fcm.clone() else {
            return Answer::Skipped;
        };
        let Ok(token) = std::str::from_utf8(&target.token) else {
            return Answer::Gone;
        };
        let Some(access) = self.access_token(&config).await else {
            return Answer::Failed;
        };
        let body = fcm_message(token, kind).to_string();
        let request = hyper::Request::post(format!(
            "{}/v1/projects/{}/messages:send",
            config.send_url, config.project_id
        ))
        .header("authorization", format!("Bearer {access}"))
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(body)));
        let Ok(request) = request else {
            return Answer::Failed;
        };
        let response =
            match tokio::time::timeout(Duration::from_secs(10), self.client.request(request)).await
            {
                Ok(Ok(response)) => response,
                _ => return Answer::Failed,
            };
        match response.status().as_u16() {
            200 => Answer::Sent,
            // UNREGISTERED: the app was removed or the token replaced.
            404 => Answer::Gone,
            401 => {
                // The access token was refused: make a new one next time.
                self.access = None;
                Answer::Failed
            }
            _ => Answer::Failed,
        }
    }

    /// FCM's OAuth access token, from an RS256 assertion signed with the
    /// service account's key; kept until shortly before it expires.
    async fn access_token(&mut self, config: &FcmConfig) -> Option<String> {
        if let Some((token, until)) = &self.access
            && Instant::now() < *until
        {
            return Some(token.clone());
        }
        let assertion = fcm_assertion(config, unix_secs())?;
        let body = format!(
            "grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer&assertion={assertion}"
        );
        let request = hyper::Request::post(&config.token_url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Full::new(Bytes::from(body)))
            .ok()?;
        let response = tokio::time::timeout(Duration::from_secs(10), self.client.request(request))
            .await
            .ok()?
            .ok()?;
        if response.status() != hyper::StatusCode::OK {
            tracing::warn!(
                "push: FCM's token endpoint answered {}",
                response.status().as_u16()
            );
            return None;
        }
        let bytes = response.into_body().collect().await.ok()?.to_bytes();
        let answer: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        let token = answer["access_token"]
            .as_str()
            .filter(|t| !t.is_empty())?
            .to_owned();
        let lasts = Duration::from_secs(answer["expires_in"].as_u64().unwrap_or(3600));
        let until = Instant::now()
            + lasts
                .saturating_sub(ACCESS_MARGIN)
                .max(Duration::from_secs(60));
        self.access = Some((token.clone(), until));
        Some(token)
    }

    async fn send_apns(&mut self, target: &Target, kind: Kind) -> Answer {
        let Some(config) = self.apns.clone() else {
            return Answer::Skipped;
        };
        let Some(jwt) = self.jwt(&config) else {
            return Answer::Failed;
        };
        let base = match (&config.url, target.sandbox) {
            (Some(url), _) => url.as_str(),
            (None, true) => SANDBOX_URL,
            (None, false) => PRODUCTION_URL,
        };
        let topic = if target.app == "macos" {
            &config.macos_topic
        } else {
            &config.ios_topic
        };
        let (push_type, priority, expires, body) = match kind {
            Kind::Background => (
                "background",
                "5",
                BACKGROUND_EXPIRES_SECS,
                BACKGROUND_PAYLOAD.to_owned(),
            ),
            Kind::Approval => (
                "alert",
                "10",
                APPROVAL_EXPIRES_SECS,
                serde_json::json!({ "aps": { "alert": { "body": APPROVAL_NOTICE }, "sound": "default" } })
                    .to_string(),
            ),
        };
        let token: String = target.token.iter().map(|b| format!("{b:02x}")).collect();
        let request = hyper::Request::post(format!("{base}/3/device/{token}"))
            .header("authorization", format!("bearer {jwt}"))
            .header("apns-topic", topic.as_str())
            .header("apns-push-type", push_type)
            .header("apns-priority", priority)
            .header("apns-expiration", (unix_secs() + expires).to_string())
            .header("content-type", "application/json")
            .body(Full::new(Bytes::from(body)));
        let Ok(request) = request else {
            return Answer::Failed;
        };
        let response =
            match tokio::time::timeout(Duration::from_secs(10), self.client.request(request)).await
            {
                Ok(Ok(response)) => response,
                _ => return Answer::Failed,
            };
        let status = response.status().as_u16();
        match status {
            200 => Answer::Sent,
            410 => Answer::Gone,
            400 => {
                let body = response.into_body().collect().await.map(|b| b.to_bytes());
                let reason = body
                    .ok()
                    .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                    .and_then(|v| v["reason"].as_str().map(str::to_owned));
                match reason.as_deref() {
                    Some("BadDeviceToken" | "DeviceTokenNotForTopic") => Answer::Gone,
                    _ => Answer::Failed,
                }
            }
            _ => Answer::Failed,
        }
    }

    /// The ES256 provider token, made again every 50 minutes.
    fn jwt(&mut self, config: &PushConfig) -> Option<String> {
        if let Some((jwt, at)) = &self.jwt
            && at.elapsed() < JWT_EVERY
        {
            return Some(jwt.clone());
        }
        let jwt = provider_token(config, unix_secs())?;
        self.jwt = Some((jwt.clone(), Instant::now()));
        Some(jwt)
    }
}

enum Answer {
    Sent,
    Gone,
    Failed,
    Skipped,
}

/// The whole FCM message: the token, the data and Android's delivery
/// settings, and nothing else (D60 §3).
fn fcm_message(token: &str, kind: Kind) -> serde_json::Value {
    match kind {
        Kind::Background => serde_json::json!({ "message": {
            "token": token,
            "data": { "t": "sync" },
            "android": { "priority": "normal", "ttl": format!("{BACKGROUND_EXPIRES_SECS}s"), "collapse_key": "sync" },
        }}),
        Kind::Approval => serde_json::json!({ "message": {
            "token": token,
            "data": { "t": "approval" },
            "android": { "priority": "high", "ttl": format!("{APPROVAL_EXPIRES_SECS}s") },
        }}),
    }
}

/// The service account's assertion for Google's token endpoint: an RS256
/// JWT for the messaging scope, valid an hour.
fn fcm_assertion(config: &FcmConfig, iat: u64) -> Option<String> {
    google_assertion(
        &config.key,
        &config.client_email,
        FCM_SCOPE,
        &config.token_url,
        iat,
    )
}

/// A Google service account's RS256 assertion for one scope, valid an hour
/// (D60 §2; Play's billing API uses it too, D6 §3).
pub(crate) fn google_assertion(
    key: &ring::signature::RsaKeyPair,
    client_email: &str,
    scope: &str,
    token_url: &str,
    iat: u64,
) -> Option<String> {
    let header = serde_json::json!({ "alg": "RS256", "typ": "JWT" });
    let claims = serde_json::json!({
        "iss": client_email,
        "scope": scope,
        "aud": token_url,
        "iat": iat,
        "exp": iat + 3600,
    });
    let signed = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header.to_string()),
        URL_SAFE_NO_PAD.encode(claims.to_string())
    );
    let mut signature = vec![0; key.public().modulus_len()];
    key.sign(
        &ring::signature::RSA_PKCS1_SHA256,
        &ring::rand::SystemRandom::new(),
        signed.as_bytes(),
        &mut signature,
    )
    .ok()?;
    Some(format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature)))
}

fn target(row: &sqlx::postgres::PgRow) -> Target {
    let environment: String = row.get(3);
    Target {
        device_id: row.get(0),
        app: row.get(1),
        token: row.get(2),
        sandbox: environment == "sandbox",
    }
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `header.claims.signature`, base64url, as APNs token auth wants.
fn provider_token(config: &PushConfig, iat: u64) -> Option<String> {
    let header = serde_json::json!({ "alg": "ES256", "kid": config.key_id });
    let claims = serde_json::json!({ "iss": config.team_id, "iat": iat });
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

#[cfg(test)]
mod tests {
    use super::*;
    use p256::ecdsa::VerifyingKey;
    use p256::ecdsa::signature::Verifier;
    use p256::pkcs8::{EncodePrivateKey, LineEnding};

    /// A throwaway RSA key made for these tests, never used anywhere else.
    const TEST_RSA_KEY: &str = include_str!("../tests/fixtures/fcm-test-key.pem");

    fn credentials() -> String {
        serde_json::json!({
            "type": "service_account",
            "project_id": "pwm-test",
            "client_email": "push@pwm-test.iam.gserviceaccount.com",
            "private_key": TEST_RSA_KEY,
        })
        .to_string()
    }

    #[test]
    fn the_fcm_assertion_is_rs256_for_the_messaging_scope() {
        let config = FcmConfig::new(&credentials(), None, None).unwrap();
        let jwt = fcm_assertion(&config, 1_700_000_000).unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(header, serde_json::json!({ "alg": "RS256", "typ": "JWT" }));
        assert_eq!(
            claims,
            serde_json::json!({
                "iss": "push@pwm-test.iam.gserviceaccount.com",
                "scope": "https://www.googleapis.com/auth/firebase.messaging",
                "aud": "https://oauth2.googleapis.com/token",
                "iat": 1_700_000_000,
                "exp": 1_700_003_600,
            })
        );
        let public = ring::signature::UnparsedPublicKey::new(
            &ring::signature::RSA_PKCS1_2048_8192_SHA256,
            config.key.public().as_ref().to_vec(),
        );
        let signed = format!("{}.{}", parts[0], parts[1]);
        let signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        assert!(public.verify(signed.as_bytes(), &signature).is_ok());
        let mut tampered = signed.into_bytes();
        tampered[0] ^= 1;
        assert!(public.verify(&tampered, &signature).is_err());
    }

    #[test]
    fn fcm_messages_carry_only_the_token_and_the_kind() {
        assert_eq!(
            fcm_message("tok", Kind::Background),
            serde_json::json!({ "message": {
                "token": "tok",
                "data": { "t": "sync" },
                "android": { "priority": "normal", "ttl": "3600s", "collapse_key": "sync" },
            }})
        );
        assert_eq!(
            fcm_message("tok", Kind::Approval),
            serde_json::json!({ "message": {
                "token": "tok",
                "data": { "t": "approval" },
                "android": { "priority": "high", "ttl": "600s" },
            }})
        );
        assert_eq!(FCM_SYNC_DATA, r#"{"t":"sync"}"#);
        assert_eq!(FCM_APPROVAL_DATA, r#"{"t":"approval"}"#);
    }

    #[test]
    fn fcm_settings_are_checked_and_never_printed() {
        let config =
            FcmConfig::new(&credentials(), Some("http://fcm/"), Some("http://token/")).unwrap();
        assert_eq!(format!("{config:?}"), "FcmConfig { .. }");
        assert_eq!(config.send_url, "http://fcm");
        assert_eq!(config.token_url, "http://token");
        assert!(FcmConfig::new("not json", None, None).is_err());
        assert!(FcmConfig::new(r#"{"project_id":"p","client_email":"e"}"#, None, None).is_err());
        let wrong_key = credentials().replace("MII", "MIJ");
        assert!(FcmConfig::new(&wrong_key, None, None).is_err());
        let odd_project = credentials().replace("pwm-test\"", "pwm test\"");
        assert!(FcmConfig::new(&odd_project, None, None).is_err());
        assert!(FcmConfig::from_vars(|_| None).unwrap().is_none());
        assert!(
            FcmConfig::from_vars(
                |n| (n == "PWM_FCM_CREDENTIALS_FILE").then(|| "/nonexistent".into())
            )
            .is_err()
        );
    }

    fn config() -> (PushConfig, VerifyingKey) {
        let key = SigningKey::from_slice(&[7u8; 32]).unwrap();
        let verifying = *key.verifying_key();
        let pem = key.to_pkcs8_pem(LineEnding::LF).unwrap();
        let config = PushConfig::new(&pem, "KEY1234567", "TEAM123456", "a.b.ios", "a.b.mac", None);
        (config.unwrap(), verifying)
    }

    #[test]
    fn the_provider_token_is_es256_with_the_key_id_and_team() {
        let (config, verifying) = config();
        let jwt = provider_token(&config, 1_700_000_000).unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        let claims: serde_json::Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(
            header,
            serde_json::json!({ "alg": "ES256", "kid": "KEY1234567" })
        );
        assert_eq!(
            claims,
            serde_json::json!({ "iss": "TEAM123456", "iat": 1_700_000_000 })
        );
        let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
        let signed = format!("{}.{}", parts[0], parts[1]);
        assert!(verifying.verify(signed.as_bytes(), &signature).is_ok());
    }

    #[test]
    fn settings_are_checked_and_never_printed() {
        let (config, _) = config();
        assert_eq!(format!("{config:?}"), "PushConfig { .. }");
        assert!(PushConfig::new("not a key", "K", "T", "a", "b", None).is_err());
        let pem = SigningKey::from_slice(&[7u8; 32])
            .unwrap()
            .to_pkcs8_pem(LineEnding::LF)
            .unwrap();
        assert!(PushConfig::new(&pem, "K/1", "T", "a", "b", None).is_err());
        assert!(PushConfig::new(&pem, "K", "T", "a b", "b", None).is_err());
        assert!(PushConfig::from_vars(|_| None).unwrap().is_none());
        assert!(
            PushConfig::from_vars(|n| (n == "PWM_APNS_KEY_FILE").then(|| "/nonexistent".into()))
                .is_err()
        );
    }

    #[test]
    fn the_background_payload_says_nothing() {
        assert_eq!(BACKGROUND_PAYLOAD, r#"{"aps":{"content-available":1}}"#);
        let parsed: serde_json::Value = serde_json::from_str(BACKGROUND_PAYLOAD).unwrap();
        assert_eq!(
            parsed,
            serde_json::json!({ "aps": { "content-available": 1 } })
        );
    }

    /// The HTTP/2 path to APNs without a database: one push, then a 410
    /// and a refused token are told apart from other failures.
    #[tokio::test]
    async fn pushes_go_over_http2_and_answers_are_read() {
        use axum::extract::Path;
        use axum::http::{HeaderMap, StatusCode, Version};
        use std::sync::Mutex;

        let seen: Arc<Mutex<Vec<(Version, String, String)>>> = Arc::default();
        let recorder = seen.clone();
        let router = axum::Router::new().route(
            "/3/device/{token}",
            axum::routing::post(
                move |Path(token): Path<String>,
                      headers: HeaderMap,
                      version: Version,
                      body: String| {
                    let recorder = recorder.clone();
                    async move {
                        let topic = headers["apns-topic"].to_str().unwrap().to_owned();
                        recorder.lock().unwrap().push((version, topic, body));
                        match &token[..2] {
                            "aa" => (StatusCode::OK, String::new()),
                            "bb" => (StatusCode::GONE, String::new()),
                            "cc" => (
                                StatusCode::BAD_REQUEST,
                                r#"{"reason":"BadDeviceToken"}"#.into(),
                            ),
                            _ => (
                                StatusCode::BAD_REQUEST,
                                r#"{"reason":"PayloadEmpty"}"#.into(),
                            ),
                        }
                    }
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, router).await });

        let key = SigningKey::from_slice(&[7u8; 32]).unwrap();
        let pem = key.to_pkcs8_pem(LineEnding::LF).unwrap();
        let config = PushConfig::new(&pem, "K", "T", "a.ios", "a.mac", Some(&url)).unwrap();
        let pool = PgPool::connect_lazy("postgres://unused@127.0.0.1:1/unused").unwrap();
        let mut sender = Sender::new(pool, Some(config), None).unwrap();
        let target = |byte: &str, app: &str| Target {
            device_id: Uuid::nil(),
            app: app.to_owned(),
            token: vec![u8::from_str_radix(byte, 16).unwrap(); 32],
            sandbox: true,
        };
        assert!(matches!(
            sender
                .send_one(&target("aa", "macos"), Kind::Background)
                .await,
            Answer::Sent
        ));
        assert!(matches!(
            sender
                .send_one(&target("bb", "ios"), Kind::Background)
                .await,
            Answer::Gone
        ));
        assert!(matches!(
            sender.send_one(&target("cc", "ios"), Kind::Approval).await,
            Answer::Gone
        ));
        assert!(matches!(
            sender
                .send_one(&target("dd", "ios"), Kind::Background)
                .await,
            Answer::Failed
        ));

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 4);
        assert!(seen.iter().all(|(v, _, _)| *v == Version::HTTP_2));
        assert_eq!(seen[0].1, "a.mac");
        assert_eq!(seen[0].2, BACKGROUND_PAYLOAD);
        assert_eq!(seen[1].1, "a.ios");
        assert!(seen[2].2.contains(APPROVAL_NOTICE));
    }
}
