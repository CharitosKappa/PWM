package pwm.kit.ui

import java.net.URL
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import pwm.kit.Account
import pwm.kit.BiometricState
import pwm.kit.CurrentCode
import pwm.kit.EmergencyKit
import pwm.kit.Item
import pwm.kit.PwmError
import pwm.kit.Redacted
import pwm.kit.Server
import pwm.kit.SyncResult

/** A copy to the clipboard, as the fake clipboard saw it. */
class Copied(val value: String, val clearAfterMs: Long)

/** The app model over fake accounts, a fake clipboard and a clock the test moves. */
class Harness(scope: TestScope, val prefs: MemoryPrefs = MemoryPrefs()) {
    val accounts = mutableListOf<FakeAccount>()
    val files = FakeFiles()
    val push = FakePush()
    val notices = FakeNotices()
    val play = FakePlayBilling()
    var nextAccount: (URL) -> FakeAccount = { FakeAccount(it) }
    val copied = mutableListOf<Copied>()
    var now = 1_000_000L
    /** What the fake scheduler was asked, in order. */
    val scheduled = mutableListOf<String>()
    val app = AppModel(
        scope, prefs, "Pixel 8", { value, after -> copied += Copied(value.value, after) },
        EmergencyKit.Paper.A4,
        makeAccount = { server -> nextAccount(server).also { accounts += it } },
        work = StandardTestDispatcher(scope.testScheduler),
        clock = { now },
        background = object : BackgroundScheduler {
            override fun schedule() {
                scheduled += "schedule"
            }

            override fun cancel() {
                scheduled += "cancel"
            }

            override fun syncNow() {
                scheduled += "syncNow"
            }
        },
        importFiles = files,
        push = push,
        notices = notices,
        playBilling = play,
    )
    val screen get() = app.state.value.screen
    val home get() = (screen as AppModel.Screen.Vault).model
    val vault get() = home.vault
}

/** An account with no server and one vault in memory: the right password is [PASSWORD]. */
class FakeAccount(override val server: URL = Server.HOSTED, signedIn: Boolean = false) : Account {
    private var signedIn = signedIn
    private var email: String? = if (signedIn) "ada@example.com" else null
    var unlocked = false
    val calls = mutableListOf<String>()
    var approval: suspend (suspend (String) -> Unit) -> Redacted<String> = { throw PwmError("approval_ended") }
    var approvalCancelled = false
    var syncResult: SyncResult = SyncResult.Synced(0, 0, 0, 0)
    var syncs = 0
    var purges = 0

    /** The vault's items by id. */
    val items = linkedMapOf<String, Item>()
    private var nextId = 0

    override suspend fun signUp(email: String, password: Redacted<String>, deviceName: String): Redacted<String> {
        calls += "signUp $email $deviceName"
        signedIn = true
        unlocked = true
        this.email = email
        return Redacted(KEY)
    }

    override suspend fun logIn(
        email: String,
        password: Redacted<String>,
        secretKey: Redacted<String>,
        deviceName: String,
    ): SyncResult {
        if (password.value != PASSWORD) throw PwmError("invalid_credentials")
        calls += "logIn $email ${secretKey.value} $deviceName"
        signedIn = true
        unlocked = true
        this.email = email
        return SyncResult.Synced(0, 0, 0, 0)
    }

    override suspend fun askForApproval(email: String, onCode: suspend (String) -> Unit): Redacted<String> = try {
        approval(onCode)
    } catch (e: kotlinx.coroutines.CancellationException) {
        approvalCancelled = true
        throw e
    }

    override suspend fun unlock(password: Redacted<String>) {
        if (password.value != PASSWORD) throw PwmError("wrong_password")
        unlocked = true
    }

    override suspend fun lock() {
        unlocked = false
    }

    override fun isSignedIn(): Boolean = signedIn

    override fun email(): String? = email

    override fun secretKey(): Redacted<String>? = if (signedIn) Redacted(KEY) else null

    override suspend fun emergencyKit(paper: EmergencyKit.Paper): Redacted<ByteArray> = Redacted("%PDF-1.4".toByteArray())

    override suspend fun signOut() {
        signedIn = false
        unlocked = false
        email = null
        biometricOn = false
    }

    /** What deleting the account fails with (D74), after the password check; null deletes. */
    var deleteError: PwmError? = null

