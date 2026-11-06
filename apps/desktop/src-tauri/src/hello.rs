//! Windows Hello's key (D68): one `KeyCredential` per account on this
//! computer, kept by Windows (in the TPM where there is one), that signs
//! after a Windows Hello gesture: face, fingerprint or PIN. The signature
//! goes straight to the core, which derives the key that seals the unlock
//! key; nothing here keeps it.

use std::sync::Mutex;

use pwm_client::Secret;

/// Why Windows Hello didn't sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelloError {
    /// Not set up on this computer, or no key by that name any more.
    Unavailable,
    /// The user closed the prompt or chose the password.
    Cancelled,
    /// The key is gone or changed (a new PIN, a reset): turn it on again.
    Changed,
}

impl HelloError {
    /// The screens' error code.
    pub fn code(self) -> &'static str {
        match self {
            Self::Unavailable => "hello_unavailable",
            Self::Cancelled => "biometric_cancelled",
            Self::Changed => "hello_changed",
        }
    }
}

pub trait HelloKey: Send + Sync + 'static {
    /// Whether Windows Hello is set up for this user.
    fn supported(&self) -> bool;
    /// Makes (or replaces) the key named `name`; Windows prompts.
    fn create(&self, name: &str) -> Result<(), HelloError>;
    /// Signs `data` with the key named `name`; Windows prompts.
    fn sign(&self, name: &str, data: &[u8]) -> Result<Secret<Vec<u8>>, HelloError>;
    /// Deletes the key; nothing if there is none.
    fn delete(&self, name: &str);
}

/// No Windows Hello: Linux, until polkit or fprintd (D68 §7).
pub struct NoHello;

impl HelloKey for NoHello {
    fn supported(&self) -> bool {
        false
    }
    fn create(&self, _: &str) -> Result<(), HelloError> {
        Err(HelloError::Unavailable)
    }
    fn sign(&self, _: &str, _: &[u8]) -> Result<Secret<Vec<u8>>, HelloError> {
        Err(HelloError::Unavailable)
    }
    fn delete(&self, _: &str) {}
}

/// A stand-in for tests: signs deterministically with a "key" that changes
/// when it's made again, and can be cancelled, missing or nondeterministic.
#[derive(Default)]
pub struct FakeHello {
    pub state: Mutex<FakeState>,
}

#[derive(Default)]
pub struct FakeState {
    pub unsupported: bool,
    pub cancel_next: bool,
    /// Each signature differs, as a key that isn't deterministic would.
    pub nondeterministic: bool,
    /// The key's material, by name; a new one on each `create`.
    pub keys: std::collections::HashMap<String, u8>,
    pub prompts: u32,
    counter: u8,
}

impl HelloKey for FakeHello {
    fn supported(&self) -> bool {
        self.state.lock().is_ok_and(|s| !s.unsupported)
    }

    fn create(&self, name: &str) -> Result<(), HelloError> {
        let mut s = self.state.lock().map_err(|_| HelloError::Unavailable)?;
        if s.unsupported {
            return Err(HelloError::Unavailable);
        }
        s.prompts += 1;
        if std::mem::take(&mut s.cancel_next) {
            return Err(HelloError::Cancelled);
        }
        s.counter = s.counter.wrapping_add(1);
        let material = s.counter;
        s.keys.insert(name.to_owned(), material);
        Ok(())
    }

    fn sign(&self, name: &str, data: &[u8]) -> Result<Secret<Vec<u8>>, HelloError> {
        let mut s = self.state.lock().map_err(|_| HelloError::Unavailable)?;
        s.prompts += 1;
        if std::mem::take(&mut s.cancel_next) {
            return Err(HelloError::Cancelled);
        }
        let Some(&material) = s.keys.get(name) else {
            return Err(HelloError::Changed);
        };
        let salt = if s.nondeterministic {
            s.counter = s.counter.wrapping_add(1);
            s.counter
        } else {
            0
        };
        // 256 bytes, as an RSA-2048 signature.
        let mut out = vec![material ^ salt; 224];
        out.extend_from_slice(&data[..data.len().min(32)]);
        out.resize(256, 0);
        Ok(Secret::new(out))
    }

