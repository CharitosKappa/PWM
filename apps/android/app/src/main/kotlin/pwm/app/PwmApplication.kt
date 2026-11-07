package pwm.app

import android.app.Activity
import android.app.Application
import android.content.BroadcastReceiver
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.PersistableBundle
import android.os.SystemClock
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import java.io.File
import java.util.Locale
import kotlinx.coroutines.MainScope
import pwm.kit.EmergencyKit
import pwm.kit.Redacted
import pwm.kit.Session
import pwm.kit.ui.AppModel
import pwm.kit.ui.AppPrefs
import pwm.kit.ui.SecretClipboard

/**
 * Owns the app's one [AppModel] for the whole process (D53 §1), so turning
 * the phone keeps the screen and what was typed.
 */
class PwmApplication : Application() {
    lateinit var model: AppModel
        private set
    private var shown: FragmentActivity? = null

    /** The sensitive clipboard, for the app and the fill screen. */
    val clipboard: AndroidClipboard by lazy { AndroidClipboard(this) }

    override fun onCreate() {
        super.onCreate()
        // The app fonts (D1), loaded before the first screen needs them.
        Thread({ preloadFonts(this) }, "pwm-fonts").start()
        val keys = KeystoreKeys(this)
        // The Activity on screen, for the fingerprint prompt (D55 §2).
        registerActivityLifecycleCallbacks(
            object : ActivityLifecycleCallbacks {
                override fun onActivityResumed(activity: Activity) {
                    shown = activity as? FragmentActivity
                }

                override fun onActivityPaused(activity: Activity) {
                    if (shown === activity) shown = null
                }

                override fun onActivityCreated(activity: Activity, state: Bundle?) = Unit

                override fun onActivityStarted(activity: Activity) = Unit

                override fun onActivityStopped(activity: Activity) = Unit

                override fun onActivitySaveInstanceState(activity: Activity, state: Bundle) = Unit

                override fun onActivityDestroyed(activity: Activity) = Unit
            },
        )
        val biometrics = KeystoreBiometrics(this) { shown }
        // The cache holds ciphertext only (D20), in a folder that is never backed up.
        val cache = File(noBackupFilesDir, "vault.db").path
        model = AppModel(
            scope = MainScope(),
            prefs = AndroidPrefs(this),
            // The model, such as "Pixel 8": no permission needed (D53 §5).
            deviceName = Build.MODEL?.takeIf { it.isNotBlank() } ?: "Android",
            clipboard = clipboard,
            paper = EmergencyKit.Paper.of(Locale.getDefault().country),
            makeAccount = { server -> Session(cache, server, keys, biometrics = biometrics) },
            // Keeps counting while the phone sleeps; ignores the wall clock (D54 §5).
            clock = SystemClock::elapsedRealtime,
            // The periodic background sync while someone is signed in (D57 §2).
            background = WorkManagerScheduler(this),
            // Files picked for import, read once through the system (D59 §7).
            importFiles = AndroidImportFiles(this),
            // Push over FCM, when the build has Firebase's settings (D60).
            push = FcmPush(this),
            notices = AndroidNotices(this),
            // Buying Premium through Google Play (D6 §2); the sheet needs the Activity on screen.
            playBilling = PlayStoreBilling(this) { shown },
        )
        model.start()
        // The whole app's background and foreground, so moving between its
        // own screens never counts as leaving.
        ProcessLifecycleOwner.get().lifecycle.addObserver(
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) = model.appForegrounded()

                override fun onStop(owner: LifecycleOwner) = model.appBackgrounded()
            },
        )
        // The screen turning off locks the vault, whatever the timeout.
        ContextCompat.registerReceiver(
            this,
            object : BroadcastReceiver() {
                override fun onReceive(context: Context, intent: Intent) = model.screenOff()
            },
            IntentFilter(Intent.ACTION_SCREEN_OFF),
            ContextCompat.RECEIVER_NOT_EXPORTED,
        )
    }
}

/** The server choice and the kit flag, in the app's private preferences (not secrets). */
class AndroidPrefs(context: Context) : AppPrefs {
    private val prefs = context.getSharedPreferences("pwm.settings", Context.MODE_PRIVATE)

