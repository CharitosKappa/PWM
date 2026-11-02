use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Router, middleware};

use crate::api::AppState;
use crate::{account, auth, items, limits, logging, reports, vaults};

/// Request bodies other than item writes (D29).
pub const MAX_BODY_BYTES: usize = 16 * 1024;

/// The HTTP API: `/healthz` and the versioned `/v1` endpoints, with size
/// limits, rate limits and one log line per request (D29).
pub fn app(state: AppState) -> Router {
    let by_ip = middleware::from_fn_with_state(state.clone(), limits::by_ip);
    Router::new()
        .route("/healthz", get(healthz))
        .merge(auth::routes().route_layer(by_ip.clone()))
        .merge(reports::routes().route_layer(by_ip))
        .merge(account::routes())
        .merge(vaults::routes())
        .merge(items::routes())
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(middleware::from_fn(logging::request_log))
        .with_state(state)
}

/// 200 when Postgres answers, 503 otherwise. No version or details, so it
/// tells an outsider nothing about the deployment.
async fn healthz(State(state): State<AppState>) -> (StatusCode, &'static str) {
    match sqlx::query("SELECT 1").execute(&state.pool).await {
        Ok(_) => (StatusCode::OK, "ok"),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
    }
}
