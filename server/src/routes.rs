use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use sqlx::PgPool;

/// The HTTP API. Versioned endpoints arrive under `/v1` from M3.2.
pub fn app(pool: PgPool) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .with_state(pool)
}

/// 200 when Postgres answers, 503 otherwise. No version or details, so it
/// tells an outsider nothing about the deployment.
async fn healthz(State(pool): State<PgPool>) -> (StatusCode, &'static str) {
    match sqlx::query("SELECT 1").execute(&pool).await {
        Ok(_) => (StatusCode::OK, "ok"),
        Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
    }
}
