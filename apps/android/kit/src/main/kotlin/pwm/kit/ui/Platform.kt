package pwm.kit.ui

import pwm.kit.PwmError
import pwm.kit.Redacted

/** Small settings that aren't secrets (D53 §5): the app's private preferences. */
interface AppPrefs {
    /** The chosen server's address; null for the hosted service. */
    var server: String?

    /**
     * Set at sign-up, cleared by "I saved it" (D53 §4): while set, the app
     * shows the Secret Key screen again after unlocking.
     */
    var kitPending: Boolean

    /** The lock timeout's minutes (D54 §5); null for the default. */
    var lockTimeoutMinutes: Int?

    /** The unlock screen offered fingerprint or face unlock once (D45 §3). */
    var biometricOffered: Boolean
}

class MemoryPrefs(
    override var server: String? = null,
    override var kitPending: Boolean = false,
    override var lockTimeoutMinutes: Int? = null,
    override var biometricOffered: Boolean = false,
) : AppPrefs

/** A copy that is hidden from the clipboard preview and cleared after a while. */
fun interface SecretClipboard {
    fun copy(value: Redacted<String>, clearAfterMs: Long)
}

/** What to show for a failure; never a value. */
internal fun message(error: Throwable): String = PwmError.from(error).userMessage
