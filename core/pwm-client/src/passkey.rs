//! Passkeys in the vault (D37 §5): creating one into the right login,
//! listing the ones a sign-in may use, and signing. The WebAuthn work is
//! `pwm-passkey`; origins come from the platform's browser, never the page.
//! A platform that builds the client data itself (iOS, D47 §6) uses the
//! hashed calls, and gets the identities it suggests above the keyboard.
//! Android's Credential Manager (D61) uses the `_android` calls: a privileged
//! browser with its origin (and usually a client data hash), or an app, which
//! gets a site's passkeys only when the site's Digital Asset Links file lists
//! it, and whose new passkey links the login to it.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pwm_autofill::{app_link, registrable_domain};
use pwm_crypto::Secret;
use pwm_passkey::{
    AndroidCaller, Caller, CreateRequest, GetRequest, PasskeyError, StoredPasskey, android_rp_id,
    asset_links_url, authenticate, authenticate_android, authenticate_hashed, effective_rp_id,
    read_asset_links, register_android_random, register_hashed_random, register_random,
};
use pwm_store::Store;
use pwm_vault::{Item, ItemId, ItemKind, Login, LoginUrl, Passkey, UrlMatch};

use crate::client::{parse_id, parse_item_id};
use crate::{Client, ClientError};

/// What a `create` request gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasskeyCreateCheck {
    /// Ask the user to create a passkey for this account.
    Ready { rp_id: String, user_name: String },
    /// The site excludes a passkey this vault already holds: hand it back.
    Excluded,
    /// The site accepts no ES256: hand it back to the browser.
    Unsupported,
}

/// A stored passkey a sign-in may use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasskeyCandidate {
    pub vault_id: String,
    pub item_id: String,
    /// Base64url, as WebAuthn shows it.
    pub credential_id: String,
    pub user_name: String,
    pub title: String,
}

/// A passkey made from a platform's request (D47 §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashedPasskey {
    pub credential_id: Vec<u8>,
    pub attestation_object: Vec<u8>,
}

/// What a platform puts in its assertion credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashedAssertion {
    pub credential_id: Vec<u8>,
    pub user_handle: Vec<u8>,
    pub authenticator_data: Vec<u8>,
    pub signature: Vec<u8>,
}

/// One suggestion the platform may show before unlock (D47 §5): a domain
/// or RP id and a username, never a password or a title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialIdentity {
    pub vault_id: String,
    pub item_id: String,
    /// `password` or `passkey`.
    pub kind: &'static str,
    /// A domain for a password; the RP id for a passkey.
    pub service: String,
    pub user: String,
    /// Passkeys only; empty for a password.
    pub credential_id: Vec<u8>,
    pub user_handle: Vec<u8>,
}

/// The domain a login URL is suggested on, if any: its registrable domain
/// for the default match, otherwise its host. Regex and never URLs are not
/// suggested.
fn suggested_domain(url: &LoginUrl) -> Option<String> {
    if matches!(url.r#match, UrlMatch::Regex | UrlMatch::Never) {
        return None;
    }
    let parsed = url::Url::parse(&url.url).ok()?;
    let host = match parsed.host()? {
        url::Host::Domain(host) => host.to_ascii_lowercase(),
        _ => return None,
    };
    if url.r#match == UrlMatch::Domain {
        return Some(registrable_domain(&host).unwrap_or(host));
    }
    Some(host)
}

/// Who asks Android's Credential Manager for a passkey (D61 §1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AndroidPasskeyCaller {
    pub package: String,
    /// SHA-256 of the app's signing certificate.
    pub cert_sha256: Vec<u8>,
    /// Only for a privileged browser: the origin Android returned for it.
    pub origin: Option<String>,
    /// A browser's hash of the client data it built. A browser without one
    /// gets client data built here for its origin.
    pub client_data_hash: Option<Vec<u8>>,
    /// For an app: the RP's asset links file and the files it includes, as
    /// fetched. Any of them listing the app is enough.
    pub asset_links: Vec<String>,
}

