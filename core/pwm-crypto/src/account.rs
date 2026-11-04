//! Key hierarchy from `docs/SECURITY.md`: AUK and auth secret, the account
//! keyset, and vault key wrapping.

use core::fmt;

use ed25519_dalek::SigningKey;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroize;

use crate::{
    Aad, CryptoError, Envelope, Id, KdfParams, Secret, SecretKey, SymmetricKey,
    derive_password_key, generate_salt, hkdf_sha256, normalize_password, open, seal,
};

/// Keyset plaintext format: `version || sym_key || x25519_secret || ed25519_seed`.
const KEYSET_VERSION: u8 = 1;
const KEYSET_LEN: usize = 1 + 3 * 32;
/// Vault key plaintext format: the 32 raw key bytes.
const VAULT_KEY_VERSION: u8 = 1;
/// Session blob format (D34): the keyset plaintext under a session key.
const SESSION_VERSION: u8 = 1;
/// Biometric unlock format (D45): the keyset plaintext under a random key.
const BIOMETRIC_VERSION: u8 = 1;
const BROWSER_UNLOCK_VERSION: u8 = 1;
/// Device secret format (D34): the raw secret under `sym_key`.
const DEVICE_SECRET_VERSION: u8 = 1;
/// Device name plaintext format: UTF-8 padded to exactly 64 bytes (D26).
const DEVICE_NAME_VERSION: u8 = 1;
const DEVICE_NAME_LEN: usize = 64;
/// Every sealed device name is an envelope of exactly this many bytes.
pub const SEALED_DEVICE_NAME_LEN: usize = Envelope::MIN_LEN + DEVICE_NAME_LEN;

/// The two keys derived from master password + Secret Key.
pub struct AccountKeys {
    auk: SymmetricKey,
    auth_secret: Secret<[u8; 32]>,
}

impl AccountKeys {
    /// Account Unlock Key: decrypts the account keyset.
    pub fn auk(&self) -> &SymmetricKey {
        &self.auk
    }

    /// OPAQUE password input; independent of the AUK.
    pub fn auth_secret(&self) -> &Secret<[u8; 32]> {
        &self.auth_secret
    }
}

impl fmt::Debug for AccountKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountKeys([REDACTED])")
    }
}

/// Derives the AUK and auth secret (decision D13):
///
/// ```text
/// pw_key      = Argon2id(normalize(password), salt, params)
/// sk_key      = HKDF(secret_key, salt = account_id, info = "pwm/sk/v1")
/// AUK         = HKDF(pw_key XOR sk_key, salt = account_id, info = "pwm/auk/v1")
/// auth_secret = HKDF(pw_key XOR sk_key, salt = account_id, info = "pwm/auth/v1")
/// ```
pub fn derive_account_keys(
    password: &str,
    secret_key: &SecretKey,
    account_id: &Id,
    salt: &[u8; 16],
    params: &KdfParams,
) -> Result<AccountKeys, CryptoError> {
    let password = normalize_password(password)?;
    let pw_key = derive_password_key(password.expose_secret().as_bytes(), salt, params)?;
    let sk_key = hkdf_sha256(secret_key.expose_bytes(), account_id, b"pwm/sk/v1");
    let mixed = Secret::<[u8; 32]>::new_with(|m| {
        for (out, (p, s)) in m
            .iter_mut()
            .zip(pw_key.expose_secret().iter().zip(sk_key.expose_secret()))
        {
            *out = p ^ s;
        }
    });
    Ok(AccountKeys {
        auk: SymmetricKey::from_secret(hkdf_sha256(
            mixed.expose_secret(),
            account_id,
            b"pwm/auk/v1",
        )),
        auth_secret: hkdf_sha256(mixed.expose_secret(), account_id, b"pwm/auth/v1"),
    })
}

/// The account's long-term keys, stored on the server sealed under the AUK.
pub struct AccountKeyset {
    // Crate-visible for the sharing code in `share.rs`.
    sym_key: SymmetricKey,
    pub(crate) x25519: StaticSecret,
    pub(crate) ed25519: SigningKey,
}

impl AccountKeyset {
    /// Fresh random keys for a new account.
    pub fn generate() -> Result<Self, CryptoError> {
        let x25519_secret = random_32()?;
        let ed25519_seed = random_32()?;
        Ok(Self {
            sym_key: SymmetricKey::generate()?,
            x25519: StaticSecret::from(*x25519_secret.expose_secret()),
            ed25519: SigningKey::from_bytes(ed25519_seed.expose_secret()),
        })
    }