    override suspend fun deleteAccount(password: Redacted<String>) {
        if (password.value != PASSWORD) throw PwmError("wrong_password")
        deleteError?.let { throw it }
        calls += "deleteAccount"
        signOut()
    }

    // Fingerprint and face unlock: [biometricResult] is what the prompt does.
    /** Off unless a test turns it on, so other tests don't meet the offer. */
    var biometricAvailable = false
    var biometricOn = false
    var biometricResult: () -> Unit = {}
    var prompts = 0

    override fun biometricState(): BiometricState = when {
        !biometricAvailable -> BiometricState.Unavailable("biometric_not_enrolled")
        biometricOn -> BiometricState.On
        else -> BiometricState.Off
    }

    override suspend fun enableBiometricUnlock() {
        prompts += 1
        biometricResult()
        biometricOn = true
    }

    override suspend fun disableBiometricUnlock() {
        biometricOn = false
    }

    override suspend fun unlockWithBiometrics() {
        if (!biometricOn) throw PwmError("biometric_unavailable")
        prompts += 1
        try {
            biometricResult()
        } catch (e: PwmError) {
            if (e.code == "biometric_changed") biometricOn = false
            throw e
        }
        unlocked = true
    }

    override suspend fun sync(): SyncResult {
        syncs += 1
        return syncResult
    }

    var backgroundSyncs = 0
    var backgroundResult: SyncResult = SyncResult.Synced(0, 0, 0, 0)

    override suspend fun backgroundSync(): SyncResult {
        backgroundSyncs += 1
        return backgroundResult
    }

    override suspend fun vaultIds(): List<String> = listOf(VAULT)

    override suspend fun items(vaultId: String): List<Item> = items.values.toList()

    override fun newItem(kind: Item.Kind, title: String): Item = Item.new(kind, title, "item-${++nextId}", 1L)

    override suspend fun save(item: Item, vaultId: String): String {
        val id = item.id ?: throw PwmError("invalid_item")
        items[id] = item
        calls += "save $id"
        return id
    }

    override suspend fun setTrashed(itemId: String, vaultId: String, trashed: Boolean) {
        val item = items[itemId] ?: throw PwmError("unknown_item")
        val json = item.json.toMutableMap()
        if (trashed) json["trashed_at"] = JsonPrimitive(5L) else json.remove("trashed_at")
        items[itemId] = Item.parse(JsonObject(json).toString())
    }

    override suspend fun delete(itemId: String, vaultId: String) {
        items.remove(itemId) ?: throw PwmError("unknown_item")
    }

    override suspend fun purgeTrash(): Int {
        purges += 1
        return 0
    }

    /** The plan doesn't include one-time codes (D73 §1). */
    var codeNeedsPlan = false

    override suspend fun currentCode(itemId: String, vaultId: String): CurrentCode? = when {
        items[itemId]?.totp.isNullOrEmpty() -> null
        codeNeedsPlan -> throw PwmError("plan_required")
        else -> CurrentCode(Redacted("123456"), 20)
    }

    // Autofill: logins whose title starts with the app's package or the web host match.
    override suspend fun item(itemId: String, vaultId: String): Item? = items[itemId]

    var classes: String = "[]"

    override suspend fun classifyFields(fieldsJson: String, pageUrl: String): String = classes

    override suspend fun autofillMatches(caller: pwm.kit.Caller): List<pwm.kit.AutofillMatch> {
        if (!unlocked) return emptyList()
        val key = when (caller) {
            is pwm.kit.Caller.Web -> java.net.URI(caller.url).host
            is pwm.kit.Caller.App -> caller.packageName
        }
        return items.values.filter { it.kind == Item.Kind.LOGIN && it.urls.any { u -> key in u.url } }
            .map { pwm.kit.AutofillMatch(VAULT, it.id!!, it.title, it.username, "fill") }
    }

    val links = mutableListOf<String>()

    override suspend fun linkApp(itemId: String, vaultId: String, packageName: String, cert: String) {
        links += "$itemId $packageName"
        val item = items.getValue(itemId)
        items[itemId] = item.edited { setUrls(item.urls + Item.Url("android-app://$packageName?sha256=$cert", "exact")) }
    }

    override suspend fun isUnlocked(): Boolean = unlocked

    // Passkeys (D61): kept by RP id; an app needs a site file naming its package.
    class FakePasskey(val rpId: String, val userName: String, val itemId: String)

