package pwm.kit

/**
 * The platform's protection for the biometric unlock key (D45 §2, D55 §1):
 * an Android Keystore key in the app, a fake in the tests. The core seals
 * the keyset under the unlock key; this only wraps and unwraps that key.
 */
interface BiometricKeys {
    /** Whether fingerprint or face unlock can be used now. */
    fun availability(): BiometricAvailability

    /** Encrypts the unlock key to the Keystore key (made if needed). No prompt. */
    fun wrap(key: Redacted<ByteArray>): ByteArray

    /**
     * Asks for a finger or face, then unwraps. `biometric_cancelled` when the
     * user chose the master password or gave up; `biometric_changed` when a
     * new finger or face was enrolled since.
     */
    suspend fun unwrap(wrapped: ByteArray, purpose: BiometricPurpose): Redacted<ByteArray>

    /** Deletes the Keystore key. */
    fun delete()
}

/** Why the prompt shows, for its title. */
enum class BiometricPurpose { UNLOCK, TURN_ON }

sealed class BiometricAvailability {
    data object Available : BiometricAvailability()

    /** [code] is one of the shared messages, saying why. */
    data class Unavailable(val code: String) : BiometricAvailability()
}

/** Whether biometric unlock is on for this account. */
sealed class BiometricState {
    data class Unavailable(val code: String) : BiometricState()

    data object Off : BiometricState()

    data object On : BiometricState()
}

/** No biometrics: the JVM, and devices without them. */
object NoBiometrics : BiometricKeys {
    override fun availability(): BiometricAvailability = BiometricAvailability.Unavailable("biometric_unavailable")

    override fun wrap(key: Redacted<ByteArray>): ByteArray = throw PwmError("biometric_unavailable")

    override suspend fun unwrap(wrapped: ByteArray, purpose: BiometricPurpose): Redacted<ByteArray> =
        throw PwmError("biometric_unavailable")

    override fun delete() = Unit
}

object BiometricRules {
    /** The master password is asked again after this long (D45 §4). */
    const val MASTER_PASSWORD_EVERY_MS = 14L * 24 * 60 * 60 * 1000
}
