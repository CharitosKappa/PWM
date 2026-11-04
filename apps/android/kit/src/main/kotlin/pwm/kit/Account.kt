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

    // --- Fingerprint and face unlock (D45, D55) ---

    fun biometricState(): BiometricState

    /** Asks for a finger or face once; `biometric_cancelled` leaves it off. */
    suspend fun enableBiometricUnlock()

    suspend fun disableBiometricUnlock()

    /** `master_password_due`, `biometric_changed` or `biometric_cancelled` leave the password to type. */
    suspend fun unlockWithBiometrics()

    suspend fun sync(): SyncResult

    /** The background job's sync: locked or not, with the stored tokens (D57 §2). */
    suspend fun backgroundSync(): SyncResult

    // --- Devices and approvals (D59 §2-3) ---

    /** The account's devices, this one marked. */
    suspend fun devices(): List<DeviceEntry>

    /** Renames one of the account's devices (63 bytes at most), sealed. */
    suspend fun renameDevice(deviceId: String, name: String)

    /** Revokes another device; to leave this one, sign out. */
    suspend fun revokeDevice(deviceId: String)

    suspend fun approvalRequests(): List<ApprovalEntry>

    /** Accepts a listed request; list again until it has a code. */
    suspend fun acceptApproval(id: String)

    suspend fun completeApproval(id: String)

    suspend fun denyApproval(id: String)

    /** After a refused refresh token (D50 §5): the master password only. */
    suspend fun signInAgain(password: Redacted<String>): SyncResult

    // --- Health, export and import (D59 §5-7) ---

    /** Weak and reused passwords across every vault; local only. */
    suspend fun passwordHealth(): PasswordHealth

    /**
     * Logins whose password is in known breaches (item id to how often it
     * was seen). Only 5 characters of each password's SHA-1 leave the
     * device, straight to the breach service with padding (D22 §4).
     */
    suspend fun breachedPasswords(): Map<String, Long>

    /** The master password again; the next [export] within 5 minutes may run. A fingerprint never counts. */
    suspend fun authorizeExport(password: Redacted<String>)

    /** Every vault as a file, on every plan. Keep the bytes only until saved. */
    suspend fun export(format: ExportFormat): Redacted<ByteArray>

    /** The format a picked file looks like, or null. */
    fun guessImport(fileName: String, data: Redacted<ByteArray>): ImportGuess?

    /** A CSV's header and the suggested columns, for the column picker. */
    fun importColumns(data: Redacted<ByteArray>): CsvColumns

    /** Reads the file into a preview for the first vault; nothing is saved yet. */
    suspend fun prepareImport(
        format: String,
        data: Redacted<ByteArray>,
        password: Redacted<String>?,
        keyFile: Redacted<ByteArray>?,
        columns: ColumnMap?,
    ): ImportSummary

    /** Saves the prepared import, then syncs it up (a failed sync leaves it queued, like any change). */
    suspend fun finishImport(): ImportSummary

    suspend fun cancelImport()

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

    // --- Autofill (D58) ---

    /** One item, or null when it's gone. */
    suspend fun item(itemId: String, vaultId: String): Item?

    /** The core's roles for the fields described (`classify_fields`), against the caller's address. */
    suspend fun classifyFields(fieldsJson: String, pageUrl: String): String

    /** The logins the caller may get, best first; empty while locked. */
    suspend fun autofillMatches(caller: Caller): List<AutofillMatch>

    /** Links a login to an app, with its certificate (D58 §4), and saves it. */
    suspend fun linkApp(itemId: String, vaultId: String, packageName: String, cert: String)

    suspend fun isUnlocked(): Boolean
}

/** One of the account's devices (D43 §6). The name is opened by the core; null if it couldn't be. */
class DeviceEntry(
    val id: String,
    val platform: String,
    val name: String?,
    val createdAtMs: Long,
    val lastSeenAtMs: Long,
    val revokedAtMs: Long?,
    val isCurrent: Boolean,
) {
    override fun toString(): String = "DeviceEntry($platform)"
}

/** A new device's request to sign in (D46): `pending`, `accepted` or `revealed`, with the code once there is one. */
class ApprovalEntry(val id: String, val platform: String, val createdAtMs: Long, val state: String, val code: String?) {
    override fun toString(): String = "ApprovalEntry($state)"
}
