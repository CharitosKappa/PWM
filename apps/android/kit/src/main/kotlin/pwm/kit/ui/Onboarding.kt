package pwm.kit.ui

import java.net.URL
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import pwm.kit.Account
import pwm.kit.BiometricState
import pwm.kit.EmergencyKit
import pwm.kit.PasswordStrength
import pwm.kit.PwmError
import pwm.kit.Redacted

// The onboarding screens' state (D53 §1-2), as on Apple (D42 §4). Each model
// runs its work in the app's scope, so turning the phone doesn't stop it. The
// master password and Secret Key stay in these states only while their
// screen shows, as `Redacted` so they never print, and are cleared once sent.

/** Create account: email, master password twice, the strength meter. */
class CreateAccountModel internal constructor(
    private val scope: CoroutineScope,
    private val work: CoroutineDispatcher,
    private val deviceName: String,
    private val account: () -> Account,
    private val onCreated: (Account, Redacted<String>) -> Unit,
    private val strengthOf: (Redacted<String>, List<String>) -> PasswordStrength = PasswordStrength::of,
) {
    data class State(
        val email: String = "",
        val password: Redacted<String> = Redacted(""),
        val confirm: Redacted<String> = Redacted(""),
        val strength: PasswordStrength? = null,
        val busy: Boolean = false,
        val error: String? = null,
    ) {
        val passwordsMatch: Boolean get() = confirm.value.isNotEmpty() && password.value == confirm.value
        val strongEnough: Boolean get() = (strength?.score ?: 0) >= MINIMUM_SCORE
        val canSubmit: Boolean get() = !busy && email.trim().contains('@') && strongEnough && passwordsMatch
    }

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable
    private var scoring: Job? = null

    fun setEmail(email: String) {
        mutable.update { it.copy(email = email) }
        rescore()
    }

    fun setPassword(password: String) {
        mutable.update { it.copy(password = Redacted(password)) }
        rescore()
    }

    fun setConfirm(confirm: String) = mutable.update { it.copy(confirm = Redacted(confirm)) }

    /** Scores off the main thread; a newer change makes an older score moot. */
    private fun rescore() {
        scoring?.cancel()
        val password = mutable.value.password
        if (password.value.isEmpty()) {
            mutable.update { it.copy(strength = null) }
            return
        }
        val context = listOf(mutable.value.email.trim())
        scoring = scope.launch {
            val scored = withContext(work) { strengthOf(password, context) }
            mutable.update { if (it.password == password) it.copy(strength = scored) else it }
        }
    }

    fun submit() {
        val now = mutable.value
        if (!now.canSubmit) return
        mutable.update { it.copy(busy = true, error = null) }
        scope.launch {
            try {
                val account = account()
                val key = account.signUp(now.email.trim(), now.password, deviceName)
                mutable.update { it.copy(password = Redacted(""), confirm = Redacted(""), busy = false) }
                onCreated(account, key)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(busy = false, error = message(e)) }
            }
        }
    }

    companion object {
        /** The same bar as the other apps: "strong" or better. */
        const val MINIMUM_SCORE = 3
    }
}

/** The Secret Key, shown once, and the Emergency Kit. */
class SecretKeyModel internal constructor(
    private val scope: CoroutineScope,
    val secretKey: Redacted<String>,
    private val account: Account,
    private val clipboard: SecretClipboard,
    private val paper: EmergencyKit.Paper,
    private val onDone: () -> Unit,
) {
    data class State(
        /** The kit PDF, kept only while this screen shows. */
        val kit: Redacted<ByteArray>? = null,
        val saved: Boolean = false,
        val copied: Boolean = false,
        val error: String? = null,
    ) {
        /** "I saved it" needs a saved or printed kit, or a copied key. */
        val canContinue: Boolean get() = saved || copied
    }

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable

    fun prepareKit() {
        scope.launch {
            try {
                val kit = account.emergencyKit(paper)
                mutable.update { it.copy(kit = kit) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(error = message(e)) }
            }
        }
    }

    /** The kit was written to the file the user picked, or sent to print. */
    fun didSaveKit() = mutable.update { it.copy(saved = true, error = null) }

    fun saveFailed() = mutable.update { it.copy(error = message(pwm.kit.PwmError("kit_save_failed"))) }

    fun copy() {
        clipboard.copy(secretKey, CLIPBOARD_MS)
        mutable.update { it.copy(copied = true) }
        Speech.say(Announcement.COPIED)
    }

    fun done() {
        if (!mutable.value.canContinue) return
        mutable.update { it.copy(kit = null) }
        onDone()
    }

    companion object {
        /** How long a copied Secret Key stays on the clipboard, as in the other apps. */
        const val CLIPBOARD_MS = 30_000L
    }
}

