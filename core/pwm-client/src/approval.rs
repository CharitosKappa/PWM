//! Approving a new device from a signed-in one (decision D46).
//!
//! The new device ("N") is not signed in: [`ApprovalAsk`] is a small step
//! machine of its own that makes the request, polls, reveals its key once
//! the signed-in device accepted, shows the code, and finally opens the
//! Secret Key. The signed-in device ("E") uses [`Client`]: list the
//! account's requests, accept one, and send the Secret Key once the user
//! saw the codes match, or deny.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pwm_crypto::{ApprovalKeys, Envelope, Id, Secret, check_approval_commitment};
use pwm_store::Store;
use pwm_sync::{HttpRequest, HttpResponse, Method};
use serde::Deserialize;
use serde_json::json;

use crate::client::{Running, parse_id};
use crate::{Client, ClientError, Platform, Step};

/// How often N asks whether its request moved on.
pub const POLL_EVERY_MS: u64 = 2_000;

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn key32(text: Option<&str>) -> Result<[u8; 32], ClientError> {
    URL_SAFE_NO_PAD
        .decode(text.ok_or(ClientError::Protocol)?)
        .ok()
        .and_then(|bytes| <[u8; 32]>::try_from(bytes).ok())
        .ok_or(ClientError::Protocol)
}

