@preconcurrency import PWMCore

/// An error from the core or this package: a stable code (D31 §7) and a
/// plain message for the UI. Neither ever contains a value.
public struct PWMError: Error, Sendable, Equatable, CustomStringConvertible {
    public let code: String

    public init(code: String) {
        self.code = code
    }

    init(_ error: PwmError) {
        switch error {
        case let .Failed(code): self.init(code: code)
        }
    }

    public var description: String { "PWMError(\(code))" }

    /// What to show the user.
    public var message: String {
        PWMError.messages[code] ?? "Something went wrong. Try again."
    }

    static let messages: [String: String] = [
        "wrong_password": "Wrong master password.",
        "invalid_credentials": "Wrong email, master password or Secret Key.",
        "invalid_secret_key": "That Secret Key is not valid. Check it on your Emergency Kit.",
        "weak_password": "Choose a stronger master password.",
        "network": "Can't reach the server. Check your connection.",
        "rate_limited": "Too many tries. Wait a minute and try again.",
        "account_exists": "There is already an account with this email.",
        "server": "The server had a problem. Try again later.",
        "device_revoked": "This device was signed out from another device.",
        "signed_out": "You were signed out. Sign in again.",
        "locked": "Unlock first.",
        "busy": "Still working on the last request.",
        "read_only_item": "This item was made by a newer version of the app. Update to edit it.",
        "item_too_large": "This item is too large to save.",
        "invalid_item": "This item could not be saved.",
        "invalid_totp": "That's not a one-time code setup key. Paste the key or scan the site's QR code.",
        "unknown_item": "This item was deleted.",
        "unknown_vault": "This device has no vault yet. Pull down to sync.",
        "invalid_expiry_month": "Enter the expiry month as a number from 1 to 12.",
        "invalid_expiry_year": "Enter the expiry year with 4 digits.",
        "no_characters": "Choose at least one kind of character.",
        "unknown_device": "That device was removed already.",
        "approval_ended": "The request ended. Ask again, or sign in with your Secret Key.",
        "unknown_approval": "That request ended. Ask the new device to try again.",
        "approval_failed": "The approval didn't arrive safely. Sign in with your Secret Key.",
        "biometric_cancelled": "Unlock with your master password, or try again.",
        "biometric_changed": "Face ID or Touch ID changed. Enter your master password to turn it back on.",
        "biometric_unavailable": "Face ID or Touch ID is not available on this device.",
        "biometric_unlock_failed": "Enter your master password.",
        "master_password_due": "Enter your master password. It's asked every 14 days so you don't forget it.",
        "invalid_device_name": "Use a shorter name, up to 63 characters.",
        "breach_check": "The leak check is not answering. Try again later.",
        "export_not_authorized": "Enter your master password to export.",
        "export_expired": "Enter your master password again to export.",
        "weak_export_password": "Choose a stronger export password.",
        "not_signed_in": "Sign in on this device first.",
        "storage": "The vault on this device could not be opened.",
        "keychain": "The keychain refused to store this device's keys.",
        "invalid_email": "Enter a valid email address.",
        "invalid_server": "Enter the server's address, starting with https://.",
        "not_a_kit": "That's not an Emergency Kit code.",
        "kit_too_new": "This Emergency Kit is from a newer version of the app. Update the app to scan it.",
        "camera": "Allow camera access in Settings to scan your Emergency Kit.",
    ]
}

/// Runs a core call, turning its error into `PWMError`.
func core<T>(_ call: () throws -> T) throws -> T {
    do {
        return try call()
    } catch let error as PwmError {
        throw PWMError(error)
    }
}
