package pwm.kit.ui

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import pwm.kit.Account
import pwm.kit.AutofillMatch
import pwm.kit.Caller
import pwm.kit.Item
import pwm.kit.Redacted

/** What to fill (D58 §7): never more than the login's username, password and current code. */
class FillValues(val username: String, val password: Redacted<String>, val code: Redacted<String>?) {
    override fun toString(): String = "FillValues(<redacted>)"
}

/** What the service offers for a request (D58 §5-6). */
class AutofillPlan(
    /** The core's classes for the fields, `[{class, confidence}]`. */
    val classes: String,
    val unlocked: Boolean,
    /** The logins offered without asking; empty while locked. */
    val suggested: List<AutofillMatch>,
)

/**
 * PWM's fill screen (D58 §6): unlock if needed, then the matches and a
 * search over every login; a tap fills and the screen closes. Picking a
 * login from search for an app can link it to that app ("Use for this app").
 */
class FillModel internal constructor(
    private val scope: CoroutineScope,
    private val account: Account,
    val caller: Caller,
    private val prefs: AppPrefs,
    private val onUnlocked: () -> Unit,
    private val onDone: (FillValues?) -> Unit,
) {
    data class State(
        val loading: Boolean = true,
        /** Set while locked: the same unlock screen as the app, without the fingerprint offer. */
        val unlock: UnlockModel? = null,
        val matches: List<AutofillMatch> = emptyList(),
        val query: String = "",
        /** Logins found by search, when there is a query. */
        val results: List<Entry> = emptyList(),
        /** "Use for this app", for a login picked from search for an app. */
        val linkForApp: Boolean = true,
        val error: String? = null,
    )

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable
    private var all: List<Entry> = emptyList()

    /** Whether picking from search can link the login to the app asking. */
    val canLink: Boolean get() = caller is Caller.App && caller.certs.isNotEmpty()

    fun start() {
        scope.launch {
            if (account.isUnlocked()) {
                load()
            } else {
                val unlock = UnlockModel(
                    scope, account.email(), account, prefs,
                    onUnlocked = {
                        onUnlocked()
                        scope.launch { load() }
                    },
                    onSignedOut = { onDone(null) },
                    offerBiometrics = false,
                )
                mutable.update { it.copy(loading = false, unlock = unlock) }
            }
        }
    }

    private suspend fun load() {
        try {
            val matches = account.autofillMatches(caller).filter { it.suggested }
            all = account.vaultIds().flatMap { v -> account.items(v).map { Entry(v, it) } }
                .filter { it.item.kind == Item.Kind.LOGIN && it.item.trashedAtMs == null }
            mutable.update { it.copy(loading = false, unlock = null, matches = matches, error = null) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            mutable.update { it.copy(loading = false, error = message(e)) }
        }
    }

    fun setQuery(query: String) = mutable.update {
        it.copy(query = query, results = if (query.isBlank()) emptyList() else VaultModel.filter(all, Section.All, query))
    }

    fun setLinkForApp(link: Boolean) = mutable.update { it.copy(linkForApp = link) }

    /** A tap on a suggestion. */
    fun pick(match: AutofillMatch) = fill(match.itemId, match.vaultId, fromSearch = false)

    /** A tap on a search result: linked to the app first, if asked. */
    fun pick(entry: Entry) = fill(entry.id, entry.vaultId, fromSearch = true)

    fun cancel() = onDone(null)

    private fun fill(itemId: String, vaultId: String, fromSearch: Boolean) {
        scope.launch {
            try {
                if (fromSearch && canLink && mutable.value.linkForApp) {
                    val app = caller as Caller.App
                    account.linkApp(itemId, vaultId, app.packageName, app.certs.first())
                }
                onDone(values(account, itemId, vaultId))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(error = message(e)) }
            }
        }
    }

    internal companion object {
        /** The login's values, with its current code when it has one. */
        suspend fun values(account: Account, itemId: String, vaultId: String): FillValues? {
            val item = account.item(itemId, vaultId) ?: return null
            val code = if (item.totp.isNullOrEmpty()) null else account.currentCode(itemId, vaultId)?.code
            return FillValues(item.username, Redacted(item.password), code)
        }
    }
}
