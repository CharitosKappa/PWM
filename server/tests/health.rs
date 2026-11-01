//! `GET /healthz` (D25): 200 when Postgres answers, 503 otherwise, with no
//! version or other details.

mod common;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use pwm_crypto::ServerSecrets;
use pwm_server::{AppState, app, connect};
use tower::ServiceExt;

fn state(pool: sqlx::PgPool) -> Router {
    app(AppState::new(pool, ServerSecrets::generate().unwrap()))
}

async fn get(app: Router, path: &str) -> (StatusCode, String) {
    let response = app
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn healthz_is_ok_when_the_database_answers() {
    let Some(db) = common::test_db().await else {
        return;
    };
    assert_eq!(
        get(state(db.pool.clone()), "/healthz").await,
        (StatusCode::OK, "ok".into())
    );
    db.drop().await;
}

#[tokio::test]
async fn healthz_is_unavailable_without_the_database() {
    // Nothing listens on port 1; the pool connects lazily, so the app starts.
    let pool = connect("postgres://pwm@127.0.0.1:1/pwm").unwrap();
    assert_eq!(
        get(state(pool), "/healthz").await,
        (StatusCode::SERVICE_UNAVAILABLE, "unavailable".into())
    );
}

#[tokio::test]
async fn unknown_paths_are_not_found() {
    let pool = connect("postgres://pwm@127.0.0.1:1/pwm").unwrap();
    assert_eq!(get(state(pool), "/v1/items").await.0, StatusCode::NOT_FOUND);
}
