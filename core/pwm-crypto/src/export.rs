//! Password-protected export file `.pwmx` v1 (decision D24, `docs/SECURITY.md`
//! "Export"): `"PWMX" || v:u8 || m:u32 || t:u32 || p:u32 || salt:16 || envelope`.

use crate::{
    Aad, CryptoError, Envelope, KdfParams, Secret, SymmetricKey, derive_password_key,
    generate_salt, hkdf_sha256, normalize_password, open, seal,
};

/// First bytes of every `.pwmx` file.
pub const EXPORT_MAGIC: [u8; 4] = *b"PWMX";

const EXPORT_INFO: &[u8] = b"pwm/export/v1";

/// The cleartext header of a `.pwmx` file. It is bound to the envelope by the
/// AAD, so changing any header byte makes decryption fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportHeader {
    pub params: KdfParams,
    pub salt: [u8; 16],
}

impl ExportHeader {
    pub const VERSION: u8 = 1;
    /// Magic, version, three params and the salt.
    pub const LEN: usize = 4 + 1 + 12 + 16;

    /// Splits a file into header and envelope. Other files, other versions
    /// and params outside the account KDF bounds are rejected here, before
    /// any key derivation.
    pub fn parse(file: &[u8]) -> Result<(Self, Envelope), CryptoError> {
        if file.get(..4) != Some(EXPORT_MAGIC.as_slice()) {
            return Err(CryptoError::NotExportFile);
        }
        let version = *file.get(4).ok_or(CryptoError::Truncated)?;
        if version != Self::VERSION {
            return Err(CryptoError::UnsupportedVersion(version));
        }
        if file.len() < Self::LEN {
            return Err(CryptoError::Truncated);
        }
        let u32_at =
            |i: usize| u32::from_be_bytes(file[i..i + 4].try_into().expect("4-byte slice"));
        let params = KdfParams {
            memory_kib: u32_at(5),
            iterations: u32_at(9),
            parallelism: u32_at(13),
        };
        params.validate()?;
        let header = Self {
            params,
            salt: file[17..Self::LEN].try_into().expect("16-byte slice"),
        };
        Ok((header, Envelope::from_bytes(&file[Self::LEN..])?))
    }

    fn to_bytes(self) -> [u8; Self::LEN] {
        let mut out = [0u8; Self::LEN];
        out[..4].copy_from_slice(&EXPORT_MAGIC);
        out[4] = Self::VERSION;
        out[5..9].copy_from_slice(&self.params.memory_kib.to_be_bytes());
        out[9..13].copy_from_slice(&self.params.iterations.to_be_bytes());
        out[13..17].copy_from_slice(&self.params.parallelism.to_be_bytes());
        out[17..].copy_from_slice(&self.salt);
        out
    }

    fn aad(&self) -> Aad {
        Aad::export(Self::VERSION, &self.params, &self.salt)
    }

    /// `HKDF(ikm = Argon2id(normalized password, salt, params), salt, "pwm/export/v1")`.
    fn key(&self, password: &str) -> Result<SymmetricKey, CryptoError> {
        let normalized = normalize_password(password)?;
        let pw_key = derive_password_key(
            normalized.expose_secret().as_bytes(),
            &self.salt,
            &self.params,
        )?;
        Ok(SymmetricKey::from_secret(hkdf_sha256(
            pw_key.expose_secret(),
            &self.salt,
            EXPORT_INFO,
        )))
    }
}

/// Encrypts `plaintext` into a `.pwmx` file under `password` with a fresh
/// random salt. Checking the password's strength is the caller's job.
pub fn seal_export(
    password: &str,
    params: KdfParams,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    params.validate()?;
    let header = ExportHeader {
        params,
        salt: generate_salt()?,
    };
    let envelope = seal(&header.key(password)?, plaintext, &header.aad())?;
    let mut file = header.to_bytes().to_vec();
    file.extend_from_slice(&envelope.to_bytes());
    Ok(file)
}

/// Decrypts a `.pwmx` file. A wrong password and a damaged file both fail as
/// [`CryptoError::DecryptionFailed`].
pub fn open_export(password: &str, file: &[u8]) -> Result<Secret<Vec<u8>>, CryptoError> {
    let (header, envelope) = ExportHeader::parse(file)?;
    open(&header.key(password)?, &envelope, &header.aad())
}
