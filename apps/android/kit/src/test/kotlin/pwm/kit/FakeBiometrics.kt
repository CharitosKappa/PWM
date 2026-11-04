package pwm.kit

/**
 * A key store with no hardware (D55 §7): wrapping is a reversible change of
 * the bytes, and the prompt does what the test says.
 */
class FakeBiometrics : BiometricKeys {
    var available: BiometricAvailability = BiometricAvailability.Available
    /** What the next prompt does: nothing (a match), or throw. */
    var prompt: () -> Unit = {}
    var prompts = 0
    var deleted = 0

    override fun availability(): BiometricAvailability = available

    override fun wrap(key: Redacted<ByteArray>): ByteArray = key.value.map { (it.toInt() xor 0x5A).toByte() }.toByteArray()

    override suspend fun unwrap(wrapped: ByteArray, purpose: BiometricPurpose): Redacted<ByteArray> {
        prompts += 1
        prompt()
        return Redacted(wrapped.map { (it.toInt() xor 0x5A).toByte() }.toByteArray())
    }

    override fun delete() {
        deleted += 1
    }
}
