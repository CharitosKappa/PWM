//! OPAQUE (RFC 9807) server login (decision D26). Both protocol sides live
//! here so the server calls the core and implements no crypto itself.
//!
//! Suite: OPRF ristretto255-SHA512, 3DH on ristretto255 with SHA-512, no
//! key stretching (the input `auth_secret` is already Argon2id-stretched and
//! mixed with the Secret Key). Context `"pwm/opaque/v1"`; the client and
//! credential identifiers are the `account_id`.

use core::fmt;

use hmac::{Hmac, KeyInit, Mac};
use opaque_ke::ksf::Identity;
use opaque_ke::{
    CipherSuite, ClientLogin, ClientLoginFinishParameters, ClientRegistration,
    ClientRegistrationFinishParameters, CredentialFinalization, CredentialRequest,
    CredentialResponse, Identifiers, RegistrationRequest, RegistrationResponse, RegistrationUpload,
    Ristretto255, ServerLogin, ServerLoginParameters, ServerRegistration, ServerSetup, TripleDh,
};
use rand_core::{CryptoRng, OsRng, RngCore};
use sha2::Sha256;
use zeroize::Zeroize;

use crate::{Aad, CryptoError, Envelope, Id, Secret, SymmetricKey, hkdf_sha256, open, seal};

struct Suite;

impl CipherSuite for Suite {
    type OprfCs = Ristretto255;
    type KeyExchange = TripleDh<Ristretto255, sha2_010::Sha512>;
    type Ksf = Identity;
}

const CONTEXT: &[u8] = b"pwm/opaque/v1";
/// Server secrets blob: `v:u8 || ServerSetup || server_key:32`.
const SECRETS_VERSION: u8 = 1;
const SERVER_KEY_LEN: usize = 32;
/// Sealed login state format version (AAD `pwm/login-state`).
const LOGIN_STATE_VERSION: u8 = 1;

fn identifiers(account_id: &Id) -> Identifiers<'_> {
    Identifiers {
        client: Some(account_id),
        server: None,
    }
}

fn server_parameters(account_id: &Id) -> ServerLoginParameters<'static, '_> {
    ServerLoginParameters {
        context: Some(CONTEXT),
        identifiers: identifiers(account_id),
    }
}

/// The server's long-term secrets: the OPAQUE setup (OPRF seed and key
/// pair) and a 32-byte `server_key`, from which the pre-login key and the
/// login-state key are derived. Losing the setup locks every account out, so
/// the serialized form belongs in a secret manager.
pub struct ServerSecrets {
    setup: ServerSetup<Suite>,
    server_key: Secret<[u8; SERVER_KEY_LEN]>,
    /// `HKDF(server_key, info = "pwm/prelogin/v1")`: fake pre-login answers.
    prelogin_key: Secret<[u8; 32]>,
    /// `HKDF(server_key, info = "pwm/login-state/v1")`: seals the login state
    /// kept between login start and finish.
    state_key: SymmetricKey,
}

impl ServerSecrets {
    pub fn generate() -> Result<Self, CryptoError> {
        Self::generate_with_rng(&mut OsRng)
    }

    /// [`generate`](Self::generate) with a caller-chosen RNG, for test vectors.
    #[doc(hidden)]
    pub fn generate_with_rng<R: RngCore + CryptoRng>(rng: &mut R) -> Result<Self, CryptoError> {
        let setup = ServerSetup::<Suite>::new(rng);
        let mut ok = true;
        let server_key = Secret::<[u8; SERVER_KEY_LEN]>::new_with(|k| {
            ok = rng.try_fill_bytes(k).is_ok();
        });
        if !ok {
            return Err(CryptoError::Rng);
        }
        Ok(Self::new(setup, server_key))
    }

    fn new(setup: ServerSetup<Suite>, server_key: Secret<[u8; SERVER_KEY_LEN]>) -> Self {
        let key = server_key.expose_secret();
        Self {
            setup,
            prelogin_key: hkdf_sha256(key, b"", b"pwm/prelogin/v1"),
            state_key: SymmetricKey::from_secret(hkdf_sha256(key, b"", b"pwm/login-state/v1")),
            server_key,
        }
    }

    /// `v:u8 (= 1) || ServerSetup || server_key:32`.
    pub fn to_bytes(&self) -> Secret<Vec<u8>> {
        Secret::<Vec<u8>>::new_with(|buf| {
            let mut setup = self.setup.serialize();
            buf.reserve_exact(1 + setup.len() + SERVER_KEY_LEN);
            buf.push(SECRETS_VERSION);
            buf.extend_from_slice(&setup);
            buf.extend_from_slice(self.server_key.expose_secret());
            setup.as_mut_slice().zeroize();
        })
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        let (&version, rest) = bytes.split_first().ok_or(CryptoError::Truncated)?;
        if version != SECRETS_VERSION {
            return Err(CryptoError::UnsupportedVersion(version));
        }
        let split = rest
            .len()
            .checked_sub(SERVER_KEY_LEN)
            .ok_or(CryptoError::Truncated)?;
        let (setup, key) = rest.split_at(split);
        let setup = ServerSetup::<Suite>::deserialize(setup).map_err(|_| CryptoError::Truncated)?;
        let server_key = Secret::<[u8; SERVER_KEY_LEN]>::new_with(|k| k.copy_from_slice(key));
        Ok(Self::new(setup, server_key))
    }