/**
 * Sign in on a new device: email, master password and Secret Key, typed,
 * scanned from the kit, or sent by a signed-in device (D46).
 */
class SignInModel internal constructor(
    private val scope: CoroutineScope,
    private val deviceName: String,
    private val account: () -> Account,
    private val onScannedServer: (URL?) -> Unit,
    private val onSignedIn: () -> Unit,
) {
    data class State(
        val email: String = "",
        val password: Redacted<String> = Redacted(""),
        val secretKey: Redacted<String> = Redacted(""),
        val busy: Boolean = false,
        val error: String? = null,
        /** Asking a signed-in device: the code to compare there, once it's known. */
        val asking: Boolean = false,
        val approvalCode: String? = null,
        /** The Secret Key came from a signed-in device; only the password is left. */
        val approved: Boolean = false,
    ) {
        val canSubmit: Boolean
            get() = !busy && email.trim().contains('@') && password.value.isNotEmpty() && secretKey.value.isNotEmpty()
        val canAsk: Boolean get() = !busy && !asking && email.trim().contains('@')
    }

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable
    private var asking: Job? = null

    fun setEmail(email: String) = mutable.update { it.copy(email = email) }

    fun setPassword(password: String) = mutable.update { it.copy(password = Redacted(password)) }

    fun setSecretKey(secretKey: String) = mutable.update { it.copy(secretKey = Redacted(secretKey)) }

    /** "Ask another device": waits for approval, then fills in the Secret Key. */
    fun startAsking() {
        if (!mutable.value.canAsk) return
        val email = mutable.value.email.trim()
        mutable.update { it.copy(asking = true, approvalCode = null, error = null) }
        asking = scope.launch {
            try {
                val key = account().askForApproval(email) { code -> mutable.update { it.copy(approvalCode = code) } }
                mutable.update { it.copy(secretKey = key, approved = true, asking = false, approvalCode = null) }
            } catch (e: CancellationException) {
                mutable.update { it.copy(asking = false, approvalCode = null) }
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(asking = false, approvalCode = null, error = message(e)) }
            }
        }
    }

    fun cancelAsking() {
        asking?.cancel()
        asking = null
        mutable.update { it.copy(asking = false, approvalCode = null) }
    }

    /**
     * A scanned QR code: a kit fills in the email, Secret Key and server,
     * never the password. Returns whether it was a kit.
     */
    fun scanned(text: String): Boolean = try {
        val kit = EmergencyKit.scan(text)
        mutable.update { it.copy(email = kit.email, secretKey = kit.secretKey, error = null) }
        onScannedServer(kit.server)
        true
    } catch (e: Exception) {
        mutable.update { it.copy(error = message(e)) }
        false
    }

    /** The camera can't be used: say how to allow it. Typing still works. */
    fun cameraRefused() = mutable.update { it.copy(error = message(pwm.kit.PwmError("camera"))) }

    fun submit() {
        val now = mutable.value
        if (!now.canSubmit) return
        mutable.update { it.copy(busy = true, error = null) }
        scope.launch {
            try {
                account().logIn(now.email.trim(), now.password, Redacted(now.secretKey.value.trim()), deviceName)
                mutable.update { it.copy(password = Redacted(""), secretKey = Redacted(""), busy = false) }
                onSignedIn()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(busy = false, error = message(e)) }
            }
        }
    }
}

