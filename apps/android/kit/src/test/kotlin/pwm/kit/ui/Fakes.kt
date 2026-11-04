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
        },
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

    override suspend fun currentCode(itemId: String, vaultId: String): CurrentCode? =
        if (items[itemId]?.totp.isNullOrEmpty()) null else CurrentCode(Redacted("123456"), 20)

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

    /** Adds an item as if it had synced from another device. */
    fun add(item: Item) {
        items[item.id!!] = item
    }

    companion object {
        const val PASSWORD = "right horse battery staple"
        const val KEY = "A1-FAKE00-SECRET-KEY000-000000-AAA"
        const val VAULT = "vault-1"
    }
}