    /// Public key that other users encrypt shared vault keys to.
    pub fn x25519_public(&self) -> [u8; 32] {
        PublicKey::from(&self.x25519).to_bytes()
    }

    /// Public key that verifies this account's shares and device approvals.
    pub fn ed25519_public(&self) -> [u8; 32] {
        self.ed25519.verifying_key().to_bytes()
    }

    /// The keyset plaintext: `version || sym_key || x25519_secret || ed25519_seed`.
    fn plaintext(&self) -> Secret<Vec<u8>> {
        Secret::<Vec<u8>>::new_with(|buf| {
            buf.reserve_exact(KEYSET_LEN);
            buf.push(KEYSET_VERSION);
            buf.extend_from_slice(self.sym_key.expose_bytes());
            buf.extend_from_slice(self.x25519.as_bytes());
            let mut seed = self.ed25519.to_bytes();
            buf.extend_from_slice(&seed);
            seed.zeroize();
        })
    }

    /// Seals the keyset under the AUK, bound to `account_id`.
    pub fn seal(&self, auk: &SymmetricKey, account_id: &Id) -> Result<Envelope, CryptoError> {
        seal(
            auk,
            self.plaintext().expose_secret(),
            &Aad::keyset(KEYSET_VERSION, account_id),
        )
    }

    /// Opens a keyset sealed by [`seal`](Self::seal).
    pub fn open(
        envelope: &Envelope,
        auk: &SymmetricKey,
        account_id: &Id,
    ) -> Result<Self, CryptoError> {
        let plaintext = open(auk, envelope, &Aad::keyset(KEYSET_VERSION, account_id))?;
        Self::from_plaintext(plaintext.expose_secret())
    }

    /// The unlocked keyset for a memory-only session store (decision D34):
    /// sealed under a fresh random key that is returned with it, AAD
    /// `pwm/session`. Both stay in memory; neither may reach disk.
    pub fn export_session(
        &self,
        account_id: &Id,
    ) -> Result<(Envelope, Secret<[u8; 32]>), CryptoError> {
        let key_bytes = random_32()?;
        let key = SymmetricKey::from_secret(Secret::<[u8; 32]>::new_with(|k| {
            k.copy_from_slice(key_bytes.expose_secret());
        }));
        let envelope = seal(
            &key,
            self.plaintext().expose_secret(),
            &Aad::session(SESSION_VERSION, account_id),
        )?;
        Ok((envelope, key_bytes))
    }

    /// Restores a keyset from [`export_session`](Self::export_session).
    pub fn import_session(
        envelope: &Envelope,
        key: &Secret<[u8; 32]>,
        account_id: &Id,
    ) -> Result<Self, CryptoError> {
        let key = SymmetricKey::from_secret(Secret::<[u8; 32]>::new_with(|k| {
            k.copy_from_slice(key.expose_secret());
        }));
        let plaintext = open(&key, envelope, &Aad::session(SESSION_VERSION, account_id))?;
        Self::from_plaintext(plaintext.expose_secret())
    }

    /// The unlocked keyset for Face ID / Touch ID on one device (decision
    /// D45): sealed under a fresh random key that is returned with it, AAD
    /// `pwm/biometric-unlock` bound to the account and the device. The
    /// platform keeps the envelope and only a hardware-wrapped copy of the key.
    pub fn export_biometric_unlock(
        &self,
        account_id: &Id,
        device_id: &Id,
    ) -> Result<(Envelope, Secret<[u8; 32]>), CryptoError> {
        self.export_under_fresh_key(&Aad::biometric_unlock(
            BIOMETRIC_VERSION,
            account_id,
            device_id,
        ))
    }

    /// The browser extension's unlocked keyset for Touch ID through the Mac
    /// app (decision D48 §5): as [`export_biometric_unlock`](Self::export_biometric_unlock),
    /// under its own label `pwm/browser-unlock`. The extension keeps the
    /// envelope; the Mac's helper keeps the key behind Touch ID.
    pub fn export_browser_unlock(
        &self,
        account_id: &Id,
        device_id: &Id,
    ) -> Result<(Envelope, Secret<[u8; 32]>), CryptoError> {
        self.export_under_fresh_key(&Aad::browser_unlock(
            BROWSER_UNLOCK_VERSION,
            account_id,
            device_id,
        ))
    }

