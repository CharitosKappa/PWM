//! The requests the engine asks the platform to send, and the answers it
//! takes back (D28: the core does no I/O). Both may carry tokens or
//! ciphertext, so they are wiped on drop and print no headers or bodies.

use core::fmt;

use zeroize::Zeroize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
        }
    }
}

pub struct HttpRequest {
    pub method: Method,
    /// Path and query, relative to the server's base URL.
    pub path: String,
    /// Lowercase names. Includes `authorization` when the request needs it.
    pub headers: Vec<(&'static str, String)>,
    /// JSON, when the request has a body.
    pub body: Option<Vec<u8>>,
}

impl HttpRequest {
    pub(crate) fn new(method: Method, path: String) -> Self {
        Self {
            method,
            path,
            headers: Vec::new(),
            body: None,
        }
    }

    pub(crate) fn header_value(mut self, name: &'static str, value: String) -> Self {
        self.headers.push((name, value));
        self
    }

    pub(crate) fn json(mut self, body: Vec<u8>) -> Self {
        self.headers
            .push(("content-type", "application/json".to_owned()));
        self.body = Some(body);
        self
    }

    /// The value of header `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

impl Drop for HttpRequest {
    fn drop(&mut self) {
        for (_, value) in &mut self.headers {
            value.zeroize();
        }
        self.body.zeroize();
    }
}

impl fmt::Debug for HttpRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = self.headers.iter().map(|(n, _)| *n).collect();
        f.debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("path", &self.path)
            .field("headers", &names)
            .field("body_len", &self.body.as_ref().map(Vec::len))
            .finish()
    }
}

pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn new(status: u16, headers: Vec<(String, String)>, body: Vec<u8>) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }

    /// The value of header `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

impl Drop for HttpResponse {
    fn drop(&mut self) {
        self.body.zeroize();
    }
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("body_len", &self.body.len())
            .finish()
    }
}
