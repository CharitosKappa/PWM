package pwm.kit.ui

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import pwm.kit.Account
import pwm.kit.ApprovalEntry
import pwm.kit.DeviceEntry
import pwm.kit.PwmError
import pwm.kit.Redacted

// Settings' account screens (D59 §2-4), as on Apple (D43 §6, D46, D50 §5).

/** The account's devices: rename any, revoke any other one. */
class DevicesModel internal constructor(private val scope: CoroutineScope, private val account: Account) {
    data class State(
        val devices: List<DeviceEntry> = emptyList(),
        val loaded: Boolean = false,
        val busy: Boolean = false,
        /** Asked before revoking. */
        val revokeAsked: DeviceEntry? = null,
        val error: String? = null,
    )

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable

    fun load() {
        scope.launch { reload() }
    }

    private suspend fun reload() {
        try {
            val devices = account.devices().filter { it.revokedAtMs == null }
            mutable.update { it.copy(devices = devices, loaded = true, error = null) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            mutable.update { it.copy(loaded = true, error = message(e)) }
        }
    }

    fun rename(device: DeviceEntry, name: String) {
        val trimmed = name.trim()
        if (trimmed.isEmpty() || device.revokedAtMs != null) return
        change { account.renameDevice(device.id, trimmed) }
    }

    fun askRevoke(device: DeviceEntry) {
        if (device.isCurrent || device.revokedAtMs != null) return
        mutable.update { it.copy(revokeAsked = device) }
    }

    fun keepDevice() = mutable.update { it.copy(revokeAsked = null) }

    /** Signs another device out for good, after asking. This device signs out instead. */
    fun revoke() {
        val device = mutable.value.revokeAsked ?: return
        mutable.update { it.copy(revokeAsked = null) }
        change { account.revokeDevice(device.id) }
    }

    private fun change(action: suspend () -> Unit) {
        mutable.update { it.copy(busy = true, error = null) }
        scope.launch {
            try {
                action()
                reload()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(error = message(e)) }
            } finally {
                mutable.update { it.copy(busy = false) }
            }
        }
    }
}

/**
 * "Approve a new device" (D46, D59 §3): the account's requests. Approving
 * one shows the code to compare; the Secret Key goes only after the user
 * says the codes match. The screen reads the list every few seconds while
 * it shows, since Android has no push yet (D57).
 */
class ApprovalsModel internal constructor(private val scope: CoroutineScope, private val account: Account) {
    data class State(
        val requests: List<ApprovalEntry> = emptyList(),
        val loaded: Boolean = false,
        /** The request this device accepted. */
        val approving: String? = null,
        val busy: Boolean = false,
        val done: Boolean = false,
        val error: String? = null,
    ) {
        /** The code for the request being approved, once the new device revealed its key. */
        val code: String? get() = requests.firstOrNull { it.id == approving }?.code
    }

    private val mutable = MutableStateFlow(State())
    val state: StateFlow<State> = mutable

    /** Reads the requests; the screen calls it every few seconds. */
    suspend fun load() {
        try {
            val requests = account.approvalRequests()
            mutable.update { s ->
                val gone = s.approving != null && requests.none { it.id == s.approving }
                s.copy(
                    requests = requests, loaded = true,
                    approving = if (gone) null else s.approving,
                    error = if (gone) message(PwmError("unknown_approval")) else s.error,
                )
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            mutable.update { it.copy(loaded = true, error = message(e)) }
        }
    }

    fun approve(request: ApprovalEntry) = change(after = { mutable.update { it.copy(approving = request.id) } }) {
        account.acceptApproval(request.id)
    }

    /** The user saw the same code on both devices. */
    fun codesMatch() {
        val s = mutable.value
        val id = s.approving ?: return
        if (s.code == null) return
        change(after = { mutable.update { it.copy(approving = null, done = true) } }) { account.completeApproval(id) }
    }

    /** The codes differ, or the user doesn't know this request. */
    fun deny(id: String) = change(after = {
        mutable.update { if (it.approving == id) it.copy(approving = null) else it }
    }) { account.denyApproval(id) }

    private fun change(after: () -> Unit, action: suspend () -> Unit) {
        mutable.update { it.copy(busy = true, error = null, done = false) }
        scope.launch {
            try {
                action()
                after()
                load()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(error = message(e)) }
            } finally {
                mutable.update { it.copy(busy = false) }
            }
        }
    }
}

/**
 * "Sign in again to sync" (D50 §5, D59 §4): the server refused this
 * device's refresh token. It shows over the vault list until the master
 * password is entered, and is kept, so a refusal a background sync met
 * shows at the next open.
 */
class SignInAgainModel internal constructor(
    private val scope: CoroutineScope,
    private val account: Account,
    private val prefs: AppPrefs,
    private val onSignedIn: () -> Unit,
) {
    data class State(
        val needed: Boolean = false,
        /** The sheet asking for the master password is open. */
        val asking: Boolean = false,
        val password: Redacted<String> = Redacted(""),
        val working: Boolean = false,
        val error: String? = null,
    ) {
        val canSignIn: Boolean get() = !working && password.value.isNotEmpty()
    }

    private val mutable = MutableStateFlow(State(needed = prefs.signInAgain))
    val state: StateFlow<State> = mutable

    /** A sync was told the session ended. */
    fun sessionEnded() {
        prefs.signInAgain = true
        mutable.update { it.copy(needed = true) }
    }

    fun open() = mutable.update { it.copy(asking = true, error = null) }

    fun close() = mutable.update { it.copy(asking = false, password = Redacted("")) }

    fun setPassword(password: String) = mutable.update { it.copy(password = Redacted(password)) }

    fun signIn() {
        val now = mutable.value
        if (!now.canSignIn) return
        mutable.update { it.copy(working = true, error = null) }
        scope.launch {
            try {
                account.signInAgain(now.password)
                prefs.signInAgain = false
                mutable.update { State() }
                onSignedIn()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                mutable.update { it.copy(working = false, password = Redacted(""), error = message(e)) }
            }
        }
    }
}