    /// Restores a keyset from [`export_browser_unlock`](Self::export_browser_unlock).
    pub fn import_browser_unlock(
        envelope: &Envelope,
        key: &Secret<[u8; 32]>,
        account_id: &Id,
        device_id: &Id,
    ) -> Result<Self, CryptoError> {
        Self::import_under_key(
            envelope,
            key,
            &Aad::browser_unlock(BROWSER_UNLOCK_VERSION, account_id, device_id),
        )
    }

    fn export_under_fresh_key(
        &self,
        aad: &Aad,
    ) -> Result<(Envelope, Secret<[u8; 32]>), CryptoError> {
        let key_bytes = random_32()?;
        let key = SymmetricKey::from_secret(Secret::<[u8; 32]>::new_with(|k| {
            k.copy_from_slice(key_bytes.expose_secret());
        }));
        let envelope = seal(&key, self.plaintext().expose_secret(), aad)?;
        Ok((envelope, key_bytes))
    }

    fn import_under_key(
        envelope: &Envelope,
        key: &Secret<[u8; 32]>,
        aad: &Aad,
    ) -> Result<Self, CryptoError> {
        let key = SymmetricKey::from_secret(Secret::<[u8; 32]>::new_with(|k| {
            k.copy_from_slice(key.expose_secret());
        }));
        let plaintext = open(&key, envelope, aad)?;
        Self::from_plaintext(plaintext.expose_secret())
    }

    /// Restores a keyset from [`export_biometric_unlock`](Self::export_biometric_unlock).
    pub fn import_biometric_unlock(
        envelope: &Envelope,
        key: &Secret<[u8; 32]>,
        account_id: &Id,
        device_id: &Id,
    ) -> Result<Self, CryptoError> {
        Self::import_under_key(
            envelope,
            key,
            &Aad::biometric_unlock(BIOMETRIC_VERSION, account_id, device_id),
        )
    }

    /// Seals a secret this device keeps on disk (the refresh token, D34)
    /// under `sym_key`, AAD `pwm/device-tokens`: usable only after unlock.
    pub fn seal_device_secret(
        &self,
        account_id: &Id,
        secret: &[u8],
    ) -> Result<Envelope, CryptoError> {
        seal(
            &self.sym_key,
            secret,
            &Aad::device_tokens(DEVICE_SECRET_VERSION, account_id),
        )
    }

    /// Opens a secret sealed by [`seal_device_secret`](Self::seal_device_secret).
    pub fn open_device_secret(
        &self,
        envelope: &Envelope,
        account_id: &Id,
    ) -> Result<Secret<Vec<u8>>, CryptoError> {
        open(
            &self.sym_key,
            envelope,
            &Aad::device_tokens(DEVICE_SECRET_VERSION, account_id),
        )
    }

    fn from_plaintext(bytes: &[u8]) -> Result<Self, CryptoError> {
        if bytes.len() != KEYSET_LEN {
            return Err(CryptoError::Truncated);
        }
        if bytes[0] != KEYSET_VERSION {
            return Err(CryptoError::UnsupportedVersion(bytes[0]));
        }
        let key_at = |offset: usize| {
            Secret::<[u8; 32]>::new_with(|k| k.copy_from_slice(&bytes[offset..offset + 32]))
        };
        let (sym, x25519, ed25519) = (key_at(1), key_at(33), key_at(65));
        Ok(Self {
            sym_key: SymmetricKey::from_secret(sym),
            x25519: StaticSecret::from(*x25519.expose_secret()),
            ed25519: SigningKey::from_bytes(ed25519.expose_secret()),
        })
    }

    /// Encrypts a vault key under `sym_key`, bound to `vault_id`.
    pub fn wrap_vault_key(
        &self,
        vault_key: &SymmetricKey,
        vault_id: &Id,
    ) -> Result<Envelope, CryptoError> {
        seal(
            &self.sym_key,
            vault_key.expose_bytes(),
            &Aad::vault_key(VAULT_KEY_VERSION, vault_id),
        )
    }

    /// Decrypts a vault key wrapped by [`wrap_vault_key`](Self::wrap_vault_key).
    pub fn unwrap_vault_key(
        &self,
        envelope: &Envelope,
        vault_id: &Id,
    ) -> Result<SymmetricKey, CryptoError> {
        let plaintext = open(
            &self.sym_key,
            envelope,
            &Aad::vault_key(VAULT_KEY_VERSION, vault_id),
        )?;
        key_from_plaintext(plaintext.expose_secret())
    }

