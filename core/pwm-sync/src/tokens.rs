//! Session tokens (D26) as the engine holds them (D28).

use core::fmt;

use pwm_crypto::Secret;

/// An access and refresh token pair. The platform keeps it in secure
/// storage; the engine hands back a new pair after each refresh.
pub struct Tokens {
    access: Secret<String>,
    refresh: Secret<String>,
}

impl Tokens {
    pub fn new(access: String, refresh: String) -> Self {
        Self {
            access: Secret::new(access),
            refresh: Secret::new(refresh),
        }
    }

    pub fn access(&self) -> &str {
        self.access.expose_secret()
    }

    pub fn refresh(&self) -> &str {
        self.refresh.expose_secret()
    }

    pub(crate) fn duplicate(&self) -> Self {
        Self::new(self.access().to_owned(), self.refresh().to_owned())
    }
}

impl fmt::Debug for Tokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Tokens([REDACTED])")
    }
}
