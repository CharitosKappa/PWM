package pwm.kit

import java.io.IOException
import java.net.URL
import java.time.Instant
import java.time.ZoneId
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.withContext
import uniffi.pwm_ffi.ApprovalAsk
import uniffi.pwm_ffi.AskStep
import uniffi.pwm_ffi.Event
import uniffi.pwm_ffi.Header
import uniffi.pwm_ffi.Paper
import uniffi.pwm_ffi.Outcome
import uniffi.pwm_ffi.Platform
import uniffi.pwm_ffi.PwmClient
import uniffi.pwm_ffi.Request
import uniffi.pwm_ffi.Response
import uniffi.pwm_ffi.Step
import uniffi.pwm_ffi.emergencyKit as kitPdf
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
    override val server: URL,
    private val keys: KeyStore,
    private val transport: HttpTransport = UrlConnectionTransport(),
    private val now: () -> Long = System::currentTimeMillis,
    private val dispatcher: CoroutineDispatcher = Dispatchers.IO.limitedParallelism(1),
    private val biometrics: BiometricKeys = NoBiometrics,
) : Account {
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
    override suspend fun signUp(email: String, password: Redacted<String>, deviceName: String): Redacted<String> = on {
        val first = core { client.startSignup(email, password.value, deviceId(), Platform.ANDROID, deviceName) }
        val outcome = run(first) as? Outcome.SignedUp ?: throw PwmError("protocol")
        keys.set(KeyName.SECRET_KEY, outcome.secretKey)
        keys.set(KeyName.EMAIL, email)
        passwordTyped()
        run(core { client.startEnsureVault() })
        Redacted(outcome.secretKey)
    }

    /** Signs in on this device with all three secrets, then syncs. */
    override suspend fun logIn(
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
        // A new sign-in starts with fingerprint and face unlock off (D45 §4).
        disableBiometrics()
        passwordTyped()
        syncNow()
    }

    /** Unlocks the account cached on this device, offline. */
    override suspend fun unlock(password: Redacted<String>) = on {
        val secretKey = keys.get(KeyName.SECRET_KEY) ?: throw PwmError("not_signed_in")
        core {
            client.unlock(
                password.value, secretKey,
                keys.get(KeyName.ACCESS_TOKEN) ?: "", keys.get(KeyName.REFRESH_TOKEN) ?: "",
            )
        }
        passwordTyped()
        // The blob is made again, so it always matches the keyset (D45 §4); no prompt.
        if (keys.get(KeyName.BIOMETRIC_BLOB) != null) {
            runCatching { storeBiometricUnlock() }.onFailure { disableBiometrics() }
        }
    }

    // --- Fingerprint and face unlock (D45, D55) ---

    override fun biometricState(): BiometricState = when (val available = biometrics.availability()) {
        is BiometricAvailability.Unavailable -> BiometricState.Unavailable(available.code)
        BiometricAvailability.Available ->
            if (keys.get(KeyName.BIOMETRIC_BLOB) == null) BiometricState.Off else BiometricState.On
    }

    /** Turns it on while unlocked. It asks for a finger or face once, so one that doesn't work is found now. */
    override suspend fun enableBiometricUnlock() {
        if (biometrics.availability() != BiometricAvailability.Available) throw PwmError("biometric_unavailable")
        val made = on { makeBiometricUnlock() }
        try {
            biometrics.unwrap(made.second, BiometricPurpose.TURN_ON)
        } catch (e: Exception) {
            runCatching { biometrics.delete() }
            throw e
        }
        on {
            keys.set(KeyName.BIOMETRIC_BLOB, made.first.encode())
            keys.set(KeyName.BIOMETRIC_KEY, made.second.encode())
            if (keys.get(KeyName.PASSWORD_UNLOCKED_AT) == null) passwordTyped()
        }
    }

    override suspend fun disableBiometricUnlock() = on { disableBiometrics() }

    /**
     * Unlocks after a finger or face. `master_password_due` every 14 days;
     * `biometric_changed` turns it off, and the master password unlocks.
     */
    override suspend fun unlockWithBiometrics() {
        if (biometricState() != BiometricState.On) throw PwmError("biometric_unavailable")
        val last = keys.get(KeyName.PASSWORD_UNLOCKED_AT)?.toLongOrNull() ?: 0
        if (now() - last >= BiometricRules.MASTER_PASSWORD_EVERY_MS) throw PwmError("master_password_due")
        val blob = keys.get(KeyName.BIOMETRIC_BLOB)?.decode()
        val wrapped = keys.get(KeyName.BIOMETRIC_KEY)?.decode()
        val access = keys.get(KeyName.ACCESS_TOKEN)
        val refresh = keys.get(KeyName.REFRESH_TOKEN)
        if (blob == null || wrapped == null || access == null || refresh == null) {
            on { disableBiometrics() }
            throw PwmError("biometric_changed")
        }
        val key = try {
            biometrics.unwrap(wrapped, BiometricPurpose.UNLOCK)
        } catch (e: PwmError) {
            if (e.code == "biometric_changed") on { disableBiometrics() }
            throw e
        }
        on {
            try {
                core { client.unlockBiometric(blob, key.value, deviceId(), access, refresh) }
            } catch (e: PwmError) {
                if (e.code != "biometric_unlock_failed") throw e
                // Not this account's or this device's blob, or a wrong key: start again.
                disableBiometrics()
                throw PwmError("biometric_changed")
            }
        }
    }

    /** The core's blob and the unlock key, wrapped by the platform. */
    private fun makeBiometricUnlock(): Pair<ByteArray, ByteArray> {
        val made = core { client.exportBiometricUnlock(deviceId()) }
        return made.blob to biometrics.wrap(Redacted(made.key))
    }

    private fun storeBiometricUnlock() {
        val (blob, wrapped) = makeBiometricUnlock()
        keys.set(KeyName.BIOMETRIC_BLOB, blob.encode())
        keys.set(KeyName.BIOMETRIC_KEY, wrapped.encode())
    }

    private fun disableBiometrics() {
        runCatching { biometrics.delete() }
        keys.delete(KeyName.BIOMETRIC_BLOB)
        keys.delete(KeyName.BIOMETRIC_KEY)
    }

    private fun passwordTyped() = keys.set(KeyName.PASSWORD_UNLOCKED_AT, now().toString())

    private fun ByteArray.encode(): String = java.util.Base64.getEncoder().encodeToString(this)

    private fun String.decode(): ByteArray? = runCatching { java.util.Base64.getDecoder().decode(this) }.getOrNull()

    override suspend fun lock() = on { client.lock() }

    suspend fun isUnlocked(): Boolean = on { client.isUnlocked() }

    /** Whether an account is signed in on this device (it may be locked). */
    override fun isSignedIn(): Boolean = keys.get(KeyName.SECRET_KEY) != null

    /** The email this device signed in with, for the unlock screen. */
    override fun email(): String? = keys.get(KeyName.EMAIL)

    override fun secretKey(): Redacted<String>? = keys.get(KeyName.SECRET_KEY)?.let(::Redacted)

    /** The Emergency Kit PDF for the signed-in account (D42 §5), dated today on this device. */
    override suspend fun emergencyKit(paper: EmergencyKit.Paper): Redacted<ByteArray> = on {
        val email = keys.get(KeyName.EMAIL)
        val secretKey = keys.get(KeyName.SECRET_KEY)
        val accountId = core { client.accountId() }
        if (email == null || secretKey == null || accountId == null) throw PwmError("not_signed_in")
        val created = Instant.ofEpochMilli(now()).atZone(ZoneId.systemDefault()).toLocalDate().toString()
        val server = if (Server.isHosted(server)) null else server.toString()
        val bytes = core {
            kitPdf(email, secretKey, server, accountId, created, if (paper == EmergencyKit.Paper.A4) Paper.A4 else Paper.LETTER)
        }
        Redacted(bytes)
    }

    /** Deletes the account from this device: its cache, tokens, Secret Key and email. */
    override suspend fun signOut() = on {
        core { client.signOut() }
        disableBiometrics()
        for (name in listOf(KeyName.SECRET_KEY, KeyName.ACCESS_TOKEN, KeyName.REFRESH_TOKEN, KeyName.EMAIL, KeyName.PASSWORD_UNLOCKED_AT)) {
            keys.delete(name)
        }
    }

    // --- Sync and items ---

    override suspend fun sync(): SyncResult = on { syncNow() }

    /**
     * A sync for the background job (D50 §1, D57 §2): it needs only the
     * tokens, so it runs while locked too. Locked, it stores what needs no
     * key and uploads what is queued; the rest waits for the next unlocked
     * sync. Unlocked, it is [sync].
     */
    override suspend fun backgroundSync(): SyncResult = on {
        val access = keys.get(KeyName.ACCESS_TOKEN)
        val refresh = keys.get(KeyName.REFRESH_TOKEN)
        if (access == null || refresh == null) return@on SyncResult.SignedOut
        val outcome = try {
            run(core { client.startLockedSync(access, refresh) })
        } catch (e: Wait) {
            return@on SyncResult.Retry(e.delayMs)
        }
        when (outcome) {
            is Outcome.Synced -> SyncResult.Synced(
                outcome.pulled.toLong(), outcome.pushed.toLong(), outcome.conflicts.toLong(), outcome.failed.toLong(),
            )
            Outcome.SignedOut -> SyncResult.SignedOut
            Outcome.DeviceRevoked -> SyncResult.DeviceRevoked
            else -> throw PwmError("protocol")
        }
    }

    override suspend fun vaultIds(): List<String> = on { core { client.vaultIds() } }

    /** Every item of a vault, trashed ones included. */
    override suspend fun items(vaultId: String): List<Item> = on {
        core { client.items(vaultId) }.map(Item::parse)
    }

    /** One item, or null when it's gone. */
    suspend fun item(itemId: String, vaultId: String): Item? = on {
        core { client.item(vaultId, itemId) }?.let(Item::parse)
    }

    override suspend fun setTrashed(itemId: String, vaultId: String, trashed: Boolean) = on {
        core { client.setTrashed(vaultId, itemId, trashed, now()) }
    }

    override suspend fun delete(itemId: String, vaultId: String) = on {
        core { client.deleteItem(vaultId, itemId) }
    }

    override suspend fun purgeTrash(): Int = on { core { client.purgeTrash(now()) }.toInt() }

    override suspend fun currentCode(itemId: String, vaultId: String): CurrentCode? = on {
        core { client.currentCode(vaultId, itemId, now()) }?.let { CurrentCode(Redacted(it.code), it.secondsLeft.toLong()) }
    }

    override fun newItem(kind: Item.Kind, title: String): Item = Item.new(kind, title, core { newId() }, now())

    /** Saves locally (history and times are the core's); sync uploads it. */
    override suspend fun save(item: Item, vaultId: String): String = on {
        core { client.saveItem(vaultId, item.toJson().value, now()) }
    }

    // --- Asking another device (D46) ---

    override suspend fun askForApproval(email: String, onCode: suspend (String) -> Unit): Redacted<String> {
        val ask = core { ApprovalAsk(email, Platform.ANDROID) }
        return ask.use {
            var request = ask.firstRequest() ?: throw PwmError("internal")
            try {
                while (true) {
                    val response = try {
                        withContext(Dispatchers.IO) { transport.send(httpRequest(request)) }
                    } catch (e: IOException) {
                        throw PwmError("network")
                    }
                    val answer = Response(
                        response.status.toUShort(),
                        response.headers.map { (n, v) -> Header(n, v) },
                        response.body,
                    )
                    when (val step = core { ask.receive(answer) }) {
                        is AskStep.Send -> request = step.request
                        is AskStep.ShowCode -> {
                            onCode(step.code)
                            delay(APPROVAL_POLL_MS)
                            request = core { ask.poll() }
                        }
                        is AskStep.Wait -> {
                            delay(step.delayMs.toLong().coerceAtLeast(APPROVAL_POLL_MS))
                            request = core { ask.poll() }
                        }
                        is AskStep.Approved -> return@use Redacted(step.secretKey)
                        AskStep.Ended -> throw PwmError("approval_ended")
                    }
                }
                @Suppress("UNREACHABLE_CODE")
                throw PwmError("internal")
            } catch (e: CancellationException) {
                // Gives up on the server too, so the other device stops showing it.
                ask.cancel()?.let { cancel ->
                    withContext(NonCancellable + Dispatchers.IO) {
                        runCatching { transport.send(httpRequest(cancel)) }
                    }
                }
                throw e
            }
        }
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

    private companion object {
        const val APPROVAL_POLL_MS = 2_000L
    }

    private fun deviceId(): String {
        keys.get(KeyName.DEVICE_ID)?.let { return it }
        val id = core { newId() }
        keys.set(KeyName.DEVICE_ID, id)
        return id
    }
}
