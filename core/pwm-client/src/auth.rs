//! Sign-up and login as server conversations (D26, D31): each step builds
//! the next request from the previous answer. The OPAQUE messages, tokens
//! and keys never leave the core.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pwm_crypto::{
    AccountKeys, AccountKeyset, Envelope, Id, KdfParams, OpaqueLogin, OpaqueRegistration, Secret,
    SecretKey,
};
use pwm_store::AccountRecord;
use pwm_sync::{HttpRequest, HttpResponse, Method, Tokens};
use pwm_vault::ItemId;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{ClientError, Device};

pub(crate) fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn unb64(text: &str) -> Result<Vec<u8>, ClientError> {
    URL_SAFE_NO_PAD
        .decode(text)
        .map_err(|_| ClientError::Protocol)
}

/// A 16-byte id as a UUID string.
pub fn id_text(id: &Id) -> String {
    ItemId::from_bytes(*id).to_string()
}

fn parse_id(text: &str) -> Result<Id, ClientError> {
    text.parse::<ItemId>()
        .map(|id| *id.as_bytes())
        .map_err(|_| ClientError::Protocol)
}

/// A JSON POST without authorization.
fn post(path: &str, body: &serde_json::Value) -> HttpRequest {
    HttpRequest {
        method: Method::Post,
        path: path.to_owned(),
        headers: vec![("content-type", "application/json".to_owned())],
        body: Some(body.to_string().into_bytes()),
    }
}

/// What an error answer during sign-up or login means.
pub(crate) fn failure(response: &HttpResponse) -> ClientError {
    let code = serde_json::from_slice::<serde_json::Value>(&response.body)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_owned));
    match (response.status, code.as_deref()) {
        (401, Some("invalid_credentials")) => ClientError::InvalidCredentials,
        (401, _) => ClientError::SignedOut,
        (403, Some("device_revoked")) => ClientError::DeviceRevoked,
        (409, Some("account_exists")) => ClientError::AccountExists,
        (429, _) => ClientError::RateLimited(
            response
                .header("retry-after")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(1),
        ),
        (status, _) => ClientError::Server(status),
    }
}

fn device_json(device: &Device, name_sealed: Option<&Envelope>) -> serde_json::Value {
    match name_sealed {
        Some(name) => json!({
            "id": id_text(&device.id), "platform": device.platform.as_str(),
            "name_sealed": b64(&name.to_bytes()),
        }),
        None => json!({ "id": id_text(&device.id), "platform": device.platform.as_str() }),
    }
}

#[derive(Deserialize)]
struct TokenJson {
    access_token: String,
    refresh_token: String,
}

