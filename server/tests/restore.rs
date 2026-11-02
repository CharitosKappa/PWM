//! Restore detection and the container health check (D30).

mod common;

use axum::http::StatusCode;
use common::flows::*;
use common::send;

async fn epoch(app: &axum::Router, token: &str) -> String {
    let reply = send(app, "GET", "/v1/vaults", Some(token), &[], None).await;
    assert_eq!(reply.status, StatusCode::OK);
    reply.headers["pwm-epoch"].to_str().unwrap().to_owned()
}

#[tokio::test]
async fn the_epoch_changes_only_when_marked_restored() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let account = register(&app, "a@example.com").await;
    let first = epoch(&app, account.access()).await;
    assert!(uuid::Uuid::parse_str(&first).is_ok());
    assert_eq!(epoch(&app, account.access()).await, first);

    let marked = pwm_server::mark_restored(&db.pool).await.unwrap();
    let second = epoch(&app, account.access()).await;
    assert_ne!(second, first);
    assert_eq!(second, marked.to_string());
    // Migrating again keeps it.
    pwm_server::migrate(&db.pool).await.unwrap();
    assert_eq!(epoch(&app, account.access()).await, second);
    db.drop().await;
}

#[tokio::test]
async fn the_health_check_sees_a_live_server() {
    let Some((db, app)) = setup().await else {
        return;
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await });
    let check = tokio::task::spawn_blocking(move || pwm_server::healthcheck(addr));
    assert!(check.await.unwrap());
    // Asking for 0.0.0.0 checks loopback, as in the container.
    let any = std::net::SocketAddr::new([0, 0, 0, 0].into(), addr.port());
    assert!(
        tokio::task::spawn_blocking(move || pwm_server::healthcheck(any))
            .await
            .unwrap()
    );
    server.abort();
    let _ = server.await;

    // Nothing listening.
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    assert!(
        !tokio::task::spawn_blocking(move || pwm_server::healthcheck(closed))
            .await
            .unwrap()
    );
    db.drop().await;
}