fn request(
    method: Method,
    path: String,
    headers: Vec<(&'static str, String)>,
    body: Option<serde_json::Value>,
) -> HttpRequest {
    let mut headers = headers;
    if body.is_some() {
        headers.push(("content-type", "application/json".to_owned()));
    }
    HttpRequest {
        method,
        path,
        headers,
        body: body.map(|b| b.to_string().into_bytes()),
    }
}

type Headers = Vec<(&'static str, String)>;

/// What N does next.
pub enum AskStep {
    /// Send this request, then pass the answer to [`ApprovalAsk::receive`].
    Send(HttpRequest),
    /// Nothing new: [`poll`](ApprovalAsk::poll) again after this long.
    Wait { delay_ms: u64 },
    /// Show this code, ask the user to compare it on the signed-in device,
    /// and keep polling.
    ShowCode { code: String },
    /// The Secret Key arrived: log in with it and the master password.
    Approved { secret_key: Secret<String> },
    /// Denied, given up or expired: sign in another way.
    Ended,
}

impl fmt::Debug for AskStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Send(request) => f.debug_tuple("Send").field(request).finish(),
            Self::Wait { delay_ms } => f.debug_struct("Wait").field("delay_ms", delay_ms).finish(),
            Self::ShowCode { .. } => f.write_str("ShowCode"),
            Self::Approved { .. } => f.write_str("Approved"),
            Self::Ended => f.write_str("Ended"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Waiting {
    Create,
    Poll,
    Reveal,
}

/// The new device's side of one approval.
pub struct ApprovalAsk {
    keys: ApprovalKeys,
    id: Option<(String, Id)>,
    token: Option<Secret<String>>,
    existing_pub: Option<[u8; 32]>,
    waiting: Waiting,
}

impl fmt::Debug for ApprovalAsk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApprovalAsk([REDACTED])")
    }
}

#[derive(Deserialize)]
struct Created {
    id: String,
    poll_token: String,
}

#[derive(Deserialize)]
struct Polled {
    state: String,
    #[serde(default)]
    existing_pub: Option<String>,
    #[serde(default)]
    envelope: Option<String>,
}

impl ApprovalAsk {
    /// A new request for the account of `email`; send the request it gives.
    pub fn start(email: &str, platform: Platform) -> Result<(Self, HttpRequest), ClientError> {
        let keys = ApprovalKeys::generate()?;
        let body = json!({
            "email": email.trim(), "commitment": b64(&keys.commitment()), "platform": platform.as_str(),
        });
        let ask = Self {
            keys,
            id: None,
            token: None,
            existing_pub: None,
            waiting: Waiting::Create,
        };
        Ok((
            ask,
            request(
                Method::Post,
                "/v1/approvals".to_owned(),
                Vec::new(),
                Some(body),
            ),
        ))
    }

    fn auth(&self) -> Result<(String, Headers), ClientError> {
        let (Some((id, _)), Some(token)) = (&self.id, &self.token) else {
            return Err(ClientError::NotRunning);
        };
        Ok((
            format!("/v1/approvals/{id}"),
            vec![(
                "authorization",
                format!("Approval {}", token.expose_secret()),
            )],
        ))
    }

    /// Asks where the request is; send it, then pass the answer to `receive`.
    pub fn poll(&mut self) -> Result<HttpRequest, ClientError> {
        let (path, headers) = self.auth()?;
        self.waiting = Waiting::Poll;
        Ok(request(Method::Get, path, headers, None))
    }

    /// Gives up; send it without waiting for the answer.
    pub fn cancel(&self) -> Option<HttpRequest> {
        let (path, headers) = self.auth().ok()?;
        Some(request(Method::Delete, path, headers, None))
    }

    pub fn receive(&mut self, response: &HttpResponse) -> Result<AskStep, ClientError> {
        match (self.waiting, response.status) {
            (_, 429) => Err(ClientError::RateLimited(
                response
                    .header("retry-after")
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(1),
            )),
            (Waiting::Create, 201) => {
                let created: Created =
                    serde_json::from_slice(&response.body).map_err(|_| ClientError::Protocol)?;
                let id = parse_id(&created.id)?;
                self.id = Some((created.id, id));
                self.token = Some(Secret::new(created.poll_token));
                Ok(AskStep::Wait {
                    delay_ms: POLL_EVERY_MS,
                })
            }
            (Waiting::Poll, 200) => self.on_poll(response),
            (Waiting::Poll, 404) => Ok(AskStep::Ended),
            (Waiting::Reveal, 204) => {
                let existing = self.existing_pub.ok_or(ClientError::Protocol)?;
                let (_, id) = self.id.as_ref().ok_or(ClientError::NotRunning)?;
                let channel = self.keys.channel(id, &self.keys.public(), &existing)?;
                Ok(AskStep::ShowCode {
                    code: channel.code_text(),
                })
            }
            (Waiting::Reveal, 404) => Ok(AskStep::Ended),
            (_, status) => Err(ClientError::Server(status)),
        }
    }

    fn on_poll(&mut self, response: &HttpResponse) -> Result<AskStep, ClientError> {
        let polled: Polled =
            serde_json::from_slice(&response.body).map_err(|_| ClientError::Protocol)?;
        let wait = Ok(AskStep::Wait {
            delay_ms: POLL_EVERY_MS,
        });
        match polled.state.as_str() {
            "pending" | "revealed" => wait,
            "accepted" if self.existing_pub.is_none() => {
                self.existing_pub = Some(key32(polled.existing_pub.as_deref())?);
                let (path, headers) = self.auth()?;
                self.waiting = Waiting::Reveal;
                let body = json!({ "new_pub": b64(&self.keys.public()) });
                Ok(AskStep::Send(request(
                    Method::Post,
                    format!("{path}/reveal"),
                    headers,
                    Some(body),
                )))
            }
            "accepted" => wait,
            "completed" => {
                let existing = self.existing_pub.ok_or(ClientError::Protocol)?;
                let (_, id) = self.id.as_ref().ok_or(ClientError::NotRunning)?;
                let envelope = URL_SAFE_NO_PAD
                    .decode(polled.envelope.as_deref().ok_or(ClientError::Protocol)?)
                    .map_err(|_| ClientError::Protocol)
                    .and_then(|bytes| {
                        Envelope::from_bytes(&bytes).map_err(|_| ClientError::Protocol)
                    })?;
                let secret = self
                    .keys
                    .channel(id, &self.keys.public(), &existing)?
                    .open(&envelope)
                    .map_err(|_| ClientError::ApprovalFailed)?;
                let text = String::from_utf8(secret.expose_secret().clone())
                    .map_err(|_| ClientError::ApprovalFailed)?;
                Ok(AskStep::Approved {
                    secret_key: Secret::new(text),
                })
            }
            _ => Ok(AskStep::Ended),
        }
    }
}

/// One of the account's open requests, as E shows it.
#[derive(Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    pub id: String,
    /// The new device's platform, as it said.
    pub platform: String,
    /// Unix ms.
    pub created_at: i64,
    /// `pending`, `accepted` or `revealed`.
    pub state: String,
    /// The code to compare, once this device accepted and the new device
    /// revealed its key. Never shown for a key that fails its commitment.
    pub code: Option<String>,
}

impl fmt::Debug for ApprovalRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApprovalRequest")
            .field("platform", &self.platform)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

/// What the signed-in device keeps between accepting and completing.
pub(crate) struct Approving {
    id: Id,
    keys: ApprovalKeys,
    commitment: [u8; 32],
    new_pub: Option<[u8; 32]>,
}

/// Which approval call is running, for its answer.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApprovalCall {
    List,
    Accept,
    Complete,
    Deny,
}

#[derive(Deserialize)]
struct Listed {
    id: String,
    platform: String,
    state: String,
    created_at: i64,
    commitment: String,
    #[serde(default)]
    new_pub: Option<String>,
}

impl<S: Store> Client<S> {
    fn start_approval_call(
        &mut self,
        call: ApprovalCall,
        method: Method,
        path: String,
        body: Option<serde_json::Value>,
    ) -> Result<Step, ClientError> {
        self.idle()?;
        let step =
            self.engine()?
                .start_call(method, path, body.map(|b| b.to_string().into_bytes()))?;
        self.running = Running::Approval(call);
        self.map_engine_step(step)
    }

