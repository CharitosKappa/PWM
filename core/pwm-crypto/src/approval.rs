//! Approving a new device from a signed-in one (decision D46): a fresh
//! X25519 key on each side, a commitment from the new device so the server
//! can't steer the code, a 6-digit code the user compares on both screens,
//! and the Secret Key sealed under the shared key.

use std::fmt;

use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::account::random_32;
use crate::{Aad, CryptoError, Envelope, Id, Secret, SymmetricKey, hkdf_sha256, open, seal};

const APPROVAL_VERSION: u8 = 1;
const COMMIT_LABEL: &[u8] = b"pwm/approval-commit";

/// One side's key pair for one approval; used once.
pub struct ApprovalKeys {
    secret: StaticSecret,
    public: [u8; 32],
}

impl fmt::Debug for ApprovalKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApprovalKeys([REDACTED])")
    }
}

impl ApprovalKeys {
    pub fn generate() -> Result<Self, CryptoError> {
        Ok(Self::from_seed(&random_32()?))
    }

    /// For test vectors: the secret is the seed (clamped by X25519).
    pub fn from_seed(seed: &Secret<[u8; 32]>) -> Self {
        let secret = StaticSecret::from(*seed.expose_secret());
        let public = PublicKey::from(&secret).to_bytes();
        Self { secret, public }
    }

    pub fn public(&self) -> [u8; 32] {
        self.public
    }

    /// What the new device sends first, instead of its key.
    pub fn commitment(&self) -> [u8; 32] {
        approval_commitment(&self.public)
    }

    /// The channel between `new_pub` (the new device) and `existing_pub`
    /// (the signed-in one). This side's key must be one of them; the other
    /// is the peer.
    pub fn channel(
        &self,
        approval_id: &Id,
        new_pub: &[u8; 32],
        existing_pub: &[u8; 32],
    ) -> Result<ApprovalChannel, CryptoError> {
        let peer = if self.public == *new_pub {
            existing_pub
        } else if self.public == *existing_pub {
            new_pub
        } else {
            return Err(CryptoError::InvalidMessage);
        };
        let dh = self.secret.diffie_hellman(&PublicKey::from(*peer));
        if !dh.was_contributory() {
            return Err(CryptoError::InvalidPublicKey);
        }
        let mut salt = [0u8; 80];
        salt[..16].copy_from_slice(approval_id);
        salt[16..48].copy_from_slice(new_pub);
        salt[48..].copy_from_slice(existing_pub);
        let key = SymmetricKey::from_secret(hkdf_sha256(dh.as_bytes(), &salt, b"pwm/approval/v1"));
        let code_bytes = hkdf_sha256(dh.as_bytes(), &salt, b"pwm/approval-code/v1");
        let first: [u8; 4] = code_bytes.expose_secret()[..4]
            .try_into()
            .expect("HKDF gives 32 bytes");
        Ok(ApprovalChannel {
            key,
            code: u32::from_be_bytes(first) % 1_000_000,
            aad: Aad::approval(APPROVAL_VERSION, approval_id, new_pub, existing_pub),
        })
    }
}

/// `SHA-256(len:u8 || "pwm/approval-commit" || n_pub)`.
pub fn approval_commitment(new_pub: &[u8; 32]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update([u8::try_from(COMMIT_LABEL.len()).expect("a short label")]);
    hash.update(COMMIT_LABEL);
    hash.update(new_pub);
    hash.finalize().into()
}

/// Checks the key the new device revealed against its commitment.
pub fn check_approval_commitment(
    commitment: &[u8; 32],
    new_pub: &[u8; 32],
) -> Result<(), CryptoError> {
    if approval_commitment(new_pub) == *commitment {
        Ok(())
    } else {
        Err(CryptoError::InvalidMessage)
    }
}

/// The shared channel of one approval: the code to compare and the key
/// that carries the Secret Key.
pub struct ApprovalChannel {
    key: SymmetricKey,
    code: u32,
    aad: Aad,
}

impl fmt::Debug for ApprovalChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApprovalChannel([REDACTED])")
    }
}

impl ApprovalChannel {
    /// 0 to 999,999; the same on both devices unless someone is in between.
    pub fn code(&self) -> u32 {
        self.code
    }

    /// The code as both screens show it: `"012 345"`.
    pub fn code_text(&self) -> String {
        let digits = format!("{:06}", self.code);
        format!("{} {}", &digits[..3], &digits[3..])
    }

    /// The signed-in device seals the Secret Key, after the user saw the
    /// codes match.
    pub fn seal(&self, secret: &[u8]) -> Result<Envelope, CryptoError> {
        seal(&self.key, secret, &self.aad)
    }

    pub fn open(&self, envelope: &Envelope) -> Result<Secret<Vec<u8>>, CryptoError> {
        open(&self.key, envelope, &self.aad)
    }
}
