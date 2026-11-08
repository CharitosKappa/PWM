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
import pwm.kit.AssetLinks
import pwm.kit.AutofillMatch
import pwm.kit.Caller
import pwm.kit.CredentialCaller
import pwm.kit.PasskeyCaller
import pwm.kit.PasskeyCheck
import pwm.kit.PasskeyOffer
import pwm.kit.PwmError
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
    /** The files the user picks for import (D59 §7). */
    private val importFiles: ImportFiles = NoImportFiles,
    /** FCM (D60), when the build has Firebase's settings. */
    private val push: PushService = NoPush,
    /** The new-device notice (D60 §4). */
    private val notices: Notices = NoNotices,
    /** The sites' asset links files for the Credential Manager (D61 §4), kept for an hour. */
    val assetLinks: AssetLinks = AssetLinks(),
    /** Google Play Billing (D6 §2), for Settings' Plan section. */
    private val playBilling: PlayBilling = NoPlayBilling,
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
        /** Why the last account left this device, on the welcome screen: deleted (D74) or signed out elsewhere. */
        val ended: String? = null,
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
                registerPush(account)
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

    private fun show(screen: Screen) = mutable.update {
        it.copy(screen = screen, error = null, ended = if (screen is Screen.Welcome) it.ended else null)
    }

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
        lateinit var home: HomeModel
        home = HomeModel(
            vault = vault,
            generator = GeneratorModel(scope, work, clipboard).also { it.regenerate() },
            settings = SettingsModel(
                scope, account, prefs, paper, onLock = { lock(account) }, onSignedOut = { didSignOut() },
                onAccountDeleted = { didSignOut("account_deleted") },
                onHideFromCapture = ::setHideFromCapture,
                vault = vault, work = work, files = importFiles,
                pushAvailable = push.available, notices = notices,
                playBilling = playBilling,
                onEditItem = { entry ->
                    vault.edit(entry)
                    home.setTab(HomeModel.Tab.VAULT)
                },
            ),
            signInAgain = signInAgain,
        )
        show(Screen.Vault(home))
        // A Play purchase that finished while the app was closed or locked goes to the server now (D6 §3).
        home.settings.plan.start()
        registerPush(account)
        if (approvalsWaiting) {
            approvalsWaiting = false
            openApprovals()
        }
        scope.launch {
            vault.load()
            vault.refresh()
        }
    }

    // --- Push (D60 §4) ---

    /** The notice was tapped before the vault was open: "Approve a new device" shows after unlocking. */
    private var approvalsWaiting = false

    /** Sends this device's FCM token when it differs from the one last sent for this sign-in. */
    private fun registerPush(account: Account, token: String? = null) {
        if (!push.available) return
        scope.launch {
            try {
                val current = token ?: push.token() ?: return@launch
                if (current == prefs.pushToken || !account.isSignedIn()) return@launch
                account.setPushToken(current)
                prefs.pushToken = current
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (e: Exception) {
                // Offline or refused: tried again at the next launch or unlock.
            }
        }
    }

    /** FCM gave this device a new token. */
    fun pushTokenChanged(token: String) {
        val account = account ?: return
        registerPush(account, token)
    }

    /**
     * A push arrived: `sync` starts the background sync now; `approval`
     * shows the new-device notice when the switch is on and Android allows
     * it. A push names nothing else, and anything else is ignored.
     */
    fun pushReceived(kind: String?) {
        if (account?.isSignedIn() != true) return
        when (kind) {
            "sync" -> background.syncNow()
            "approval" -> if (prefs.approvalNotices && notices.allowed()) notices.showApprovalNotice()
        }
    }

    /** The notice was tapped: Settings, then "Approve a new device", once the vault is open. */
    fun openApprovals() {
        val home = (mutable.value.screen as? Screen.Vault)?.model
        if (home == null) {
            approvalsWaiting = account?.isSignedIn() == true
            return
        }
        home.setTab(HomeModel.Tab.SETTINGS)
        home.settings.showApprovals()
    }

    /** Hides the vault at once, then locks the core; the unlock screen shows. */
    private fun lock(account: Account) {
        (mutable.value.screen as? Screen.Vault)?.model?.let {
            it.vault.closeAll()
            it.settings.drop()
        }
        showUnlock(account)
        forgetCredentials()
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

    // --- The Credential Manager provider (D61) ---

    /** When the user last unlocked or confirmed, for passkeys right after (D61 §6). */
    private var verifiedAt: Long? = null

    /** Whether the provider answers, and how: entries, or "Unlock PWM". */
    suspend fun credentialAccess(): CredentialAccess {
        val account = account?.takeIf { it.isSignedIn() } ?: return CredentialAccess.SIGNED_OUT
        return if (account.isUnlocked()) CredentialAccess.UNLOCKED else CredentialAccess.LOCKED
    }

    /**
     * The logins to offer as passwords, while unlocked: suggested ones only,
     * as for autofill, and only those with a password (a passkey's login may
     * have none).
     */
    suspend fun passwordOffers(caller: CredentialCaller): List<AutofillMatch> {
        val account = account?.takeIf { it.isUnlocked() } ?: return emptyList()
        return account.autofillMatches(caller.autofillCaller)
            .filter { it.suggested && !account.item(it.itemId, it.vaultId)?.password.isNullOrEmpty() }
    }

    /** The passkeys to offer, while unlocked; none when the request is refused (another site, an app not listed). */
    suspend fun passkeyOffers(caller: PasskeyCaller, requestJson: String): List<PasskeyOffer> {
        val account = account?.takeIf { it.isUnlocked() } ?: return emptyList()
        return try {
            account.passkeyOffers(caller, requestJson)
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (e: Exception) {
            emptyList()
        }
    }

    /**
     * Whether to offer "Create a passkey in PWM". Unlocked, the core decides;
     * locked, an app needs a site file at least, and the rest is checked after
     * unlocking.
     */
    suspend fun passkeyCreatable(caller: PasskeyCaller, requestJson: String): Boolean {
        val account = account?.takeIf { it.isSignedIn() } ?: return false
        if (caller.caller.origin == null && caller.assetLinks.isEmpty()) return false
        if (!account.isUnlocked()) return true
        return try {
            account.passkeyCreateCheck(caller, requestJson) is PasskeyCheck.Ready
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (e: Exception) {
            false
        }
    }

    /** PWM's screen for an entry the user picked in the sheet; [onDone] gets the result, or null. */
    fun credentialModel(task: CredentialTask, onDone: (CredentialResult?) -> Unit): CredentialModel? {
        val account = account?.takeIf { it.isSignedIn() } ?: return null
        return CredentialModel(
            scope, account, prefs, task,
            verifiedRecently = { verifiedAt?.let { clock() - it < VERIFIED_FOR_MS } == true },
            verified = { verifiedAt = clock() },
            onUnlocked = { unlockedElsewhere(account) },
            onDone = onDone,
        ).also { it.start() }
    }

    private fun forgetCredentials() {
        verifiedAt = null
        assetLinks.clear()
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
                SyncResult.DeviceRevoked, SyncResult.AccountDeleted -> {
                    // The core has cleared the cache; finish signing out.
                    runCatching { account.signOut() }
                    didSignOut(if (result == SyncResult.AccountDeleted) "account_deleted" else "device_revoked")
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

    /** Back to welcome once the account is gone from this device; [ended] says why when it wasn't signed out here. */
    private fun didSignOut(ended: String? = null) {
        (mutable.value.screen as? Screen.Vault)?.model?.settings?.plan?.stop()
        forgetCredentials()
        background.cancel()
        prefs.pushToken = null
        approvalsWaiting = false
        prefs.signInAgain = false
        prefs.kitPending = false
        backgroundedAt = null
        mutable.update { it.copy(ended = ended?.let { code -> PwmError(code).userMessage }) }
        showWelcome()
    }

    private companion object {
        /** How long a system screen the app opened may stay before it counts as leaving. */
        const val SYSTEM_SCREEN_GRACE_MS = 10 * 60_000L

        /** Unlocking or confirming this long ago still confirms a passkey (D61 §6). */
        const val VERIFIED_FOR_MS = 60_000L
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
