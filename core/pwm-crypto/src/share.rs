//! Ed25519 signatures and X25519 vault shares (decision D17,
//! `docs/SECURITY.md` "Sharing").

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::account::{key_from_plaintext, random_32};
use crate::{
    Aad, AccountKeyset, CryptoError, Envelope, Id, Secret, SymmetricKey, hkdf_sha256, open, seal,
};

const SHARE_VERSION: u8 = 1;
const PUBLIC_KEY_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;

/// What a signature is for. Each context has its own label, so a signature
/// made for one purpose never verifies for another. New purposes add a
/// variant with a new label; labels are never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SignatureContext {
    /// A vault share (`pwm/share-sig`).
    Share,
    /// The signed autofill rules file (`pwm/autofill-rules`, D33).
    AutofillRules,
}

impl SignatureContext {
    fn label(self) -> &'static str {
        match self {
            Self::Share => "pwm/share-sig",
            Self::AutofillRules => "pwm/autofill-rules",
        }
    }

    /// `len(label):u8 || label || message`.
    fn signed_bytes(self, message: &[u8]) -> Vec<u8> {
        let label = self.label();
        let len = u8::try_from(label.len()).expect("signature labels are short ASCII constants");
        let mut bytes = Vec::with_capacity(1 + label.len() + message.len());
        bytes.push(len);
        bytes.extend_from_slice(label.as_bytes());
        bytes.extend_from_slice(message);
        bytes
    }
}

/// Verifies an Ed25519 signature in strict mode. Small-order public keys are
/// rejected as [`CryptoError::InvalidPublicKey`].
pub fn verify_signature(
    public_key: &[u8; 32],
    context: SignatureContext,
    message: &[u8],
    signature: &[u8; 64],
) -> Result<(), CryptoError> {
    let key = VerifyingKey::from_bytes(public_key).map_err(|_| CryptoError::InvalidPublicKey)?;
    if key.is_weak() {
        return Err(CryptoError::InvalidPublicKey);
    }
    key.verify_strict(
        &context.signed_bytes(message),
        &Signature::from_bytes(signature),
    )
    .map_err(|_| CryptoError::SignatureInvalid)
}

/// The Ed25519 public key of a signing key given as its 32-byte seed. For
/// release signing keys (the autofill rules file, D33), which are not
/// account keys.
pub fn public_key_from_seed(seed: &Secret<[u8; 32]>) -> [u8; 32] {
    SigningKey::from_bytes(seed.expose_secret())
        .verifying_key()
        .to_bytes()
}

/// Signs `message` for `context` with the key whose seed is `seed`. The
/// signing tool for release keys runs this offline; apps only verify.
pub fn sign_with_seed(
    seed: &Secret<[u8; 32]>,
    context: SignatureContext,
    message: &[u8],
) -> [u8; 64] {
    SigningKey::from_bytes(seed.expose_secret())
        .sign(&context.signed_bytes(message))
        .to_bytes()
}

/// A vault key encrypted to one recipient and signed by the sender, in the v1
/// wire format `v || eph_pub || signature || envelope`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultShare {
    eph_pub: [u8; PUBLIC_KEY_LEN],
    signature: [u8; SIGNATURE_LEN],
    envelope: Envelope,
}

impl VaultShare {
    pub const VERSION: u8 = SHARE_VERSION;
    /// Length of a share around an empty envelope payload.
    pub const MIN_LEN: usize = 1 + PUBLIC_KEY_LEN + SIGNATURE_LEN + Envelope::MIN_LEN;

    pub fn to_bytes(&self) -> Vec<u8> {
        let envelope = self.envelope.to_bytes();
        let mut out = Vec::with_capacity(1 + PUBLIC_KEY_LEN + SIGNATURE_LEN + envelope.len());
        out.push(Self::VERSION);
        out.extend_from_slice(&self.eph_pub);
        out.extend_from_slice(&self.signature);
        out.extend_from_slice(&envelope);
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        let (&version, rest) = bytes.split_first().ok_or(CryptoError::Truncated)?;
        if version != Self::VERSION {
            return Err(CryptoError::UnsupportedVersion(version));
        }
        if bytes.len() < Self::MIN_LEN {
            return Err(CryptoError::Truncated);
        }
        let (eph_pub, rest) = rest.split_at(PUBLIC_KEY_LEN);
        let (signature, envelope) = rest.split_at(SIGNATURE_LEN);
        Ok(Self {
            eph_pub: eph_pub.try_into().expect("split at PUBLIC_KEY_LEN"),
            signature: signature.try_into().expect("split at SIGNATURE_LEN"),
            envelope: Envelope::from_bytes(envelope)?,
        })
    }
}

