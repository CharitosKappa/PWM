//! Argon2id password KDF with per-account params (decisions D12, D15).

use argon2::{Algorithm, Argon2, ParamsBuilder, Version};

use crate::{CryptoError, Secret};

/// Argon2id (v1.3) cost parameters, stored per account so they can be raised
/// later without breaking existing accounts. Every client must use the stored
/// values exactly, or it derives a different key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl KdfParams {
    /// Signup default: 64 MiB, t = 3, p = 4.
    pub const DEFAULT: Self = Self {
        memory_kib: 64 * 1024,
        iterations: 3,
        parallelism: 4,
    };

    /// Weakest params ever used, only for signups on devices that cannot
    /// allocate 64 MiB: 32 MiB, t = 4, p = 4.
    pub const MOBILE_FLOOR: Self = Self {
        memory_kib: 32 * 1024,
        iterations: 4,
        parallelism: 4,
    };

    const MAX_MEMORY_KIB: u32 = 1024 * 1024;
    const MIN_ITERATIONS: u32 = 3;
    const MAX_ITERATIONS: u32 = 16;
    const MAX_PARALLELISM: u32 = 16;

    /// Rejects params weaker than [`MOBILE_FLOOR`](Self::MOBILE_FLOOR) (so a
    /// server cannot downgrade the KDF) and params too costly to run (so it
    /// cannot exhaust a client's memory).
    pub fn validate(&self) -> Result<(), CryptoError> {
        let floor = Self::MOBILE_FLOOR;
        let work = u64::from(self.memory_kib) * u64::from(self.iterations);
        let floor_work = u64::from(floor.memory_kib) * u64::from(floor.iterations);
        let ok = (floor.memory_kib..=Self::MAX_MEMORY_KIB).contains(&self.memory_kib)
            && (Self::MIN_ITERATIONS..=Self::MAX_ITERATIONS).contains(&self.iterations)
            && (1..=Self::MAX_PARALLELISM).contains(&self.parallelism)
            && work >= floor_work;
        if ok {
            Ok(())
        } else {
            Err(CryptoError::KdfParamsRejected)
        }
    }
}

/// A new random 16-byte password salt (generated once at signup).
pub fn generate_salt() -> Result<[u8; 16], CryptoError> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|_| CryptoError::Rng)?;
    Ok(salt)
}

/// `pw_key = Argon2id(password, salt_pw, params)`, 32 bytes.
pub fn derive_password_key(
    password: &[u8],
    salt: &[u8; 16],
    params: &KdfParams,
) -> Result<Secret<[u8; 32]>, CryptoError> {
    params.validate()?;
    let argon2_params = ParamsBuilder::new()
        .m_cost(params.memory_kib)
        .t_cost(params.iterations)
        .p_cost(params.parallelism)
        .output_len(32)
        .build()
        .map_err(|_| CryptoError::KdfParamsRejected)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, argon2_params);

    let mut failed = false;
    let key = Secret::<[u8; 32]>::new_with(|out| {
        failed = argon2.hash_password_into(password, salt, out).is_err();
    });
    if failed {
        Err(CryptoError::Kdf)
    } else {
        Ok(key)
    }
}

#[cfg(test)]
mod tests {
    use argon2::AssociatedData;

    use super::*;

    /// RFC 9106 section 5.3 (Argon2id, v0x13). Checks the primitive and
    /// version we configure; the RFC vector also uses a secret and AD, which
    /// our KDF does not.
    #[test]
    fn argon2id_rfc9106_known_answer() {
        let params = ParamsBuilder::new()
            .m_cost(32)
            .t_cost(3)
            .p_cost(4)
            .output_len(32)
            .data(AssociatedData::new(&[0x04; 12]).unwrap())
            .build()
            .unwrap();
        let argon2 =
            Argon2::new_with_secret(&[0x03; 8], Algorithm::Argon2id, Version::V0x13, params)
                .unwrap();
        let mut tag = [0u8; 32];
        argon2
            .hash_password_into(&[0x01; 32], &[0x02; 16], &mut tag)
            .unwrap();
        assert_eq!(
            hex::encode(tag),
            "0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659"
        );
    }
}
