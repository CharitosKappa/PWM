//! Passkeys in the vault (D37 §5): creating one into the right login,
//! listing the ones a sign-in may use, and signing. The WebAuthn work is
//! `pwm-passkey`; origins come from the platform's browser, never the page.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use pwm_crypto::Secret;
use pwm_passkey::{
    Caller, CreateRequest, GetRequest, PasskeyError, StoredPasskey, authenticate, effective_rp_id,
    register_random,
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
        let stored = &registration.passkey;
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
                login.passkeys = vec![passkey];
                let item = Item::new(ItemKind::Login(login), stored.rp_id.clone(), now_ms);
                self.engine()?
                    .save_item(&vault, &ItemId::generate()?, &item)?;
            }
        }
        Ok(registration.response_json)
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
            .find(|p| b64(&p.credential_id) == credential_id)
            .ok_or(ClientError::UnknownItem)?;
        let stored = StoredPasskey {
            credential_id: passkey.credential_id.clone(),
            rp_id: passkey.rp_id.clone(),
            user_handle: passkey.user_handle.clone(),
            user_name: passkey.user_name.clone(),
            private_key: Secret::new(passkey.private_key.clone()),
        };
        Ok(authenticate(caller(origin, top_origin), &request, &stored)?)
    }
}
