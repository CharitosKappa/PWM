//! Windows Hello unlock (M7.5, D68) under D45's rules: a Settings switch and
//! one offer, the master password every 14 days and after signing in, the
//! blob made again after each Windows Hello unlock, and everything deleted
//! when it's turned off or the account signs out. Locking keeps it.

use base64::Engine;
use pwm_client::{Secret, Tokens};
use serde_json::Value;

use crate::backend::{Backend, Clock, Failure};
use crate::clipboard::Clipboard;
use crate::hello::HelloError;
use crate::http::Transport;
use crate::keys::{KeyStore, name};

type Result<T> = std::result::Result<T, Failure>;

/// The master password is asked again after this long (D45 §4).
pub const PASSWORD_EVERY_MS: i64 = 14 * 24 * 60 * 60 * 1000;

impl From<HelloError> for Failure {
    fn from(e: HelloError) -> Self {
        Self::code(e.code())
    }
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

impl<T: Transport, K: KeyStore, C: Clipboard, Q: Clock> Backend<T, K, C, Q> {
    /// The key credential's name: one per account on this computer.
    fn hello_name(&self) -> Result<String> {
        let account = self
            .client
            .account_id()?
            .ok_or_else(|| Failure::code("signed_out"))?;
        Ok(format!("PWM {account}"))
    }

    async fn device(&mut self) -> Result<String> {
        Ok(self
            .keys
            .get(name::DEVICE_ID)
            .await?
            .ok_or_else(|| Failure::code("signed_out"))?
            .expose_secret()
            .clone())
    }

    /// Whether Windows Hello unlock is on for this account.
    pub(crate) async fn hello_on(&mut self) -> Result<bool> {
        Ok(self.keys.get(name::HELLO_BLOB).await?.is_some())
    }

    async fn keep_hello(&mut self, unlock: &pwm_client::WindowsHelloUnlock) -> Result<()> {
        self.keys
            .set(name::HELLO_BLOB, &b64().encode(&unlock.blob))
            .await?;
        self.keys
            .set(name::HELLO_SEALED_KEY, &b64().encode(&unlock.sealed_key))
            .await?;
        Ok(())
    }

    /// Turns it off: the key credential and everything kept for it go.
    pub(crate) async fn forget_hello(&mut self) -> Result<()> {
        if let Ok(name) = self.hello_name() {
            self.hello.delete(&name);
        }
        for key in [
            name::HELLO_BLOB,
            name::HELLO_SEALED_KEY,
            name::HELLO_CHALLENGE,
        ] {
            self.keys.remove(key).await?;
        }
        Ok(())
    }

    /// The master password was typed: the 14 days start again.
    pub(crate) async fn password_typed(&mut self) -> Result<()> {
        self.keys
            .set(name::PASSWORD_AT, &self.clock.now_ms().to_string())
            .await?;
        Ok(())
    }

    /// Turns Windows Hello unlock on (needs the vault unlocked) or off.
    /// Turning it on makes the key, then signs a new challenge twice: the
    /// signatures must match, or Windows Hello can't protect the key here.
    pub(crate) async fn set_hello(&mut self, on: bool) -> Result<bool> {
        if !on {
            self.forget_hello().await?;
            return Ok(false);
        }
        if !self.client.is_unlocked() {
            return Err(Failure::code("locked"));
        }
        if !self.hello.supported() {
            return Err(Failure::code("hello_unavailable"));
        }
        let name = self.hello_name()?;
        let device = self.device().await?;
        self.hello.create(&name)?;
        let challenge = pwm_client::windows_hello_challenge()?;
        let first = self.hello.sign(&name, challenge.expose_secret())?;
        let second = self.hello.sign(&name, challenge.expose_secret())?;
        if first.expose_secret() != second.expose_secret() {
            self.hello.delete(&name);
            return Err(Failure::code("hello_unavailable"));
        }
        let unlock = self.client.export_windows_hello(
            &device,
            challenge.expose_secret(),
            first.expose_secret(),
        )?;
        self.keys
            .set(
                name::HELLO_CHALLENGE,
                &b64().encode(challenge.expose_secret()),
            )
            .await?;
        self.keep_hello(&unlock).await?;
        self.keys.set(name::HELLO_OFFERED, "1").await?;
        Ok(true)
    }

    /// Unlocks with Windows Hello. The master password every 14 days; a key
    /// that changed turns it off and asks for the password.
    pub(crate) async fn unlock_hello(&mut self, tokens: Tokens) -> Result<()> {
        let stored = |v: Option<Secret<String>>| -> Result<Vec<u8>> {
            let v = v.ok_or_else(|| Failure::code("hello_off"))?;
            b64()
                .decode(v.expose_secret())
                .map_err(|_| Failure::code("storage"))
        };
        let blob = stored(self.keys.get(name::HELLO_BLOB).await?)?;
        let sealed = stored(self.keys.get(name::HELLO_SEALED_KEY).await?)?;
        let challenge = stored(self.keys.get(name::HELLO_CHALLENGE).await?)?;
        let typed_at = self
            .keys
            .get(name::PASSWORD_AT)
            .await?
            .and_then(|v| v.expose_secret().parse::<i64>().ok())
            .unwrap_or(0);
        if self.clock.now_ms().saturating_sub(typed_at) >= PASSWORD_EVERY_MS {
            return Err(Failure::code("master_password_due"));
        }
        let name = self.hello_name()?;
        let device = self.device().await?;
        let signature = match self.hello.sign(&name, &challenge) {
            Ok(signature) => signature,
            Err(HelloError::Changed) => {
                self.forget_hello().await?;
                return Err(Failure::code("hello_changed"));
            }
            Err(other) => return Err(other.into()),
        };
        match self.client.unlock_windows_hello(
            &blob,
            &sealed,
            &challenge,
            signature.expose_secret(),
            &device,
            tokens,
        ) {
            Ok(fresh) => self.keep_hello(&fresh).await,
            Err(_) => {
                // Another key, or a keyset that changed: the password, then on again.
                self.forget_hello().await?;
                Err(Failure::code("hello_changed"))
            }
        }
    }

    /// The one offer after a master password unlock (D45 §3): Windows Hello
    /// is set up, not on, and wasn't offered before.
    pub(crate) async fn hello_offer(&mut self) -> Result<bool> {
        Ok(self.client.is_unlocked()
            && self.hello.supported()
            && !self.hello_on().await?
            && self.keys.get(name::HELLO_OFFERED).await?.is_none())
    }

    pub(crate) async fn dismiss_hello_offer(&mut self) -> Result<()> {
        self.keys.set(name::HELLO_OFFERED, "1").await?;
        Ok(())
    }

    /// The tokens kept in a secure store (D66 §4), for an unlock.
    pub(crate) async fn stored_tokens(&mut self) -> Result<Tokens> {
        Ok(match self.keys.get(name::TOKENS).await? {
            Some(pair) => {
                let pair: Value = serde_json::from_str(pair.expose_secret())
                    .map_err(|_| Failure::code("storage"))?;
                Tokens::new(
                    pair["access"].as_str().unwrap_or("").to_owned(),
                    pair["refresh"].as_str().unwrap_or("").to_owned(),
                )
            }
            None => Tokens::new(String::new(), String::new()),
        })
    }
}