    /// The account's open requests; ends with `Approvals`.
    pub fn start_list_approvals(&mut self) -> Result<Step, ClientError> {
        self.start_approval_call(
            ApprovalCall::List,
            Method::Get,
            "/v1/approvals".to_owned(),
            None,
        )
    }

    /// Accepts a request from the list with a fresh key; ends with
    /// `ApprovalUpdated`. List again to see the code once the new device
    /// revealed its key.
    pub fn start_accept_approval(&mut self, approval_id: &str) -> Result<Step, ClientError> {
        let id = parse_id(approval_id)?;
        let commitment = self
            .approval_commitments
            .get(&id)
            .copied()
            .ok_or(ClientError::UnknownApproval)?;
        let keys = ApprovalKeys::generate()?;
        let body = json!({ "existing_pub": b64(&keys.public()) });
        self.approving = Some(Approving {
            id,
            keys,
            commitment,
            new_pub: None,
        });
        self.start_approval_call(
            ApprovalCall::Accept,
            Method::Post,
            format!("/v1/approvals/{approval_id}/accept"),
            Some(body),
        )
    }

    /// After the user saw the same code on both devices: seals this
    /// device's Secret Key for the new one; ends with `ApprovalUpdated`.
    pub fn start_complete_approval(
        &mut self,
        approval_id: &str,
        secret_key: &str,
    ) -> Result<Step, ClientError> {
        let id = parse_id(approval_id)?;
        let approving = self
            .approving
            .as_ref()
            .filter(|a| a.id == id)
            .ok_or(ClientError::UnknownApproval)?;
        let new_pub = approving.new_pub.ok_or(ClientError::UnknownApproval)?;
        let envelope = approving
            .keys
            .channel(&id, &new_pub, &approving.keys.public())?
            .seal(secret_key.as_bytes())?;
        let body = json!({ "envelope": b64(&envelope.to_bytes()) });
        self.start_approval_call(
            ApprovalCall::Complete,
            Method::Post,
            format!("/v1/approvals/{approval_id}/complete"),
            Some(body),
        )
    }

    /// Ends a request: the codes didn't match, or it isn't the user's.
    pub fn start_deny_approval(&mut self, approval_id: &str) -> Result<Step, ClientError> {
        parse_id(approval_id)?;
        self.start_approval_call(
            ApprovalCall::Deny,
            Method::Post,
            format!("/v1/approvals/{approval_id}/deny"),
            None,
        )
    }

    /// Reads the answer to an approval call.
    pub(crate) fn approval_answer(
        &mut self,
        call: ApprovalCall,
        status: u16,
        body: &[u8],
    ) -> Result<crate::Outcome, ClientError> {
        match (call, status) {
            (ApprovalCall::List, 200) => Ok(crate::Outcome::Approvals(self.read_approvals(body)?)),
            (ApprovalCall::Accept, 204) => Ok(crate::Outcome::ApprovalUpdated),
            (ApprovalCall::Complete | ApprovalCall::Deny, 204) => {
                self.approving = None;
                Ok(crate::Outcome::ApprovalUpdated)
            }
            (ApprovalCall::Accept | ApprovalCall::Complete, 409) => {
                Err(ClientError::UnknownApproval)
            }
            (_, 404) => {
                self.approving = None;
                Err(ClientError::UnknownApproval)
            }
            (_, status) => Err(ClientError::Server(status)),
        }
    }

    fn read_approvals(&mut self, body: &[u8]) -> Result<Vec<ApprovalRequest>, ClientError> {
        let listed: Vec<Listed> =
            serde_json::from_slice(body).map_err(|_| ClientError::Protocol)?;
        self.approval_commitments.clear();
        let mut out = Vec::with_capacity(listed.len());
        for request in listed {
            let id = parse_id(&request.id)?;
            let commitment = key32(Some(&request.commitment))?;
            self.approval_commitments.insert(id, commitment);
            let mut code = None;
            if let (Some(approving), Some(new_pub)) =
                (self.approving.as_mut(), request.new_pub.as_deref())
                && approving.id == id
                && approving.commitment == commitment
            {
                let new_pub = key32(Some(new_pub))?;
                // A key that fails its commitment never gets a code.
                if check_approval_commitment(&commitment, &new_pub).is_ok() {
                    let channel =
                        approving
                            .keys
                            .channel(&id, &new_pub, &approving.keys.public())?;
                    approving.new_pub = Some(new_pub);
                    code = Some(channel.code_text());
                }
            }
            out.push(ApprovalRequest {
                id: request.id,
                platform: request.platform,
                created_at: request.created_at,
                state: request.state,
                code,
            });
        }
        Ok(out)
    }
}
