//! Deleting the account (D74): the master password proven to the server
//! with an OPAQUE login made only for it, then everything deleted there and
//! on this device. The Secret Key and password are used as at unlock and
//! never leave the core.

use pwm_crypto::{AccountKeys, Id, OpaqueLogin, SecretKey};
use pwm_store::Store;
use pwm_sync::Method;
use serde::Deserialize;
use serde_json::json;

use crate::auth::b64;
use crate::client::{Running, Step};
use crate::{Client, ClientError, Outcome};

/// Where a deletion is.
pub(crate) enum Deleting {
    /// Waiting for the server's credential response.
    Start {
        opaque: Box<OpaqueLogin>,
        keys: Box<AccountKeys>,
        account_id: Id,
    },
    /// Waiting for the deletion itself.
    Finish,
}

#[derive(Deserialize)]
struct StartJson {
    login_id: String,
    credential_response: String,
}

#[derive(Deserialize)]
struct ErrorJson {
    error: String,
}

impl<S: Store> Client<S> {
    /// Deletes the account and everything in it, on the server and on this
    /// device (D74). Needs the client unlocked and a connection.
    ///
    /// Ends with `AccountDeleted`: the cache is cleared and the client
    /// locked, and the app removes what it keeps itself (tokens, Secret Key)
    /// as on sign-out. A wrong password or Secret Key is `wrong_password`,
    /// found on this device. A subscription Stripe or Play couldn't end
    /// answers with a wait (the server's 503): nothing was deleted, try
    /// again later.
    pub fn start_delete_account(
        &mut self,
        password: &str,
        secret_key: &str,
    ) -> Result<Step, ClientError> {
        self.idle()?;
        let record = self.account_record()?;
        let secret_key = SecretKey::parse(secret_key).map_err(|_| ClientError::InvalidSecretKey)?;
        let keys = pwm_crypto::derive_account_keys(
            password,
            &secret_key,
            &record.account_id,
            &record.salt,
            &record.kdf_params,
        )?;
        let (opaque, request) = OpaqueLogin::start(keys.auth_secret())?;
        let body = json!({ "credential_request": b64(&request) });
        let step = self.engine()?.start_call(
            Method::Post,
            "/v1/account/delete/start".to_owned(),
            Some(body.to_string().into_bytes()),
        )?;
        self.running = Running::DeleteAccount(Box::new(Deleting::Start {
            opaque: Box::new(opaque),
            keys: Box::new(keys),
            account_id: record.account_id,
        }));
        self.map_engine_step(step)
    }

    pub(crate) fn deletion_answer(
        &mut self,
        deleting: Deleting,
        status: u16,
        body: &[u8],
    ) -> Result<Step, ClientError> {
        match (deleting, status) {
            (
                Deleting::Start {
                    opaque,
                    keys,
                    account_id,
                },
                200,
            ) => {
                let answer: StartJson =
                    serde_json::from_slice(body).map_err(|_| ClientError::Protocol)?;
                let credential = crate::auth::unb64(&answer.credential_response)?;
                // A wrong password or Secret Key fails here, on the device.
                let finalization = opaque
                    .finish(keys.auth_secret(), &account_id, &credential)
                    .map_err(|_| ClientError::WrongPassword)?;
                let body = json!({
                    "login_id": answer.login_id,
                    "credential_finalization": b64(&finalization),
                });
                let step = self.engine()?.start_call(
                    Method::Post,
                    "/v1/account/delete/finish".to_owned(),
                    Some(body.to_string().into_bytes()),
                )?;
                self.running = Running::DeleteAccount(Box::new(Deleting::Finish));
                self.map_engine_step(step)
            }
            (Deleting::Finish, 204) => {
                self.sign_out()?;
                Ok(Step::Done(Outcome::AccountDeleted))
            }
            (_, status) => Err(deletion_error(status, body)),
        }
    }
}

fn deletion_error(status: u16, body: &[u8]) -> ClientError {
    let code = serde_json::from_slice::<ErrorJson>(body)
        .ok()
        .map(|e| e.error);
    match (status, code.as_deref()) {
        // The proof expired or was used up: as for a wrong password.
        (403, Some("invalid_credentials")) => ClientError::WrongPassword,
        (status, _) => ClientError::Server(status),
    }
}
