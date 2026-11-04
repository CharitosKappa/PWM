package pwm.kit

import java.net.URL

/**
 * What the screens need from a signed-in (or signing-in) device (D53 §1,
 * D54 §4): [Session] in the app, a fake in the tests.
 */
interface Account {
    val server: URL

    suspend fun signUp(email: String, password: Redacted<String>, deviceName: String): Redacted<String>

    suspend fun logIn(
        email: String,
        password: Redacted<String>,
        secretKey: Redacted<String>,
        deviceName: String,
    ): SyncResult

    /**
     * Asks a signed-in device to approve this one (D46), calling [onCode]
     * with the code to compare there. Returns the Secret Key it sent.
     * `approval_ended` when it was denied or expired; cancelling gives up on
     * the server too.
     */
    suspend fun askForApproval(email: String, onCode: suspend (String) -> Unit): Redacted<String>

    suspend fun unlock(password: Redacted<String>)

    suspend fun lock()

    fun isSignedIn(): Boolean

    fun email(): String?

    /** This device's Secret Key, for the kit; null when signed out. */
    fun secretKey(): Redacted<String>?

    /** The Emergency Kit PDF for this account, dated today. */
    suspend fun emergencyKit(paper: EmergencyKit.Paper): Redacted<ByteArray>

    suspend fun signOut()

    suspend fun sync(): SyncResult

    // --- Items (D54 §4) ---

    suspend fun vaultIds(): List<String>

    /** Every item of a vault, trashed ones and folders included. */
    suspend fun items(vaultId: String): List<Item>

    fun newItem(kind: Item.Kind, title: String): Item

    /** Saves locally (the core checks it); sync uploads it. Returns its id. */
    suspend fun save(item: Item, vaultId: String): String

    suspend fun setTrashed(itemId: String, vaultId: String, trashed: Boolean)

    /** Deletes a trashed item for good. */
    suspend fun delete(itemId: String, vaultId: String)

    /** Deletes items trashed long enough ago (D19); how many went. */
    suspend fun purgeTrash(): Int

    /** The login's one-time code now; null when it has none, or a kind this version can't make. */
    suspend fun currentCode(itemId: String, vaultId: String): CurrentCode?
}