    val passkeys = mutableListOf<FakePasskey>()
    var passkeyChecks = 0

    private fun rpOf(caller: pwm.kit.PasskeyCaller, requestJson: String): String {
        if (caller.caller.origin == null && caller.assetLinks.none { caller.caller.packageName in it }) {
            throw PwmError("passkey_app_not_linked")
        }
        return Regex("\"(?:id|rpId)\":\"([a-z.]+)\"").find(requestJson)?.groupValues?.get(1) ?: throw PwmError("invalid_passkey_request")
    }

    override suspend fun passkeyCreateCheck(caller: pwm.kit.PasskeyCaller, requestJson: String): pwm.kit.PasskeyCheck {
        if (!unlocked) throw PwmError("locked")
        passkeyChecks += 1
        val rp = rpOf(caller, requestJson)
        val user = Regex("\"name\":\"([^\"]+)\"").findAll(requestJson).last().groupValues[1]
        return if (passkeys.any { it.rpId == rp && it.userName == user }) pwm.kit.PasskeyCheck.Excluded else pwm.kit.PasskeyCheck.Ready(rp, user)
    }

    override suspend fun passkeyCreate(vaultId: String, caller: pwm.kit.PasskeyCaller, requestJson: String): String {
        if (!unlocked) throw PwmError("locked")
        val rp = rpOf(caller, requestJson)
        val user = Regex("\"name\":\"([^\"]+)\"").findAll(requestJson).last().groupValues[1]
        val item = newItem(Item.Kind.LOGIN, rp).edited { username = user }
        save(item, vaultId)
        passkeys += FakePasskey(rp, user, item.id!!)
        return "{\"id\":\"cred-${passkeys.size}\"}"
    }

    override suspend fun passkeyOffers(caller: pwm.kit.PasskeyCaller, requestJson: String): List<pwm.kit.PasskeyOffer> {
        if (!unlocked) return emptyList()
        val rp = rpOf(caller, requestJson)
        return passkeys.withIndex().filter { it.value.rpId == rp }
            .map { (i, p) -> pwm.kit.PasskeyOffer(VAULT, p.itemId, "cred-${i + 1}", p.userName, p.rpId) }
    }

    override suspend fun passkeyGet(caller: pwm.kit.PasskeyCaller, requestJson: String, offer: pwm.kit.PasskeyOffer): String {
        if (!unlocked) throw PwmError("locked")
        rpOf(caller, requestJson)
        calls += "passkeyGet ${offer.credentialId}"
        return "{\"id\":\"${offer.credentialId}\",\"signed\":true}"
    }

    // Devices and approvals: a list in memory.
    val devices = mutableListOf(
        pwm.kit.DeviceEntry("d1", "android", "Pixel 8", 1, 2, null, true),
        pwm.kit.DeviceEntry("d2", "ios", "iPhone", 1, 2, null, false),
    )
    val approvals = mutableListOf<pwm.kit.ApprovalEntry>()
    var signInAgains = 0

    override suspend fun devices(): List<pwm.kit.DeviceEntry> = devices.toList()

    override suspend fun renameDevice(deviceId: String, name: String) {
        val i = devices.indexOfFirst { it.id == deviceId }.takeIf { it >= 0 } ?: throw PwmError("unknown_device")
        val d = devices[i]
        devices[i] = pwm.kit.DeviceEntry(d.id, d.platform, name, d.createdAtMs, d.lastSeenAtMs, d.revokedAtMs, d.isCurrent)
    }

    override suspend fun revokeDevice(deviceId: String) {
        if (!devices.removeIf { it.id == deviceId }) throw PwmError("unknown_device")
    }

    override suspend fun approvalRequests(): List<pwm.kit.ApprovalEntry> = approvals.toList()

    override suspend fun acceptApproval(id: String) = replaceApproval(id, "accepted", null)

    override suspend fun completeApproval(id: String) {
        if (!approvals.removeIf { it.id == id }) throw PwmError("unknown_approval")
        calls += "complete $id"
    }

    override suspend fun denyApproval(id: String) {
        approvals.removeIf { it.id == id }
        calls += "deny $id"
    }

    /** The new device revealed its key: the code shows. */
    fun reveal(id: String, code: String) = replaceApproval(id, "revealed", code)