/**
 * Unlock on later launches: the master password, or a finger or face when
 * that is on (D45, D55). After a master password unlock it offers the
 * finger or face once, never again after "Not now".
 */
class UnlockModel internal constructor(
    private val scope: CoroutineScope,
    val email: String?,
    private val account: Account,
    private val prefs: AppPrefs,
    private val onUnlocked: () -> Unit,
    private val onSignedOut: () -> Unit,
    /** False on the fill screen: no offer while filling (D45 §3). */
    private val offerBiometrics: Boolean = true,
) {
    data class State(
        val password: Redacted<String> = Redacted(""),
        val busy: Boolean = false,
        val error: String? = null,
        val biometric: BiometricState? = null,
        /** "Use fingerprint or face unlock next time?", after a master password unlock. */
        val offer: Boolean = false,
    ) {
        val canSubmit: Boolean get() = !busy && password.value.isNotEmpty()
        val canUseBiometrics: Boolean get() = !busy && biometric == BiometricState.On
    }

    private val mutable = MutableStateFlow(State(biometric = runCatching { account.biometricState() }.getOrNull()))
    val state: StateFlow<State> = mutable
    /** A biometric change: turn it back on after this password unlock (D45 §4). */
    private var reenable = false
    private var tried = false

    fun setPassword(password: String) = mutable.update { it.copy(password = Redacted(password)) }

    fun submit() {
        val now = mutable.value
        if (!now.canSubmit) return
        mutable.update { it.copy(busy = true, error = null) }
        scope.launch {
            try {
                account.unlock(now.password)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(password = Redacted(""), busy = false, error = message(e)) }
                return@launch
            }
            mutable.update { it.copy(password = Redacted("")) }
            if (reenable) {
                reenable = false
                runCatching { account.enableBiometricUnlock() }
            } else if (offerBiometrics && mutable.value.biometric == BiometricState.Off && !prefs.biometricOffered) {
                mutable.update { it.copy(busy = false, offer = true) }
                return@launch
            }
            mutable.update { it.copy(busy = false) }
            onUnlocked()
        }
    }

    /** The prompt once by itself when the screen shows; a cancel leaves the password field. */
    fun tryBiometricsOnce() {
        if (tried) return
        tried = true
        unlockWithBiometrics()
    }

    fun unlockWithBiometrics() {
        if (!mutable.value.canUseBiometrics) return
        mutable.update { it.copy(busy = true, error = null) }
        scope.launch {
            try {
                account.unlockWithBiometrics()
                mutable.update { it.copy(busy = false) }
                onUnlocked()
            } catch (e: CancellationException) {
                throw e
            } catch (e: PwmError) {
                when (e.code) {
                    "biometric_cancelled" -> mutable.update { it.copy(busy = false) }
                    "biometric_changed" -> {
                        reenable = true
                        mutable.update { it.copy(busy = false, error = e.userMessage, biometric = account.biometricState()) }
                    }
                    else -> mutable.update { it.copy(busy = false, error = e.userMessage) }
                }
            } catch (e: Exception) {
                mutable.update { it.copy(busy = false, error = message(e)) }
            }
        }
    }

    /** The one-time offer, answered either way, is never shown again. */
    fun acceptOffer() {
        prefs.biometricOffered = true
        mutable.update { it.copy(busy = true) }
        scope.launch {
            // Left off when it fails or is cancelled; Settings can turn it on later.
            try {
                account.enableBiometricUnlock()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Unit
            }
            mutable.update { it.copy(busy = false, offer = false) }
            onUnlocked()
        }
    }

    fun declineOffer() {
        prefs.biometricOffered = true
        mutable.update { it.copy(offer = false) }
        onUnlocked()
    }

    fun signOut() {
        scope.launch {
            try {
                account.signOut()
                mutable.update { State() }
                onSignedOut()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(error = message(e)) }
            }
        }
    }
}
