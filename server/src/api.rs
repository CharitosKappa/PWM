//! Shared API plumbing (D26): state, JSON extraction, errors and the
//! base64url encoding of byte fields.

use std::sync::Arc;

use axum::extract::{FromRequest, FromRequestParts, Path, Request};
use axum::http::request::Parts;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use pwm_crypto::ServerSecrets;
use serde::de::DeserializeOwned;
use sqlx::PgPool;

use crate::billing::Billing;
use crate::deletion::DeletionLog;
use crate::limits::{Clock, Limits, RateLimiter, TrustedProxies, system_clock};
use crate::push::Push;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub secrets: Arc<ServerSecrets>,
    pub(crate) limiter: Arc<RateLimiter>,
    pub(crate) trusted_proxies: Arc<TrustedProxies>,
    /// APNs (D50 §3); off unless configured.
    pub(crate) push: Push,
    /// Plans and billing (D6); off (every account Premium) unless configured.
    pub(crate) billing: Billing,
    /// Deleted accounts outside the database (D74 §6); off unless configured.
    pub(crate) deletions: DeletionLog,
}

impl AppState {
    /// The default limits (D29) and no trusted proxies.
    pub fn new(pool: PgPool, secrets: ServerSecrets) -> Self {
        Self {
            pool,
            secrets: Arc::new(secrets),
            limiter: Arc::new(RateLimiter::new(Limits::default(), system_clock())),
            trusted_proxies: Arc::new(TrustedProxies::default()),
            push: Push::off(),
            billing: Billing::off(),
            deletions: DeletionLog::off(),
        }
    }

    pub fn with_deletions(mut self, deletions: DeletionLog) -> Self {
        self.deletions = deletions;
        self
    }

    pub fn with_billing(mut self, billing: Billing) -> Self {
        self.billing = billing;
        self
    }

    pub fn with_push(mut self, push: Push) -> Self {
        self.push = push;
        self
    }

    pub fn with_limits(mut self, limits: Limits, clock: Arc<dyn Clock>) -> Self {
        self.limiter = Arc::new(RateLimiter::new(limits, clock));
        self
    }

    pub fn with_trusted_proxies(mut self, proxies: TrustedProxies) -> Self {
        self.trusted_proxies = Arc::new(proxies);
        self
    }
}

pub type ApiRouter = Router<AppState>;

/// API errors: `{"error": "<code>"}`, never with input values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiError {
    InvalidRequest,
    /// Login failed; the same for every cause.
    InvalidCredentials,
    /// Missing, malformed, expired or ended access or refresh token.
    Unauthorized,
    DeviceRevoked,
    /// A refresh token of a deleted account's session (D74 §5).
    AccountDeleted,
    /// A body or blob over its size limit (D29).
    TooLarge,
    /// A vault or item count cap (D29).
    LimitReached,
    /// Over a rate limit; retry after this many seconds (D29).
    RateLimited(u64),
    /// The caller may read this vault but not change it.
    ReadOnly,
    /// A write or delete without `If-Match` / `If-None-Match` (D27).
    PreconditionRequired,
    /// A delta sync from before compacted tombstones (D76 §2): pull from 0.
    ResyncRequired,
    Conflict(&'static str),
    /// 403 with its own code, such as `purchase_not_yours` (D6).
    Forbidden(&'static str),
    /// Billing is not set up here, or its provider can't be reached (D6).
    BillingUnavailable,
    /// The account's plan doesn't include this (D73 §3).
    PlanRequired,
    NotFound,
    Internal,
}

impl ApiError {
    fn status_and_code(self) -> (StatusCode, &'static str) {
        match self {
            Self::InvalidRequest => (StatusCode::BAD_REQUEST, "invalid_request"),
            Self::InvalidCredentials => (StatusCode::UNAUTHORIZED, "invalid_credentials"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::DeviceRevoked => (StatusCode::FORBIDDEN, "device_revoked"),
            Self::AccountDeleted => (StatusCode::FORBIDDEN, "account_deleted"),
            Self::TooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "too_large"),
            Self::LimitReached => (StatusCode::FORBIDDEN, "limit_reached"),
            Self::RateLimited(_) => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            Self::ReadOnly => (StatusCode::FORBIDDEN, "read_only"),
            Self::PreconditionRequired => {
                (StatusCode::PRECONDITION_REQUIRED, "precondition_required")
            }
            Self::ResyncRequired => (StatusCode::GONE, "resync_required"),
            Self::Conflict(code) => (StatusCode::CONFLICT, code),
            Self::Forbidden(code) => (StatusCode::FORBIDDEN, code),
            Self::BillingUnavailable => (StatusCode::SERVICE_UNAVAILABLE, "billing_unavailable"),
            Self::PlanRequired => (StatusCode::FORBIDDEN, "plan_required"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = self.status_and_code();
        let mut response = (status, Json(serde_json::json!({ "error": code }))).into_response();
        if self == Self::Unauthorized {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }
        if let Self::RateLimited(secs) = self {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from(secs));
        }
        response
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        // sqlx errors carry no bind values, only statement and constraint names.
        tracing::error!("database: {err}");
        Self::Internal
    }
}

/// `true` if `err` is a unique-constraint violation.
pub fn is_unique_violation(err: &sqlx::Error) -> bool {
    err.as_database_error()
        .is_some_and(|e| e.is_unique_violation())
}

/// `Json<T>` whose rejection is a plain `invalid_request` (or `too_large`
/// over the body limit): axum's default message can quote the request body.
pub struct ApiJson<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for ApiJson<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|rejection| {
                if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
                    ApiError::TooLarge
                } else {
                    ApiError::InvalidRequest
                }
            })?;
        Ok(Self(value))
    }
}

/// `Path<T>` whose rejection is a plain `invalid_request`, like [`ApiJson`].
pub struct ApiPath<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned + Send> FromRequestParts<S> for ApiPath<T> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Path(value) = Path::<T>::from_request_parts(parts, state)
            .await
            .map_err(|_| ApiError::InvalidRequest)?;
        Ok(Self(value))
    }
}

/// A revision or change counter as it travels on the wire (D25): a decimal
/// string without sign or leading zeros, so each value has one spelling.
pub fn parse_counter(text: &str) -> Option<i64> {
    let canonical = !text.is_empty()
        && text.bytes().all(|b| b.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'));
    if canonical { text.parse().ok() } else { None }
}

/// Byte fields as base64url without padding (D10).
pub mod b64 {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&URL_SAFE_NO_PAD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let text = <&str>::deserialize(d)?;
        URL_SAFE_NO_PAD
            .decode(text)
            .map_err(|_| serde::de::Error::custom("not base64url"))
    }

    pub mod option {
        use super::*;

        pub fn serialize<S: Serializer>(bytes: &Option<Vec<u8>>, s: S) -> Result<S::Ok, S::Error> {
            match bytes {
                Some(b) => super::serialize(b, s),
                None => s.serialize_none(),
            }
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<u8>>, D::Error> {
            Option::<&str>::deserialize(d)?
                .map(|text| {
                    URL_SAFE_NO_PAD
                        .decode(text)
                        .map_err(|_| serde::de::Error::custom("not base64url"))
                })
                .transpose()
        }
    }
}