    /// Seals a device name under `sym_key` (decision D26). The name (at most
    /// 63 bytes of UTF-8, no control characters) is padded to exactly 64
    /// bytes, so every sealed name has the same size.
    pub fn seal_device_name(
        &self,
        account_id: &Id,
        device_id: &Id,
        name: &str,
    ) -> Result<Envelope, CryptoError> {
        if name.len() >= DEVICE_NAME_LEN || name.chars().any(char::is_control) {
            return Err(CryptoError::DeviceNameRejected);
        }
        let mut padded = [0u8; DEVICE_NAME_LEN];
        padded[..name.len()].copy_from_slice(name.as_bytes());
        padded[name.len()] = 0x80;
        seal(
            &self.sym_key,
            &padded,
            &Aad::device_name(DEVICE_NAME_VERSION, account_id, device_id),
        )
    }

    /// Opens a device name sealed by [`seal_device_name`](Self::seal_device_name).
    pub fn open_device_name(
        &self,
        envelope: &Envelope,
        account_id: &Id,
        device_id: &Id,
    ) -> Result<String, CryptoError> {
        let plaintext = open(
            &self.sym_key,
            envelope,
            &Aad::device_name(DEVICE_NAME_VERSION, account_id, device_id),
        )?;
        let bytes = plaintext.expose_secret();
        if bytes.len() != DEVICE_NAME_LEN {
            return Err(CryptoError::Truncated);
        }
        let end = bytes
            .iter()
            .rposition(|&b| b != 0)
            .ok_or(CryptoError::Truncated)?;
        if bytes[end] != 0x80 {
            return Err(CryptoError::Truncated);
        }
        String::from_utf8(bytes[..end].to_vec()).map_err(|_| CryptoError::DeviceNameRejected)
    }
}

impl fmt::Debug for AccountKeyset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountKeyset([REDACTED])")
    }
}

/// Everything produced at signup. `secret_key` goes on the Emergency Kit;
/// `salt`, `params`, `sealed_keyset` and the OPAQUE registration from
/// `auth_secret` go to the server.
#[derive(Debug)]
pub struct NewAccount {
    pub secret_key: SecretKey,
    pub salt: [u8; 16],
    pub params: KdfParams,
    pub keyset: AccountKeyset,
    pub sealed_keyset: Envelope,
    pub auth_secret: Secret<[u8; 32]>,
}

/// Creates the keys for a new account.
pub fn create_account(
    password: &str,
    account_id: &Id,
    params: KdfParams,
) -> Result<NewAccount, CryptoError> {
    let secret_key = SecretKey::generate()?;
    let salt = generate_salt()?;
    let keys = derive_account_keys(password, &secret_key, account_id, &salt, &params)?;
    let keyset = AccountKeyset::generate()?;
    let sealed_keyset = keyset.seal(&keys.auk, account_id)?;
    Ok(NewAccount {
        secret_key,
        salt,
        params,
        keyset,
        sealed_keyset,
        auth_secret: keys.auth_secret,
    })
}

/// Unlocks an account: derives the AUK and opens the sealed keyset. A wrong
/// password, Secret Key, salt or account id all fail as
/// [`CryptoError::DecryptionFailed`].
pub fn unlock_account(
    password: &str,
    secret_key: &SecretKey,
    account_id: &Id,
    salt: &[u8; 16],
    params: &KdfParams,
    sealed_keyset: &Envelope,
) -> Result<AccountKeyset, CryptoError> {
    let keys = derive_account_keys(password, secret_key, account_id, salt, params)?;
    AccountKeyset::open(sealed_keyset, &keys.auk, account_id)
}

/// 32 random bytes from the OS RNG, for a new key seed (the offline rules
/// signing key, D34).
pub fn random_seed() -> Result<Secret<[u8; 32]>, CryptoError> {
    random_32()
}

pub(crate) fn random_32() -> Result<Secret<[u8; 32]>, CryptoError> {
    let mut ok = false;
    let bytes = Secret::<[u8; 32]>::new_with(|b| ok = getrandom::fill(b).is_ok());
    if ok { Ok(bytes) } else { Err(CryptoError::Rng) }
}

/// A [`SymmetricKey`] from a decrypted 32-byte key plaintext.
pub(crate) fn key_from_plaintext(bytes: &[u8]) -> Result<SymmetricKey, CryptoError> {
    if bytes.len() != 32 {
        return Err(CryptoError::Truncated);
    }
    Ok(SymmetricKey::from_secret(Secret::<[u8; 32]>::new_with(
        |k| k.copy_from_slice(bytes),
    )))
}
