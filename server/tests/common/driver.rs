//! Drives `pwm-client` (D31) against the app in process: each request the
//! client asks for goes to the router, and each answer comes back to it.

use axum::Router;
use http_body_util::BodyExt;
use pwm_client::{Client, ClientError, HttpRequest, HttpResponse, MemoryStore, Outcome, Step};
use tower::ServiceExt;

/// The time every answer is given at.
pub const T0: i64 = 1_700_000_000_000;

pub async fn send(app: &Router, request: &HttpRequest) -> HttpResponse {
    let mut builder = axum::http::Request::builder()
        .method(request.method.as_str())
        .uri(&request.path);
    for (name, value) in &request.headers {
        builder = builder.header(*name, value.as_str());
    }
    let body = request
        .body
        .clone()
        .map_or_else(axum::body::Body::empty, axum::body::Body::from);
    let response = app
        .clone()
        .oneshot(builder.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(n, v)| (n.to_string(), v.to_str().unwrap_or_default().to_owned()))
        .collect();
    let body = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    HttpResponse::new(status, headers, body)
}

/// Sends every request of a conversation until it is done.
pub async fn run(
    app: &Router,
    client: &mut Client<MemoryStore>,
    step: Result<Step, ClientError>,
) -> Result<Outcome, ClientError> {
    let mut step = step?;
    loop {
        step = match step {
            Step::Send(request) => client.receive(send(app, &request).await, T0)?,
            Step::Wait { delay_ms } => panic!("the server failed; would wait {delay_ms} ms"),
            Step::Done(outcome) => return Ok(outcome),
        };
    }
}
