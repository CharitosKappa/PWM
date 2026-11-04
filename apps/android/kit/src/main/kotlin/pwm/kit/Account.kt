package pwm.kit

import java.net.URL

/**
 * What the screens need from a signed-in (or signing-in) device (D53 §1):
 * [Session] in the app, a fake in the tests.
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

    /** Items not in the trash, across this account's vaults. */
    suspend fun itemCount(): Int
}