    /// The fake `(account_id, salt)` returned for an unknown email (D12):
    /// `HMAC-SHA256(prelogin_key, len(label) || label || email)` with labels
    /// `pwm/prelogin/id` and `pwm/prelogin/salt`, the id shaped as UUIDv4.
    /// `email` must already be normalized.
    pub fn prelogin_fake(&self, email: &str) -> (Id, [u8; 16]) {
        let mut id = self.prelogin_mac("pwm/prelogin/id", email);
        id[6] = (id[6] & 0x0F) | 0x40;
        id[8] = (id[8] & 0x3F) | 0x80;
        (id, self.prelogin_mac("pwm/prelogin/salt", email))
    }

    fn prelogin_mac(&self, label: &str, email: &str) -> [u8; 16] {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.prelogin_key.expose_secret())
            .expect("HMAC accepts any key length");
        mac.update(&[u8::try_from(label.len()).expect("short label")]);
        mac.update(label.as_bytes());
        mac.update(email.as_bytes());
        let tag = mac.finalize().into_bytes();
        let mut out = [0u8; 16];
        out.copy_from_slice(&tag[..16]);
        out
    }

    /// Answers a client's registration request. Stores nothing.
    pub fn register_respond(
        &self,
        account_id: &Id,
        request: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let request = RegistrationRequest::<Suite>::deserialize(request)
            .map_err(|_| CryptoError::InvalidMessage)?;
        let result = ServerRegistration::<Suite>::start(&self.setup, request, account_id)
            .map_err(|_| CryptoError::InvalidMessage)?;
        Ok(result.message.serialize().to_vec())
    }

    /// Answers a login request. `record` is the stored registration record,
    /// or `None` for an unknown account, which gets a fake response of the
    /// same shape. Returns the sealed server state to keep until
    /// [`login_finish`](Self::login_finish), and the response for the client.
    ///
    /// The plain state holds the MAC the client must send, so it is sealed
    /// under the login-state key (AAD `pwm/login-state`, bound to the
    /// account): someone who can read the stored state still cannot finish
    /// the login.
    pub fn login_respond(
        &self,
        record: Option<&[u8]>,
        account_id: &Id,
        request: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        self.login_respond_with_rng(&mut OsRng, record, account_id, request)
    }

    /// [`login_respond`](Self::login_respond) with a caller-chosen RNG, for test vectors.
    #[doc(hidden)]
    pub fn login_respond_with_rng<R: RngCore + CryptoRng>(
        &self,
        rng: &mut R,
        record: Option<&[u8]>,
        account_id: &Id,
        request: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
        let record = record
            .map(ServerRegistration::<Suite>::deserialize)
            .transpose()
            .map_err(|_| CryptoError::InvalidMessage)?;
        let request = CredentialRequest::<Suite>::deserialize(request)
            .map_err(|_| CryptoError::InvalidMessage)?;
        let result = ServerLogin::start(
            rng,
            &self.setup,
            record,
            request,
            account_id,
            server_parameters(account_id),
        )
        .map_err(|_| CryptoError::InvalidMessage)?;
        let mut state = result.state.serialize();
        let sealed = seal(
            &self.state_key,
            &state,
            &Aad::login_state(LOGIN_STATE_VERSION, account_id),
        );
        state.as_mut_slice().zeroize();
        Ok((sealed?.to_bytes(), result.message.serialize().to_vec()))
    }

    /// Completes a login on the server with the sealed state from
    /// [`login_respond`](Self::login_respond). Fails with
    /// [`CryptoError::AuthenticationFailed`] for a wrong `auth_secret`, an
    /// unknown account, a different `account_id` or a state that was not
    /// sealed by this server.
    pub fn login_finish(
        &self,
        sealed_state: &[u8],
        finalization: &[u8],
        account_id: &Id,
    ) -> Result<(), CryptoError> {
        let sealed = Envelope::from_bytes(sealed_state).map_err(|_| CryptoError::InvalidMessage)?;
        let state = open(
            &self.state_key,
            &sealed,
            &Aad::login_state(LOGIN_STATE_VERSION, account_id),
        )
        .map_err(|_| CryptoError::AuthenticationFailed)?;
        let state = ServerLogin::<Suite>::deserialize(state.expose_secret())
            .map_err(|_| CryptoError::InvalidMessage)?;
        let finalization = CredentialFinalization::<Suite>::deserialize(finalization)
            .map_err(|_| CryptoError::InvalidMessage)?;
        state
            .finish(finalization, server_parameters(account_id))
            .map_err(|_| CryptoError::AuthenticationFailed)?;
        Ok(())
    }
}