    override var server: String?
        get() = prefs.getString(SERVER, null)
        set(value) = prefs.edit().apply { if (value == null) remove(SERVER) else putString(SERVER, value) }.apply()

    override var kitPending: Boolean
        get() = prefs.getBoolean(KIT_PENDING, false)
        set(value) = prefs.edit().putBoolean(KIT_PENDING, value).apply()

    override var signInAgain: Boolean
        get() = prefs.getBoolean(SIGN_IN_AGAIN, false)
        set(value) = prefs.edit().putBoolean(SIGN_IN_AGAIN, value).apply()

    override var approvalNotices: Boolean
        get() = prefs.getBoolean(APPROVAL_NOTICES, false)
        set(value) = prefs.edit().putBoolean(APPROVAL_NOTICES, value).apply()

    override var pushToken: String?
        get() = prefs.getString(PUSH_TOKEN, null)
        set(value) = prefs.edit().apply { if (value == null) remove(PUSH_TOKEN) else putString(PUSH_TOKEN, value) }.apply()

    override var breachCheck: Boolean
        get() = prefs.getBoolean(BREACH_CHECK, true)
        set(value) = prefs.edit().putBoolean(BREACH_CHECK, value).apply()

    override var hideFromCapture: Boolean
        get() = prefs.getBoolean(HIDE_FROM_CAPTURE, true)
        set(value) = prefs.edit().putBoolean(HIDE_FROM_CAPTURE, value).apply()

    override var biometricOffered: Boolean
        get() = prefs.getBoolean(BIOMETRIC_OFFERED, false)
        set(value) = prefs.edit().putBoolean(BIOMETRIC_OFFERED, value).apply()

    override var lockTimeoutMinutes: Int?
        get() = if (prefs.contains(LOCK_TIMEOUT)) prefs.getInt(LOCK_TIMEOUT, 0) else null
        set(value) = prefs.edit().apply { if (value == null) remove(LOCK_TIMEOUT) else putInt(LOCK_TIMEOUT, value) }.apply()

    private companion object {
        const val SERVER = "server"
        const val KIT_PENDING = "kitPending"
        const val LOCK_TIMEOUT = "lockTimeoutMinutes"
        const val BIOMETRIC_OFFERED = "biometricOffered"
        const val HIDE_FROM_CAPTURE = "hideFromCapture"
        const val SIGN_IN_AGAIN = "signInAgain"
        const val BREACH_CHECK = "breachCheck"
        const val APPROVAL_NOTICES = "approvalNotices"
        const val PUSH_TOKEN = "pushToken"
    }
}

/**
 * Copies a secret marked sensitive, so Android 13+ hides it in the
 * clipboard preview, then clears it (D53 §2). If something else was copied
 * since and the app can still see that, it is left alone.
 */
class AndroidClipboard(private val context: Context) : SecretClipboard {
    private val handler = Handler(Looper.getMainLooper())
    private var clear: Runnable? = null

    override fun copy(value: Redacted<String>, clearAfterMs: Long) {
        val manager = context.getSystemService(ClipboardManager::class.java) ?: return
        val clip = ClipData.newPlainText(context.getString(R.string.secret_key_label), value.value)
        clip.description.extras = PersistableBundle().apply {
            putBoolean(IS_SENSITIVE, true)
            putBoolean(OURS, true)
        }
        manager.setPrimaryClip(clip)
        clear?.let(handler::removeCallbacks)
        clear = Runnable {
            // In the background Android hides the clipboard from apps (null): clear it then too.
            val now = manager.primaryClipDescription
            if (now == null || now.extras?.getBoolean(OURS) == true) manager.clearPrimaryClip()
        }.also { handler.postDelayed(it, clearAfterMs) }
    }

    private companion object {
        /** `ClipDescription.EXTRA_IS_SENSITIVE` (Android 13), by value so older versions build. */
        const val IS_SENSITIVE = "android.content.extra.IS_SENSITIVE"
        const val OURS = "pwm.secret"
    }
}
