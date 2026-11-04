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
import pwm.kit.AutofillMatch
import pwm.kit.Caller
import pwm.kit.EmergencyKit
import pwm.kit.Redacted
import pwm.kit.Server
import pwm.kit.SyncResult

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
    private val background: BackgroundScheduler = NoBackgroundScheduler,
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
        /** The Settings switch (D56 §2). */
        val hideFromCapture: Boolean = true,
    ) {
        /**
         * The window is left out of screenshots, recordings and screen sharing
         * (D56 §1): always on the Secret Key screen, and on the vault unless
         * the switch is off. The locked screens show only masked passwords.
         */
        val secureWindow: Boolean
            get() = screen is Screen.SecretKey || (screen is Screen.Vault && hideFromCapture)

        /** No picture in the app switcher (D56 §3), whatever the switch says. */
        val hideFromRecents: Boolean get() = screen is Screen.SecretKey || screen is Screen.Vault
    }

    private val mutable = MutableStateFlow(State(hideFromCapture = prefs.hideFromCapture))
    val state: StateFlow<State> = mutable

    /** The Settings switch: kept in the app's preferences. */
    fun setHideFromCapture(hide: Boolean) {
        prefs.hideFromCapture = hide
        mutable.update { it.copy(hideFromCapture = hide) }
    }

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
            if (account.isSignedIn()) {
                background.schedule()
                showUnlock(account)
            } else {
                show(Screen.Welcome)
            }
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
                    background.schedule()
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
                    onSignedIn = {
                        prefs.signInAgain = false
                        background.schedule()
                        account?.let(::showVault)
                    },
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
        lateinit var vault: VaultModel
        val signInAgain = SignInAgainModel(scope, account, prefs, onSignedIn = { vault.refresh() })
        vault = VaultModel(scope, account, clipboard, work, onSignedOut = ::didSignOut, onSessionEnded = signInAgain::sessionEnded)
        val home = HomeModel(
            vault = vault,
            generator = GeneratorModel(scope, work, clipboard).also { it.regenerate() },
            settings = SettingsModel(
                scope, account, prefs, paper, onLock = { lock(account) }, onSignedOut = ::didSignOut,
                onHideFromCapture = ::setHideFromCapture,
            ),
            signInAgain = signInAgain,
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

    // --- Autofill (D58) ---

    /**
     * What the service offers for a request: the fields' classes, and the
     * logins to suggest when unlocked. Null with nobody signed in.
     */
    suspend fun autofillPlan(caller: Caller, fieldsJson: String): AutofillPlan? {
        val account = account?.takeIf { it.isSignedIn() } ?: return null
        val unlocked = account.isUnlocked()
        val classes = account.classifyFields(fieldsJson, caller.pageUrl)
        val suggested = if (unlocked) account.autofillMatches(caller).filter { it.suggested } else emptyList()
        return AutofillPlan(classes, unlocked, suggested)
    }

    /** A suggestion's values, while unlocked. */
    suspend fun fillValues(match: AutofillMatch): FillValues? {
        val account = account?.takeIf { it.isUnlocked() } ?: return null
        return FillModel.values(account, match.itemId, match.vaultId)
    }

    /** The fill screen for a request; [onDone] gets what to fill, or null. */
    fun fillModel(caller: Caller, onDone: (FillValues?) -> Unit): FillModel? {
        val account = account?.takeIf { it.isSignedIn() } ?: return null
        return FillModel(scope, account, caller, prefs, onUnlocked = { unlockedElsewhere(account) }, onDone = onDone)
            .also { it.start() }
    }

    /** The fill screen unlocked the vault: the app shows it too when it comes back. */
    private fun unlockedElsewhere(account: Account) {
        if (mutable.value.screen is Screen.Unlock) afterUnlock(account)
    }

    // --- Background sync (D57 §2) ---

    /**
     * The periodic job's run: the locked sync with the stored tokens. A
     * refused refresh token shows at the next open, when the vault's own
     * sync meets it; a revoked device is signed out.
     */
    suspend fun backgroundSync(): BackgroundResult {
        val account = account?.takeIf { it.isSignedIn() } ?: return BackgroundResult.NO_DATA
        return try {
            when (val result = account.backgroundSync()) {
                is SyncResult.Synced -> {
                    (mutable.value.screen as? Screen.Vault)?.model?.vault?.load()
                    if (result.pulled + result.pushed > 0) BackgroundResult.NEW_DATA else BackgroundResult.NO_DATA
                }
                is SyncResult.Retry -> BackgroundResult.FAILED
                SyncResult.SignedOut -> {
                    // Shows at the next open, or now if the vault shows (D50 §5).
                    prefs.signInAgain = true
                    (mutable.value.screen as? Screen.Vault)?.model?.signInAgain?.sessionEnded()
                    BackgroundResult.NO_DATA
                }
                SyncResult.DeviceRevoked -> {
                    // The core has cleared the cache; finish signing out.
                    runCatching { account.signOut() }
                    didSignOut()
                    BackgroundResult.NO_DATA
                }
            }
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (e: Exception) {
            BackgroundResult.FAILED
        }
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
        background.cancel()
        prefs.signInAgain = false
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
