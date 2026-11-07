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
import kotlinx.coroutines.withTimeoutOrNull
import uniffi.pwm_ffi.AndroidCaller
import uniffi.pwm_ffi.ApprovalAsk
import uniffi.pwm_ffi.PasskeyCreateCheck
import uniffi.pwm_ffi.androidAppLink
import uniffi.pwm_ffi.breachPaddingHeader
import uniffi.pwm_ffi.breachRangeUrl
import uniffi.pwm_ffi.detectImport
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
    /** For the breach check only: straight to the breach service, never through our server (D22 §4). */
    private val breachTransport: HttpTransport = UrlConnectionTransport(),
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
        // A new account is a new device: the old id may belong to an account signed out here.
        val first = core { client.startSignup(email, password.value, deviceIdFor(email, fresh = true), Platform.ANDROID, deviceName) }
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
            client.startLogin(email, password.value, secretKey.value, deviceIdFor(email), Platform.ANDROID, deviceName)
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

    override suspend fun isUnlocked(): Boolean = on { client.isUnlocked() }

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

    /**
     * Ends this device's session on the server (D62), then deletes the
     * account from this device: its cache, tokens, Secret Key and email.
     * The server gets at most [SIGN_OUT_MS] and any failure is ignored, so
     * being offline never blocks signing out.
     */
    override suspend fun signOut() = on {
        endSession()
        core { client.signOut() }
        disableBiometrics()
        for (name in listOf(KeyName.SECRET_KEY, KeyName.ACCESS_TOKEN, KeyName.REFRESH_TOKEN, KeyName.EMAIL, KeyName.PASSWORD_UNLOCKED_AT)) {
            keys.delete(name)
        }
    }

    /** `POST /v1/auth/logout` with the stored tokens, locked or not; a revoked device's sends nothing. */
    private suspend fun endSession() {
        val access = keys.get(KeyName.ACCESS_TOKEN) ?: return
        val refresh = keys.get(KeyName.REFRESH_TOKEN) ?: return
        try {
            val ended = withTimeoutOrNull(SIGN_OUT_MS) {
                run(core { client.startSignOut(access, refresh) }, transport.withTimeout(SIGN_OUT_MS.toInt()))
            }
            if (ended == null) client.cancel()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            // Offline, refused or timed out: the device is wiped all the same.
            client.cancel()
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

    override suspend fun setPushToken(token: String?) = on {
        val access = keys.get(KeyName.ACCESS_TOKEN)
        val refresh = keys.get(KeyName.REFRESH_TOKEN)
        if (access == null || refresh == null) throw PwmError("signed_out")
        val first = core { client.startPushToken(access, refresh, "android", token, false) }
        expect<Outcome.PushTokenSet>(runOnline(first))
    }

    override suspend fun vaultIds(): List<String> = on { core { client.vaultIds() } }

    /** Every item of a vault, trashed ones included. */
    override suspend fun items(vaultId: String): List<Item> = on {
        core { client.items(vaultId) }.map(Item::parse)
    }

    /** One item, or null when it's gone. */
    override suspend fun item(itemId: String, vaultId: String): Item? = on {
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

    // --- Autofill (D58) ---

    override suspend fun classifyFields(fieldsJson: String, pageUrl: String): String = on {
        core { client.classifyFields(fieldsJson, pageUrl) }
    }

    override suspend fun autofillMatches(caller: Caller): List<AutofillMatch> = on {
        if (!client.isUnlocked()) return@on emptyList()
        val found = when (caller) {
            is Caller.Web -> core { client.autofillMatches(caller.url, null) }
            is Caller.App -> core { client.autofillAppMatches(caller.packageName, caller.certs.map(Callers::digest)) }
        }
        found.map { AutofillMatch(it.vaultId, it.itemId, it.title, it.username, it.decision) }
    }

    override suspend fun linkApp(itemId: String, vaultId: String, packageName: String, cert: String) = on {
        val item = core { client.item(vaultId, itemId) }?.let(Item::parse) ?: throw PwmError("unknown_item")
        val link = core { androidAppLink(packageName, cert) }
        if (item.urls.any { it.url == link }) return@on
        val linked = item.edited { setUrls(item.urls + Item.Url(link, "exact")) }
        core { client.saveItem(vaultId, linked.toJson().value, now()) }
        Unit
    }

    // --- Passkeys through the Credential Manager (D61) ---

    private fun PasskeyCaller.ffi(): AndroidCaller = AndroidCaller(
        caller.packageName, CredentialCallers.bytes(caller.cert), caller.origin, clientDataHash, assetLinks,
    )

    override suspend fun passkeyCreateCheck(caller: PasskeyCaller, requestJson: String): PasskeyCheck = on {
        when (val check = core { client.passkeyCreateCheckAndroid(caller.ffi(), requestJson) }) {
            is PasskeyCreateCheck.Ready -> PasskeyCheck.Ready(check.rpId, check.userName)
            PasskeyCreateCheck.Excluded -> PasskeyCheck.Excluded
            PasskeyCreateCheck.Unsupported -> PasskeyCheck.Unsupported
        }
    }

    override suspend fun passkeyCreate(vaultId: String, caller: PasskeyCaller, requestJson: String): String = on {
        core { client.passkeyCreateAndroid(vaultId, caller.ffi(), requestJson, now()) }
    }

    override suspend fun passkeyOffers(caller: PasskeyCaller, requestJson: String): List<PasskeyOffer> = on {
        if (!client.isUnlocked()) return@on emptyList()
        core { client.passkeyCandidatesAndroid(caller.ffi(), requestJson) }
            .map { PasskeyOffer(it.vaultId, it.itemId, it.credentialId, it.userName, it.title) }
    }

    override suspend fun passkeyGet(caller: PasskeyCaller, requestJson: String, offer: PasskeyOffer): String = on {
        core { client.passkeyGetAndroid(caller.ffi(), requestJson, offer.vaultId, offer.itemId, offer.credentialId) }
    }

    // --- Devices (D43 §6, D59 §2) ---

    override suspend fun devices(): List<DeviceEntry> = on {
        val outcome = runOnline(core { client.startListDevices() })
        (outcome as? Outcome.Devices ?: throw PwmError("protocol")).devices.map {
            DeviceEntry(it.id, it.platform, it.name, it.createdAt, it.lastSeenAt, it.revokedAt, it.current)
        }
    }

    override suspend fun renameDevice(deviceId: String, name: String) = on {
        expect<Outcome.DeviceUpdated>(runOnline(core { client.startRenameDevice(deviceId, name) }))
    }

    override suspend fun revokeDevice(deviceId: String) = on {
        expect<Outcome.DeviceUpdated>(runOnline(core { client.startRevokeDevice(deviceId) }))
    }

    // --- Approving a new device, the signed-in side (D46, D59 §3) ---

    override suspend fun approvalRequests(): List<ApprovalEntry> = on {
        val outcome = runOnline(core { client.startListApprovals() })
        (outcome as? Outcome.Approvals ?: throw PwmError("protocol")).approvals.map {
            ApprovalEntry(it.id, it.platform, it.createdAt, it.state, it.code)
        }
    }

    override suspend fun acceptApproval(id: String) = on {
        expect<Outcome.ApprovalUpdated>(runOnline(core { client.startAcceptApproval(id) }))
    }

    /** After the user saw the same code on both devices: the Secret Key, sealed for the new one. */
    override suspend fun completeApproval(id: String) = on {
        val secretKey = keys.get(KeyName.SECRET_KEY) ?: throw PwmError("not_signed_in")
        expect<Outcome.ApprovalUpdated>(runOnline(core { client.startCompleteApproval(id, secretKey) }))
    }

    override suspend fun denyApproval(id: String) = on {
        expect<Outcome.ApprovalUpdated>(runOnline(core { client.startDenyApproval(id) }))
    }

    // --- Health (D22, D44, D59 §5) ---

    override suspend fun passwordHealth(): PasswordHealth = on {
        val report = core { client.healthReport() }
        PasswordHealth(report.weak.associate { it.itemId to it.score.toInt() }, report.reused)
    }

    /** Each 5-character prefix is asked once; a refusal or a dead network ends the check. */
    override suspend fun breachedPasswords(): Map<String, Long> = on {
        core { client.breachCheck() }.use { check ->
            val header = breachPaddingHeader()
            for (prefix in check.prefixes()) {
                val request = HttpRequest("GET", URL(breachRangeUrl(prefix)), listOf(header.name to header.value), null)
                val response = try {
                    breachTransport.send(request)
                } catch (e: IOException) {
                    throw PwmError("network")
                }
                currentCoroutineContext().ensureActive()
                if (response.status != 200) throw PwmError("breach_check")
                core { check.addResponse(prefix, response.body.decodeToString()) }
            }
            check.finish().associate { it.itemId to it.count.toLong() }
        }
    }

    // --- Export (D24, D59 §6) ---

    override suspend fun authorizeExport(password: Redacted<String>) = on {
        val secretKey = keys.get(KeyName.SECRET_KEY) ?: throw PwmError("not_signed_in")
        core { client.authorizeExport(password.value, secretKey, now()) }
    }

    override suspend fun export(format: ExportFormat): Redacted<ByteArray> = on {
        Redacted(core { client.export(format.core, now()) })
    }

    // --- Import (D51, D59 §7) ---

    override fun guessImport(fileName: String, data: Redacted<ByteArray>): ImportGuess? =
        detectImport(fileName, data.value)?.let { ImportGuess(it.format, it.needsPassword) }

    override fun importColumns(data: Redacted<ByteArray>): CsvColumns {
        val columns = core { uniffi.pwm_ffi.importColumns(data.value) }
        return CsvColumns(columns.headers, ColumnMap.of(columns.suggested), columns.rows.toInt())
    }

    override suspend fun prepareImport(
        format: String,
        data: Redacted<ByteArray>,
        password: Redacted<String>?,
        keyFile: Redacted<ByteArray>?,
        columns: ColumnMap?,
    ): ImportSummary = on {
        ImportSummary.of(core { client.prepareImport(null, format, data.value, password?.value, keyFile?.value, columns?.core, now()) })
    }

    override suspend fun finishImport(): ImportSummary = on {
        val done = ImportSummary.of(core { client.finishImport(now()) })
        try {
            syncNow()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            // Left queued, like any change.
        }
        done
    }

    override suspend fun cancelImport() = on { client.cancelImport() }

    // --- Signing in again (D50 §5, D59 §4) ---

    /**
     * After the server refused this device's refresh token: the master
     * password only, since the email and Secret Key are here. The cache and
     * its unsent changes stay, and so does fingerprint unlock; it counts as
     * typing the master password. Then syncs.
     */
    override suspend fun signInAgain(password: Redacted<String>): SyncResult = on {
        val email = keys.get(KeyName.EMAIL)
        val secretKey = keys.get(KeyName.SECRET_KEY)
        if (email == null || secretKey == null) throw PwmError("signed_out")
        val first = core { client.startLogin(email, password.value, secretKey, deviceIdFor(email), Platform.ANDROID, "") }
        if (run(first) != Outcome.LoggedIn) throw PwmError("protocol")
        passwordTyped()
        syncNow()
    }

    // --- Plan and billing (D6) ---

    override suspend fun billingStatus(): BillingStatus = on {
        billing(core { client.startBillingStatus() })
    }

    override suspend fun playPurchase(productId: String, purchaseToken: Redacted<String>): BillingStatus = on {
        billing(core { client.startPlayPurchase(productId, purchaseToken.value) })
    }

    /** Offline or a 503 (the server can't reach the provider) both mean try again later. */
    private suspend fun billing(first: Step): BillingStatus {
        val outcome = runOnline(first, waited = "billing_unavailable")
        return BillingStatus.of((outcome as? Outcome.Billing ?: throw PwmError("protocol")).billing)
    }

    /** A conversation that needs the server: offline is `network` (or [waited]), and a refused session says so. */
    private suspend fun runOnline(first: Step, waited: String = "network"): Outcome {
        val outcome = try {
            run(first)
        } catch (e: Wait) {
            throw PwmError(waited)
        }
        return when (outcome) {
            Outcome.DeviceRevoked -> throw PwmError("device_revoked")
            Outcome.SignedOut -> throw PwmError("signed_out")
            else -> outcome
        }
    }

    private inline fun <reified T : Outcome> expect(outcome: Outcome) {
        if (outcome !is T) throw PwmError("protocol")
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
    private suspend fun run(first: Step, via: HttpTransport = transport): Outcome {
        var step = first
        while (true) {
            try {
                step = when (step) {
                    is Step.Done -> return step.outcome
                    is Step.Wait -> throw Wait(step.delayMs.toLong())
                    is Step.Send -> {
                        val response = try {
                            via.send(httpRequest(step.request))
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

        /** How long signing out waits for the server (D62), as for the push token (D60 §4). */
        const val SIGN_OUT_MS = 5_000L
    }

    private fun deviceId(): String {
        keys.get(KeyName.DEVICE_ID)?.let { return it }
        val id = core { newId() }
        keys.set(KeyName.DEVICE_ID, id)
        return id
    }

    /**
     * The device id to sign in with: the stored one only when it belongs to
     * [email], since the server refuses one device in two accounts
     * (`device_conflict`). Signing out and back in keeps the same device.
     */
    private fun deviceIdFor(email: String, fresh: Boolean = false): String {
        val stored = keys.get(KeyName.DEVICE_ID)
        val owner = keys.get(KeyName.DEVICE_ACCOUNT)
        if (!fresh && stored != null && owner.equals(email, ignoreCase = true)) return stored
        val id = core { newId() }
        keys.set(KeyName.DEVICE_ID, id)
        keys.set(KeyName.DEVICE_ACCOUNT, email)
        return id
    }
}
