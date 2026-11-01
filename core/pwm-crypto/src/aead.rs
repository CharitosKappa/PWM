//! XChaCha20-Poly1305 with the v1 envelope `v || nonce || ciphertext || tag`
//! (decision D10, `docs/SECURITY.md` "Encodings").

use core::fmt;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::{Aad, CryptoError, Secret};

const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;

/// A 256-bit symmetric key (sym_key, vault keys, AUK).
pub struct SymmetricKey(Secret<[u8; 32]>);

impl SymmetricKey {
    /// A new random key from the OS CSPRNG.
    pub fn generate() -> Result<Self, CryptoError> {
        let mut rng_ok = false;
        let key = Secret::<[u8; 32]>::new_with(|k| rng_ok = getrandom::fill(k).is_ok());
        if rng_ok {
            Ok(Self(key))
        } else {
            Err(CryptoError::Rng)
        }
    }

    pub fn from_secret(bytes: Secret<[u8; 32]>) -> Self {
        Self(bytes)
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new_from_slice(self.0.expose_secret()).expect("key is 32 bytes")
    }
}

impl fmt::Debug for SymmetricKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SymmetricKey({:?})", self.0)
    }
}

/// An encrypted value in the v1 wire format. The version byte is implied by
/// the type: decoding rejects anything but [`Envelope::VERSION`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    nonce: [u8; NONCE_LEN],
    /// Ciphertext followed by the 16-byte Poly1305 tag.
    ciphertext: Vec<u8>,
}

impl Envelope {
    pub const VERSION: u8 = 1;
    /// Length of an envelope with an empty plaintext.
    pub const MIN_LEN: usize = 1 + NONCE_LEN + TAG_LEN;

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + NONCE_LEN + self.ciphertext.len());
        out.push(Self::VERSION);
        out.extend_from_slice(&self.nonce);
        out.extend_from_slice(&self.ciphertext);
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
        let (nonce, ciphertext) = rest.split_at(NONCE_LEN);
        Ok(Self {
            nonce: nonce.try_into().expect("split at NONCE_LEN"),
            ciphertext: ciphertext.to_vec(),
        })
    }
}

/// Encrypts `plaintext` under `key` with a fresh random nonce, bound to `aad`.
pub fn seal(key: &SymmetricKey, plaintext: &[u8], aad: &Aad) -> Result<Envelope, CryptoError> {
    let mut nonce = [0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|_| CryptoError::Rng)?;
    seal_with_nonce(key, nonce, plaintext, aad.as_bytes())
}

/// Authenticates and decrypts `envelope`. Fails without revealing whether the
/// key, the AAD or the ciphertext was wrong.
pub fn open(
    key: &SymmetricKey,
    envelope: &Envelope,
    aad: &Aad,
) -> Result<Secret<Vec<u8>>, CryptoError> {
    let payload = Payload {
        msg: &envelope.ciphertext,
        aad: aad.as_bytes(),
    };
    key.cipher()
        .decrypt(&XNonce::from(envelope.nonce), payload)
        .map(Secret::new)
        .map_err(|_| CryptoError::DecryptionFailed)
}

/// Fixed-nonce sealing. Private: outside known-answer tests every nonce is
/// random.
fn seal_with_nonce(
    key: &SymmetricKey,
    nonce: [u8; NONCE_LEN],
    plaintext: &[u8],
    aad: &[u8],
) -> Result<Envelope, CryptoError> {
    let payload = Payload {
        msg: plaintext,
        aad,
    };
    let ciphertext = key
        .cipher()
        .encrypt(&XNonce::from(nonce), payload)
        .map_err(|_| CryptoError::EncryptionFailed)?;
    Ok(Envelope { nonce, ciphertext })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        hex::decode(s.split_whitespace().collect::<String>()).unwrap()
    }

    /// draft-irtf-cfrg-xchacha-03, Appendix A.3.1 (AEAD_XChaCha20_Poly1305).
    #[test]
    fn xchacha20poly1305_known_answer() {
        let key = SymmetricKey::from_secret(Secret::new(
            unhex("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f")
                .try_into()
                .unwrap(),
        ));
        let nonce: [u8; 24] = unhex("404142434445464748494a4b4c4d4e4f5051525354555657")
            .try_into()
            .unwrap();
        let aad = unhex("50515253c0c1c2c3c4c5c6c7");
        let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let expected = unhex(
            "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb
             731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b452
             2f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9
             21f9664c97637da9768812f615c68b13b52e
             c0875924c1c7987947deafd8780acf49",
        );

        let env = seal_with_nonce(&key, nonce, plaintext, &aad).unwrap();
        assert_eq!(env.ciphertext, expected);

        let opened = key
            .cipher()
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: &expected,
                    aad: &aad,
                },
            )
            .unwrap();
        assert_eq!(opened, plaintext);
    }
}
