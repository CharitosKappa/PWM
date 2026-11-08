package pwm.kit.ui

import pwm.kit.PwmError
import pwm.kit.Redacted

/** Small settings that aren't secrets (D53 §5): the app's private preferences. */
interface AppPrefs {
    /** The chosen server's address; null for the hosted service. */
    var server: String?

    /**
     * Set at sign-up, cleared by "I saved it" (D53 §4): while set, the app
     * shows the Secret Key screen again after unlocking.
     */
    var kitPending: Boolean

    /** The lock timeout's minutes (D54 §5); null for the default. */
    var lockTimeoutMinutes: Int?

    /** The unlock screen offered fingerprint or face unlock once (D45 §3). */
    var biometricOffered: Boolean

    /** "Hide PWM from screenshots and screen sharing" (D56 §2), on by default. */
    var hideFromCapture: Boolean

    /** The server refused this device's refresh token: "Sign in again to sync" shows (D50 §5). */
    var signInAgain: Boolean

    /** "Check for leaked passwords" (D22 §5, D59 §5), on by default. */
    var breachCheck: Boolean

    /** "Notify me when a new device asks to sign in" (D60 §4), off until turned on. */
    var approvalNotices: Boolean

    /** The FCM token last sent to the server for this sign-in; null to send it again. */
    var pushToken: String?
}

class MemoryPrefs(
    override var server: String? = null,
    override var kitPending: Boolean = false,
    override var lockTimeoutMinutes: Int? = null,
    override var biometricOffered: Boolean = false,
    override var hideFromCapture: Boolean = true,
    override var signInAgain: Boolean = false,
    override var breachCheck: Boolean = true,
    override var approvalNotices: Boolean = false,
    override var pushToken: String? = null,
) : AppPrefs

/** Push (D60): FCM in the app, when the build has Firebase's settings; a fake in tests. */
interface PushService {
    /** Firebase is set up in this build; without it, push is off. */
    val available: Boolean

    /** This device's current FCM token, or null. */
    suspend fun token(): String?
}

object NoPush : PushService {
    override val available: Boolean = false

    override suspend fun token(): String? = null
}

/** The new-device notice (D60 §4): a notification in the app, a fake in tests. */
interface Notices {
    /** The system lets the app show notifications (always below Android 13). */
    fun allowed(): Boolean

    /** "A new device wants to sign in. Open PWM to compare the code." */
    fun showApprovalNotice()
}

object NoNotices : Notices {
    override fun allowed(): Boolean = false

    override fun showApprovalNotice() = Unit
}

/**
 * The files the user picks for import (D59 §7), by the address the system's
 * picker gave: the content resolver in the app, a map in tests. Never a
 * copy of ours.
 */
interface ImportFiles {
    /** The file's name as the picker shows it. */
    fun name(uri: String): String?

    /** Its size in bytes, if the system knows it. */
    fun size(uri: String): Long?

    /** Reads it into memory once. */
    fun read(uri: String): ByteArray

    /** Asks the place it came from to delete it; false where it can't (some cloud drives). */
    fun delete(uri: String): Boolean
}

object NoImportFiles : ImportFiles {
    override fun name(uri: String): String? = null

    override fun size(uri: String): Long? = null

    override fun read(uri: String): ByteArray = throw PwmError("import_unreadable")

    override fun delete(uri: String): Boolean = false
}

/** The periodic background sync (D57 §2): WorkManager in the app, a fake in tests. */
interface BackgroundScheduler {
    /** While someone is signed in on this device. */
    fun schedule()

    fun cancel()

    /** A push said something changed: one sync now, as soon as Android allows (D60 §4). */
    fun syncNow() = Unit
}

object NoBackgroundScheduler : BackgroundScheduler {
    override fun schedule() = Unit

    override fun cancel() = Unit
}

/** What a background run found, for the job's result. */
enum class BackgroundResult { NEW_DATA, NO_DATA, FAILED }

/**
 * What is said to TalkBack without moving focus: copies and locking
 * (design/README.md, Accessibility). The screen has the words; never a secret.
 */
enum class Announcement { COPIED, LOCKED }

/** Where the models' announcements go: the activity says them, tests listen. */
object Speech {
    @Volatile
    var listener: (Announcement) -> Unit = {}

    fun say(announcement: Announcement) = listener(announcement)
}

/** A copy that is hidden from the clipboard preview and cleared after a while. */
fun interface SecretClipboard {
    fun copy(value: Redacted<String>, clearAfterMs: Long)
}

/** What to show for a failure; never a value. */
internal fun message(error: Throwable): String = PwmError.from(error).userMessage
