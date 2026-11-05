package pwm.kit.ui

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import pwm.kit.Account
import pwm.kit.CredentialCaller
import pwm.kit.Item
import pwm.kit.PasskeyCaller
import pwm.kit.PasskeyCheck
import pwm.kit.PasskeyOffer
import pwm.kit.PwmError
import pwm.kit.Redacted

/** What an entry in Android's sheet asked PWM to do (D61 §5-7). */
sealed class CredentialTask {
    /** "Unlock PWM": unlock, then the sheet gets the entries. */
    data object Unlock : CredentialTask()

    /** A password entry: its username and password. */
    class GetPassword(val itemId: String, val vaultId: String) : CredentialTask()

    /** A passkey entry: confirmed, then signed. */
    class GetPasskey(val caller: PasskeyCaller, val requestJson: String, val offer: PasskeyOffer) : CredentialTask()

    /** "Save in PWM" for a password the caller asked to save. */
    class CreatePassword(
        val caller: CredentialCaller,
        /** The app's name as the system shows it; null for a browser. */
        val appName: String?,
        val username: String,
        val password: Redacted<String>,
    ) : CredentialTask()

    /** A new passkey: confirmed, then made. */
    class CreatePasskey(val caller: PasskeyCaller, val requestJson: String) : CredentialTask()
}

/** What goes back to the caller; null from the model means cancelled. */
sealed class CredentialResult {
    data object Unlocked : CredentialResult()

    class Password(val username: String, val password: Redacted<String>) : CredentialResult() {
        override fun toString(): String = "Password(<redacted>)"
    }

    class Passkey(val responseJson: String) : CredentialResult() {
        override fun toString(): String = "Passkey(<redacted>)"
    }

    data object Saved : CredentialResult()
}

/** Whether the provider can answer at all, and how (D61 §5). */
enum class CredentialAccess { SIGNED_OUT, LOCKED, UNLOCKED }

/**
 * PWM's screen for one Credential Manager entry (D61 §5-7):
 * - passwords: unlock if needed, then the values; the tap in the sheet was the confirmation
 * - passkeys: every use asks for a fingerprint or face, or the master password, right
 *   before signing; unlocking a moment earlier counts
 * - "Save in PWM": unlock if needed, then what will be saved, and Save
 */