    private fun replaceApproval(id: String, state: String, code: String?) {
        val i = approvals.indexOfFirst { it.id == id }.takeIf { it >= 0 } ?: throw PwmError("unknown_approval")
        val a = approvals[i]
        approvals[i] = pwm.kit.ApprovalEntry(a.id, a.platform, a.createdAtMs, state, code)
    }

    override suspend fun signInAgain(password: Redacted<String>): SyncResult {
        if (password.value != PASSWORD) throw PwmError("invalid_credentials")
        signInAgains += 1
        syncResult = SyncResult.Synced(0, 0, 0, 0)
        return syncResult
    }

    /** The push tokens sent, in order; null removes it. */
    val pushTokens = mutableListOf<String?>()
    var pushTokenError: PwmError? = null

    override suspend fun setPushToken(token: String?) {
        pushTokenError?.let { throw it }
        pushTokens += token
    }

    // Plan and billing (D6): free on a server selling through Play until a purchase arrives.
    var billing = pwm.kit.BillingStatus(pwm.kit.BillingStatus.FREE, null, listOf("stripe", "play"), Redacted(BILLING_TOKEN))
    var billingError: PwmError? = null
    var billingCalls = 0
    /** What the server was handed: "product purchaseToken". */
    val playPurchases = mutableListOf<String>()
    var playPurchaseError: PwmError? = null
    /** Refunds asked for, by provider. */
    val refunds = mutableListOf<String>()
    var refundError: PwmError? = null

    override suspend fun billingStatus(): pwm.kit.BillingStatus {
        billingCalls += 1
        billingError?.let { throw it }
        return billing
    }

    override suspend fun playPurchase(productId: String, purchaseToken: Redacted<String>): pwm.kit.BillingStatus {
        playPurchaseError?.let { throw it }
        playPurchases += "$productId ${purchaseToken.value}"
        billing = billing.copy(
            plan = "premium",
            // Just charged: refundable for 30 days (D72 §1).
            subscription = pwm.kit.BillingStatus.Subscription("play", "premium", "active", 2_000_000_000_000, false, null, REFUNDABLE_UNTIL, emptyList()),
        )
        // The server's answer to a purchase has no token.
        return billing.copy(token = null)
    }

    /** As the core and server answer: Stripe or Play only, inside the window; then Free. */
    override suspend fun refund(provider: String): pwm.kit.BillingStatus {
        refundError?.let { throw it }
        if (provider != "stripe" && provider != "play") throw PwmError("managed_elsewhere")
        val subscription = billing.subscription ?: throw PwmError("no_subscription")
        if (subscription.provider != provider) throw PwmError("managed_elsewhere")
        if (subscription.refundableUntilMs == null) throw PwmError("refund_window_passed")
        refunds += provider
        billing = billing.copy(plan = pwm.kit.BillingStatus.FREE, subscription = null)
        return billing.copy(token = null)
    }

    // Health, export and import.
    var health = pwm.kit.PasswordHealth(emptyMap(), emptyList())
    var breached: Map<String, Long> = emptyMap()
    var breachError: PwmError? = null
    var breachChecks = 0
    var exportAuthorized = false
    var guess: pwm.kit.ImportGuess? = pwm.kit.ImportGuess("csv", false)
    var columns = pwm.kit.CsvColumns(listOf("name", "url", "username", "password"), pwm.kit.ColumnMap(title = 0, url = 1, username = 2, password = 3), 2)
    var preview = pwm.kit.ImportSummary(2, 0, 0, 0, 0, 0, listOf(pwm.kit.ImportSummary.LeftOut("empty", 1)))
    /** The import's password, when the file needs one. */
    var importPassword: String? = null
    /** What prepareImport was given last. */
    var prepared: Pair<String, pwm.kit.ColumnMap?>? = null

    override suspend fun passwordHealth(): pwm.kit.PasswordHealth = health

    override suspend fun breachedPasswords(): Map<String, Long> {
        breachChecks += 1
        breachError?.let { throw it }
        return breached
    }

    override suspend fun authorizeExport(password: Redacted<String>) {
        if (password.value != PASSWORD) throw PwmError("wrong_password")
        exportAuthorized = true
    }

    override suspend fun export(format: pwm.kit.ExportFormat): Redacted<ByteArray> {
        if (!exportAuthorized) throw PwmError("export_not_authorized")
        exportAuthorized = false
        calls += "export ${format.fileExtension}"
        return Redacted("exported".toByteArray())
    }

