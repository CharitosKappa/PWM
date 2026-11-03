@preconcurrency import PWMCore

/// Checking a login's one-time-code setup before it is saved (D23 §3).
public enum OneTimeCode {
    /// What to store as `Login.totp` for what the user pasted or scanned: a
    /// URI as entered, or a bare secret as a URI named after `title`.
    /// Anything else throws `invalid_totp`.
    public static func normalize(_ input: String, title: String) throws -> String {
        try core { try normalizeTotp(input: input, title: title) }
    }
}
