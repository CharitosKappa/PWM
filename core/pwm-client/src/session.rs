//! What the browser extension keeps between service-worker restarts and
//! browser starts (decision D34): a memory-only session blob, and the
//! tokens sealed under the account keyset on disk. Also the current
//! one-time code of an item, for the popup.

use pwm_crypto::{AccountKeyset, Envelope, Secret};
use pwm_store::Store;
use pwm_vault::{ItemKind, Totp};
use serde::{Deserialize, Serialize};

use crate::auth;
use crate::client::parse_id;
use crate::{Client, ClientError, Tokens};

/// Session blob: `v:u8 (= 1) || account_id:16 || envelope`.
const SESSION_VERSION: u8 = 1;
/// Sealed tokens: `v:u8 (= 1) || envelope` over `{"access","refresh"}` JSON.
const TOKENS_VERSION: u8 = 1;

/// The unlocked session, for `chrome.storage.session` (memory only). Both
/// parts are needed to restore it.
pub struct SessionExport {
    pub blob: Vec<u8>,
    pub key: Secret<[u8; 32]>,
}

#[derive(Serialize, Deserialize)]
struct TokensJson {
    access: String,
    refresh: String,
}

impl<S: Store> Client<S> {
    fn account_id_bytes(&self) -> Result<pwm_crypto::Id, ClientError> {
        Ok(self
            .store()
            .account()?
            .ok_or(ClientError::Locked)?
            .account_id)
    }

    /// The unlocked keyset sealed under a fresh key, to survive the service
    /// worker being stopped. Neither part may reach disk.
    pub fn export_session(&self) -> Result<SessionExport, ClientError> {
        let account_id = self.account_id_bytes()?;
        let (envelope, key) = self.engine_ref()?.keyset().export_session(&account_id)?;
        let mut blob = vec![SESSION_VERSION];
        blob.extend_from_slice(&account_id);
        blob.extend_from_slice(&envelope.to_bytes());
        Ok(SessionExport { blob, key })
    }

    /// Unlocks from an exported session and the sealed tokens, without the
    /// master password. Refused for another account's session.
    pub fn restore_session(
        &mut self,
        blob: &[u8],
        key: &[u8],
        sealed_tokens: Option<&[u8]>,
    ) -> Result<(), ClientError> {
        let account_id = self.account_id_bytes()?;
        let key: [u8; 32] = key.try_into().map_err(|_| ClientError::WrongPassword)?;
        match blob.split_first() {
            Some((&SESSION_VERSION, rest)) if rest.len() > 16 && rest[..16] == account_id => {
                let envelope =
                    Envelope::from_bytes(&rest[16..]).map_err(|_| ClientError::WrongPassword)?;
                let keyset =
                    AccountKeyset::import_session(&envelope, &Secret::new(key), &account_id)
                        .map_err(|_| ClientError::WrongPassword)?;
                let tokens = open_tokens(&keyset, &account_id, sealed_tokens)?;
                self.open_unlocked(keyset, tokens)
            }
            _ => Err(ClientError::WrongPassword),
        }
    }

    /// Seals tokens for disk under the account keyset: usable only after
    /// unlock (D34).
    pub fn seal_tokens(&self, access: &str, refresh: &str) -> Result<Vec<u8>, ClientError> {
        let account_id = self.account_id_bytes()?;
        let json = Secret::new(
            serde_json::to_vec(&TokensJson {
                access: access.to_owned(),
                refresh: refresh.to_owned(),
            })
            .map_err(|_| ClientError::Internal)?,
        );
        let envelope = self
            .engine_ref()?
            .keyset()
            .seal_device_secret(&account_id, json.expose_secret())?;
        let mut out = vec![TOKENS_VERSION];
        out.extend_from_slice(&envelope.to_bytes());
        Ok(out)
    }

    /// Unlocks with the master password and Secret Key, then opens the
    /// sealed tokens. Without tokens the vault opens, but syncing will ask
    /// to log in again.
    pub fn unlock_with_sealed_tokens(
        &mut self,
        password: &str,
        secret_key: &str,
        sealed_tokens: Option<&[u8]>,
    ) -> Result<(), ClientError> {
        let record = self.store().account()?.ok_or(ClientError::WrongPassword)?;
        let keyset = auth::unlock(&record, password, secret_key)?;
        let tokens = open_tokens(&keyset, &record.account_id, sealed_tokens)?;
        self.open_unlocked(keyset, tokens)
    }

    /// The current one-time code of a login, and the seconds it stays valid.
    pub fn current_code(
        &self,
        vault_id: &str,
        item_id: &str,
        now_ms: i64,
    ) -> Result<Option<(Secret<String>, u64)>, ClientError> {
        let vault = parse_id(vault_id)?;
        let item_id = item_id.parse().map_err(|_| ClientError::InvalidId)?;
        let Some(item) = self.engine_ref()?.item(&vault, &item_id)? else {
            return Err(ClientError::UnknownItem);
        };
        let ItemKind::Login(login) = &item.kind else {
            return Ok(None);
        };
        let Some(uri) = &login.totp else {
            return Ok(None);
        };
        let totp = Totp::parse(uri).map_err(|_| ClientError::InvalidItem)?;
        let code = totp.code(u64::try_from(now_ms / 1000).unwrap_or(0));
        Ok(Some((code.code, code.seconds_left)))
    }
}

pub(crate) fn open_tokens(
    keyset: &AccountKeyset,
    account_id: &pwm_crypto::Id,
    sealed: Option<&[u8]>,
) -> Result<Tokens, ClientError> {
    let Some(sealed) = sealed else {
        return Ok(Tokens::new(String::new(), String::new()));
    };
    let Some((&TOKENS_VERSION, rest)) = sealed.split_first() else {
        return Err(ClientError::Storage);
    };
    let envelope = Envelope::from_bytes(rest).map_err(|_| ClientError::Storage)?;
    let json = keyset
        .open_device_secret(&envelope, account_id)
        .map_err(|_| ClientError::Storage)?;
    let tokens: TokensJson =
        serde_json::from_slice(json.expose_secret()).map_err(|_| ClientError::Storage)?;
    Ok(Tokens::new(tokens.access, tokens.refresh))
}
