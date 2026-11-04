package pwm.app

import android.app.Application
import android.content.BroadcastReceiver
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.PersistableBundle
import android.os.SystemClock
import androidx.core.content.ContextCompat
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

    override fun onCreate() {
        super.onCreate()
        val keys = KeystoreKeys(this)
        // The cache holds ciphertext only (D20), in a folder that is never backed up.
        val cache = File(noBackupFilesDir, "vault.db").path
        model = AppModel(
            scope = MainScope(),
            prefs = AndroidPrefs(this),
            // The model, such as "Pixel 8": no permission needed (D53 §5).
            deviceName = Build.MODEL?.takeIf { it.isNotBlank() } ?: "Android",
            clipboard = AndroidClipboard(this),
            paper = EmergencyKit.Paper.of(Locale.getDefault().country),
            makeAccount = { server -> Session(cache, server, keys) },
            // Keeps counting while the phone sleeps; ignores the wall clock (D54 §5).
            clock = SystemClock::elapsedRealtime,
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

    override var lockTimeoutMinutes: Int?
        get() = if (prefs.contains(LOCK_TIMEOUT)) prefs.getInt(LOCK_TIMEOUT, 0) else null
        set(value) = prefs.edit().apply { if (value == null) remove(LOCK_TIMEOUT) else putInt(LOCK_TIMEOUT, value) }.apply()

    private companion object {
        const val SERVER = "server"
        const val KIT_PENDING = "kitPending"
        const val LOCK_TIMEOUT = "lockTimeoutMinutes"
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
