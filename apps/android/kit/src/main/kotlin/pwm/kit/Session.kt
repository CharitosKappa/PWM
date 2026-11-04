package pwm.kit

import java.io.IOException
import java.net.URL
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.withContext
import uniffi.pwm_ffi.Event
import uniffi.pwm_ffi.Header
import uniffi.pwm_ffi.Outcome
import uniffi.pwm_ffi.Platform
import uniffi.pwm_ffi.PwmClient
import uniffi.pwm_ffi.Request
import uniffi.pwm_ffi.Response
import uniffi.pwm_ffi.Step
import uniffi.pwm_ffi.newId

/** How a sync run ended. */
sealed class SyncResult {
    data class Synced(val pulled: Long, val pushed: Long, val conflicts: Long, val failed: Long) : SyncResult()

    /** The run stopped on a failure; try again after this long. */
    data class Retry(val afterMs: Long) : SyncResult()

    /** The refresh token was refused: sign in again. Unsent changes are kept. */
    data object SignedOut : SyncResult()

    /** This device was revoked: its cache is cleared and the session locked. */
    data object DeviceRevoked : SyncResult()
}

/** Something the app may want to show. */
sealed class SessionEvent {
    /** The server refused this item ("couldn't sync"); it stays queued. */
    data class ItemFailed(val vaultId: String, val itemId: String, val status: Int) : SessionEvent()
}

