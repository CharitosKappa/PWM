package pwm.kit

import uniffi.pwm_ffi.PwmException

/**
 * A value that must never be printed or logged (D52 §4): `toString` is
 * `<redacted>`. Kotlin strings can't be wiped, so secrets are held only as
 * long as needed and passed straight to the core.
 */
class Redacted<T>(val value: T) {
    override fun toString(): String = "<redacted>"

    override fun equals(other: Any?): Boolean = other is Redacted<*> && other.value == value

    override fun hashCode(): Int = value.hashCode()
}

/** An error with one of the core's stable codes, and its plain message. */
class PwmError(val code: String) : Exception("code=$code") {
    /** What to show the user; never a value. */
    val userMessage: String get() = Messages.byCode[code] ?: Messages.FALLBACK

    companion object {
        /** The core's error as a [PwmError]; anything else as `internal`. */
        fun from(error: Throwable): PwmError = when (error) {
            is PwmError -> error
            is PwmException.Failed -> PwmError(error.code)
            else -> PwmError("internal")
        }
    }
}

/** Runs a core call, turning its errors into [PwmError]. */
internal inline fun <T> core(call: () -> T): T =
    try {
        call()
    } catch (e: PwmException) {
        throw PwmError.from(e)
    }

/** What this device keeps outside the cache (D20 §2), by name. */
enum class KeyName(val key: String) {
    SECRET_KEY("secret-key"),
    ACCESS_TOKEN("access-token"),
    REFRESH_TOKEN("refresh-token"),
    DEVICE_ID("device-id"),

    /** The account [DEVICE_ID] belongs to, kept at sign-out: the id is reused only for that account. */
    DEVICE_ACCOUNT("device-account"),
    EMAIL("email"),

    /** Fingerprint and face unlock (D55 §4): the core's blob, the wrapped unlock key, and when the master password was last typed. */
    BIOMETRIC_BLOB("biometric-blob"),
    BIOMETRIC_KEY("biometric-key"),
    PASSWORD_UNLOCKED_AT("password-unlocked-at"),
}

/**
 * Secure storage for [KeyName] values: the Android Keystore in the app
 * (D52 §5), memory in tests. The core decides what they are.
 */
interface KeyStore {
    fun get(name: KeyName): String?

    fun set(name: KeyName, value: String)

    fun delete(name: KeyName)
}

/** Keeps values in memory only: tests and previews. */
class MemoryKeyStore : KeyStore {
    private val values = mutableMapOf<KeyName, String>()

    @Synchronized override fun get(name: KeyName): String? = values[name]

    @Synchronized override fun set(name: KeyName, value: String) {
        values[name] = value
    }

    @Synchronized override fun delete(name: KeyName) {
        values.remove(name)
    }
}
