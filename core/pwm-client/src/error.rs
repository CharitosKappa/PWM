use pwm_crypto::CryptoError;
use pwm_store::StoreError;
use pwm_sync::SyncError;
use pwm_vault::VaultError;
use thiserror::Error;

/// Client errors. Each has a stable [`code`](Self::code) that the bindings
/// pass on (D31). Messages never contain values.
#[derive(Debug, Error)]
pub enum ClientError {
    #[error("the vault is locked")]
    Locked,
    #[error("another operation is in progress")]
    Busy,
    #[error("no operation is waiting for a response")]
    NotRunning,
    #[error("an id is not a valid UUID")]
    InvalidId,
    #[error("not a valid page URL")]
    InvalidUrl,
    #[error("the autofill rules file was refused")]
    InvalidRules,
    #[error("unknown vault")]
    UnknownVault,
    #[error("no such item")]
    UnknownItem,
    #[error("the item is deleted")]
    ItemDeleted,
    #[error("the item is too large")]
    ItemTooLarge,
    #[error("the item is not valid")]
    InvalidItem,
    #[error("the item is read-only in this app version")]
    ReadOnlyItem,
    /// A one-time-code setup that is neither a supported URI nor a base32
    /// secret (D23).
    #[error("not a one-time-code secret")]
    InvalidTotp,
    /// A device name over 63 bytes or with control characters (D26).
    #[error("the device name is not valid")]
    InvalidDeviceName,
    /// Not one of the account's devices, or revoked already.
    #[error("unknown device")]
    UnknownDevice,
    /// Export needs the master password again first (D24).
    #[error("enter the master password to export")]
    ExportNotAuthorized,
    /// The master password was entered more than 5 minutes ago.
    #[error("enter the master password again to export")]
    ExportExpired,
    #[error("the export password is too easy to guess")]
    WeakExportPassword,
    #[error("the master password is too weak")]
    WeakPassword,
    #[error("the Secret Key is not valid")]
    InvalidSecretKey,
    /// Unlocking on this device: the password or Secret Key is wrong.
    #[error("wrong master password or Secret Key")]
    WrongPassword,
    /// Login: the server refused the credentials (the same for every cause).
    #[error("wrong email, master password or Secret Key")]
    InvalidCredentials,
    #[error("an account with this email exists")]
    AccountExists,
    #[error("too many requests; retry after {0} s")]
    RateLimited(u64),
    #[error("this device was revoked")]
    DeviceRevoked,
    #[error("signed out; log in again")]
    SignedOut,
    #[error("the server could not be reached")]
    Network,
    #[error("the server answered {0}")]
    Server(u16),
    #[error("the server's answer was not understood")]
    Protocol,
    #[error("local storage failed")]
    Storage,
    #[error("an internal error occurred")]
    Internal,
    /// A passkey request was refused (D37); the code says why.
    #[error("passkey: {0}")]
    Passkey(pwm_passkey::PasskeyError),
    /// The Emergency Kit or a scanned kit code was refused (D42); the code says why.
    #[error("emergency kit: {0}")]
    Kit(pwm_kit::KitError),
}

