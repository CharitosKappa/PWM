package pwm.kit

import uniffi.pwm_ffi.generatePassphrase
import uniffi.pwm_ffi.generatePassword
import uniffi.pwm_ffi.normalizeTotp

/** The core's generator (D21), for the generator tab and the password field. */
object Generator {
    /** The D21 defaults: 20 characters, every class. */
    fun password(
        length: Int = 20,
        lowercase: Boolean = true,
        uppercase: Boolean = true,
        digits: Boolean = true,
        symbols: Boolean = true,
        avoidLookalikes: Boolean = false,
    ): Redacted<String> {
        if (!(lowercase || uppercase || digits || symbols)) throw PwmError("no_characters")
        require(length > 0) { "length" }
        val made = core { generatePassword(length.toUInt(), lowercase, uppercase, digits, symbols, avoidLookalikes) }
        return Redacted(made.value)
    }

    /** The D21 defaults: 5 words from the EFF list, separated by `-`. */
    fun passphrase(words: Int = 5, separator: String = "-", capitalize: Boolean = false, addDigit: Boolean = false): Redacted<String> {
        require(words > 0) { "words" }
        return Redacted(core { generatePassphrase(words.toUInt(), separator, capitalize, addDigit) }.value)
    }
}

/** Checking a login's one-time-code setup before it is saved (D23 §3). */
object OneTimeCode {
    /**
     * What to store as the login's `totp` for what the user pasted or
     * scanned: a URI as entered, or a bare secret as a URI named after
     * [title]. Anything else is `invalid_totp`.
     */
    fun normalize(input: String, title: String): String = core { normalizeTotp(input, title) }
}

/** A login's current one-time code and how long it lasts. */
class CurrentCode(val code: Redacted<String>, val secondsLeft: Long) {
    override fun toString(): String = "CurrentCode(<redacted>)"
}