impl fmt::Debug for ServerSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ServerSecrets([REDACTED])")
    }
}

/// Checks a client's registration upload and returns the record to store.
pub fn opaque_record(upload: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let upload = RegistrationUpload::<Suite>::deserialize(upload)
        .map_err(|_| CryptoError::InvalidMessage)?;
    Ok(ServerRegistration::finish(upload).serialize().to_vec())
}

/// Client side of registration, started from the account's `auth_secret`.
pub struct OpaqueRegistration(ClientRegistration<Suite>);

impl OpaqueRegistration {
    /// Returns the client state and the request for the server.
    pub fn start(auth_secret: &Secret<[u8; 32]>) -> Result<(Self, Vec<u8>), CryptoError> {
        Self::start_with_rng(&mut OsRng, auth_secret)
    }

    #[doc(hidden)]
    pub fn start_with_rng<R: RngCore + CryptoRng>(
        rng: &mut R,
        auth_secret: &Secret<[u8; 32]>,
    ) -> Result<(Self, Vec<u8>), CryptoError> {
        let result = ClientRegistration::<Suite>::start(rng, auth_secret.expose_secret())
            .map_err(|_| CryptoError::InvalidMessage)?;
        Ok((Self(result.state), result.message.serialize().to_vec()))
    }

    /// Returns the upload that completes registration.
    pub fn finish(
        self,
        auth_secret: &Secret<[u8; 32]>,
        account_id: &Id,
        response: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.finish_with_rng(&mut OsRng, auth_secret, account_id, response)
    }

    #[doc(hidden)]
    pub fn finish_with_rng<R: RngCore + CryptoRng>(
        self,
        rng: &mut R,
        auth_secret: &Secret<[u8; 32]>,
        account_id: &Id,
        response: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let response = RegistrationResponse::<Suite>::deserialize(response)
            .map_err(|_| CryptoError::InvalidMessage)?;
        let result = self
            .0
            .finish(
                rng,
                auth_secret.expose_secret(),
                response,
                ClientRegistrationFinishParameters::new(identifiers(account_id), None),
            )
            .map_err(|_| CryptoError::InvalidMessage)?;
        Ok(result.message.serialize().to_vec())
    }
}

impl fmt::Debug for OpaqueRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OpaqueRegistration([REDACTED])")
    }
}

/// Client side of login, started from the account's `auth_secret`.
pub struct OpaqueLogin(ClientLogin<Suite>);

impl OpaqueLogin {
    /// Returns the client state and the request for the server.
    pub fn start(auth_secret: &Secret<[u8; 32]>) -> Result<(Self, Vec<u8>), CryptoError> {
        Self::start_with_rng(&mut OsRng, auth_secret)
    }

    #[doc(hidden)]
    pub fn start_with_rng<R: RngCore + CryptoRng>(
        rng: &mut R,
        auth_secret: &Secret<[u8; 32]>,
    ) -> Result<(Self, Vec<u8>), CryptoError> {
        let result = ClientLogin::<Suite>::start(rng, auth_secret.expose_secret())
            .map_err(|_| CryptoError::InvalidMessage)?;
        Ok((Self(result.state), result.message.serialize().to_vec()))
    }

    /// Returns the finalization message for the server. Fails with
    /// [`CryptoError::AuthenticationFailed`] when the response does not
    /// match this `auth_secret` (wrong password or Secret Key, unknown
    /// account) or does not come from the server the account registered with.
    pub fn finish(
        self,
        auth_secret: &Secret<[u8; 32]>,
        account_id: &Id,
        response: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        self.finish_with_rng(&mut OsRng, auth_secret, account_id, response)
    }

    #[doc(hidden)]
    pub fn finish_with_rng<R: RngCore + CryptoRng>(
        self,
        rng: &mut R,
        auth_secret: &Secret<[u8; 32]>,
        account_id: &Id,
        response: &[u8],
    ) -> Result<Vec<u8>, CryptoError> {
        let response = CredentialResponse::<Suite>::deserialize(response)
            .map_err(|_| CryptoError::InvalidMessage)?;
        let result = self
            .0
            .finish(
                rng,
                auth_secret.expose_secret(),
                response,
                ClientLoginFinishParameters {
                    context: Some(CONTEXT),
                    identifiers: identifiers(account_id),
                    ksf: None,
                },
            )
            .map_err(|_| CryptoError::AuthenticationFailed)?;
        Ok(result.message.serialize().to_vec())
    }
}

impl fmt::Debug for OpaqueLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OpaqueLogin([REDACTED])")
    }
}