/**
 * One signed-in device's vault: the app's only way into the core (D52 §4).
 * Every server conversation is the core's step loop (D28, D31) over
 * [HttpTransport]; the Secret Key, tokens and device id go to [KeyStore].
 * Calls run one at a time on their own dispatcher, so they never overlap.
 * Never logs values.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class Session(
    cachePath: String?,
    private val server: URL,
    private val keys: KeyStore,
    private val transport: HttpTransport = UrlConnectionTransport(),
    private val now: () -> Long = System::currentTimeMillis,
    private val dispatcher: CoroutineDispatcher = Dispatchers.IO.limitedParallelism(1),
) {
    private val client: PwmClient = core {
        if (cachePath != null) PwmClient.open(cachePath) else PwmClient.inMemory()
    }
    private val eventsFlow = MutableSharedFlow<SessionEvent>(extraBufferCapacity = 64)

    /** Item failures and other notices, for the app's UI. */
    val events: SharedFlow<SessionEvent> = eventsFlow

    // --- Account ---

    /**
     * Creates the account and its first vault. Returns the Secret Key to
     * show on the Emergency Kit now; it is kept in [KeyStore] for unlocking.
     */
    suspend fun signUp(email: String, password: Redacted<String>, deviceName: String): Redacted<String> = on {
        val first = core { client.startSignup(email, password.value, deviceId(), Platform.ANDROID, deviceName) }
        val outcome = run(first) as? Outcome.SignedUp ?: throw PwmError("protocol")
        keys.set(KeyName.SECRET_KEY, outcome.secretKey)
        keys.set(KeyName.EMAIL, email)
        run(core { client.startEnsureVault() })
        Redacted(outcome.secretKey)
    }

    /** Signs in on this device with all three secrets, then syncs. */
    suspend fun logIn(
        email: String,
        password: Redacted<String>,
        secretKey: Redacted<String>,
        deviceName: String,
    ): SyncResult = on {
        val first = core {
            client.startLogin(email, password.value, secretKey.value, deviceId(), Platform.ANDROID, deviceName)
        }
        if (run(first) != Outcome.LoggedIn) throw PwmError("protocol")
        keys.set(KeyName.SECRET_KEY, secretKey.value)
        keys.set(KeyName.EMAIL, email)
        syncNow()
    }

    /** Unlocks the account cached on this device, offline. */
    suspend fun unlock(password: Redacted<String>) = on {
        val secretKey = keys.get(KeyName.SECRET_KEY) ?: throw PwmError("not_signed_in")
        core {
            client.unlock(
                password.value, secretKey,
                keys.get(KeyName.ACCESS_TOKEN) ?: "", keys.get(KeyName.REFRESH_TOKEN) ?: "",
            )
        }
    }

    suspend fun lock() = on { client.lock() }

    suspend fun isUnlocked(): Boolean = on { client.isUnlocked() }

    /** Whether an account is signed in on this device (it may be locked). */
    fun isSignedIn(): Boolean = keys.get(KeyName.SECRET_KEY) != null

    /** The email this device signed in with, for the unlock screen. */
    fun email(): String? = keys.get(KeyName.EMAIL)

    /** Deletes the account from this device: its cache, tokens, Secret Key and email. */
    suspend fun signOut() = on {
        core { client.signOut() }
        for (name in listOf(KeyName.SECRET_KEY, KeyName.ACCESS_TOKEN, KeyName.REFRESH_TOKEN, KeyName.EMAIL)) {
            keys.delete(name)
        }
    }

    // --- Sync and items ---

    suspend fun sync(): SyncResult = on { syncNow() }

    suspend fun vaultIds(): List<String> = on { core { client.vaultIds() } }

    /** Every item of a vault, trashed ones included. */
    suspend fun items(vaultId: String): List<Item> = on {
        core { client.items(vaultId) }.map(Item::parse)
    }

    fun newItem(kind: Item.Kind, title: String): Item = Item.new(kind, title, core { newId() }, now())

    /** Saves locally (history and times are the core's); sync uploads it. */
    suspend fun save(item: Item, vaultId: String): String = on {
        core { client.saveItem(vaultId, item.toJson().value, now()) }
    }

    // --- The step loop ---

    private suspend fun <T> on(work: suspend () -> T): T = withContext(dispatcher) { work() }

    private class Wait(val delayMs: Long) : Exception()

    private suspend fun syncNow(): SyncResult {
        val outcome = try {
            run(core { client.startSync() })
        } catch (e: Wait) {
            return SyncResult.Retry(e.delayMs)
        }
        return when (outcome) {
            is Outcome.Synced -> SyncResult.Synced(
                outcome.pulled.toLong(), outcome.pushed.toLong(), outcome.conflicts.toLong(), outcome.failed.toLong(),
            )
            Outcome.SignedOut -> SyncResult.SignedOut
            Outcome.DeviceRevoked -> SyncResult.DeviceRevoked
            else -> throw PwmError("protocol")
        }
    }

    /**
     * Sends each request the core asks for until it is done. A transport
     * failure goes back to the core as `networkFailed`; a cancelled coroutine
     * cancels the conversation.
     */
    private suspend fun run(first: Step): Outcome {
        var step = first
        while (true) {
            try {
                step = when (step) {
                    is Step.Done -> return step.outcome
                    is Step.Wait -> throw Wait(step.delayMs.toLong())
                    is Step.Send -> {
                        val response = try {
                            transport.send(httpRequest(step.request))
                        } catch (e: IOException) {
                            null
                        }
                        try {
                            currentCoroutineContext().ensureActive()
                        } catch (e: Exception) {
                            client.cancel()
                            throw e
                        }
                        if (response == null) {
                            core { client.networkFailed() }
                        } else {
                            val answer = Response(
                                response.status.toUShort(),
                                response.headers.map { (n, v) -> Header(n, v) },
                                response.body,
                            )
                            core { client.receive(answer, now()) }
                        }
                    }
                }
            } finally {
                drainEvents()
            }
        }
    }

    private fun httpRequest(request: Request): HttpRequest {
        val base = server.toString().trimEnd('/')
        return HttpRequest(
            method = request.method,
            url = URL(base + request.path),
            headers = request.headers.map { it.name to it.value },
            body = request.body,
        )
    }

    /** New tokens go to [KeyStore]; the rest to [events]. */
    private fun drainEvents() {
        for (event in client.takeEvents()) {
            when (event) {
                is Event.Tokens -> {
                    keys.set(KeyName.ACCESS_TOKEN, event.access)
                    keys.set(KeyName.REFRESH_TOKEN, event.refresh)
                }
                is Event.ItemFailed ->
                    eventsFlow.tryEmit(SessionEvent.ItemFailed(event.vaultId, event.itemId, event.status.toInt()))
            }
        }
    }

    private fun deviceId(): String {
        keys.get(KeyName.DEVICE_ID)?.let { return it }
        val id = core { newId() }
        keys.set(KeyName.DEVICE_ID, id)
        return id
    }
}