    override fun guessImport(fileName: String, data: Redacted<ByteArray>): pwm.kit.ImportGuess? = guess

    override fun importColumns(data: Redacted<ByteArray>): pwm.kit.CsvColumns = columns

    override suspend fun prepareImport(
        format: String,
        data: Redacted<ByteArray>,
        password: Redacted<String>?,
        keyFile: Redacted<ByteArray>?,
        columns: pwm.kit.ColumnMap?,
    ): pwm.kit.ImportSummary {
        importPassword?.let { if (password?.value != it) throw PwmError(if (password == null) "import_password_needed" else "import_wrong_password") }
        prepared = format to columns
        calls += "prepare $format"
        return preview
    }

    override suspend fun finishImport(): pwm.kit.ImportSummary {
        if (prepared == null) throw PwmError("import_not_prepared")
        calls += "finish"
        prepared = null
        return preview
    }

    override suspend fun cancelImport() {
        prepared = null
        calls += "cancelImport"
    }

    /** Adds an item as if it had synced from another device. */
    fun add(item: Item) {
        items[item.id!!] = item
    }

    companion object {
        const val PASSWORD = "right horse battery staple"
        const val KEY = "A1-FAKE00-SECRET-KEY000-000000-AAA"
        const val VAULT = "vault-1"
        const val BILLING_TOKEN = "6f1c2a5e-0000-4000-8000-00000000b111"
        const val REFUNDABLE_UNTIL = 1_900_000_000_000L
    }
}

/** Picked files by address; [deletable] is false for a place that can't delete. */
class FakeFiles : ImportFiles {
    class File(val name: String, val bytes: ByteArray, val size: Long? = bytes.size.toLong())

    val files = mutableMapOf<String, File>()
    var deletable = true
    /** Files read, by address: a refused one is never read. */
    val reads = mutableListOf<String>()

    override fun name(uri: String): String? = files[uri]?.name

    override fun size(uri: String): Long? = files[uri]?.size

    override fun read(uri: String): ByteArray {
        reads += uri
        return files[uri]?.bytes ?: throw PwmError("import_unreadable")
    }

    override fun delete(uri: String): Boolean {
        if (!deletable) return false
        return files.remove(uri) != null
    }
}

/** FCM as the app would see it; off unless a test turns it on. */
class FakePush : PushService {
    override var available = false
    var current: String? = "fcm-token-1"

    override suspend fun token(): String? = current
}

/** The notices shown, and whether Android allows them. */
class FakeNotices : Notices {
    var allowed = true
    var shown = 0

    override fun allowed(): Boolean = allowed

    override fun showApprovalNotice() {
        shown += 1
    }
}

/** Play Billing as the app would see it: [next] is what Play's sheet answers. */
class FakePlayBilling : PlayBilling {
    override val packageName = "invalid.pwm.app"
    val later = kotlinx.coroutines.flow.MutableSharedFlow<PlayResult>(extraBufferCapacity = 8)
    /** Off, [results] ends at once, so tests that never lock don't leave a listener running. */
    var live = false
    override val results: kotlinx.coroutines.flow.Flow<PlayResult>
        get() = if (live) later else kotlinx.coroutines.flow.emptyFlow()
    /** What Play holds for this Google account. */
    val owned = mutableListOf<PlayPurchase>()
    /** The sheets shown: "product accountToken". */
    val bought = mutableListOf<String>()
    var next: (String, Redacted<String>) -> PlayResult = { product, account ->
        PlayResult.Purchases(listOf(purchase(product, "purchase-1", account.value)))
    }

    override suspend fun buy(productId: String, accountToken: Redacted<String>): PlayResult {
        bought += "$productId ${accountToken.value}"
        return next(productId, accountToken)
    }

    override suspend fun owned(): List<PlayPurchase> = owned.toList()

    companion object {
        fun purchase(
            product: String = pwm.kit.PlayProducts.PREMIUM,
            token: String = "purchase-1",
            account: String? = FakeAccount.BILLING_TOKEN,
            state: PlayPurchase.State = PlayPurchase.State.PURCHASED,
            acknowledged: Boolean = false,
        ) = PlayPurchase(listOf(product), Redacted(token), state, account?.let(::Redacted), acknowledged)
    }
}