fn tokens(response: &HttpResponse) -> Result<Tokens, ClientError> {
    let t: TokenJson = serde_json::from_slice(&response.body).map_err(|_| ClientError::Protocol)?;
    Ok(Tokens::new(t.access_token, t.refresh_token))
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct KdfJson {
    memory_kib: u32,
    iterations: u32,
    parallelism: u32,
}

impl From<KdfJson> for KdfParams {
    fn from(k: KdfJson) -> Self {
        Self {
            memory_kib: k.memory_kib,
            iterations: k.iterations,
            parallelism: k.parallelism,
        }
    }
}

/// Where a sign-up or login is.
pub(crate) enum AuthFlow {
    RegisterStart(Box<Registration>),
    RegisterFinish(Box<Registration>),
    Prelogin(Box<PreloginState>),
    LoginStart(Box<LoginState>),
    LoginFinish(Box<LoginState>),
    FetchAccount(Box<LoginState>, Tokens),
    /// Logged in; the device's name is being sent (D43 §6). Its answer, or
    /// no answer, does not change the outcome.
    NameDevice(Box<(AccountRecord, AccountKeyset, Tokens)>),
}

pub(crate) struct Registration {
    pub email: String,
    pub account_id: Id,
    pub device: Device,
    pub new: pwm_crypto::NewAccount,
    pub opaque: Option<OpaqueRegistration>,
}

pub(crate) struct PreloginState {
    pub password: Secret<String>,
    pub secret_key: SecretKey,
    pub device: Device,
}

pub(crate) struct LoginState {
    pub account_id: Id,
    pub keys: AccountKeys,
    pub opaque: Option<OpaqueLogin>,
    pub device: Device,
}

/// What a finished flow hands back to the client.
pub(crate) enum Finished {
    /// Signed up: the account record, its unlocked keyset, tokens and the
    /// Secret Key to show once.
    SignedUp(AccountRecord, AccountKeyset, Tokens, Secret<String>),
    LoggedIn(AccountRecord, AccountKeyset, Tokens),
}

pub(crate) enum Next {
    Send(AuthFlow, HttpRequest),
    Done(Box<Finished>),
}

/// The first request of a sign-up. The keys are made here, so the slow KDF
/// runs before anything is sent.
pub(crate) fn start_signup(
    email: &str,
    password: &str,
    device: Device,
    params: KdfParams,
) -> Result<(AuthFlow, HttpRequest), ClientError> {
    let account_id = *ItemId::generate()?.as_bytes();
    let new = pwm_crypto::create_account(password, &account_id, params)?;
    let (opaque, request) = OpaqueRegistration::start(&new.auth_secret)?;
    let body = json!({ "account_id": id_text(&account_id), "registration_request": b64(&request) });
    let flow = AuthFlow::RegisterStart(Box::new(Registration {
        email: email.to_owned(),
        account_id,
        device,
        new,
        opaque: Some(opaque),
    }));
    Ok((flow, post("/v1/auth/register/start", &body)))
}

/// The first request of a login: pre-login fetches the account id, salt and
/// KDF params for the email (D12).
pub(crate) fn start_login(
    email: &str,
    password: &str,
    secret_key: &str,
    device: Device,
) -> Result<(AuthFlow, HttpRequest), ClientError> {
    let secret_key = SecretKey::parse(secret_key).map_err(|_| ClientError::InvalidSecretKey)?;
    let flow = AuthFlow::Prelogin(Box::new(PreloginState {
        password: Secret::new(password.to_owned()),
        secret_key,
        device,
    }));
    Ok((flow, post("/v1/auth/prelogin", &json!({ "email": email }))))
}

pub(crate) fn advance(flow: AuthFlow, response: &HttpResponse) -> Result<Next, ClientError> {
    let ok = |expected: u16| {
        if response.status == expected {
            Ok(())
        } else {
            Err(failure(response))
        }
    };
    let body = || -> Result<serde_json::Value, ClientError> {
        serde_json::from_slice(&response.body).map_err(|_| ClientError::Protocol)
    };
    match flow {
        AuthFlow::RegisterStart(mut reg) => {
            ok(200)?;
            let answer = unb64(
                body()?["registration_response"]
                    .as_str()
                    .ok_or(ClientError::Protocol)?,
            )?;
            let opaque = reg.opaque.take().ok_or(ClientError::Internal)?;
            let upload = opaque
                .finish(&reg.new.auth_secret, &reg.account_id, &answer)
                .map_err(|_| ClientError::Protocol)?;
            let name = reg.new.keyset.seal_device_name(
                &reg.account_id,
                &reg.device.id,
                &reg.device.name,
            )?;
            let p = reg.new.params;
            let request = post(
                "/v1/auth/register/finish",
                &json!({
                    "email": reg.email, "account_id": id_text(&reg.account_id),
                    "registration_upload": b64(&upload), "salt": b64(&reg.new.salt),
                    "kdf": KdfJson { memory_kib: p.memory_kib, iterations: p.iterations, parallelism: p.parallelism },
                    "sealed_keyset": b64(&reg.new.sealed_keyset.to_bytes()),
                    "x25519_pub": b64(&reg.new.keyset.x25519_public()),
                    "ed25519_pub": b64(&reg.new.keyset.ed25519_public()),
                    "device": device_json(&reg.device, Some(&name)),
                }),
            );
            Ok(Next::Send(AuthFlow::RegisterFinish(reg), request))
        }
        AuthFlow::RegisterFinish(reg) => {
            ok(201)?;
            let tokens = tokens(response)?;
            let reg = *reg;
            let record = AccountRecord {
                account_id: reg.account_id,
                salt: reg.new.salt,
                kdf_params: reg.new.params,
                sealed_keyset: reg.new.sealed_keyset,
            };
            let secret_key = reg.new.secret_key.to_text();
            Ok(Next::Done(Box::new(Finished::SignedUp(
                record,
                reg.new.keyset,
                tokens,
                secret_key,
            ))))
        }
        AuthFlow::Prelogin(pre) => {
            ok(200)?;
            let answer = body()?;
            let account_id = parse_id(answer["account_id"].as_str().ok_or(ClientError::Protocol)?)?;
            let salt: [u8; 16] = unb64(answer["salt"].as_str().ok_or(ClientError::Protocol)?)?
                .try_into()
                .map_err(|_| ClientError::Protocol)?;
            let kdf: KdfJson =
                serde_json::from_value(answer["kdf"].clone()).map_err(|_| ClientError::Protocol)?;
            let params = KdfParams::from(kdf);
            // A server must not make the device run an absurd KDF.
            params.validate().map_err(|_| ClientError::Protocol)?;
            let keys = pwm_crypto::derive_account_keys(
                pre.password.expose_secret(),
                &pre.secret_key,
                &account_id,
                &salt,
                &params,
            )?;
            let (opaque, request) = OpaqueLogin::start(keys.auth_secret())?;
            let body =
                json!({ "account_id": id_text(&account_id), "credential_request": b64(&request) });
            let state = LoginState {
                account_id,
                keys,
                opaque: Some(opaque),
                device: pre.device,
            };
            Ok(Next::Send(
                AuthFlow::LoginStart(Box::new(state)),
                post("/v1/auth/login/start", &body),
            ))
        }
        AuthFlow::LoginStart(mut state) => {
            ok(200)?;
            let answer = body()?;
            let credential = unb64(
                answer["credential_response"]
                    .as_str()
                    .ok_or(ClientError::Protocol)?,
            )?;
            let opaque = state.opaque.take().ok_or(ClientError::Internal)?;
            // A wrong password, Secret Key or email fails here, on the device.
            let finalization = opaque
                .finish(state.keys.auth_secret(), &state.account_id, &credential)
                .map_err(|_| ClientError::InvalidCredentials)?;
            // A new device has no keyset yet, so it cannot seal its name: it
            // is sent once the keyset is open (`NameDevice`).
            let body = json!({
                "login_id": answer["login_id"], "credential_finalization": b64(&finalization),
                "device": device_json(&state.device, None),
            });
            Ok(Next::Send(
                AuthFlow::LoginFinish(state),
                post("/v1/auth/login/finish", &body),
            ))
        }
        AuthFlow::LoginFinish(state) => {
            ok(200)?;
            let tokens = tokens(response)?;
            let request = HttpRequest {
                method: Method::Get,
                path: "/v1/account".to_owned(),
                headers: vec![("authorization", format!("Bearer {}", tokens.access()))],
                body: None,
            };
            Ok(Next::Send(AuthFlow::FetchAccount(state, tokens), request))
        }
        AuthFlow::FetchAccount(state, tokens) => {
            ok(200)?;
            let answer = body()?;
            let sealed = Envelope::from_bytes(&unb64(
                answer["sealed_keyset"]
                    .as_str()
                    .ok_or(ClientError::Protocol)?,
            )?)
            .map_err(|_| ClientError::Protocol)?;
            let salt: [u8; 16] = unb64(answer["salt"].as_str().ok_or(ClientError::Protocol)?)?
                .try_into()
                .map_err(|_| ClientError::Protocol)?;
            let kdf: KdfJson =
                serde_json::from_value(answer["kdf"].clone()).map_err(|_| ClientError::Protocol)?;
            let keyset = AccountKeyset::open(&sealed, state.keys.auk(), &state.account_id)
                .map_err(|_| ClientError::WrongPassword)?;
            let record = AccountRecord {
                account_id: state.account_id,
                salt,
                kdf_params: kdf.into(),
                sealed_keyset: sealed,
            };
            // The keyset is open now, so the device can seal its name. A
            // name that cannot be sealed is left out, as before D43.
            let named = (!state.device.name.is_empty())
                .then(|| {
                    keyset.seal_device_name(&state.account_id, &state.device.id, &state.device.name)
                })
                .and_then(Result::ok);
            match named {
                Some(name) => {
                    let request = HttpRequest {
                        method: Method::Patch,
                        path: format!("/v1/devices/{}", id_text(&state.device.id)),
                        headers: vec![
                            ("authorization", format!("Bearer {}", tokens.access())),
                            ("content-type", "application/json".to_owned()),
                        ],
                        body: Some(
                            json!({ "name_sealed": b64(&name.to_bytes()) })
                                .to_string()
                                .into_bytes(),
                        ),
                    };
                    Ok(Next::Send(
                        AuthFlow::NameDevice(Box::new((record, keyset, tokens))),
                        request,
                    ))
                }
                None => Ok(Next::Done(Box::new(Finished::LoggedIn(
                    record, keyset, tokens,
                )))),
            }
        }
        AuthFlow::NameDevice(done) => Ok(Next::Done(Box::new(named(done)))),
    }
}

fn named(done: Box<(AccountRecord, AccountKeyset, Tokens)>) -> Finished {
    let (record, keyset, tokens) = *done;
    Finished::LoggedIn(record, keyset, tokens)
}

/// The login is done even when its device name got no answer.
pub(crate) fn after_network_failure(flow: AuthFlow) -> Option<Finished> {
    match flow {
        AuthFlow::NameDevice(done) => Some(named(done)),
        _ => None,
    }
}

/// Opens the keyset of the account cached on this device.
pub(crate) fn unlock(
    record: &AccountRecord,
    password: &str,
    secret_key: &str,
) -> Result<AccountKeyset, ClientError> {
    let secret_key = SecretKey::parse(secret_key).map_err(|_| ClientError::InvalidSecretKey)?;
    pwm_crypto::unlock_account(
        password,
        &secret_key,
        &record.account_id,
        &record.salt,
        &record.kdf_params,
        &record.sealed_keyset,
    )
    .map_err(|_| ClientError::WrongPassword)
}