impl AccountKeyset {
    /// Signs `message` for `context` with this account's Ed25519 key.
    pub fn sign(&self, context: SignatureContext, message: &[u8]) -> [u8; 64] {
        self.ed25519.sign(&context.signed_bytes(message)).to_bytes()
    }

    /// Encrypts `vault_key` to `recipient_x25519_pub` and signs the share.
    pub fn share_vault_key(
        &self,
        vault_key: &SymmetricKey,
        vault_id: &Id,
        recipient_x25519_pub: &[u8; 32],
    ) -> Result<VaultShare, CryptoError> {
        let eph = StaticSecret::from(*random_32()?.expose_secret());
        let eph_pub = PublicKey::from(&eph).to_bytes();
        let share_key =
            derive_share_key(&eph, recipient_x25519_pub, &eph_pub, recipient_x25519_pub)?;
        let aad = Aad::share(
            SHARE_VERSION,
            vault_id,
            &self.ed25519_public(),
            recipient_x25519_pub,
        );
        let envelope = seal(&share_key, vault_key.expose_bytes(), &aad)?;
        let message = share_message(
            vault_id,
            recipient_x25519_pub,
            &eph_pub,
            &envelope.to_bytes(),
        );
        Ok(VaultShare {
            eph_pub,
            signature: self.sign(SignatureContext::Share, &message),
            envelope,
        })
    }

    /// Verifies a share addressed to this account and returns the vault key.
    /// The signature is checked before any key agreement or decryption.
    pub fn accept_vault_share(
        &self,
        share: &VaultShare,
        vault_id: &Id,
        sender_ed25519_pub: &[u8; 32],
    ) -> Result<SymmetricKey, CryptoError> {
        let own_pub = self.x25519_public();
        let message = share_message(
            vault_id,
            &own_pub,
            &share.eph_pub,
            &share.envelope.to_bytes(),
        );
        verify_signature(
            sender_ed25519_pub,
            SignatureContext::Share,
            &message,
            &share.signature,
        )?;
        let share_key = derive_share_key(&self.x25519, &share.eph_pub, &share.eph_pub, &own_pub)?;
        let aad = Aad::share(SHARE_VERSION, vault_id, sender_ed25519_pub, &own_pub);
        let plaintext = open(&share_key, &share.envelope, &aad)?;
        key_from_plaintext(plaintext.expose_secret())
    }
}

/// `share_key = HKDF(X25519(own_secret, peer_pub), salt = eph_pub || recipient_pub, info = "pwm/share/v1")`.
/// When sharing, `own_secret` is the ephemeral secret and the peer is the
/// recipient; when accepting, it is the recipient's secret and the peer is
/// the ephemeral key.
fn derive_share_key(
    own_secret: &StaticSecret,
    peer_pub: &[u8; 32],
    eph_pub: &[u8; 32],
    recipient_pub: &[u8; 32],
) -> Result<SymmetricKey, CryptoError> {
    let dh = own_secret.diffie_hellman(&PublicKey::from(*peer_pub));
    if !dh.was_contributory() {
        return Err(CryptoError::InvalidPublicKey);
    }
    let mut salt = [0u8; 64];
    salt[..32].copy_from_slice(eph_pub);
    salt[32..].copy_from_slice(recipient_pub);
    Ok(SymmetricKey::from_secret(hkdf_sha256(
        dh.as_bytes(),
        &salt,
        b"pwm/share/v1",
    )))
}

/// `version || vault_id || recipient_x25519_pub || eph_pub || envelope`.
fn share_message(
    vault_id: &Id,
    recipient_pub: &[u8; 32],
    eph_pub: &[u8; 32],
    envelope: &[u8],
) -> Vec<u8> {
    let mut msg = Vec::with_capacity(1 + 16 + 64 + envelope.len());
    msg.push(SHARE_VERSION);
    msg.extend_from_slice(vault_id);
    msg.extend_from_slice(recipient_pub);
    msg.extend_from_slice(eph_pub);
    msg.extend_from_slice(envelope);
    msg
}