    fn delete(&self, name: &str) {
        if let Ok(mut s) = self.state.lock() {
            s.keys.remove(name);
        }
    }
}

#[cfg(windows)]
pub use windows_hello::WindowsHello;

#[cfg(windows)]
mod windows_hello {
    use pwm_client::Secret;
    use windows::Security::Credentials::{
        KeyCredentialCreationOption, KeyCredentialManager, KeyCredentialStatus,
    };
    use windows::Security::Cryptography::CryptographicBuffer;
    use windows::core::{Array, HSTRING};

    use super::{HelloError, HelloKey};

    /// `KeyCredentialManager`: WinRT calls, so no `unsafe` code here.
    pub struct WindowsHello;

    fn status_error(status: KeyCredentialStatus) -> HelloError {
        match status {
            KeyCredentialStatus::UserCanceled | KeyCredentialStatus::UserPrefersPassword => {
                HelloError::Cancelled
            }
            KeyCredentialStatus::NotFound => HelloError::Changed,
            _ => HelloError::Unavailable,
        }
    }

    impl HelloKey for WindowsHello {
        fn supported(&self) -> bool {
            KeyCredentialManager::IsSupportedAsync()
                .and_then(|op| op.join())
                .unwrap_or(false)
        }

        fn create(&self, name: &str) -> Result<(), HelloError> {
            let result = KeyCredentialManager::RequestCreateAsync(
                &HSTRING::from(name),
                KeyCredentialCreationOption::ReplaceExisting,
            )
            .and_then(|op| op.join())
            .map_err(|_| HelloError::Unavailable)?;
            match result.Status().map_err(|_| HelloError::Unavailable)? {
                KeyCredentialStatus::Success => Ok(()),
                other => Err(status_error(other)),
            }
        }

        fn sign(&self, name: &str, data: &[u8]) -> Result<Secret<Vec<u8>>, HelloError> {
            let opened = KeyCredentialManager::OpenAsync(&HSTRING::from(name))
                .and_then(|op| op.join())
                .map_err(|_| HelloError::Unavailable)?;
            match opened.Status().map_err(|_| HelloError::Unavailable)? {
                KeyCredentialStatus::Success => {}
                other => return Err(status_error(other)),
            }
            let credential = opened.Credential().map_err(|_| HelloError::Unavailable)?;
            let buffer = CryptographicBuffer::CreateFromByteArray(data)
                .map_err(|_| HelloError::Unavailable)?;
            let signed = credential
                .RequestSignAsync(&buffer)
                .and_then(|op| op.join())
                .map_err(|_| HelloError::Unavailable)?;
            match signed.Status().map_err(|_| HelloError::Unavailable)? {
                KeyCredentialStatus::Success => {}
                other => return Err(status_error(other)),
            }
            let result = signed.Result().map_err(|_| HelloError::Unavailable)?;
            let mut bytes = Array::<u8>::new();
            CryptographicBuffer::CopyToByteArray(&result, &mut bytes)
                .map_err(|_| HelloError::Unavailable)?;
            Ok(Secret::new(bytes.to_vec()))
        }

        fn delete(&self, name: &str) {
            let _ =
                KeyCredentialManager::DeleteAsync(&HSTRING::from(name)).and_then(|op| op.join());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real WinRT call answers, set up or not; it never prompts.
    #[cfg(windows)]
    #[test]
    fn windows_says_whether_hello_is_set_up() {
        let _ = WindowsHello.supported();
    }

    #[test]
    fn the_fake_signs_the_same_way_until_its_key_is_made_again() {
        let hello = FakeHello::default();
        hello.create("a").unwrap();
        let one = hello.sign("a", &[1; 32]).unwrap();
        assert_eq!(
            one.expose_secret(),
            hello.sign("a", &[1; 32]).unwrap().expose_secret()
        );
        hello.create("a").unwrap();
        assert_ne!(
            one.expose_secret(),
            hello.sign("a", &[1; 32]).unwrap().expose_secret()
        );
        hello.delete("a");
        assert_eq!(hello.sign("a", &[1; 32]).unwrap_err(), HelloError::Changed);
        assert!(!NoHello.supported());
    }
}
