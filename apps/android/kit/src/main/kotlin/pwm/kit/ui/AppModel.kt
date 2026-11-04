package pwm.kit.ui

import java.net.URL
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import pwm.kit.Account
import pwm.kit.EmergencyKit
import pwm.kit.Redacted
import pwm.kit.Server

/**
 * Which screen shows, and the models behind it (D53 §1). The app keeps one
 * for its whole process, so turning the phone keeps the screen and what was
 * typed. No navigation library: the Activity draws [state]'s screen.
 */
class AppModel(
    private val scope: CoroutineScope,
    private val prefs: AppPrefs,
    private val deviceName: String,
    private val clipboard: SecretClipboard,
    private val paper: EmergencyKit.Paper,
    private val makeAccount: (URL) -> Account,
    private val work: CoroutineDispatcher = Dispatchers.Default,
    /**
     * Milliseconds that keep counting while the phone sleeps and ignore the
     * wall clock (D54 §5): `SystemClock.elapsedRealtime` in the app.
     */
    private val clock: () -> Long = { System.nanoTime() / 1_000_000 },
) {
    sealed class Screen {
        data object Loading : Screen()

        data object Welcome : Screen()

        class CreateAccount(val model: CreateAccountModel) : Screen()

        class SecretKey(val model: SecretKeyModel) : Screen()

        class SignIn(val model: SignInModel) : Screen()

        class Unlock(val model: UnlockModel) : Screen()

        /** The unlocked vault: its three tabs (D54 §2). */
        class Vault(val model: HomeModel) : Screen()
    }

    data class State(
        val screen: Screen = Screen.Loading,
        /** The server choice on the welcome screen: hosted, or a self-hosted address. */
        val usesOtherServer: Boolean = false,
        val otherServer: String = "",
        val error: String? = null,
    )

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable

    private var account: Account? = null
    private var accountServer: String? = null

    /** When the app went to the background, while the vault showed. */
    private var backgroundedAt: Long? = null
    /** The app opened a system screen (Save to, Print) that puts it in the background. */
    private var systemScreenOpened = false

    fun start() {
        val saved = prefs.server?.let { runCatching { Server.address(it) }.getOrNull() } ?: Server.HOSTED
        val other = !Server.isHosted(saved)
        mutable.update { it.copy(usesOtherServer = other, otherServer = if (other) saved.toString() else "") }
        try {
            val account = open(saved)
            if (account.isSignedIn()) showUnlock(account) else show(Screen.Welcome)
        } catch (e: Exception) {
            mutable.update { it.copy(error = message(e), screen = Screen.Welcome) }
        }
    }

    // --- The server choice ---

    fun setUsesOtherServer(uses: Boolean) = mutable.update { it.copy(usesOtherServer = uses, error = null) }

    fun setOtherServer(text: String) = mutable.update { it.copy(otherServer = text, error = null) }

    // --- Navigation ---

    fun showCreateAccount() {
        show(
            Screen.CreateAccount(
                CreateAccountModel(scope, work, deviceName, ::chosenAccount, onCreated = { account, key ->
                    // Until "I saved it", the kit is offered again after unlocking (D53 §4).
                    prefs.kitPending = true
                    showSecretKey(account, key)
                }),
            ),
        )
    }

    fun showSignIn() {
        show(
            Screen.SignIn(
                SignInModel(
                    scope, deviceName, ::chosenAccount,
                    onScannedServer = { server ->
                        mutable.update {
                            it.copy(usesOtherServer = server != null, otherServer = server?.toString() ?: "")
                        }
                    },
                    onSignedIn = { account?.let(::showVault) },
                ),
            ),
        )
    }

    fun showWelcome() = show(Screen.Welcome)

    /** The system back gesture: true when it was handled here. */
    fun back(): Boolean = when (val screen = mutable.value.screen) {
        is Screen.CreateAccount -> {
            showWelcome()
            true
        }
        is Screen.SignIn -> {
            screen.model.cancelAsking()
            showWelcome()
            true
        }
        is Screen.Vault -> screen.model.back()
        else -> false
    }

    private fun show(screen: Screen) = mutable.update { it.copy(screen = screen, error = null) }

    private fun showSecretKey(account: Account, key: Redacted<String>) {
        val model = SecretKeyModel(scope, key, account, clipboard, paper, onDone = {
            prefs.kitPending = false
            showVault(account)
        })
        model.prepareKit()
        show(Screen.SecretKey(model))
    }

    private fun showUnlock(account: Account) {
        show(
            Screen.Unlock(
                UnlockModel(scope, account.email(), account, prefs, onUnlocked = { afterUnlock(account) }, onSignedOut = ::didSignOut),
            ),
        )
    }

    private fun afterUnlock(account: Account) {
        val key = if (prefs.kitPending) account.secretKey() else null
        if (key != null) showSecretKey(account, key) else showVault(account)
    }

    private fun showVault(account: Account) {
        val vault = VaultModel(scope, account, clipboard, work, onSignedOut = ::didSignOut)
        val home = HomeModel(
            vault = vault,
            generator = GeneratorModel(scope, work, clipboard).also { it.regenerate() },
            settings = SettingsModel(scope, account, prefs, paper, onLock = { lock(account) }, onSignedOut = ::didSignOut),
        )
        show(Screen.Vault(home))
        scope.launch {
            vault.load()
            vault.refresh()
        }
    }

    /** Hides the vault at once, then locks the core; the unlock screen shows. */
    private fun lock(account: Account) {
        (mutable.value.screen as? Screen.Vault)?.model?.let {
            it.vault.closeAll()
            it.settings.drop()
        }
        showUnlock(account)
        scope.launch { account.lock() }
    }

    // --- The lock timeout (D54 §5) ---

    /** The whole app left the screen. */
    fun appBackgrounded() {
        if (mutable.value.screen !is Screen.Vault) return
        if (!systemScreenOpened && LockTimeout.of(prefs.lockTimeoutMinutes) == LockTimeout.IMMEDIATELY) {
            account?.let(::lock)
            return
        }
        backgroundedAt = clock()
    }

    /** The app is back: it locks first if it was away for longer than the timeout. */
    fun appForegrounded() {
        val at = backgroundedAt
        backgroundedAt = null
        val forSystemScreen = systemScreenOpened
        systemScreenOpened = false
        if (at == null || mutable.value.screen !is Screen.Vault) return
        val timeout = LockTimeout.of(prefs.lockTimeoutMinutes).millis
        // A picker or print dialog the app opened counts only once it runs long.
        val limit = if (forSystemScreen) maxOf(timeout, SYSTEM_SCREEN_GRACE_MS) else timeout
        if (clock() - at >= limit) account?.let(::lock)
    }

    /** Right before the app opens Save to or Print, which belong to the system. */
    fun openingSystemScreen() {
        systemScreenOpened = true
    }

    /** The screen turned off: the vault locks, whatever the timeout. */
    fun screenOff() {
        if (mutable.value.screen is Screen.Vault) account?.let(::lock)
    }

    private fun didSignOut() {
        prefs.kitPending = false
        backgroundedAt = null
        showWelcome()
    }

    private companion object {
        /** How long a system screen the app opened may stay before it counts as leaving. */
        const val SYSTEM_SCREEN_GRACE_MS = 10 * 60_000L
    }

    // --- Accounts ---

    private fun chosenAccount(): Account = open(chosenServer())

    private fun chosenServer(): URL {
        val now = mutable.value
        return if (now.usesOtherServer) Server.address(now.otherServer) else Server.HOSTED
    }

    /** One account at a time; choosing another server before signing in opens a new one. */
    private fun open(server: URL): Account {
        val current = account
        if (current != null && accountServer == server.toString()) return current
        val made = makeAccount(server)
        account = made
        accountServer = server.toString()
        prefs.server = if (Server.isHosted(server)) null else server.toString()
        return made
    }
}
