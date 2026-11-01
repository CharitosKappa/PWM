use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;

use crate::api::AppState;
use crate::{account, auth};

/// The HTTP API: `/healthz` and the versioned `/v1` endpoints.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .merge(auth::routes())
        .merge(account::routes())
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
