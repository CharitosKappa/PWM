package pwm.kit.ui

import java.net.URL
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
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
) {
    sealed class Screen {
        data object Loading : Screen()

        data object Welcome : Screen()

        class CreateAccount(val model: CreateAccountModel) : Screen()

        class SecretKey(val model: SecretKeyModel) : Screen()

        class SignIn(val model: SignInModel) : Screen()

        class Unlock(val model: UnlockModel) : Screen()

        class Vault(val model: VaultModel) : Screen()
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
                UnlockModel(scope, account.email(), account, onUnlocked = { afterUnlock(account) }, onSignedOut = ::didSignOut),
            ),
        )
    }

    private fun afterUnlock(account: Account) {
        val key = if (prefs.kitPending) account.secretKey() else null
        if (key != null) showSecretKey(account, key) else showVault(account)
    }

    private fun showVault(account: Account) {
        val model = VaultModel(scope, account, onLocked = { showUnlock(account) }, onSignedOut = ::didSignOut)
        model.load()
        show(Screen.Vault(model))
    }

    private fun didSignOut() {
        prefs.kitPending = false
        showWelcome()
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