impl ClientError {
    /// A stable code for the bindings and the UI.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Locked => "locked",
            Self::Busy => "busy",
            Self::NotRunning => "not_running",
            Self::InvalidId => "invalid_id",
            Self::InvalidUrl => "invalid_url",
            Self::InvalidRules => "invalid_rules",
            Self::UnknownVault => "unknown_vault",
            Self::UnknownItem => "unknown_item",
            Self::ItemDeleted => "item_deleted",
            Self::ItemTooLarge => "item_too_large",
            Self::InvalidItem => "invalid_item",
            Self::ReadOnlyItem => "read_only_item",
            Self::InvalidTotp => "invalid_totp",
            Self::InvalidDeviceName => "invalid_device_name",
            Self::UnknownDevice => "unknown_device",
            Self::ExportNotAuthorized => "export_not_authorized",
            Self::ExportExpired => "export_expired",
            Self::WeakExportPassword => "weak_export_password",
            Self::WeakPassword => "weak_password",
            Self::InvalidSecretKey => "invalid_secret_key",
            Self::WrongPassword => "wrong_password",
            Self::InvalidCredentials => "invalid_credentials",
            Self::AccountExists => "account_exists",
            Self::RateLimited(_) => "rate_limited",
            Self::DeviceRevoked => "device_revoked",
            Self::SignedOut => "signed_out",
            Self::Network => "network",
            Self::Server(_) => "server",
            Self::Protocol => "protocol",
            Self::Storage => "storage",
            Self::Internal => "internal",
            Self::Passkey(e) => match e {
                pwm_passkey::PasskeyError::InvalidRequest => "invalid_passkey_request",
                pwm_passkey::PasskeyError::InsecureOrigin => "insecure_origin",
                pwm_passkey::PasskeyError::InvalidRpId => "invalid_rp_id",
                pwm_passkey::PasskeyError::UnsupportedAlgorithm => "unsupported_algorithm",
                pwm_passkey::PasskeyError::InvalidKey => "invalid_passkey",
            },
            Self::Kit(e) => match e {
                pwm_kit::KitError::InvalidEmail => "invalid_email",
                pwm_kit::KitError::InvalidServer => "invalid_server",
                pwm_kit::KitError::InvalidAccountId => "invalid_id",
                pwm_kit::KitError::InvalidDate => "invalid_date",
                pwm_kit::KitError::InvalidSecretKey => "invalid_secret_key",
                pwm_kit::KitError::NotAKit => "not_a_kit",
                pwm_kit::KitError::UnsupportedVersion => "kit_too_new",
                pwm_kit::KitError::Qr => "internal",
            },
        }
    }
}

impl From<VaultError> for ClientError {
    fn from(err: VaultError) -> Self {
        match err {
            VaultError::TooLarge => Self::ItemTooLarge,
            VaultError::ReadOnly => Self::ReadOnlyItem,
            VaultError::InvalidItem | VaultError::Invalid(_) | VaultError::Unsupported(_) => {
                Self::InvalidItem
            }
            VaultError::Crypto(_) => Self::Internal,
        }
    }
}

impl From<pwm_export::ExportError> for ClientError {
    fn from(err: pwm_export::ExportError) -> Self {
        match err {
            pwm_export::ExportError::GrantExpired => Self::ExportExpired,
            pwm_export::ExportError::WeakPassword => Self::WeakExportPassword,
            pwm_export::ExportError::Vault(e) => e.into(),
            _ => Self::Internal,
        }
    }
}

impl From<pwm_health::HealthError> for ClientError {
    fn from(err: pwm_health::HealthError) -> Self {
        match err {
            pwm_health::HealthError::BadResponse | pwm_health::HealthError::UnexpectedPrefix => {
                Self::Protocol
            }
            _ => Self::Internal,
        }
    }
}

impl From<StoreError> for ClientError {
    fn from(_: StoreError) -> Self {
        Self::Storage
    }
}

impl From<CryptoError> for ClientError {
    fn from(_: CryptoError) -> Self {
        Self::Internal
    }
}

impl From<SyncError> for ClientError {
    fn from(err: SyncError) -> Self {
        match err {
            SyncError::Busy => Self::Busy,
            SyncError::NotRunning => Self::NotRunning,
            SyncError::UnknownVault => Self::UnknownVault,
            SyncError::ItemDeleted => Self::ItemDeleted,
            SyncError::UnknownDevice => Self::UnknownDevice,
            SyncError::Refused(status) => Self::Server(status),
            SyncError::Store(e) => e.into(),
            SyncError::Vault(e) => e.into(),
            SyncError::Crypto(e) => e.into(),
        }
    }
}
