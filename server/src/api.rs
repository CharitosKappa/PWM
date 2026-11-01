//! Shared API plumbing (D26): state, JSON extraction, errors and the
//! base64url encoding of byte fields.

use std::sync::Arc;

use axum::extract::{FromRequest, Request};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use pwm_crypto::ServerSecrets;
use serde::de::DeserializeOwned;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub secrets: Arc<ServerSecrets>,
}

impl AppState {
    pub fn new(pool: PgPool, secrets: ServerSecrets) -> Self {
        Self {
            pool,
            secrets: Arc::new(secrets),
        }
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
    Conflict(&'static str),
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
            Self::Conflict(code) => (StatusCode::CONFLICT, code),
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

/// `Json<T>` whose rejection is a plain `invalid_request`: axum's default
/// message can quote the request body.
pub struct ApiJson<T>(pub T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for ApiJson<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(|_| ApiError::InvalidRequest)?;
        Ok(Self(value))
    }
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
