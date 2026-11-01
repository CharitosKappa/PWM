//! The 128-bit Secret Key and its printed form (decision D14).
//!
//! `A1-XXXXXX-XXXXXX-XXXXXX-XXXXXX-XXX`: format version, 26 Crockford base32
//! characters (most significant bit first, top two bits zero) and one
//! Crockford mod-37 check character.

use core::fmt;

use zeroize::Zeroize;

use crate::{CryptoError, Secret};

const PREFIX: &str = "A1";
const DATA_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const CHECK_ALPHABET: &[u8; 37] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ*~$=U";
const DATA_CHARS: usize = 26;
const GROUP: usize = 6;

/// The account Secret Key, shown once on the Emergency Kit.
pub struct SecretKey(Secret<[u8; 16]>);

impl SecretKey {
    /// A new random Secret Key from the OS CSPRNG.
    pub fn generate() -> Result<Self, CryptoError> {
        let mut rng_ok = false;
        let key = Secret::<[u8; 16]>::new_with(|k| rng_ok = getrandom::fill(k).is_ok());
        if rng_ok {
            Ok(Self(key))
        } else {
            Err(CryptoError::Rng)
        }
    }

    pub fn from_bytes(bytes: Secret<[u8; 16]>) -> Self {
        Self(bytes)
    }

    #[allow(dead_code)] // used by the key hierarchy (M1.4)
    pub(crate) fn expose_bytes(&self) -> &[u8; 16] {
        self.0.expose_secret()
    }

    /// The printed form, e.g. `A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA`.
    pub fn to_text(&self) -> Secret<String> {
        let mut n = u128::from_be_bytes(*self.0.expose_secret());
        let text = Secret::<String>::new_with(|s| {
            s.reserve_exact(PREFIX.len() + DATA_CHARS + 1 + 5);
            s.push_str(PREFIX);
            for i in 0..DATA_CHARS {
                if i % GROUP == 0 {
                    s.push('-');
                }
                let shift = 5 * (DATA_CHARS - 1 - i);
                let digit = ((n >> shift) & 0x1F) as usize;
                s.push(char::from(DATA_ALPHABET[digit]));
            }
            s.push(char::from(CHECK_ALPHABET[(n % 37) as usize]));
        });
        n.zeroize();
        text
    }

    /// Parses the printed form. Case-insensitive; ignores `-` and whitespace;
    /// reads `O` as `0` and `I`/`L` as `1`; the `A1` prefix may be omitted.
    pub fn parse(input: &str) -> Result<Self, CryptoError> {
        let compact = Secret::<Vec<u8>>::new_with(|buf| {
            buf.reserve_exact(input.len());
            for c in input.chars().filter(|c| *c != '-' && !c.is_whitespace()) {
                let c = match c.to_ascii_uppercase() {
                    'O' => '0',
                    'I' | 'L' => '1',
                    other => other,
                };
                // Non-ASCII becomes a byte no alphabet contains.
                buf.push(u8::try_from(c).unwrap_or(0));
            }
        });
        let body = match compact.expose_secret().as_slice() {
            b if b.len() == DATA_CHARS + 1 => b,
            b if b.len() == PREFIX.len() + DATA_CHARS + 1 && b.starts_with(PREFIX.as_bytes()) => {
                &b[PREFIX.len()..]
            }
            _ => return Err(CryptoError::SecretKeyFormat),
        };
        let (data, check) = body.split_at(DATA_CHARS);

        let mut n: u128 = 0;
        let mut result = Ok(());
        for (i, &c) in data.iter().enumerate() {
            match DATA_ALPHABET.iter().position(|&a| a == c) {
                // The first character carries only 3 of the 128 bits.
                Some(v) if i > 0 || v < 8 => n = (n << 5) | v as u128,
                _ => {
                    result = Err(CryptoError::SecretKeyFormat);
                    break;
                }
            }
        }
        if result.is_ok() {
            result = match CHECK_ALPHABET.iter().position(|&a| a == check[0]) {
                Some(v) if v as u128 == n % 37 => Ok(()),
                Some(_) => Err(CryptoError::SecretKeyChecksum),
                None => Err(CryptoError::SecretKeyFormat),
            };
        }
        let key = result.map(|()| Self(Secret::new(n.to_be_bytes())));
        n.zeroize();
        key
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretKey({:?})", self.0)
    }
}
