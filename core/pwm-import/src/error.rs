use pwm_crypto::CryptoError;
use thiserror::Error;

/// Import errors. Messages never contain file content (D51 §7).
#[derive(Debug, Error)]
pub enum ImportError {
    /// Over [`MAX_FILE_BYTES`](crate::MAX_FILE_BYTES), too many items, or a
    /// ZIP that unpacks to too much.
    #[error("the file is too large to import")]
    TooLarge,
    /// Not the chosen format, or damaged.
    #[error("the file can't be read as this format")]
    Unreadable,
    /// An encrypted export and no password given.
    #[error("the export's password is needed")]
    PasswordNeeded,
    #[error("wrong password for this export")]
    WrongPassword,
    /// A variant we don't open, such as Bitwarden's account-restricted or
    /// Proton's PGP-encrypted export: the reason says what to export instead.
    #[error("this export can't be imported: {0}")]
    Unsupported(&'static str),
    #[error(transparent)]
    Crypto(#[from] CryptoError),
}

impl ImportError {
    /// A stable code for the bindings and the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Self::TooLarge => "import_too_large",
            Self::Unreadable => "import_unreadable",
            Self::PasswordNeeded => "import_password_needed",
            Self::WrongPassword => "import_wrong_password",
            Self::Unsupported(reason) => reason,
            Self::Crypto(_) => "internal",
        }
    }
}

impl From<pwm_vault::VaultError> for ImportError {
    fn from(err: pwm_vault::VaultError) -> Self {
        match err {
            pwm_vault::VaultError::Crypto(e) => Self::Crypto(e),
            _ => Self::Unreadable,
        }
    }
}