/// A checked Android caller, for the passkey crate.
enum Checked<'a> {
    Web(Caller<'a>),
    Android(AndroidCaller<'a>),
}

impl AndroidPasskeyCaller {
    /// The RP id this caller may use, and how to sign for it. An app must
    /// be listed by one of its asset links files.
    fn check(&self, requested: Option<&str>) -> Result<(String, Checked<'_>), ClientError> {
        let checked = match (&self.origin, &self.client_data_hash) {
            (Some(origin), None) => {
                let caller = Caller {
                    origin,
                    top_origin: None,
                };
                return Ok((effective_rp_id(origin, requested)?, Checked::Web(caller)));
            }
            (Some(origin), Some(hash)) => AndroidCaller::Browser {
                origin,
                client_data_hash: hash,
            },
            (None, _) => AndroidCaller::App {
                package: &self.package,
                cert_sha256: &self.cert_sha256,
            },
        };
        let rp_id = android_rp_id(&checked, requested)?;
        if self.origin.is_none() {
            let listed = self.asset_links.iter().any(|text| {
                read_asset_links(text, &self.package, &self.cert_sha256)
                    .is_ok_and(|links| links.linked)
            });
            if !listed {
                return Err(PasskeyError::AppNotLinked.into());
            }
        }
        Ok((rp_id, Checked::Android(checked)))
    }

    /// The `android-app://` link a new passkey adds to its login, for apps.
    fn app_link(&self) -> Result<Option<String>, ClientError> {
        if self.origin.is_some() {
            return Ok(None);
        }
        let hex: String = self
            .cert_sha256
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        app_link(&self.package, &hex)
            .map(Some)
            .map_err(|_| PasskeyError::InvalidRequest.into())
    }
}

/// The asset links file to fetch for a Credential Manager request from an
/// app (D61 §4): the RP id it names, which it must.
pub fn passkey_asset_links_url(request_json: &str, create: bool) -> Result<String, ClientError> {
    let rp_id = if create {
        CreateRequest::from_json(request_json)?.rp_id
    } else {
        GetRequest::from_json(request_json)?.rp_id
    };
    Ok(asset_links_url(
        &rp_id.ok_or(PasskeyError::InvalidRequest)?,
    )?)
}

/// The files an asset links file includes, for the platform to fetch one
/// level deep; none when it is malformed.
pub fn asset_links_includes(text: &str) -> Vec<String> {
    pwm_passkey::asset_links_includes(text).unwrap_or_default()
}

impl From<PasskeyError> for ClientError {
    fn from(err: PasskeyError) -> Self {
        Self::Passkey(err)
    }
}

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn caller<'a>(origin: &'a str, top_origin: Option<&'a str>) -> Caller<'a> {
    Caller { origin, top_origin }
}

impl<S: Store> Client<S> {
    /// Every login with its passkeys, as `(vault, item id, item)`.
    fn logins_with_passkeys(&self) -> Result<Vec<(String, ItemId, Item)>, ClientError> {
        let engine = self.engine_ref()?;
        let mut out = Vec::new();
        for vault_id in self.vault_ids()? {
            let id = parse_id(&vault_id)?;
            for (item_id, item) in engine.items(&id)? {
                if !item.is_trashed() && matches!(item.kind, ItemKind::Login(_)) {
                    out.push((vault_id.clone(), item_id, item));
                }
            }
        }
        Ok(out)
    }

    /// Checks a `create` before the user is asked (D37 §2, §3).
    pub fn passkey_create_check(
        &self,
        origin: &str,
        _top_origin: Option<&str>,
        request_json: &str,
    ) -> Result<PasskeyCreateCheck, ClientError> {
        let request = CreateRequest::from_json(request_json)?;
        let rp_id = effective_rp_id(origin, request.rp_id.as_deref())?;
        self.create_check(rp_id, request)
    }

    /// [`Self::passkey_create_check`] for a Credential Manager request.
    pub fn passkey_create_check_android(
        &self,
        caller: &AndroidPasskeyCaller,
        request_json: &str,
    ) -> Result<PasskeyCreateCheck, ClientError> {
        let request = CreateRequest::from_json(request_json)?;
        let (rp_id, _) = caller.check(request.rp_id.as_deref())?;
        self.create_check(rp_id, request)
    }

    fn create_check(
        &self,
        rp_id: String,
        request: CreateRequest,
    ) -> Result<PasskeyCreateCheck, ClientError> {
        if !request.es256 {
            return Ok(PasskeyCreateCheck::Unsupported);
        }
        let ours = self
            .logins_with_passkeys()?
            .into_iter()
            .any(|(_, _, item)| {
                let ItemKind::Login(login) = &item.kind else {
                    return false;
                };
                login
                    .passkeys
                    .iter()
                    .any(|p| p.rp_id == rp_id && request.exclude.contains(&p.credential_id))
            });
        if ours {
            return Ok(PasskeyCreateCheck::Excluded);
        }
        Ok(PasskeyCreateCheck::Ready {
            rp_id,
            user_name: request.user_name,
        })
    }

    /// Creates a passkey after the user confirmed, stores it, and returns the
    /// `RegistrationResponseJSON` for the page. It joins the site's login
    /// with that username, or a new login titled with the RP id.
    pub fn passkey_create(
        &mut self,
        vault_id: &str,
        origin: &str,
        top_origin: Option<&str>,
        request_json: &str,
        now_ms: i64,
    ) -> Result<String, ClientError> {
        self.engine_ref()?;
        let request = CreateRequest::from_json(request_json)?;
        let registration = register_random(caller(origin, top_origin), &request)?;
        self.store_passkey(vault_id, origin, &registration.passkey, now_ms, None)?;
        Ok(registration.response_json)
    }

    /// [`Self::passkey_create`] for a Credential Manager request (D61 §3).
    /// An app's passkey also links the login to the app.
    pub fn passkey_create_android(
        &mut self,
        vault_id: &str,
        caller: &AndroidPasskeyCaller,
        request_json: &str,
        now_ms: i64,
    ) -> Result<String, ClientError> {
        self.engine_ref()?;
        let request = CreateRequest::from_json(request_json)?;
        let (rp_id, checked) = caller.check(request.rp_id.as_deref())?;
        let registration = match checked {
            Checked::Web(web) => register_random(web, &request)?,
            Checked::Android(android) => register_android_random(android, &request)?,
        };
        let origin = match &caller.origin {
            Some(origin) => origin.clone(),
            None => format!("https://{rp_id}"),
        };
        let link = caller.app_link()?;
        self.store_passkey(
            vault_id,
            &origin,
            &registration.passkey,
            now_ms,
            link.as_deref(),
        )?;
        Ok(registration.response_json)
    }

    /// Creates a passkey for a platform that checked `rp_id` itself and
    /// hands over only the client data hash (D47 §6), after the user
    /// confirmed. It is stored as [`Self::passkey_create`] stores one.
    pub fn passkey_register_hashed(
        &mut self,
        vault_id: &str,
        rp_id: &str,
        user_name: &str,
        user_handle: &[u8],
        now_ms: i64,
    ) -> Result<HashedPasskey, ClientError> {
        self.engine_ref()?;
        let registration = register_hashed_random(rp_id, user_handle, user_name)?;
        let origin = format!("https://{}", registration.passkey.rp_id);
        self.store_passkey(vault_id, &origin, &registration.passkey, now_ms, None)?;
        Ok(HashedPasskey {
            credential_id: registration.passkey.credential_id.clone(),
            attestation_object: registration.attestation_object,
        })
    }

    /// Stores a new passkey in the site's login with that username, or in
    /// a new login titled with the RP id. `app_link` is added to the login
    /// when it doesn't have it yet.
    fn store_passkey(
        &mut self,
        vault_id: &str,
        origin: &str,
        stored: &StoredPasskey,
        now_ms: i64,
        app_link: Option<&str>,
    ) -> Result<(), ClientError> {
        let link_url = |url: &str| {
            let mut link = LoginUrl::default();
            link.url = url.to_owned();
            link.r#match = UrlMatch::Exact;
            link
        };
        let mut passkey = Passkey::default();
        passkey.credential_id = stored.credential_id.clone();
        passkey.rp_id = stored.rp_id.clone();
        passkey.user_handle = stored.user_handle.clone();
        passkey.user_name = stored.user_name.clone();
        passkey.private_key = stored.private_key.expose_secret().clone();
        passkey.sign_count = 0;
        passkey.created_at = now_ms;

        // The site's login for this username, if there is one.
        let user = stored.user_name.trim().to_owned();
        let matches = self.autofill_matches(origin, None)?;
        let existing = matches.into_iter().find(|m| {
            !m.decision.starts_with("refuse") && m.username.trim().eq_ignore_ascii_case(&user)
        });
        match existing {
            Some(found) => {
                let (vault, id) = (parse_id(&found.vault_id)?, parse_item_id(&found.item_id)?);
                let engine = self.engine()?;
                let mut current = engine.item(&vault, &id)?.ok_or(ClientError::UnknownItem)?;
                let mut edited = current.clone();
                if let ItemKind::Login(login) = &mut edited.kind {
                    login.passkeys.push(passkey);
                    if let Some(app) = app_link
                        && !login.urls.iter().any(|u| u.url == app)
                    {
                        login.urls.push(link_url(app));
                    }
                }
                current.apply_edit(edited, now_ms)?;
                engine.save_item(&vault, &id, &current)?;
            }
            None => {
                let vault = parse_id(vault_id)?;
                let url = url::Url::parse(origin).map_err(|_| ClientError::InvalidUrl)?;
                let mut login_url = LoginUrl::default();
                login_url.url = url.origin().ascii_serialization();
                login_url.r#match = UrlMatch::Domain;
                let mut login = Login::default();
                login.username = user;
                login.urls = vec![login_url];
                login.urls.extend(app_link.map(link_url));
                login.passkeys = vec![passkey];
                let item = Item::new(ItemKind::Login(login), stored.rp_id.clone(), now_ms);
                self.engine()?
                    .save_item(&vault, &ItemId::generate()?, &item)?;
            }
        }
        Ok(())
    }

    /// The stored passkeys a `get` on `origin` may use, newest first.
    pub fn passkey_candidates(
        &self,
        origin: &str,
        _top_origin: Option<&str>,
        request_json: &str,
    ) -> Result<Vec<PasskeyCandidate>, ClientError> {
        self.engine_ref()?;
        let request = GetRequest::from_json(request_json)?;
        let rp_id = effective_rp_id(origin, request.rp_id.as_deref())?;
        self.candidates(&rp_id, &request)
    }

    /// [`Self::passkey_candidates`] for a Credential Manager request.
    pub fn passkey_candidates_android(
        &self,
        caller: &AndroidPasskeyCaller,
        request_json: &str,
    ) -> Result<Vec<PasskeyCandidate>, ClientError> {
        self.engine_ref()?;
        let request = GetRequest::from_json(request_json)?;
        let (rp_id, _) = caller.check(request.rp_id.as_deref())?;
        self.candidates(&rp_id, &request)
    }

    fn candidates(
        &self,
        rp_id: &str,
        request: &GetRequest,
    ) -> Result<Vec<PasskeyCandidate>, ClientError> {
        let mut out: Vec<(i64, PasskeyCandidate)> = Vec::new();
        for (vault_id, item_id, item) in self.logins_with_passkeys()? {
            let ItemKind::Login(login) = &item.kind else {
                continue;
            };
            for p in &login.passkeys {
                let allowed = request.allow.is_empty() || request.allow.contains(&p.credential_id);
                if p.rp_id == rp_id && allowed {
                    out.push((
                        p.created_at,
                        PasskeyCandidate {
                            vault_id: vault_id.clone(),
                            item_id: item_id.to_string(),
                            credential_id: b64(&p.credential_id),
                            user_name: p.user_name.clone(),
                            title: item.title.clone(),
                        },
                    ));
                }
            }
        }
        out.sort_by_key(|(created, _)| std::cmp::Reverse(*created));
        Ok(out.into_iter().map(|(_, c)| c).collect())
    }

    /// Signs a `get` with the chosen passkey: `AuthenticationResponseJSON`.
    pub fn passkey_get(
        &self,
        origin: &str,
        top_origin: Option<&str>,
        request_json: &str,
        vault_id: &str,
        item_id: &str,
        credential_id: &str,
    ) -> Result<String, ClientError> {
        let request = GetRequest::from_json(request_json)?;
        let stored = self.stored_passkey(vault_id, item_id, |id| b64(id) == credential_id)?;
        Ok(authenticate(caller(origin, top_origin), &request, &stored)?)
    }

    /// [`Self::passkey_get`] for a Credential Manager request (D61 §3).
    pub fn passkey_get_android(
        &self,
        caller: &AndroidPasskeyCaller,
        request_json: &str,
        vault_id: &str,
        item_id: &str,
        credential_id: &str,
    ) -> Result<String, ClientError> {
        let request = GetRequest::from_json(request_json)?;
        let (_, checked) = caller.check(request.rp_id.as_deref())?;
        let stored = self.stored_passkey(vault_id, item_id, |id| b64(id) == credential_id)?;
        Ok(match checked {
            Checked::Web(web) => authenticate(web, &request, &stored)?,
            Checked::Android(android) => authenticate_android(android, &request, &stored)?,
        })
    }

    /// Signs a sign-in the platform built, given its client data hash
    /// (D47 §6), with the passkey the user chose.
    pub fn passkey_assert_hashed(
        &self,
        vault_id: &str,
        item_id: &str,
        credential_id: &[u8],
        rp_id: &str,
        client_data_hash: &[u8],
    ) -> Result<HashedAssertion, ClientError> {
        let stored = self.stored_passkey(vault_id, item_id, |id| id == credential_id)?;
        let signed = authenticate_hashed(rp_id, client_data_hash, &stored)?;
        Ok(HashedAssertion {
            credential_id: stored.credential_id.clone(),
            user_handle: stored.user_handle.clone(),
            authenticator_data: signed.authenticator_data,
            signature: signed.signature,
        })
    }

    /// The passkey of a login whose credential id passes `is_it`.
    fn stored_passkey(
        &self,
        vault_id: &str,
        item_id: &str,
        is_it: impl Fn(&[u8]) -> bool,
    ) -> Result<StoredPasskey, ClientError> {
        let (vault, id) = (parse_id(vault_id)?, parse_item_id(item_id)?);
        let item = self
            .engine_ref()?
            .item(&vault, &id)?
            .ok_or(ClientError::UnknownItem)?;
        let ItemKind::Login(login) = &item.kind else {
            return Err(ClientError::UnknownItem);
        };
        let passkey = login
            .passkeys
            .iter()
            .find(|p| is_it(&p.credential_id))
            .ok_or(ClientError::UnknownItem)?;
        Ok(StoredPasskey {
            credential_id: passkey.credential_id.clone(),
            rp_id: passkey.rp_id.clone(),
            user_handle: passkey.user_handle.clone(),
            user_name: passkey.user_name.clone(),
            private_key: Secret::new(passkey.private_key.clone()),
        })
    }

    /// What the platform may suggest above the keyboard before unlock
    /// (D47 §5): each login with a password on each domain it fills, and
    /// each passkey on its RP id. Trashed items are left out.
    pub fn credential_identities(&self) -> Result<Vec<CredentialIdentity>, ClientError> {
        let mut out = Vec::new();
        for (vault_id, item_id, item) in self.logins_with_passkeys()? {
            let ItemKind::Login(login) = &item.kind else {
                continue;
            };
            if !login.password.is_empty() {
                let mut domains: Vec<String> = Vec::new();
                for domain in login.urls.iter().filter_map(suggested_domain) {
                    if !domains.contains(&domain) {
                        domains.push(domain);
                    }
                }
                for service in domains {
                    out.push(CredentialIdentity {
                        vault_id: vault_id.clone(),
                        item_id: item_id.to_string(),
                        kind: "password",
                        service,
                        user: login.username.clone(),
                        credential_id: Vec::new(),
                        user_handle: Vec::new(),
                    });
                }
            }
            for p in &login.passkeys {
                out.push(CredentialIdentity {
                    vault_id: vault_id.clone(),
                    item_id: item_id.to_string(),
                    kind: "passkey",
                    service: p.rp_id.clone(),
                    user: p.user_name.clone(),
                    credential_id: p.credential_id.clone(),
                    user_handle: p.user_handle.clone(),
                });
            }
        }
        Ok(out)
    }
}
