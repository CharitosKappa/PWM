//! Sends the core's requests (D28 steps) until it is done or asks to wait
//! (D66 §5). Nothing here reads or logs bodies or headers: they can carry
//! tokens.

use std::future::Future;
use std::time::Duration;

use pwm_client::{HttpRequest, HttpResponse, Method};

/// No answer: offline, refused, timed out, or a redirect (never followed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoAnswer;

/// One HTTP exchange with the server at `base` (no trailing slash).
pub trait Transport: Send + Sync + 'static {
    fn send(
        &self,
        base: &str,
        request: HttpRequest,
    ) -> impl Future<Output = Result<HttpResponse, NoAnswer>> + Send;
}

/// `reqwest` over rustls with the system's trust store: no cookies, no
/// cache, no redirects.
pub struct Reqwest {
    client: reqwest::Client,
}

impl Reqwest {
    pub fn new() -> Result<Self, NoAnswer> {
        // rustls with the ring provider, as the server (D66 §5); only TLS uses it.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60))
            .user_agent(concat!("PWM-desktop/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| NoAnswer)?;
        Ok(Self { client })
    }
}

fn method(m: Method) -> reqwest::Method {
    match m {
        Method::Get => reqwest::Method::GET,
        Method::Post => reqwest::Method::POST,
        Method::Put => reqwest::Method::PUT,
        Method::Patch => reqwest::Method::PATCH,
        Method::Delete => reqwest::Method::DELETE,
    }
}

impl Transport for Reqwest {
    async fn send(&self, base: &str, mut request: HttpRequest) -> Result<HttpResponse, NoAnswer> {
        let mut builder = self
            .client
            .request(method(request.method), format!("{base}{}", request.path));
        for (name, value) in &request.headers {
            builder = builder.header(*name, value);
        }
        // The request wipes its own copy when dropped; reqwest needs its own.
        if let Some(body) = request.body.take() {
            builder = builder.body(body);
        }
        let response = builder.send().await.map_err(|_| NoAnswer)?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|v| (name.as_str().to_owned(), v.to_owned()))
            })
            .collect();
        let body = response.bytes().await.map_err(|_| NoAnswer)?.to_vec();
        Ok(HttpResponse::new(status, headers, body))
    }
}

/// The server's base URL without trailing slashes.
pub fn base_url(server: &str) -> String {
    server.trim_end_matches('/').to_owned()
}
