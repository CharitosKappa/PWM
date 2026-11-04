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
    val app = AppModel(
        scope, prefs, "Pixel 8", { value, after -> copied += Copied(value.value, after) },
        EmergencyKit.Paper.A4,
        makeAccount = { server -> nextAccount(server).also { accounts += it } },
        work = StandardTestDispatcher(scope.testScheduler),
        clock = { now },
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