class CredentialModel internal constructor(
    private val scope: CoroutineScope,
    private val account: Account,
    private val prefs: AppPrefs,
    val task: CredentialTask,
    private val verifiedRecently: () -> Boolean,
    private val verified: () -> Unit,
    private val onUnlocked: () -> Unit,
    private val onDone: (CredentialResult?) -> Unit,
) {
    /** What "Save in PWM" will do. */
    class SaveSummary(val username: String, val where: String, val updating: Boolean) {
        override fun toString(): String = "SaveSummary(updating=$updating)"
    }

    data class State(
        val loading: Boolean = true,
        /** The unlock screen: to unlock, or, with [confirming], to confirm a passkey. */
        val unlock: UnlockModel? = null,
        val confirming: Boolean = false,
        val save: SaveSummary? = null,
        val busy: Boolean = false,
        val error: String? = null,
    )

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable
    private var existing: Pair<String, String>? = null
    private var finished = false

    fun start() {
        scope.launch {
            when (task) {
                CredentialTask.Unlock -> unlocked { finish(CredentialResult.Unlocked) }
                is CredentialTask.GetPassword -> unlocked { password(task) }
                is CredentialTask.GetPasskey, is CredentialTask.CreatePasskey -> confirmed { passkey() }
                is CredentialTask.CreatePassword -> unlocked { summary(task) }
            }
        }
    }

    fun cancel() = finish(null)

    /** "Save" on the summary. */
    fun save() {
        val task = task as? CredentialTask.CreatePassword ?: return
        if (mutable.value.busy) return
        mutable.update { it.copy(busy = true, error = null) }
        scope.launch {
            attempt {
                val found = existing
                if (found != null) {
                    val (itemId, vaultId) = found
                    val item = account.item(itemId, vaultId) ?: throw PwmError("unknown_item")
                    account.save(item.edited { password = task.password.value }, vaultId)
                    linkIfApp(task.caller, itemId, vaultId)
                } else {
                    val vaultId = account.vaultIds().firstOrNull() ?: throw PwmError("unknown_vault")
                    val title = task.appName ?: task.caller.label
                    val url = task.caller.origin
                    val item = account.newItem(Item.Kind.LOGIN, title).edited {
                        username = task.username
                        password = task.password.value
                        if (url != null) setUrls(listOf(Item.Url(url)))
                    }
                    val itemId = account.save(item, vaultId)
                    linkIfApp(task.caller, itemId, vaultId)
                }
                finish(CredentialResult.Saved)
            }
        }
    }

    private suspend fun linkIfApp(caller: CredentialCaller, itemId: String, vaultId: String) {
        if (caller.origin == null) account.linkApp(itemId, vaultId, caller.packageName, caller.cert)
    }

    /** Runs [then] once the vault is open, unlocking first if needed. */
    private suspend fun unlocked(then: suspend () -> Unit) {
        if (account.isUnlocked()) return attempt { then() }
        showUnlock(confirming = false, then)
    }

    /** Runs [then] after the user confirms, unless they just did. */
    private suspend fun confirmed(then: suspend () -> Unit) {
        if (account.isUnlocked() && verifiedRecently()) return attempt { then() }
        showUnlock(confirming = true, then)
    }

    private fun showUnlock(confirming: Boolean, then: suspend () -> Unit) {
        val unlock = UnlockModel(
            scope, account.email(), account, prefs,
            onUnlocked = {
                verified()
                onUnlocked()
                mutable.update { it.copy(unlock = null, confirming = false, loading = true) }
                scope.launch { attempt { then() } }
            },
            onSignedOut = { finish(null) },
            offerBiometrics = false,
        )
        mutable.update { it.copy(loading = false, unlock = unlock, confirming = confirming) }
    }

    private suspend fun password(task: CredentialTask.GetPassword) {
        val item = account.item(task.itemId, task.vaultId) ?: throw PwmError("unknown_item")
        // Android takes no empty password; such a login isn't offered (passwordOffers).
        if (item.password.isEmpty()) throw PwmError("unknown_item")
        finish(CredentialResult.Password(item.username, Redacted(item.password)))
    }

    private suspend fun passkey() {
        when (task) {
            is CredentialTask.GetPasskey -> finish(CredentialResult.Passkey(account.passkeyGet(task.caller, task.requestJson, task.offer)))
            is CredentialTask.CreatePasskey -> {
                // Checked again here: locked, the sheet's entry was offered before the vault could say.
                when (account.passkeyCreateCheck(task.caller, task.requestJson)) {
                    is PasskeyCheck.Ready -> Unit
                    PasskeyCheck.Excluded -> throw PwmError("passkey_exists")
                    PasskeyCheck.Unsupported -> throw PwmError("unsupported_algorithm")
                }
                val vaultId = account.vaultIds().firstOrNull() ?: throw PwmError("unknown_vault")
                finish(CredentialResult.Passkey(account.passkeyCreate(vaultId, task.caller, task.requestJson)))
            }
            else -> Unit
        }
    }

    private suspend fun summary(task: CredentialTask.CreatePassword) {
        val same = account.autofillMatches(task.caller.autofillCaller)
            .firstOrNull { !it.decision.startsWith("refuse") && it.username.trim().equals(task.username.trim(), ignoreCase = true) }
        existing = same?.let { it.itemId to it.vaultId }
        val where = task.appName ?: task.caller.label
        mutable.update { it.copy(loading = false, save = SaveSummary(task.username, where, updating = same != null)) }
    }

    private suspend fun attempt(work: suspend () -> Unit) {
        try {
            work()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            mutable.update { it.copy(loading = false, busy = false, error = message(e)) }
        }
    }

    private fun finish(result: CredentialResult?) {
        if (finished) return
        finished = true
        onDone(result)
    }
}
