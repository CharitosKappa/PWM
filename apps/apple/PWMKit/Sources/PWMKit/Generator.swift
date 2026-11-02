@preconcurrency import PWMCore

/// The core's generator and strength meter (D21), for the app's UI.
public enum Generator {
    public struct Generated: Sendable {
        public let value: Redacted<String>
        public let entropyBits: Double
    }

    public struct Strength: Sendable, Equatable {
        /// 0 (too guessable) to 4; sign-up needs at least 3.
        public let score: Int
        public let crackTime: String
        public let warning: String?
        public let suggestions: [String]
    }

    /// The D21 defaults: 20 characters, every class.
    public static func password(
        length: Int = 20,
        lowercase: Bool = true,
        uppercase: Bool = true,
        digits: Bool = true,
        symbols: Bool = true,
        avoidLookalikes: Bool = false
    ) throws -> Generated {
        guard let length = UInt32(exactly: length) else { throw PWMError(code: "invalid_item") }
        let generated = try core {
            try generatePassword(
                length: length, lowercase: lowercase, uppercase: uppercase,
                digits: digits, symbols: symbols, avoidLookalikes: avoidLookalikes
            )
        }
        return Generated(value: Redacted(generated.value), entropyBits: generated.entropyBits)
    }

    /// The D21 defaults: 5 words from the EFF list, separated by `-`.
    public static func passphrase(
        words: Int = 5,
        separator: String = "-",
        capitalize: Bool = false,
        addDigit: Bool = false
    ) throws -> Generated {
        guard let words = UInt32(exactly: words) else { throw PWMError(code: "invalid_item") }
        let generated = try core {
            try generatePassphrase(words: words, separator: separator, capitalize: capitalize, addDigit: addDigit)
        }
        return Generated(value: Redacted(generated.value), entropyBits: generated.entropyBits)
    }

    /// How guessable `password` is; `context` holds words the user is likely
    /// to reuse (email, name, item title).
    public static func strength(of password: Redacted<String>, context: [String] = []) -> Strength {
        let s = passwordStrength(password: password.value, context: context)
        return Strength(score: Int(s.score), crackTime: s.crackTime, warning: s.warning, suggestions: s.suggestions)
    }
}
