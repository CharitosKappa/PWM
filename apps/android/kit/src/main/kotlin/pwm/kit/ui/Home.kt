package pwm.kit.ui

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import pwm.kit.Account
import pwm.kit.BiometricState
import pwm.kit.EmergencyKit
import pwm.kit.Generator
import pwm.kit.PasswordStrength
import pwm.kit.PwmError
import pwm.kit.Redacted

/** How long the vault stays unlocked while the app is in the background (D43 §5, D54 §5). */
enum class LockTimeout(val minutes: Int) {
    IMMEDIATELY(0), ONE_MINUTE(1), FIVE_MINUTES(5), FIFTEEN_MINUTES(15), ONE_HOUR(60), EIGHT_HOURS(480);

    val millis: Long get() = minutes * 60_000L

    companion object {
        val STANDARD = FIFTEEN_MINUTES

        /** The saved choice, or the default. */
        fun of(minutes: Int?): LockTimeout = entries.firstOrNull { it.minutes == minutes } ?: STANDARD
    }
}

/** The three tabs while the vault is unlocked (D54 §2). */
class HomeModel internal constructor(
    val vault: VaultModel,
    val generator: GeneratorModel,
    val settings: SettingsModel,
) {
    enum class Tab { VAULT, GENERATOR, SETTINGS }

    data class State(val tab: Tab = Tab.VAULT)

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable

    /** Each tab keeps its own place. */
    fun setTab(tab: Tab) = mutable.update { it.copy(tab = tab) }

    /** The back gesture: the vault's stack first, then back to the vault tab. */
    fun back(): Boolean = when (mutable.value.tab) {
        Tab.VAULT -> vault.back()
        Tab.SETTINGS -> settings.back() || run { setTab(Tab.VAULT); true }
        Tab.GENERATOR -> run { setTab(Tab.VAULT); true }
    }
}

/** The generator tab and the password field's sheet (D43 §2). Values stay in memory, never stored. */
class GeneratorModel internal constructor(
    private val scope: CoroutineScope,
    private val work: CoroutineDispatcher,
    private val clipboard: SecretClipboard,
) {
    enum class Mode { PASSWORD, PASSPHRASE }

    data class State(
        val mode: Mode = Mode.PASSWORD,
        val length: Int = 20,
        val lowercase: Boolean = true,
        val uppercase: Boolean = true,
        val digits: Boolean = true,
        val symbols: Boolean = true,
        val avoidLookalikes: Boolean = false,
        val words: Int = 5,
        val separator: String = "-",
        val capitalize: Boolean = false,
        val addDigit: Boolean = false,
        val value: Redacted<String>? = null,
        val strength: PasswordStrength? = null,
        val copied: Boolean = false,
        val error: String? = null,
    )

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable

    /** Changes an option and makes a new value for it. */
    fun options(change: (State) -> State) {
        mutable.update(change)
        regenerate()
    }

    /** A new value for the current options. */
    fun regenerate() {
        val s = mutable.value
        val value = try {
            when (s.mode) {
                Mode.PASSWORD -> Generator.password(s.length, s.lowercase, s.uppercase, s.digits, s.symbols, s.avoidLookalikes)
                Mode.PASSPHRASE -> Generator.passphrase(s.words, s.separator, s.capitalize, s.addDigit)
            }
        } catch (e: PwmError) {
            mutable.update { it.copy(value = null, strength = null, copied = false, error = e.userMessage) }
            return
        }
        mutable.update { it.copy(value = value, strength = null, copied = false, error = null) }
        scope.launch {
            val scored = withContext(work) { PasswordStrength.of(value) }
            mutable.update { if (it.value == value) it.copy(strength = scored) else it }
        }
    }

    fun copy() {
        val value = mutable.value.value ?: return
        clipboard.copy(value, SecretKeyModel.CLIPBOARD_MS)
        mutable.update { it.copy(copied = true) }
    }

    companion object {
        val LENGTHS = 8..64
        val WORD_COUNTS = 3..10
        val SEPARATORS = listOf("-", ".", "_", " ", ",")
    }
}

/** The settings tab (D43 §2; devices, health and export come in M6.8). */
class SettingsModel internal constructor(
    private val scope: CoroutineScope,
    private val account: Account,
    private val prefs: AppPrefs,
    private val paper: EmergencyKit.Paper,
    private val onLock: () -> Unit,
    private val onSignedOut: () -> Unit,
) {
    enum class Page { MAIN, NOTICES }

    data class State(
        val email: String? = null,
        /** The self-hosted address, or null for the hosted service. */
        val server: String? = null,
        val lockTimeout: LockTimeout = LockTimeout.STANDARD,
        /** Fingerprint and face unlock (D55): on, off, or why it can't be used. */
        val biometric: BiometricState? = null,
        val biometricBusy: Boolean = false,
        /** The Emergency Kit PDF, made when Settings opens. */
        val kit: Redacted<ByteArray>? = null,
        val kitSaved: Boolean = false,
        val signOutAsked: Boolean = false,
        val page: Page = Page.MAIN,
        val error: String? = null,
    )

    private val mutable = MutableStateFlow(
        State(
            email = account.email(),
            server = prefs.server,
            lockTimeout = LockTimeout.of(prefs.lockTimeoutMinutes),
            biometric = runCatching { account.biometricState() }.getOrNull(),
        ),
    )

    /** The switch: turning it on asks for a finger or face once; a cancel leaves it off. */
    fun setBiometric(on: Boolean) {
        mutable.update { it.copy(biometricBusy = true, error = null) }
        scope.launch {
            try {
                if (on) account.enableBiometricUnlock() else account.disableBiometricUnlock()
            } catch (e: CancellationException) {
                throw e
            } catch (e: PwmError) {
                if (e.code != "biometric_cancelled") mutable.update { it.copy(error = e.userMessage) }
            } catch (e: Exception) {
                mutable.update { it.copy(error = message(e)) }
            }
            mutable.update { it.copy(biometricBusy = false, biometric = runCatching { account.biometricState() }.getOrNull()) }
        }
    }
    val state: StateFlow<State> = mutable

    fun setLockTimeout(timeout: LockTimeout) {
        prefs.lockTimeoutMinutes = timeout.minutes
        mutable.update { it.copy(lockTimeout = timeout) }
    }

    fun prepareKit() {
        if (mutable.value.kit != null) return
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

    fun didSaveKit() = mutable.update { it.copy(kitSaved = true, error = null) }

    fun saveFailed() = mutable.update { it.copy(error = message(PwmError("kit_save_failed"))) }

    fun lockNow() = onLock()

    fun showNotices() = mutable.update { it.copy(page = Page.NOTICES) }

    /** Back from a page of Settings; false on Settings itself. */
    fun back(): Boolean {
        if (mutable.value.page == Page.MAIN) return false
        mutable.update { it.copy(page = Page.MAIN) }
        return true
    }

    fun askSignOut() = mutable.update { it.copy(signOutAsked = true) }

    fun keepSignedIn() = mutable.update { it.copy(signOutAsked = false) }

    fun signOut() {
        mutable.update { it.copy(signOutAsked = false) }
        scope.launch {
            try {
                account.signOut()
                onSignedOut()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(error = message(e)) }
            }
        }
    }

    /** The kit leaves memory when the vault locks. */
    internal fun drop() = mutable.update { it.copy(kit = null) }
}
